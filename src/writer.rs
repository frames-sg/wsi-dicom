use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use dicom_core::value::DataSetSequence;
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::tags;
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};

use crate::tile::PixelProfile;
use crate::{DicomMetadata, WsiDicomError, VL_WSI_SOP_CLASS_UID};

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_dicom_object(
    metadata: &DicomMetadata,
    study_uid: &str,
    series_uid: &str,
    sop_instance_uid: &str,
    level_idx: u32,
    tile_size: u32,
    matrix_columns: u64,
    matrix_rows: u64,
    frame_count: u32,
    profile: PixelProfile,
    offsets: Vec<u64>,
    lengths: Vec<u64>,
) -> Result<InMemDicomObject, WsiDicomError> {
    let mut object = InMemDicomObject::new_empty();
    put_str(
        &mut object,
        tags::SOP_CLASS_UID,
        VR::UI,
        VL_WSI_SOP_CLASS_UID,
    );
    put_str(
        &mut object,
        tags::SOP_INSTANCE_UID,
        VR::UI,
        sop_instance_uid,
    );
    put_str(&mut object, tags::STUDY_INSTANCE_UID, VR::UI, study_uid);
    put_str(&mut object, tags::SERIES_INSTANCE_UID, VR::UI, series_uid);
    put_str(&mut object, tags::MODALITY, VR::CS, "SM");
    put_str(
        &mut object,
        tags::IMAGE_TYPE,
        VR::CS,
        "ORIGINAL\\PRIMARY\\VOLUME\\NONE",
    );
    put_str(
        &mut object,
        tags::PATIENT_NAME,
        VR::PN,
        metadata.patient_name.as_deref().unwrap_or_default(),
    );
    put_str(
        &mut object,
        tags::PATIENT_ID,
        VR::LO,
        metadata.patient_id.as_deref().unwrap_or_default(),
    );
    put_str(
        &mut object,
        tags::ACCESSION_NUMBER,
        VR::SH,
        metadata.accession_number.as_deref().unwrap_or_default(),
    );
    put_str(
        &mut object,
        tags::STUDY_ID,
        VR::SH,
        metadata.study_id.as_deref().unwrap_or("1"),
    );
    put_str(
        &mut object,
        tags::STUDY_DESCRIPTION,
        VR::LO,
        metadata.study_description.as_deref().unwrap_or_default(),
    );
    put_u16(&mut object, tags::ROWS, tile_size as u16);
    put_u16(&mut object, tags::COLUMNS, tile_size as u16);
    put_u32(
        &mut object,
        tags::TOTAL_PIXEL_MATRIX_COLUMNS,
        matrix_columns as u32,
    );
    put_u32(
        &mut object,
        tags::TOTAL_PIXEL_MATRIX_ROWS,
        matrix_rows as u32,
    );
    put_str(
        &mut object,
        tags::NUMBER_OF_FRAMES,
        VR::IS,
        &frame_count.to_string(),
    );
    put_u16(
        &mut object,
        tags::SAMPLES_PER_PIXEL,
        profile.components as u16,
    );
    put_str(
        &mut object,
        tags::PHOTOMETRIC_INTERPRETATION,
        VR::CS,
        profile.photometric_interpretation,
    );
    if profile.components > 1 {
        put_u16(&mut object, tags::PLANAR_CONFIGURATION, 0);
    }
    put_u16(&mut object, tags::BITS_ALLOCATED, profile.bits_allocated);
    put_u16(&mut object, tags::BITS_STORED, profile.bits_allocated);
    put_u16(&mut object, tags::HIGH_BIT, profile.bits_allocated - 1);
    put_u16(&mut object, tags::PIXEL_REPRESENTATION, 0);
    put_str(
        &mut object,
        tags::DIMENSION_ORGANIZATION_TYPE,
        VR::CS,
        "TILED_FULL",
    );
    put_u16(&mut object, tags::NUMBER_OF_OPTICAL_PATHS, 1);
    put_u32(&mut object, tags::TOTAL_PIXEL_MATRIX_FOCAL_PLANES, 1);
    put_str(&mut object, tags::SPECIMEN_LABEL_IN_IMAGE, VR::CS, "NO");
    put_u32(&mut object, tags::SERIES_NUMBER, level_idx + 1);
    object.put(DataElement::new(
        tags::EXTENDED_OFFSET_TABLE,
        VR::OV,
        PrimitiveValue::U64(offsets.into()),
    ));
    object.put(DataElement::new(
        tags::EXTENDED_OFFSET_TABLE_LENGTHS,
        VR::OV,
        PrimitiveValue::U64(lengths.into()),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::OPTICAL_PATH_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![optical_path_item()]),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(per_frame_items(frame_count, tile_size, matrix_columns)?),
    ));
    Ok(object)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SpooledPixelDataFragment {
    pub(crate) spool_offset: u64,
    pub(crate) padded_len: u32,
}

pub(crate) struct PixelDataSpool {
    path: PathBuf,
    file: File,
    fragments: Vec<SpooledPixelDataFragment>,
    offsets: Vec<u64>,
    lengths: Vec<u64>,
    next_extended_offset: u64,
}

impl PixelDataSpool {
    pub(crate) fn create(path: PathBuf, frame_count: usize) -> Result<Self, WsiDicomError> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| WsiDicomError::Io {
                path: path.clone(),
                source,
            })?;
        Ok(Self {
            path,
            file,
            fragments: Vec::with_capacity(frame_count),
            offsets: Vec::with_capacity(frame_count),
            lengths: Vec::with_capacity(frame_count),
            next_extended_offset: 0,
        })
    }

    pub(crate) fn push_frame(&mut self, codestream: &[u8]) -> Result<(), WsiDicomError> {
        let raw_len = u64::try_from(codestream.len()).map_err(|_| WsiDicomError::Unsupported {
            reason: "encoded frame length exceeds u64".into(),
        })?;
        let padded_len =
            raw_len
                .checked_add(raw_len % 2)
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "encoded frame padded length overflow".into(),
                })?;
        let padded_len_u32 = u32::try_from(padded_len).map_err(|_| WsiDicomError::Unsupported {
            reason: "encoded frame exceeds DICOM fragment item length limit".into(),
        })?;
        let spool_offset = self
            .file
            .stream_position()
            .map_err(|source| WsiDicomError::Io {
                path: self.path.clone(),
                source,
            })?;
        self.file
            .write_all(codestream)
            .map_err(|source| WsiDicomError::Io {
                path: self.path.clone(),
                source,
            })?;
        if raw_len != padded_len {
            self.file
                .write_all(&[0])
                .map_err(|source| WsiDicomError::Io {
                    path: self.path.clone(),
                    source,
                })?;
        }
        self.offsets.push(self.next_extended_offset);
        self.lengths.push(raw_len);
        self.fragments.push(SpooledPixelDataFragment {
            spool_offset,
            padded_len: padded_len_u32,
        });
        self.next_extended_offset = self
            .next_extended_offset
            .checked_add(8)
            .and_then(|offset| offset.checked_add(padded_len))
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "extended offset table overflow".into(),
            })?;
        Ok(())
    }

    pub(crate) fn offsets(&self) -> Vec<u64> {
        self.offsets.clone()
    }

    pub(crate) fn lengths(&self) -> Vec<u64> {
        self.lengths.clone()
    }
}

impl Drop for PixelDataSpool {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn write_dicom_object_with_spooled_pixel_data(
    path: &Path,
    object: InMemDicomObject,
    meta: FileMetaTableBuilder,
    spool: &mut PixelDataSpool,
) -> Result<(), WsiDicomError> {
    spool.file.flush().map_err(|source| WsiDicomError::Io {
        path: spool.path.clone(),
        source,
    })?;
    spool
        .file
        .seek(SeekFrom::Start(0))
        .map_err(|source| WsiDicomError::Io {
            path: spool.path.clone(),
            source,
        })?;

    let mut file = File::create(path).map_err(|source| WsiDicomError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    object
        .with_meta(meta)
        .map_err(|err| WsiDicomError::DicomWrite {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?
        .write_all(&mut file)
        .map_err(|err| WsiDicomError::DicomWrite {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
    write_encapsulated_pixel_data_from_spool(&mut file, &mut spool.file, &spool.fragments)
        .map_err(|source| WsiDicomError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.flush().map_err(|source| WsiDicomError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

pub(crate) fn write_encapsulated_pixel_data_from_spool(
    output: &mut impl Write,
    spool: &mut (impl Read + Seek),
    fragments: &[SpooledPixelDataFragment],
) -> std::io::Result<()> {
    write_tag(output, 0x7FE0, 0x0010)?;
    output.write_all(b"OB")?;
    output.write_all(&[0, 0])?;
    output.write_all(&u32::MAX.to_le_bytes())?;
    write_item_header(output, 0)?;
    for fragment in fragments {
        spool.seek(SeekFrom::Start(fragment.spool_offset))?;
        write_item_header(output, fragment.padded_len)?;
        let mut limited = spool.by_ref().take(u64::from(fragment.padded_len));
        let copied = std::io::copy(&mut limited, output)?;
        if copied != u64::from(fragment.padded_len) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "spooled PixelData fragment ended before padded length",
            ));
        }
    }
    write_tag(output, 0xFFFE, 0xE0DD)?;
    output.write_all(&0u32.to_le_bytes())
}

fn write_item_header(output: &mut impl Write, length: u32) -> std::io::Result<()> {
    write_tag(output, 0xFFFE, 0xE000)?;
    output.write_all(&length.to_le_bytes())
}

fn write_tag(output: &mut impl Write, group: u16, element: u16) -> std::io::Result<()> {
    output.write_all(&group.to_le_bytes())?;
    output.write_all(&element.to_le_bytes())
}

fn optical_path_item() -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    put_str(&mut item, tags::OPTICAL_PATH_IDENTIFIER, VR::SH, "0");
    put_str(
        &mut item,
        tags::OPTICAL_PATH_DESCRIPTION,
        VR::ST,
        "Default optical path",
    );
    item
}

fn per_frame_items(
    frame_count: u32,
    tile_size: u32,
    matrix_columns: u64,
) -> Result<Vec<InMemDicomObject>, WsiDicomError> {
    let tiles_across = matrix_columns.div_ceil(u64::from(tile_size));
    let mut items = Vec::with_capacity(frame_count as usize);
    for frame in 0..frame_count {
        let row = u64::from(frame) / tiles_across;
        let col = u64::from(frame) % tiles_across;
        let mut position = InMemDicomObject::new_empty();
        position.put(DataElement::new(
            tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
            VR::SL,
            PrimitiveValue::from((col * u64::from(tile_size) + 1) as i32),
        ));
        position.put(DataElement::new(
            tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
            VR::SL,
            PrimitiveValue::from((row * u64::from(tile_size) + 1) as i32),
        ));
        let mut item = InMemDicomObject::new_empty();
        item.put(DataElement::<InMemDicomObject>::new(
            tags::PLANE_POSITION_SLIDE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![position]),
        ));
        items.push(item);
    }
    Ok(items)
}

fn put_str(object: &mut InMemDicomObject, tag: Tag, vr: VR, value: &str) {
    object.put(DataElement::new(tag, vr, value));
}

fn put_u16(object: &mut InMemDicomObject, tag: Tag, value: u16) {
    object.put(DataElement::new(tag, VR::US, PrimitiveValue::from(value)));
}

fn put_u32(object: &mut InMemDicomObject, tag: Tag, value: u32) {
    object.put(DataElement::new(tag, VR::UL, PrimitiveValue::from(value)));
}

#[cfg(test)]
mod tests {
    use super::{write_encapsulated_pixel_data_from_spool, SpooledPixelDataFragment};
    use std::io::Write;

    #[test]
    fn spooled_pixel_data_writer_appends_encapsulated_fragments_with_padding() {
        let tmp = tempfile::tempdir().unwrap();
        let spool_path = tmp.path().join("frames.bin");
        let mut spool = std::fs::File::create(&spool_path).unwrap();
        spool.write_all(&[1, 2, 3, 0, 4, 5]).unwrap();
        drop(spool);

        let mut spool = std::fs::File::open(&spool_path).unwrap();
        let fragments = [
            SpooledPixelDataFragment {
                spool_offset: 0,
                padded_len: 4,
            },
            SpooledPixelDataFragment {
                spool_offset: 4,
                padded_len: 2,
            },
        ];
        let mut out = Vec::new();

        write_encapsulated_pixel_data_from_spool(&mut out, &mut spool, &fragments).unwrap();

        assert_eq!(
            out,
            vec![
                0xE0, 0x7F, 0x10, 0x00, b'O', b'B', 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE, 0xFF,
                0x00, 0xE0, 0x00, 0x00, 0x00, 0x00, 0xFE, 0xFF, 0x00, 0xE0, 0x04, 0x00, 0x00, 0x00,
                1, 2, 3, 0, 0xFE, 0xFF, 0x00, 0xE0, 0x02, 0x00, 0x00, 0x00, 4, 5, 0xFE, 0xFF, 0xDD,
                0xE0, 0x00, 0x00, 0x00, 0x00,
            ]
        );
    }

    #[test]
    fn pixel_data_spool_records_padded_extended_offsets_and_raw_lengths() {
        let tmp = tempfile::tempdir().unwrap();
        let mut spool = super::PixelDataSpool::create(tmp.path().join("frames.bin"), 2).unwrap();

        spool.push_frame(&[1, 2, 3]).unwrap();
        spool.push_frame(&[4, 5]).unwrap();

        assert_eq!(spool.offsets(), vec![0, 12]);
        assert_eq!(spool.lengths(), vec![3, 2]);
    }
}
