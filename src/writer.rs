use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dicom_core::value::DataSetSequence;
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::tags;
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};

use crate::tile::PixelProfile;
use crate::uid::uid_from_seed;
use crate::{DicomMetadata, Error, VL_WSI_SOP_CLASS_UID};

const DEFAULT_DATE: &str = "19700101";
const DEFAULT_TIME: &str = "000000";
const DEFAULT_DATE_TIME: &str = "19700101000000";
const DEFAULT_POSITION_REFERENCE: &str = "SLIDE_CORNER";
const DEFAULT_MANUFACTURER: &str = "wsi-dicom";
const DEFAULT_DEVICE_SERIAL_NUMBER: &str = "RESEARCH";
const DEFAULT_CONTAINER_IDENTIFIER: &str = "RESEARCH-CONTAINER";
const DEFAULT_SPECIMEN_IDENTIFIER: &str = "RESEARCH-SPECIMEN";
const DEFAULT_SPECIMEN_DESCRIPTION: &str = "Research placeholder specimen";
const DEFAULT_IMAGED_VOLUME_DEPTH_MM: f64 = 0.001;
const DEFAULT_FOCUS_METHOD: &str = "AUTO";
const DICOM_FILE_WRITE_BUFFER_BYTES: usize = 4 * 1024 * 1024;
static SPOOL_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) struct LossyCompressionMetadata {
    pub(crate) method: &'static str,
    pub(crate) ratio: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameGrid {
    pub(crate) frame_columns: u32,
    pub(crate) frame_rows: u32,
    pub(crate) matrix_columns: u64,
    pub(crate) matrix_rows: u64,
}

impl FrameGrid {
    fn validate(self) -> Result<(), Error> {
        if self.frame_columns == 0 || self.frame_rows == 0 {
            return Err(Error::Unsupported {
                reason: "DICOM per-frame positions require non-zero frame dimensions".into(),
            });
        }
        if self.matrix_columns == 0 || self.matrix_rows == 0 {
            return Err(Error::Unsupported {
                reason: "DICOM total pixel matrix requires non-zero dimensions".into(),
            });
        }
        Ok(())
    }

    fn tiles_across(self) -> Result<u64, Error> {
        self.validate()?;
        Ok(self.matrix_columns.div_ceil(u64::from(self.frame_columns)))
    }

    fn location_for_frame(self, frame_index: u32) -> Result<FrameLocation, Error> {
        let tiles_across = self.tiles_across()?;
        Ok(FrameLocation {
            row: u64::from(frame_index) / tiles_across,
            column: u64::from(frame_index) % tiles_across,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameLocation {
    row: u64,
    column: u64,
}

impl FrameLocation {
    fn dimension_index_values(self) -> Result<[u32; 2], Error> {
        Ok([
            checked_dimension_index_value(self.column, "column")?,
            checked_dimension_index_value(self.row, "row")?,
        ])
    }

    fn slide_matrix_positions(self, grid: FrameGrid) -> Result<(i32, i32), Error> {
        Ok((
            checked_slide_matrix_position(self.column, grid.frame_columns, "column")?,
            checked_slide_matrix_position(self.row, grid.frame_rows, "row")?,
        ))
    }

    fn slide_coordinate_offsets(
        self,
        grid: FrameGrid,
        row_spacing_mm: f64,
        column_spacing_mm: f64,
    ) -> (f64, f64) {
        (
            self.column as f64 * f64::from(grid.frame_columns) * column_spacing_mm,
            self.row as f64 * f64::from(grid.frame_rows) * row_spacing_mm,
        )
    }
}

pub(crate) struct PixelDataOffsetTables {
    pub(crate) offsets: Vec<u64>,
    pub(crate) lengths: Vec<u64>,
}

pub(crate) struct DicomObjectIdentifiers<'a> {
    pub(crate) study_uid: &'a str,
    pub(crate) series_uid: &'a str,
    pub(crate) sop_instance_uid: &'a str,
    pub(crate) frame_of_reference_uid: &'a str,
    pub(crate) pyramid_uid: &'a str,
    pub(crate) dimension_organization_uid: &'a str,
    pub(crate) pyramid_label: &'a str,
}

pub(crate) struct DicomObjectParams<'a> {
    pub(crate) metadata: &'a DicomMetadata,
    pub(crate) identifiers: DicomObjectIdentifiers<'a>,
    pub(crate) series_number: u32,
    pub(crate) instance_number: u32,
    pub(crate) level_idx: u32,
    pub(crate) frame_grid: FrameGrid,
    pub(crate) frame_count: u32,
    pub(crate) profile: PixelProfile,
    pub(crate) pixel_spacing_mm: Option<(f64, f64)>,
    pub(crate) pixel_data_offsets: PixelDataOffsetTables,
    pub(crate) icc_profile: Option<&'a [u8]>,
    pub(crate) lossy_compression: Option<LossyCompressionMetadata>,
}

pub(crate) fn build_dicom_object(params: DicomObjectParams<'_>) -> Result<InMemDicomObject, Error> {
    let mut object = InMemDicomObject::new_empty();
    let metadata = params.metadata.validated_for_writer()?;
    let identifiers = params.identifiers;
    let frame_grid = params.frame_grid;
    frame_grid.validate()?;
    let (row_spacing_mm, column_spacing_mm) =
        params.pixel_spacing_mm.ok_or_else(|| Error::Metadata {
            reason: "VL WSI VOLUME export requires pixel spacing metadata".into(),
        })?;
    let dicom_frame_rows = checked_u16_attribute(frame_grid.frame_rows, "Rows")?;
    let dicom_frame_columns = checked_u16_attribute(frame_grid.frame_columns, "Columns")?;
    let dicom_matrix_columns =
        checked_u32_attribute(frame_grid.matrix_columns, "Total Pixel Matrix Columns")?;
    let dicom_matrix_rows =
        checked_u32_attribute(frame_grid.matrix_rows, "Total Pixel Matrix Rows")?;
    let image_type = if params.level_idx == 0 {
        "ORIGINAL\\PRIMARY\\VOLUME\\NONE"
    } else {
        "DERIVED\\PRIMARY\\VOLUME\\RESAMPLED"
    };
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
        identifiers.sop_instance_uid,
    );
    put_str(
        &mut object,
        tags::STUDY_INSTANCE_UID,
        VR::UI,
        identifiers.study_uid,
    );
    put_str(
        &mut object,
        tags::SERIES_INSTANCE_UID,
        VR::UI,
        identifiers.series_uid,
    );
    put_str(
        &mut object,
        tags::FRAME_OF_REFERENCE_UID,
        VR::UI,
        identifiers.frame_of_reference_uid,
    );
    put_str(
        &mut object,
        tags::PYRAMID_UID,
        VR::UI,
        identifiers.pyramid_uid,
    );
    put_str(
        &mut object,
        tags::PYRAMID_LABEL,
        VR::LO,
        identifiers.pyramid_label,
    );
    put_str(&mut object, tags::MODALITY, VR::CS, "SM");
    put_str(
        &mut object,
        tags::ACQUISITION_DATE,
        VR::DA,
        metadata.content_date.as_deref().unwrap_or(DEFAULT_DATE),
    );
    put_str(
        &mut object,
        tags::ACQUISITION_TIME,
        VR::TM,
        metadata.content_time.as_deref().unwrap_or(DEFAULT_TIME),
    );
    put_str(&mut object, tags::IMAGE_TYPE, VR::CS, image_type);
    put_str(&mut object, tags::LOSSY_IMAGE_COMPRESSION, VR::CS, "00");
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
        tags::PATIENT_BIRTH_DATE,
        VR::DA,
        metadata.patient_birth_date.as_deref().unwrap_or_default(),
    );
    put_str(
        &mut object,
        tags::PATIENT_SEX,
        VR::CS,
        metadata.patient_sex.as_deref().unwrap_or_default(),
    );
    put_str(
        &mut object,
        tags::ACCESSION_NUMBER,
        VR::SH,
        metadata.accession_number.as_deref().unwrap_or_default(),
    );
    put_str(
        &mut object,
        tags::STUDY_DATE,
        VR::DA,
        metadata.study_date.as_deref().unwrap_or(DEFAULT_DATE),
    );
    put_str(
        &mut object,
        tags::STUDY_TIME,
        VR::TM,
        metadata.study_time.as_deref().unwrap_or(DEFAULT_TIME),
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
    put_str(
        &mut object,
        tags::REFERRING_PHYSICIAN_NAME,
        VR::PN,
        metadata
            .referring_physician_name
            .as_deref()
            .unwrap_or_default(),
    );
    if let Some(laterality) = non_empty(metadata.laterality.as_deref()) {
        put_str(&mut object, tags::LATERALITY, VR::CS, laterality);
    }
    put_str(
        &mut object,
        tags::POSITION_REFERENCE_INDICATOR,
        VR::LO,
        DEFAULT_POSITION_REFERENCE,
    );
    put_str(
        &mut object,
        tags::MANUFACTURER,
        VR::LO,
        metadata
            .manufacturer
            .as_deref()
            .unwrap_or(DEFAULT_MANUFACTURER),
    );
    put_str(
        &mut object,
        tags::MANUFACTURER_MODEL_NAME,
        VR::LO,
        metadata
            .manufacturer_model_name
            .as_deref()
            .unwrap_or(DEFAULT_MANUFACTURER),
    );
    put_str(
        &mut object,
        tags::DEVICE_SERIAL_NUMBER,
        VR::LO,
        metadata
            .device_serial_number
            .as_deref()
            .unwrap_or(DEFAULT_DEVICE_SERIAL_NUMBER),
    );
    put_str(
        &mut object,
        tags::SOFTWARE_VERSIONS,
        VR::LO,
        metadata
            .software_versions
            .as_deref()
            .unwrap_or(env!("CARGO_PKG_VERSION")),
    );
    put_str(
        &mut object,
        tags::CONTENT_DATE,
        VR::DA,
        metadata.content_date.as_deref().unwrap_or(DEFAULT_DATE),
    );
    put_str(
        &mut object,
        tags::CONTENT_TIME,
        VR::TM,
        metadata.content_time.as_deref().unwrap_or(DEFAULT_TIME),
    );
    put_str(
        &mut object,
        tags::ACQUISITION_DATE_TIME,
        VR::DT,
        metadata
            .acquisition_date_time
            .as_deref()
            .unwrap_or(DEFAULT_DATE_TIME),
    );
    put_str(
        &mut object,
        tags::CONTAINER_IDENTIFIER,
        VR::LO,
        metadata
            .container_identifier
            .as_deref()
            .unwrap_or(DEFAULT_CONTAINER_IDENTIFIER),
    );
    put_u16(&mut object, tags::ROWS, dicom_frame_rows);
    put_u16(&mut object, tags::COLUMNS, dicom_frame_columns);
    put_u32(
        &mut object,
        tags::TOTAL_PIXEL_MATRIX_COLUMNS,
        dicom_matrix_columns,
    );
    put_u32(
        &mut object,
        tags::TOTAL_PIXEL_MATRIX_ROWS,
        dicom_matrix_rows,
    );
    put_fl(
        &mut object,
        tags::IMAGED_VOLUME_WIDTH,
        frame_grid.matrix_columns as f64 * column_spacing_mm,
    );
    put_fl(
        &mut object,
        tags::IMAGED_VOLUME_HEIGHT,
        frame_grid.matrix_rows as f64 * row_spacing_mm,
    );
    put_fl(
        &mut object,
        tags::IMAGED_VOLUME_DEPTH,
        metadata
            .imaged_volume_depth_mm
            .unwrap_or(DEFAULT_IMAGED_VOLUME_DEPTH_MM),
    );
    put_str(
        &mut object,
        tags::NUMBER_OF_FRAMES,
        VR::IS,
        &params.frame_count.to_string(),
    );
    put_u16(
        &mut object,
        tags::SAMPLES_PER_PIXEL,
        params.profile.components as u16,
    );
    put_str(
        &mut object,
        tags::PHOTOMETRIC_INTERPRETATION,
        VR::CS,
        params.profile.photometric_interpretation,
    );
    if params.profile.components > 1 {
        put_u16(&mut object, tags::PLANAR_CONFIGURATION, 0);
    }
    put_u16(
        &mut object,
        tags::BITS_ALLOCATED,
        params.profile.bits_allocated,
    );
    put_u16(
        &mut object,
        tags::BITS_STORED,
        params.profile.bits_allocated,
    );
    put_u16(
        &mut object,
        tags::HIGH_BIT,
        params.profile.bits_allocated - 1,
    );
    put_u16(&mut object, tags::PIXEL_REPRESENTATION, 0);
    if let Some(lossy) = params.lossy_compression {
        put_str(&mut object, tags::LOSSY_IMAGE_COMPRESSION, VR::CS, "01");
        if let Some(ratio) = lossy.ratio {
            let ratio = format!("{ratio:.3}");
            put_str(
                &mut object,
                tags::LOSSY_IMAGE_COMPRESSION_RATIO,
                VR::DS,
                &ratio,
            );
        }
        put_str(
            &mut object,
            tags::LOSSY_IMAGE_COMPRESSION_METHOD,
            VR::CS,
            lossy.method,
        );
    }
    put_str(
        &mut object,
        tags::DIMENSION_ORGANIZATION_TYPE,
        VR::CS,
        "TILED_FULL",
    );
    put_u32(&mut object, tags::NUMBER_OF_OPTICAL_PATHS, 1);
    put_u32(&mut object, tags::TOTAL_PIXEL_MATRIX_FOCAL_PLANES, 1);
    put_str(&mut object, tags::SPECIMEN_LABEL_IN_IMAGE, VR::CS, "NO");
    put_str(&mut object, tags::BURNED_IN_ANNOTATION, VR::CS, "NO");
    put_str(&mut object, tags::VOLUMETRIC_PROPERTIES, VR::CS, "VOLUME");
    put_str(
        &mut object,
        tags::FOCUS_METHOD,
        VR::CS,
        metadata
            .focus_method
            .as_deref()
            .unwrap_or(DEFAULT_FOCUS_METHOD),
    );
    put_str(&mut object, tags::EXTENDED_DEPTH_OF_FIELD, VR::CS, "NO");
    put_is(&mut object, tags::SERIES_NUMBER, params.series_number);
    put_is(&mut object, tags::INSTANCE_NUMBER, params.instance_number);
    put_u16(&mut object, tags::REPRESENTATIVE_FRAME_NUMBER, 1);
    put_str(
        &mut object,
        tags::IMAGE_ORIENTATION_SLIDE,
        VR::DS,
        "1\\0\\0\\0\\1\\0",
    );
    object.put(DataElement::new(
        tags::EXTENDED_OFFSET_TABLE,
        VR::OV,
        PrimitiveValue::U64(params.pixel_data_offsets.offsets.into()),
    ));
    object.put(DataElement::new(
        tags::EXTENDED_OFFSET_TABLE_LENGTHS,
        VR::OV,
        PrimitiveValue::U64(params.pixel_data_offsets.lengths.into()),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::OPTICAL_PATH_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![optical_path_item(params.icc_profile)]),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::ACQUISITION_CONTEXT_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(Vec::<InMemDicomObject>::new()),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::ISSUER_OF_THE_CONTAINER_IDENTIFIER_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(Vec::<InMemDicomObject>::new()),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::CONTAINER_TYPE_CODE_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![code_item("433466003", "SCT", "Microscope slide")]),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::SPECIMEN_DESCRIPTION_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![specimen_description_item(metadata.as_metadata())]),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![total_pixel_matrix_origin_item()]),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![shared_functional_groups_item(
            image_type,
            row_spacing_mm,
            column_spacing_mm,
            metadata
                .imaged_volume_depth_mm
                .unwrap_or(DEFAULT_IMAGED_VOLUME_DEPTH_MM),
        )]),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::DIMENSION_ORGANIZATION_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![dimension_organization_item(
            identifiers.dimension_organization_uid,
        )]),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::DIMENSION_INDEX_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(dimension_index_items(
            identifiers.dimension_organization_uid,
        )),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(per_frame_items(
            params.frame_count,
            frame_grid,
            row_spacing_mm,
            column_spacing_mm,
        )?),
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
    pub(crate) fn create(path: PathBuf, frame_count: usize) -> Result<Self, Error> {
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| Error::Io {
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

    pub(crate) fn push_frame(&mut self, codestream: &[u8]) -> Result<(), Error> {
        let raw_len = u64::try_from(codestream.len()).map_err(|_| Error::Unsupported {
            reason: "encoded frame length exceeds u64".into(),
        })?;
        let padded_len_u32 = padded_fragment_len(raw_len)?;
        let padded_len = u64::from(padded_len_u32);
        let spool_offset = self.file.stream_position().map_err(|source| Error::Io {
            path: self.path.clone(),
            source,
        })?;
        self.file
            .write_all(codestream)
            .map_err(|source| Error::Io {
                path: self.path.clone(),
                source,
            })?;
        if raw_len != padded_len {
            self.file.write_all(&[0]).map_err(|source| Error::Io {
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
            .ok_or_else(|| Error::Unsupported {
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

    pub(crate) fn stream_frames_to(
        &mut self,
        writer: &mut StreamingPixelDataFrameWriter<'_>,
    ) -> Result<(), Error> {
        self.file.flush().map_err(|source| Error::Io {
            path: self.path.clone(),
            source,
        })?;
        let mut current_offset =
            self.file
                .seek(SeekFrom::Start(0))
                .map_err(|source| Error::Io {
                    path: self.path.clone(),
                    source,
                })?;
        for (fragment, &raw_len) in self.fragments.iter().zip(&self.lengths) {
            if fragment.spool_offset < current_offset {
                current_offset = self
                    .file
                    .seek(SeekFrom::Start(fragment.spool_offset))
                    .map_err(|source| Error::Io {
                        path: self.path.clone(),
                        source,
                    })?;
            } else if fragment.spool_offset > current_offset {
                let gap = fragment.spool_offset - current_offset;
                let skipped =
                    io::copy(&mut Read::by_ref(&mut self.file).take(gap), &mut io::sink())
                        .map_err(|source| Error::Io {
                            path: self.path.clone(),
                            source,
                        })?;
                if skipped != gap {
                    return Err(Error::DicomWrite {
                        path: self.path.clone(),
                        message: "spooled PixelData gap ended before next frame".into(),
                    });
                }
                current_offset = fragment.spool_offset;
            }
            writer.push_frame_from_reader(raw_len, &mut self.file)?;
            current_offset =
                current_offset
                    .checked_add(raw_len)
                    .ok_or_else(|| Error::Unsupported {
                        reason: "spooled PixelData frame offset overflow".into(),
                    })?;
        }
        Ok(())
    }
}

pub(crate) trait PixelDataSink {
    fn push_frame(&mut self, codestream: &[u8]) -> Result<(), Error>;

    fn push_owned_frame(&mut self, codestream: Vec<u8>) -> Result<(), Error> {
        self.push_frame(&codestream)
    }

    fn lengths(&self) -> Vec<u64>;

    fn stream_frames_to(
        &mut self,
        writer: &mut StreamingPixelDataFrameWriter<'_>,
    ) -> Result<(), Error>;
}

pub(crate) enum BufferedPixelDataSink {
    InMemory(InMemoryPixelDataSink),
    Spool(PixelDataSpool),
}

impl BufferedPixelDataSink {
    pub(crate) fn create(
        spool_path: PathBuf,
        frame_count: usize,
        use_in_memory_buffer: bool,
    ) -> Result<Self, Error> {
        if use_in_memory_buffer {
            Ok(Self::InMemory(InMemoryPixelDataSink::with_capacity(
                frame_count,
            )))
        } else {
            Ok(Self::Spool(PixelDataSpool::create(
                spool_path,
                frame_count,
            )?))
        }
    }
}

impl PixelDataSink for BufferedPixelDataSink {
    fn push_frame(&mut self, codestream: &[u8]) -> Result<(), Error> {
        match self {
            Self::InMemory(buffer) => buffer.push_frame(codestream),
            Self::Spool(spool) => spool.push_frame(codestream),
        }
    }

    fn push_owned_frame(&mut self, codestream: Vec<u8>) -> Result<(), Error> {
        match self {
            Self::InMemory(buffer) => buffer.push_owned_frame(codestream),
            Self::Spool(spool) => spool.push_frame(&codestream),
        }
    }

    fn lengths(&self) -> Vec<u64> {
        match self {
            Self::InMemory(buffer) => buffer.lengths(),
            Self::Spool(spool) => spool.lengths(),
        }
    }

    fn stream_frames_to(
        &mut self,
        writer: &mut StreamingPixelDataFrameWriter<'_>,
    ) -> Result<(), Error> {
        match self {
            Self::InMemory(buffer) => buffer.stream_frames_to(writer),
            Self::Spool(spool) => spool.stream_frames_to(writer),
        }
    }
}

pub(crate) struct InMemoryPixelDataSink {
    frames: Vec<Vec<u8>>,
}

impl InMemoryPixelDataSink {
    fn with_capacity(frame_count: usize) -> Self {
        Self {
            frames: Vec::with_capacity(frame_count),
        }
    }
}

impl PixelDataSink for InMemoryPixelDataSink {
    fn push_frame(&mut self, codestream: &[u8]) -> Result<(), Error> {
        checked_frame_len(codestream.len())?;
        self.frames.push(codestream.to_vec());
        Ok(())
    }

    fn push_owned_frame(&mut self, codestream: Vec<u8>) -> Result<(), Error> {
        checked_frame_len(codestream.len())?;
        self.frames.push(codestream);
        Ok(())
    }

    fn lengths(&self) -> Vec<u64> {
        self.frames
            .iter()
            .map(|frame| {
                u64::try_from(frame.len())
                    .unwrap_or_else(|_| unreachable!("frame length was validated before storage"))
            })
            .collect()
    }

    fn stream_frames_to(
        &mut self,
        writer: &mut StreamingPixelDataFrameWriter<'_>,
    ) -> Result<(), Error> {
        for frame in &self.frames {
            writer.push_frame(frame)?;
        }
        Ok(())
    }
}

impl Drop for PixelDataSpool {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn pixel_data_offsets_from_lengths(lengths: &[u64]) -> Result<Vec<u64>, Error> {
    let mut offsets = Vec::with_capacity(lengths.len());
    let mut next_extended_offset = 0u64;
    for &raw_len in lengths {
        let padded_len = u64::from(padded_fragment_len(raw_len)?);
        offsets.push(next_extended_offset);
        next_extended_offset = next_extended_offset
            .checked_add(8)
            .and_then(|offset| offset.checked_add(padded_len))
            .ok_or_else(|| Error::Unsupported {
                reason: "extended offset table overflow".into(),
            })?;
    }
    Ok(offsets)
}

pub(crate) fn write_dicom_object_with_direct_pixel_data(
    path: &Path,
    object: InMemDicomObject,
    meta: FileMetaTableBuilder,
    overwrite: bool,
    lengths: &[u64],
    write_frame: impl FnMut(usize, &mut dyn Write) -> io::Result<()>,
) -> Result<(), Error> {
    write_dicom_object_with_pixel_data(path, object, meta, overwrite, |file| {
        write_encapsulated_pixel_data_from_frames(file, lengths, write_frame)
    })
}

pub(crate) fn write_dicom_object_with_spooled_pixel_data(
    path: &Path,
    object: InMemDicomObject,
    meta: FileMetaTableBuilder,
    overwrite: bool,
    spool: &mut PixelDataSpool,
) -> Result<(), Error> {
    spool.file.flush().map_err(|source| Error::Io {
        path: spool.path.clone(),
        source,
    })?;
    spool
        .file
        .seek(SeekFrom::Start(0))
        .map_err(|source| Error::Io {
            path: spool.path.clone(),
            source,
        })?;

    write_dicom_object_with_pixel_data(path, object, meta, overwrite, |file| {
        write_encapsulated_pixel_data_from_spool(file, &mut spool.file, &spool.fragments)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamedPixelDataWriteReport {
    pub(crate) offsets: Vec<u64>,
    pub(crate) lengths: Vec<u64>,
    pub(crate) streaming_write_duration: Duration,
    pub(crate) pixel_data_patch_duration: Duration,
}

pub(crate) struct StreamingPixelDataFrameWriter<'a> {
    path: PathBuf,
    output: &'a mut BufWriter<File>,
    frame_count: usize,
    frames_written: usize,
    offsets: Vec<u64>,
    lengths: Vec<u64>,
    next_extended_offset: u64,
    streaming_write_duration: Duration,
}

impl StreamingPixelDataFrameWriter<'_> {
    pub(crate) fn push_frame(&mut self, codestream: &[u8]) -> Result<(), Error> {
        let raw_len = u64::try_from(codestream.len()).map_err(|_| Error::Unsupported {
            reason: "encoded frame length exceeds u64".into(),
        })?;
        self.push_frame_impl(raw_len, |output| output.write_all(codestream))
    }

    pub(crate) fn push_frame_from_reader(
        &mut self,
        raw_len: u64,
        reader: &mut impl Read,
    ) -> Result<(), Error> {
        self.push_frame_impl(raw_len, |output| {
            let copied = io::copy(&mut reader.take(raw_len), output)?;
            if copied != raw_len {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "streamed PixelData frame reader ended before declared length",
                ));
            }
            Ok(())
        })
    }

    fn push_frame_impl(
        &mut self,
        raw_len: u64,
        write_frame: impl FnOnce(&mut BufWriter<File>) -> io::Result<()>,
    ) -> Result<(), Error> {
        if self.frames_written >= self.frame_count {
            return Err(Error::DicomWrite {
                path: self.path.clone(),
                message: format!(
                    "streamed PixelData received more than {} frame(s)",
                    self.frame_count
                ),
            });
        }
        let padded_len_u32 = padded_fragment_len(raw_len)?;
        let padded_len = u64::from(padded_len_u32);
        let started = Instant::now();
        write_item_header(self.output, padded_len_u32).map_err(|source| Error::Io {
            path: self.path.clone(),
            source,
        })?;
        write_frame(self.output).map_err(|source| Error::Io {
            path: self.path.clone(),
            source,
        })?;
        if raw_len != padded_len {
            self.output.write_all(&[0]).map_err(|source| Error::Io {
                path: self.path.clone(),
                source,
            })?;
        }
        self.streaming_write_duration = self
            .streaming_write_duration
            .saturating_add(started.elapsed());
        self.offsets.push(self.next_extended_offset);
        self.lengths.push(raw_len);
        self.next_extended_offset = self
            .next_extended_offset
            .checked_add(8)
            .and_then(|offset| offset.checked_add(padded_len))
            .ok_or_else(|| Error::Unsupported {
                reason: "extended offset table overflow".into(),
            })?;
        self.frames_written += 1;
        Ok(())
    }

    fn finish(self) -> Result<StreamedPixelDataWriteReport, Error> {
        if self.frames_written != self.frame_count {
            return Err(Error::DicomWrite {
                path: self.path,
                message: format!(
                    "streamed PixelData wrote {} frame(s), expected {}",
                    self.frames_written, self.frame_count
                ),
            });
        }
        Ok(StreamedPixelDataWriteReport {
            offsets: self.offsets,
            lengths: self.lengths,
            streaming_write_duration: self.streaming_write_duration,
            pixel_data_patch_duration: Duration::ZERO,
        })
    }
}

pub(crate) fn write_dicom_object_with_streamed_pixel_data(
    path: &Path,
    mut object: InMemDicomObject,
    meta: FileMetaTableBuilder,
    overwrite: bool,
    frame_count: usize,
    write_frames: impl FnOnce(&mut StreamingPixelDataFrameWriter<'_>) -> Result<(), Error>,
) -> Result<StreamedPixelDataWriteReport, Error> {
    let output = PendingDicomOutput::create(path, overwrite)?;
    let file = output.reopen()?;
    let mut file = dicom_file_writer(file);
    object.remove_element(tags::EXTENDED_OFFSET_TABLE);
    object.remove_element(tags::EXTENDED_OFFSET_TABLE_LENGTHS);
    object
        .with_meta(meta)
        .map_err(|err| Error::DicomWrite {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?
        .write_all(&mut file)
        .map_err(|err| Error::DicomWrite {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
    let extended_offset_table_locations =
        write_empty_extended_offset_tables(&mut file, frame_count).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
    write_encapsulated_pixel_data_header(&mut file).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let mut writer = StreamingPixelDataFrameWriter {
        path: path.to_path_buf(),
        output: &mut file,
        frame_count,
        frames_written: 0,
        offsets: Vec::with_capacity(frame_count),
        lengths: Vec::with_capacity(frame_count),
        next_extended_offset: 0,
        streaming_write_duration: Duration::ZERO,
    };
    write_frames(&mut writer)?;
    let mut report = writer.finish()?;
    write_encapsulated_pixel_data_trailer(&mut file).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    flush_and_sync_dicom_writer(&mut file, output.path())?;
    drop(file);

    let patch_started = Instant::now();
    patch_extended_offset_tables(
        output.path(),
        extended_offset_table_locations,
        &report.offsets,
        &report.lengths,
    )?;
    report.pixel_data_patch_duration = patch_started.elapsed();
    output.persist()?;
    Ok(report)
}

fn write_dicom_object_with_pixel_data(
    path: &Path,
    object: InMemDicomObject,
    meta: FileMetaTableBuilder,
    overwrite: bool,
    write_pixel_data: impl FnOnce(&mut BufWriter<File>) -> io::Result<()>,
) -> Result<(), Error> {
    let output = PendingDicomOutput::create(path, overwrite)?;
    let file = output.reopen()?;
    let mut file = dicom_file_writer(file);
    object
        .with_meta(meta)
        .map_err(|err| Error::DicomWrite {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?
        .write_all(&mut file)
        .map_err(|err| Error::DicomWrite {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
    write_pixel_data(&mut file).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    flush_and_sync_dicom_writer(&mut file, output.path())?;
    drop(file);
    output.persist()
}

fn dicom_file_writer(file: File) -> BufWriter<File> {
    BufWriter::with_capacity(DICOM_FILE_WRITE_BUFFER_BYTES, file)
}

pub(crate) fn unique_spool_path(output_path: &Path) -> PathBuf {
    let counter = SPOOL_COUNTER.fetch_add(1, Ordering::Relaxed);
    let extension = output_path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("dcm");
    output_path.with_extension(format!(
        "{extension}.pixeldata.{}.{}.tmp",
        std::process::id(),
        counter
    ))
}

struct PendingDicomOutput {
    final_path: PathBuf,
    temp_file: tempfile::NamedTempFile,
    overwrite: bool,
}

impl PendingDicomOutput {
    fn create(final_path: &Path, overwrite: bool) -> Result<Self, Error> {
        validate_final_output_path(final_path, overwrite)?;
        let parent = output_parent_dir(final_path);
        let temp_file = tempfile::Builder::new()
            .prefix(".wsi-dicom-output-")
            .suffix(".tmp")
            .tempfile_in(parent)
            .map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        Ok(Self {
            final_path: final_path.to_path_buf(),
            temp_file,
            overwrite,
        })
    }

    fn path(&self) -> &Path {
        self.temp_file.path()
    }

    fn reopen(&self) -> Result<File, Error> {
        self.temp_file.reopen().map_err(|source| Error::Io {
            path: self.temp_file.path().to_path_buf(),
            source,
        })
    }

    fn persist(self) -> Result<(), Error> {
        if self.overwrite {
            reject_symlink_output_path(&self.final_path)?;
        }
        let final_path = self.final_path.clone();
        let result = if self.overwrite {
            self.temp_file.persist(&final_path)
        } else {
            self.temp_file.persist_noclobber(&final_path)
        };
        result.map(|_| ()).map_err(|error| Error::Io {
            path: final_path,
            source: error.error,
        })
    }
}

fn output_parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn validate_final_output_path(path: &Path, overwrite: bool) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(_) if !overwrite => Err(Error::Io {
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::AlreadyExists, "output path already exists"),
        }),
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::Io {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "refusing to overwrite symlink output path",
            ),
        }),
        Ok(metadata) if !metadata.file_type().is_file() => Err(Error::Io {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "refusing to overwrite non-file output path",
            ),
        }),
        Ok(_) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn reject_symlink_output_path(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::Io {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "refusing to overwrite symlink output path",
            ),
        }),
        Ok(_) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn flush_and_sync_dicom_writer(file: &mut BufWriter<File>, path: &Path) -> Result<(), Error> {
    file.flush().map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.get_ref().sync_all().map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn write_encapsulated_pixel_data_from_frames(
    output: &mut impl Write,
    lengths: &[u64],
    mut write_frame: impl FnMut(usize, &mut dyn Write) -> io::Result<()>,
) -> io::Result<()> {
    write_encapsulated_pixel_data_header(output)?;
    for (idx, &raw_len) in lengths.iter().enumerate() {
        let padded_len = padded_fragment_len_io(raw_len)?;
        write_item_header(output, padded_len)?;
        {
            let mut limited = LimitedFragmentWriter {
                inner: output,
                remaining: raw_len,
            };
            write_frame(idx, &mut limited)?;
            if limited.remaining != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "direct PixelData frame ended before declared length",
                ));
            }
        }
        if raw_len % 2 != 0 {
            output.write_all(&[0])?;
        }
    }
    write_encapsulated_pixel_data_trailer(output)
}

pub(crate) fn write_encapsulated_pixel_data_from_spool(
    output: &mut impl Write,
    spool: &mut (impl Read + Seek),
    fragments: &[SpooledPixelDataFragment],
) -> std::io::Result<()> {
    write_encapsulated_pixel_data_header(output)?;
    let mut current_offset = 0u64;
    for fragment in fragments {
        if fragment.spool_offset < current_offset {
            spool.seek(SeekFrom::Start(fragment.spool_offset))?;
            current_offset = fragment.spool_offset;
        } else if fragment.spool_offset > current_offset {
            let gap = fragment.spool_offset - current_offset;
            let skipped = std::io::copy(&mut spool.by_ref().take(gap), &mut std::io::sink())?;
            if skipped != gap {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "spooled PixelData gap ended before next fragment",
                ));
            }
            current_offset = fragment.spool_offset;
        }
        write_item_header(output, fragment.padded_len)?;
        let mut limited = spool.by_ref().take(u64::from(fragment.padded_len));
        let copied = std::io::copy(&mut limited, output)?;
        if copied != u64::from(fragment.padded_len) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "spooled PixelData fragment ended before padded length",
            ));
        }
        current_offset = current_offset
            .checked_add(u64::from(fragment.padded_len))
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "spooled PixelData fragment offset overflow",
                )
            })?;
    }
    write_encapsulated_pixel_data_trailer(output)
}

fn write_encapsulated_pixel_data_header(output: &mut impl Write) -> std::io::Result<()> {
    write_tag(output, 0x7FE0, 0x0010)?;
    output.write_all(b"OB")?;
    output.write_all(&[0, 0])?;
    output.write_all(&u32::MAX.to_le_bytes())?;
    write_item_header(output, 0)
}

fn write_encapsulated_pixel_data_trailer(output: &mut impl Write) -> std::io::Result<()> {
    write_tag(output, 0xFFFE, 0xE0DD)?;
    output.write_all(&0u32.to_le_bytes())
}

struct LimitedFragmentWriter<'a, W: Write + ?Sized> {
    inner: &'a mut W,
    remaining: u64,
}

impl<W: Write + ?Sized> Write for LimitedFragmentWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if u64::try_from(buf.len()).unwrap_or(u64::MAX) > self.remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "direct PixelData frame exceeded declared length",
            ));
        }
        let written = self.inner.write(buf)?;
        self.remaining = self
            .remaining
            .checked_sub(u64::try_from(written).unwrap_or(u64::MAX))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "direct PixelData frame length accounting underflowed",
                )
            })?;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn padded_fragment_len(raw_len: u64) -> Result<u32, Error> {
    let padded_len = raw_len
        .checked_add(raw_len % 2)
        .ok_or_else(|| Error::Unsupported {
            reason: "encoded frame padded length overflow".into(),
        })?;
    u32::try_from(padded_len).map_err(|_| Error::Unsupported {
        reason: "encoded frame exceeds DICOM fragment item length limit".into(),
    })
}

fn checked_frame_len(len: usize) -> Result<u64, Error> {
    u64::try_from(len).map_err(|_| Error::Unsupported {
        reason: "encoded frame length exceeds u64".into(),
    })
}

fn padded_fragment_len_io(raw_len: u64) -> io::Result<u32> {
    padded_fragment_len(raw_len).map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))
}

fn write_empty_extended_offset_tables(
    output: &mut BufWriter<File>,
    frame_count: usize,
) -> io::Result<ExtendedOffsetTableLocations> {
    let value_bytes = frame_count
        .checked_mul(std::mem::size_of::<u64>())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "extended offset table byte length overflow",
            )
        })?;
    let value_bytes = u32::try_from(value_bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "extended offset table exceeds DICOM element length limit",
        )
    })?;
    let offset_table_value_offset = write_empty_ov_element(output, 0x7FE0, 0x0001, value_bytes)?;
    let length_table_value_offset = write_empty_ov_element(output, 0x7FE0, 0x0002, value_bytes)?;
    Ok(ExtendedOffsetTableLocations {
        offset_table_value_offset,
        length_table_value_offset,
    })
}

fn write_empty_ov_element(
    output: &mut BufWriter<File>,
    group: u16,
    element: u16,
    value_len: u32,
) -> io::Result<u64> {
    write_tag(output, group, element)?;
    output.write_all(b"OV")?;
    output.write_all(&[0, 0])?;
    output.write_all(&value_len.to_le_bytes())?;
    let value_offset = output.stream_position()?;
    write_zero_bytes(output, u64::from(value_len))?;
    Ok(value_offset)
}

fn write_zero_bytes(output: &mut impl Write, mut count: u64) -> io::Result<()> {
    const ZERO_CHUNK: [u8; 8192] = [0; 8192];
    while count != 0 {
        let len = usize::try_from(count.min(ZERO_CHUNK.len() as u64)).unwrap();
        output.write_all(&ZERO_CHUNK[..len])?;
        count -= len as u64;
    }
    Ok(())
}

fn patch_extended_offset_tables(
    path: &Path,
    locations: ExtendedOffsetTableLocations,
    offsets: &[u64],
    lengths: &[u64],
) -> Result<(), Error> {
    if lengths.len() != offsets.len() {
        return Err(Error::DicomWrite {
            path: path.to_path_buf(),
            message: format!(
                "streamed PixelData has {} offset(s) but {} length(s)",
                offsets.len(),
                lengths.len()
            ),
        });
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
    patch_u64_table(&mut file, locations.offset_table_value_offset, offsets).map_err(|source| {
        Error::Io {
            path: path.to_path_buf(),
            source,
        }
    })?;
    patch_u64_table(&mut file, locations.length_table_value_offset, lengths).map_err(|source| {
        Error::Io {
            path: path.to_path_buf(),
            source,
        }
    })?;
    file.sync_all().map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

struct ExtendedOffsetTableLocations {
    offset_table_value_offset: u64,
    length_table_value_offset: u64,
}

fn patch_u64_table(file: &mut File, value_offset: u64, values: &[u64]) -> io::Result<()> {
    file.seek(SeekFrom::Start(value_offset))?;
    for value in values {
        file.write_all(&value.to_le_bytes())?;
    }
    Ok(())
}

fn write_item_header(output: &mut impl Write, length: u32) -> std::io::Result<()> {
    write_tag(output, 0xFFFE, 0xE000)?;
    output.write_all(&length.to_le_bytes())
}

fn write_tag(output: &mut impl Write, group: u16, element: u16) -> std::io::Result<()> {
    output.write_all(&group.to_le_bytes())?;
    output.write_all(&element.to_le_bytes())
}

fn optical_path_item(icc_profile: Option<&[u8]>) -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    put_str(&mut item, tags::OPTICAL_PATH_IDENTIFIER, VR::SH, "0");
    put_str(
        &mut item,
        tags::OPTICAL_PATH_DESCRIPTION,
        VR::ST,
        "Default optical path",
    );
    item.put(DataElement::<InMemDicomObject>::new(
        tags::ILLUMINATION_TYPE_CODE_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![code_item("111744", "DCM", "Brightfield illumination")]),
    ));
    item.put(DataElement::<InMemDicomObject>::new(
        tags::ILLUMINATION_COLOR_CODE_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![code_item("371251000", "SCT", "White")]),
    ));
    put_fl(&mut item, tags::ILLUMINATION_WAVE_LENGTH, 550.0);
    if let Some(icc_profile) = icc_profile {
        item.put(DataElement::new(
            tags::ICC_PROFILE,
            VR::OB,
            PrimitiveValue::from(icc_profile.to_vec()),
        ));
    }
    item
}

fn specimen_description_item(metadata: &DicomMetadata) -> InMemDicomObject {
    let identifier = metadata
        .specimen_identifier
        .as_deref()
        .unwrap_or(DEFAULT_SPECIMEN_IDENTIFIER);
    let description = metadata
        .specimen_description
        .as_deref()
        .unwrap_or(DEFAULT_SPECIMEN_DESCRIPTION);
    let mut item = InMemDicomObject::new_empty();
    put_str(&mut item, tags::SPECIMEN_IDENTIFIER, VR::LO, identifier);
    put_str(
        &mut item,
        tags::SPECIMEN_UID,
        VR::UI,
        &uid_from_seed(&format!("specimen:{identifier}")),
    );
    put_str(
        &mut item,
        tags::SPECIMEN_SHORT_DESCRIPTION,
        VR::LO,
        description,
    );
    put_str(
        &mut item,
        tags::SPECIMEN_DETAILED_DESCRIPTION,
        VR::UT,
        description,
    );
    put_empty_sequence(&mut item, tags::ISSUER_OF_THE_SPECIMEN_IDENTIFIER_SEQUENCE);
    put_empty_sequence(&mut item, tags::SPECIMEN_PREPARATION_SEQUENCE);
    item
}

pub(crate) fn synthetic_srgb_icc_profile() -> Result<Vec<u8>, Error> {
    moxcms::ColorProfile::new_srgb()
        .encode()
        .map_err(|err| Error::Metadata {
            reason: format!("failed to generate synthetic sRGB ICC profile: {err}"),
        })
}

pub(crate) fn synthetic_display_p3_icc_profile() -> Result<Vec<u8>, Error> {
    moxcms::ColorProfile::new_display_p3()
        .encode()
        .map_err(|err| Error::Metadata {
            reason: format!("failed to generate synthetic Display P3 ICC profile: {err}"),
        })
}

fn code_item(code_value: &str, coding_scheme: &str, code_meaning: &str) -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    put_str(&mut item, tags::CODE_VALUE, VR::SH, code_value);
    put_str(
        &mut item,
        tags::CODING_SCHEME_DESIGNATOR,
        VR::SH,
        coding_scheme,
    );
    put_str(&mut item, tags::CODE_MEANING, VR::LO, code_meaning);
    item
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.and_then(|value| (!value.is_empty()).then_some(value))
}

fn total_pixel_matrix_origin_item() -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    put_ds(&mut item, tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, 0.0);
    put_ds(&mut item, tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, 0.0);
    put_ds(&mut item, tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, 0.0);
    item
}

fn shared_functional_groups_item(
    image_type: &str,
    row_spacing_mm: f64,
    column_spacing_mm: f64,
    slice_thickness_mm: f64,
) -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    item.put(DataElement::<InMemDicomObject>::new(
        tags::PIXEL_MEASURES_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![pixel_measures_item(
            row_spacing_mm,
            column_spacing_mm,
            slice_thickness_mm,
        )]),
    ));
    item.put(DataElement::<InMemDicomObject>::new(
        tags::WHOLE_SLIDE_MICROSCOPY_IMAGE_FRAME_TYPE_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![frame_type_item(image_type)]),
    ));
    item.put(DataElement::<InMemDicomObject>::new(
        tags::OPTICAL_PATH_IDENTIFICATION_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![optical_path_identification_item()]),
    ));
    item
}

fn pixel_measures_item(
    row_spacing_mm: f64,
    column_spacing_mm: f64,
    slice_thickness_mm: f64,
) -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    put_ds_pair(
        &mut item,
        tags::PIXEL_SPACING,
        row_spacing_mm,
        column_spacing_mm,
    );
    put_ds(&mut item, tags::SLICE_THICKNESS, slice_thickness_mm);
    item
}

fn frame_type_item(image_type: &str) -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    put_str(&mut item, tags::FRAME_TYPE, VR::CS, image_type);
    item
}

fn optical_path_identification_item() -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    put_str(&mut item, tags::OPTICAL_PATH_IDENTIFIER, VR::SH, "0");
    item
}

fn dimension_organization_item(dimension_organization_uid: &str) -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    put_str(
        &mut item,
        tags::DIMENSION_ORGANIZATION_UID,
        VR::UI,
        dimension_organization_uid,
    );
    item
}

fn dimension_index_items(dimension_organization_uid: &str) -> Vec<InMemDicomObject> {
    vec![
        dimension_index_item(
            dimension_organization_uid,
            tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
        ),
        dimension_index_item(
            dimension_organization_uid,
            tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
        ),
    ]
}

fn dimension_index_item(
    dimension_organization_uid: &str,
    dimension_index_pointer: Tag,
) -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    put_str(
        &mut item,
        tags::DIMENSION_ORGANIZATION_UID,
        VR::UI,
        dimension_organization_uid,
    );
    put_tag(
        &mut item,
        tags::DIMENSION_INDEX_POINTER,
        dimension_index_pointer,
    );
    put_tag(
        &mut item,
        tags::FUNCTIONAL_GROUP_POINTER,
        tags::PLANE_POSITION_SLIDE_SEQUENCE,
    );
    item
}

fn per_frame_items(
    frame_count: u32,
    frame_grid: FrameGrid,
    row_spacing_mm: f64,
    column_spacing_mm: f64,
) -> Result<Vec<InMemDicomObject>, Error> {
    frame_grid.validate()?;
    let mut items = Vec::with_capacity(frame_count as usize);
    for frame_index in 0..frame_count {
        let location = frame_grid.location_for_frame(frame_index)?;
        let [column_index_value, row_index_value] = location.dimension_index_values()?;
        let (column_position, row_position) = location.slide_matrix_positions(frame_grid)?;
        let (x_offset, y_offset) =
            location.slide_coordinate_offsets(frame_grid, row_spacing_mm, column_spacing_mm);
        let mut position = InMemDicomObject::new_empty();
        position.put(DataElement::new(
            tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
            VR::SL,
            PrimitiveValue::from(column_position),
        ));
        position.put(DataElement::new(
            tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
            VR::SL,
            PrimitiveValue::from(row_position),
        ));
        put_ds(
            &mut position,
            tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
            x_offset,
        );
        put_ds(
            &mut position,
            tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
            y_offset,
        );
        put_ds(
            &mut position,
            tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
            0.0,
        );
        let mut frame_content = InMemDicomObject::new_empty();
        frame_content.put(DataElement::new(
            tags::DIMENSION_INDEX_VALUES,
            VR::UL,
            PrimitiveValue::U32(vec![column_index_value, row_index_value].into()),
        ));
        let mut item = InMemDicomObject::new_empty();
        item.put(DataElement::<InMemDicomObject>::new(
            tags::FRAME_CONTENT_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![frame_content]),
        ));
        item.put(DataElement::<InMemDicomObject>::new(
            tags::PLANE_POSITION_SLIDE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![position]),
        ));
        items.push(item);
    }
    Ok(items)
}

fn checked_slide_matrix_position(
    index: u64,
    frame_extent: u32,
    axis: &'static str,
) -> Result<i32, Error> {
    let position = index
        .checked_mul(u64::from(frame_extent))
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| Error::Unsupported {
            reason: format!("DICOM {axis} position overflow"),
        })?;
    i32::try_from(position).map_err(|_| Error::Unsupported {
        reason: format!("DICOM {axis} position exceeds SL range: {position}"),
    })
}

fn checked_dimension_index_value(index: u64, axis: &'static str) -> Result<u32, Error> {
    let value = index.checked_add(1).ok_or_else(|| Error::Unsupported {
        reason: format!("DICOM {axis} dimension index overflow"),
    })?;
    u32::try_from(value).map_err(|_| Error::Unsupported {
        reason: format!("DICOM {axis} dimension index exceeds UL range: {value}"),
    })
}

fn checked_u16_attribute(value: u32, name: &'static str) -> Result<u16, Error> {
    u16::try_from(value).map_err(|_| Error::Unsupported {
        reason: format!("DICOM {name} exceeds US range: {value}"),
    })
}

fn checked_u32_attribute(value: u64, name: &'static str) -> Result<u32, Error> {
    u32::try_from(value).map_err(|_| Error::Unsupported {
        reason: format!("DICOM {name} exceeds UL range: {value}"),
    })
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

fn put_is(object: &mut InMemDicomObject, tag: Tag, value: u32) {
    object.put(DataElement::new(tag, VR::IS, value.to_string()));
}

fn put_ds(object: &mut InMemDicomObject, tag: Tag, value: f64) {
    object.put(DataElement::new(tag, VR::DS, format_ds(value)));
}

fn put_fl(object: &mut InMemDicomObject, tag: Tag, value: f64) {
    object.put(DataElement::new(
        tag,
        VR::FL,
        PrimitiveValue::from(value as f32),
    ));
}

fn put_ds_pair(object: &mut InMemDicomObject, tag: Tag, first: f64, second: f64) {
    object.put(DataElement::new(
        tag,
        VR::DS,
        format!("{}\\{}", format_ds(first), format_ds(second)),
    ));
}

fn put_tag(object: &mut InMemDicomObject, tag: Tag, value: Tag) {
    object.put(DataElement::new(
        tag,
        VR::AT,
        PrimitiveValue::Tags(vec![value].into()),
    ));
}

fn put_empty_sequence(object: &mut InMemDicomObject, tag: Tag) {
    object.put(DataElement::<InMemDicomObject>::new(
        tag,
        VR::SQ,
        DataSetSequence::from(Vec::<InMemDicomObject>::new()),
    ));
}

fn format_ds(value: f64) -> String {
    for precision in (0..=12).rev() {
        let mut text = format!("{value:.precision$}");
        while text.contains('.') && text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
        if text.len() <= 16 {
            return text;
        }
    }
    format!("{value:.8e}")
}

#[cfg(test)]
mod tests {
    use super::{
        format_ds, synthetic_srgb_icc_profile, write_encapsulated_pixel_data_from_frames,
        write_encapsulated_pixel_data_from_spool, SpooledPixelDataFragment,
    };
    use crate::{tile::PixelProfile, DicomMetadata, Error};
    use dicom_core::Tag;
    use dicom_dictionary_std::{tags, uids};
    use dicom_object::InMemDicomObject;
    use std::io::{Read, Seek, SeekFrom, Write};

    #[test]
    fn dicom_file_writer_uses_large_buffer_for_pixel_data_streams() {
        let tmp = tempfile::tempdir().unwrap();
        let file = std::fs::File::create(tmp.path().join("buffered.dcm")).unwrap();
        let writer = super::dicom_file_writer(file);

        assert!(writer.capacity() >= 1024 * 1024);
    }

    #[test]
    fn overwrite_failure_leaves_existing_output_bytes_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("existing.dcm");
        std::fs::write(&path, b"existing output").unwrap();

        let err = super::write_dicom_object_with_pixel_data(
            &path,
            sample_object_with_offset_tables(vec![0], vec![3]),
            sample_file_meta(),
            true,
            |file| {
                file.write_all(b"partial")?;
                Err(std::io::Error::other("intentional pixel data failure"))
            },
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("intentional pixel data failure"),
            "unexpected error: {err}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"existing output");
    }

    #[cfg(unix)]
    #[test]
    fn overwrite_rejects_symlink_output_path() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target.dcm");
        let link = tmp.path().join("link.dcm");
        std::fs::write(&target, b"target output").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let err = super::write_dicom_object_with_pixel_data(
            &link,
            sample_object_with_offset_tables(vec![0], vec![0]),
            sample_file_meta(),
            true,
            |_| Ok(()),
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("symlink output path"),
            "unexpected error: {err}"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"target output");
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
    }

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
    fn spooled_pixel_data_writer_reads_fragments_sequentially() {
        let mut spool = SeekCountingReader::new(vec![1, 2, 3, 0, 4, 5]);
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

        assert_eq!(spool.seek_count, 0);
        assert!(out.ends_with(&[0xFE, 0xFF, 0xDD, 0xE0, 0, 0, 0, 0]));
    }

    #[test]
    fn direct_pixel_data_writer_matches_spooled_output() {
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
        let mut spooled = Vec::new();
        write_encapsulated_pixel_data_from_spool(&mut spooled, &mut spool, &fragments).unwrap();

        let frames = [vec![1, 2, 3], vec![4, 5]];
        let lengths = frames
            .iter()
            .map(|frame| frame.len() as u64)
            .collect::<Vec<_>>();
        let mut direct = Vec::new();
        write_encapsulated_pixel_data_from_frames(&mut direct, &lengths, |idx, output| {
            output.write_all(&frames[idx])
        })
        .unwrap();

        assert_eq!(direct, spooled);
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

    #[test]
    fn streamed_pixel_data_writer_matches_spooled_output_and_patches_offset_tables() {
        let tmp = tempfile::tempdir().unwrap();
        let frames = [vec![1, 2, 3], vec![4, 5]];
        let lengths = frames
            .iter()
            .map(|frame| frame.len() as u64)
            .collect::<Vec<_>>();
        let offsets = super::pixel_data_offsets_from_lengths(&lengths).unwrap();

        let mut spool =
            super::PixelDataSpool::create(tmp.path().join("frames.bin"), frames.len()).unwrap();
        for frame in &frames {
            spool.push_frame(frame).unwrap();
        }
        let spooled_path = tmp.path().join("spooled.dcm");
        super::write_dicom_object_with_spooled_pixel_data(
            &spooled_path,
            sample_object_with_offset_tables(offsets.clone(), lengths.clone()),
            sample_file_meta(),
            false,
            &mut spool,
        )
        .unwrap();

        let streamed_path = tmp.path().join("streamed.dcm");
        let report = super::write_dicom_object_with_streamed_pixel_data(
            &streamed_path,
            sample_object_with_offset_tables(vec![0; frames.len()], vec![0; frames.len()]),
            sample_file_meta(),
            false,
            frames.len(),
            |writer| {
                for frame in &frames {
                    writer.push_frame(frame)?;
                }
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(report.offsets, offsets);
        assert_eq!(report.lengths, lengths);
        assert_eq!(
            std::fs::read(streamed_path).unwrap(),
            std::fs::read(spooled_path).unwrap()
        );
    }

    #[test]
    fn streamed_pixel_data_writer_copies_reader_frame_in_chunks() {
        let tmp = tempfile::tempdir().unwrap();
        let frame_len = super::DICOM_FILE_WRITE_BUFFER_BYTES + 17;
        let frame = (0..frame_len)
            .map(|value| (value % 251) as u8)
            .collect::<Vec<_>>();
        let max_read_len = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let mut reader = MaxReadLenReader {
            bytes: frame.clone(),
            position: 0,
            max_read_len: max_read_len.clone(),
        };

        let streamed_path = tmp.path().join("streamed-reader.dcm");
        let report = super::write_dicom_object_with_streamed_pixel_data(
            &streamed_path,
            sample_object_with_offset_tables(vec![0], vec![0]),
            sample_file_meta(),
            false,
            1,
            |writer| writer.push_frame_from_reader(frame.len() as u64, &mut reader),
        )
        .unwrap();

        assert_eq!(report.offsets, vec![0]);
        assert_eq!(report.lengths, vec![frame.len() as u64]);
        assert_eq!(reader.position, frame.len());
        assert!(max_read_len.get() <= super::DICOM_FILE_WRITE_BUFFER_BYTES);
        assert!(max_read_len.get() < frame.len());
    }

    #[test]
    fn streamed_pixel_data_writer_rejects_wrong_frame_count() {
        let tmp = tempfile::tempdir().unwrap();
        let err = super::write_dicom_object_with_streamed_pixel_data(
            &tmp.path().join("streamed.dcm"),
            sample_object_with_offset_tables(vec![0; 2], vec![0; 2]),
            sample_file_meta(),
            false,
            2,
            |writer| writer.push_frame(&[1, 2, 3]),
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("expected 2"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn decimal_string_formatting_stays_within_dicom_limit() {
        assert_eq!(format_ds(0.0002528), "0.0002528");
        assert!(format_ds(123_456.789_123_456).len() <= 16);
    }

    #[test]
    fn pyramid_resampled_level_metadata_is_grouped_and_labeled() {
        let object = sample_object(1);

        assert_eq!(
            object
                .element(tags::IMAGE_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "DERIVED\\PRIMARY\\VOLUME\\RESAMPLED"
        );
        assert_eq!(
            object
                .element(tags::PYRAMID_UID)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "1.2.826.0.1.3680043.10.999.5"
        );
        assert_eq!(
            object
                .element(tags::PYRAMID_LABEL)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "WSI pyramid s0 ser0 z0 c0 t0"
        );
        assert_eq!(
            object
                .element(tags::SERIES_NUMBER)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            7
        );
        assert_eq!(
            object
                .element(tags::INSTANCE_NUMBER)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            42
        );
        assert_eq!(
            object
                .element(tags::ACQUISITION_DATE)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "19700101"
        );
        assert_eq!(
            object
                .element(tags::ACQUISITION_TIME)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "000000"
        );
        assert!(object.element(tags::PIXEL_SPACING).is_err());
    }

    #[test]
    fn vl_wsi_multiframe_metadata_contains_required_shared_and_dimension_sequences() {
        let object = sample_object(0);
        let image_type = object
            .element(tags::IMAGE_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .into_owned();

        let shared = sequence_items(&object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE);
        assert_eq!(shared.len(), 1);
        let pixel_measures = sequence_items(&shared[0], tags::PIXEL_MEASURES_SEQUENCE);
        assert_eq!(pixel_measures.len(), 1);
        assert_eq!(
            pixel_measures[0]
                .element(tags::PIXEL_SPACING)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "0.0005\\0.0005"
        );
        assert_eq!(
            pixel_measures[0]
                .element(tags::SLICE_THICKNESS)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "0.001"
        );
        let frame_type = sequence_items(
            &shared[0],
            tags::WHOLE_SLIDE_MICROSCOPY_IMAGE_FRAME_TYPE_SEQUENCE,
        );
        assert_eq!(frame_type.len(), 1);
        assert_eq!(
            frame_type[0]
                .element(tags::FRAME_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            image_type.as_str()
        );

        let dimension_organization = sequence_items(&object, tags::DIMENSION_ORGANIZATION_SEQUENCE);
        assert_eq!(dimension_organization.len(), 1);
        let dimension_uid = dimension_organization[0]
            .element(tags::DIMENSION_ORGANIZATION_UID)
            .unwrap()
            .to_str()
            .unwrap();

        let dimension_index = sequence_items(&object, tags::DIMENSION_INDEX_SEQUENCE);
        assert_eq!(dimension_index.len(), 2);
        assert_dimension_index_item(
            &dimension_index[0],
            tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
            dimension_uid.as_ref(),
        );
        assert_dimension_index_item(
            &dimension_index[1],
            tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
            dimension_uid.as_ref(),
        );

        let per_frame = sequence_items(&object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE);
        assert_eq!(per_frame.len(), 6);
        let frame_content = sequence_items(&per_frame[0], tags::FRAME_CONTENT_SEQUENCE);
        assert_eq!(frame_content.len(), 1);
        assert_eq!(
            frame_content[0]
                .element(tags::DIMENSION_INDEX_VALUES)
                .unwrap()
                .to_multi_int::<u32>()
                .unwrap(),
            vec![1, 1]
        );
        let frame_position = sequence_items(&per_frame[5], tags::PLANE_POSITION_SLIDE_SEQUENCE);
        assert_eq!(frame_position.len(), 1);
        assert_eq!(
            frame_position[0]
                .element(tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX)
                .unwrap()
                .to_int::<i32>()
                .unwrap(),
            513
        );
        assert_eq!(
            frame_position[0]
                .element(tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX)
                .unwrap()
                .to_int::<i32>()
                .unwrap(),
            1025
        );
        assert_eq!(
            frame_position[0]
                .element(tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "0.256"
        );
        assert_eq!(
            frame_position[0]
                .element(tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "0.512"
        );
        assert_eq!(
            frame_position[0]
                .element(tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "0"
        );
    }

    #[test]
    fn vl_wsi_research_placeholder_contains_required_conformance_metadata() {
        let object = sample_object_with_metadata(DicomMetadata::research_placeholder());

        assert_eq!(tag_str(&object, tags::PATIENT_BIRTH_DATE), "");
        assert_eq!(tag_str(&object, tags::PATIENT_SEX), "");
        assert_eq!(tag_str(&object, tags::STUDY_DATE), "19700101");
        assert_eq!(tag_str(&object, tags::STUDY_TIME), "000000");
        assert_eq!(tag_str(&object, tags::REFERRING_PHYSICIAN_NAME), "");
        assert!(object.element(tags::LATERALITY).is_err());
        assert_eq!(
            tag_str(&object, tags::POSITION_REFERENCE_INDICATOR),
            "SLIDE_CORNER"
        );
        assert_eq!(tag_str(&object, tags::MANUFACTURER), "wsi-dicom");
        assert_eq!(tag_str(&object, tags::MANUFACTURER_MODEL_NAME), "wsi-dicom");
        assert_eq!(tag_str(&object, tags::DEVICE_SERIAL_NUMBER), "RESEARCH");
        assert_eq!(
            tag_str(&object, tags::SOFTWARE_VERSIONS),
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(tag_str(&object, tags::CONTENT_DATE), "19700101");
        assert_eq!(tag_str(&object, tags::CONTENT_TIME), "000000");
        assert_eq!(
            tag_str(&object, tags::ACQUISITION_DATE_TIME),
            "19700101000000"
        );
        assert_eq!(
            tag_str(&object, tags::CONTAINER_IDENTIFIER),
            "RESEARCH-CONTAINER"
        );
        assert_eq!(tag_str(&object, tags::VOLUMETRIC_PROPERTIES), "VOLUME");
        assert_eq!(tag_str(&object, tags::BURNED_IN_ANNOTATION), "NO");
        assert_eq!(tag_str(&object, tags::FOCUS_METHOD), "AUTO");
        assert_eq!(tag_str(&object, tags::EXTENDED_DEPTH_OF_FIELD), "NO");
        assert_eq!(tag_str(&object, tags::IMAGED_VOLUME_WIDTH), "0.512");
        assert_eq!(tag_str(&object, tags::IMAGED_VOLUME_HEIGHT), "0.768");
        assert_eq!(tag_str(&object, tags::IMAGED_VOLUME_DEPTH), "0.001");

        assert_eq!(
            sequence_items(&object, tags::ACQUISITION_CONTEXT_SEQUENCE).len(),
            0
        );
        assert_eq!(
            sequence_items(&object, tags::ISSUER_OF_THE_CONTAINER_IDENTIFIER_SEQUENCE).len(),
            0
        );
        let container_type = sequence_items(&object, tags::CONTAINER_TYPE_CODE_SEQUENCE);
        assert_eq!(container_type.len(), 1);
        assert_code_item(&container_type[0], "433466003", "SCT", "Microscope slide");

        let specimen = sequence_items(&object, tags::SPECIMEN_DESCRIPTION_SEQUENCE);
        assert_eq!(specimen.len(), 1);
        assert_eq!(
            tag_str(&specimen[0], tags::SPECIMEN_IDENTIFIER),
            "RESEARCH-SPECIMEN"
        );
        assert!(!tag_str(&specimen[0], tags::SPECIMEN_UID).is_empty());
        assert_eq!(
            tag_str(&specimen[0], tags::SPECIMEN_SHORT_DESCRIPTION),
            "Research placeholder specimen"
        );
        assert_eq!(
            tag_str(&specimen[0], tags::SPECIMEN_DETAILED_DESCRIPTION),
            "Research placeholder specimen"
        );

        let optical_path = sequence_items(&object, tags::OPTICAL_PATH_SEQUENCE);
        assert_eq!(optical_path.len(), 1);
        let illumination_type =
            sequence_items(&optical_path[0], tags::ILLUMINATION_TYPE_CODE_SEQUENCE);
        assert_eq!(illumination_type.len(), 1);
        assert_code_item(
            &illumination_type[0],
            "111744",
            "DCM",
            "Brightfield illumination",
        );
        let illumination_color =
            sequence_items(&optical_path[0], tags::ILLUMINATION_COLOR_CODE_SEQUENCE);
        assert_eq!(illumination_color.len(), 1);
        assert_code_item(&illumination_color[0], "371251000", "SCT", "White");
        assert!(optical_path[0].element(tags::ICC_PROFILE).is_ok());
    }

    #[test]
    fn vl_wsi_strict_metadata_overrides_conformance_defaults() {
        let metadata: DicomMetadata = serde_json::from_value(serde_json::json!({
            "patient_name": "REAL^PATIENT",
            "patient_id": "P-123",
            "patient_birth_date": "19650504",
            "patient_sex": "F",
            "study_date": "20260504",
            "study_time": "142233",
            "referring_physician_name": "REFERRING^DOC",
            "laterality": "L",
            "manufacturer": "ScannerCo",
            "manufacturer_model_name": "Model X",
            "device_serial_number": "SN123",
            "software_versions": "9.8.7",
            "content_date": "20260504",
            "content_time": "142300",
            "acquisition_date_time": "20260504142233",
            "container_identifier": "SLIDE-123",
            "specimen_identifier": "SPEC-123",
            "specimen_description": "H&E section",
            "imaged_volume_depth_mm": 0.004,
            "focus_method": "MANUAL"
        }))
        .unwrap();
        let object = sample_object_with_metadata(metadata);

        assert_eq!(tag_str(&object, tags::PATIENT_BIRTH_DATE), "19650504");
        assert_eq!(tag_str(&object, tags::PATIENT_SEX), "F");
        assert_eq!(tag_str(&object, tags::STUDY_DATE), "20260504");
        assert_eq!(tag_str(&object, tags::STUDY_TIME), "142233");
        assert_eq!(
            tag_str(&object, tags::REFERRING_PHYSICIAN_NAME),
            "REFERRING^DOC"
        );
        assert_eq!(tag_str(&object, tags::LATERALITY), "L");
        assert_eq!(tag_str(&object, tags::MANUFACTURER), "ScannerCo");
        assert_eq!(tag_str(&object, tags::MANUFACTURER_MODEL_NAME), "Model X");
        assert_eq!(tag_str(&object, tags::DEVICE_SERIAL_NUMBER), "SN123");
        assert_eq!(tag_str(&object, tags::SOFTWARE_VERSIONS), "9.8.7");
        assert_eq!(tag_str(&object, tags::CONTENT_DATE), "20260504");
        assert_eq!(tag_str(&object, tags::CONTENT_TIME), "142300");
        assert_eq!(
            tag_str(&object, tags::ACQUISITION_DATE_TIME),
            "20260504142233"
        );
        assert_eq!(tag_str(&object, tags::CONTAINER_IDENTIFIER), "SLIDE-123");
        assert_eq!(tag_str(&object, tags::IMAGED_VOLUME_DEPTH), "0.004");
        assert_eq!(tag_str(&object, tags::FOCUS_METHOD), "MANUAL");

        let specimen = sequence_items(&object, tags::SPECIMEN_DESCRIPTION_SEQUENCE);
        assert_eq!(tag_str(&specimen[0], tags::SPECIMEN_IDENTIFIER), "SPEC-123");
        assert_eq!(
            tag_str(&specimen[0], tags::SPECIMEN_SHORT_DESCRIPTION),
            "H&E section"
        );
    }

    #[test]
    fn vl_wsi_object_contains_tiled_full_origin_orientation_and_representative_frame() {
        let object = sample_object(0);

        let origin = sequence_items(&object, tags::TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE);
        assert_eq!(origin.len(), 1);
        assert_eq!(
            origin[0]
                .element(tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "0"
        );
        assert_eq!(
            origin[0]
                .element(tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "0"
        );
        assert_eq!(
            object
                .element(tags::IMAGE_ORIENTATION_SLIDE)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "1\\0\\0\\0\\1\\0"
        );
        assert_eq!(
            object
                .element(tags::REPRESENTATIVE_FRAME_NUMBER)
                .unwrap()
                .to_int::<u16>()
                .unwrap(),
            1
        );
        assert_eq!(
            object
                .element(tags::LOSSY_IMAGE_COMPRESSION)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "00"
        );
    }

    #[test]
    fn vl_wsi_volume_requires_pixel_spacing_for_pixel_measures() {
        let metadata = DicomMetadata::research_placeholder();
        let mut params = sample_dicom_object_params(&metadata, None);
        params.pixel_spacing_mm = None;
        let err = super::build_dicom_object(params).unwrap_err();

        assert!(
            err.to_string().contains("pixel spacing"),
            "unexpected error: {err}"
        );
    }

    fn sample_object(level_idx: u32) -> InMemDicomObject {
        sample_object_with_metadata_level(DicomMetadata::research_placeholder(), level_idx)
    }

    fn sample_object_with_metadata(metadata: DicomMetadata) -> InMemDicomObject {
        sample_object_with_metadata_level(metadata, 0)
    }

    fn sample_object_with_metadata_level(
        metadata: DicomMetadata,
        level_idx: u32,
    ) -> InMemDicomObject {
        sample_object_with_metadata_level_and_offset_tables(
            metadata,
            level_idx,
            vec![0; 6],
            vec![128; 6],
        )
    }

    fn sample_object_with_offset_tables(offsets: Vec<u64>, lengths: Vec<u64>) -> InMemDicomObject {
        sample_object_with_metadata_level_and_offset_tables(
            DicomMetadata::research_placeholder(),
            0,
            offsets,
            lengths,
        )
    }

    fn sample_object_with_metadata_level_and_offset_tables(
        metadata: DicomMetadata,
        level_idx: u32,
        offsets: Vec<u64>,
        lengths: Vec<u64>,
    ) -> InMemDicomObject {
        let frame_count = u32::try_from(lengths.len()).unwrap();
        let icc_profile = synthetic_srgb_icc_profile().unwrap();
        let mut params = sample_dicom_object_params(&metadata, Some(&icc_profile));
        params.level_idx = level_idx;
        params.frame_count = frame_count;
        params.pixel_data_offsets = super::PixelDataOffsetTables { offsets, lengths };
        super::build_dicom_object(params).unwrap()
    }

    fn sample_file_meta() -> dicom_object::FileMetaTableBuilder {
        dicom_object::FileMetaTableBuilder::new()
            .media_storage_sop_class_uid(uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE)
            .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.999.3")
            .transfer_syntax("1.2.840.10008.1.2.4.202")
    }

    #[test]
    fn vl_wsi_rejects_dimensions_that_exceed_dicom_attribute_ranges() {
        let rows_err =
            sample_object_with_dimensions(512, u32::from(u16::MAX) + 1, 512, 512).unwrap_err();
        assert!(
            rows_err.to_string().contains("Rows exceeds US range"),
            "unexpected error: {rows_err}"
        );

        let columns_err =
            sample_object_with_dimensions(u32::from(u16::MAX) + 1, 512, 512, 512).unwrap_err();
        assert!(
            columns_err.to_string().contains("Columns exceeds US range"),
            "unexpected error: {columns_err}"
        );

        let matrix_columns_err =
            sample_object_with_dimensions(512, 512, u64::from(u32::MAX) + 1, 512).unwrap_err();
        assert!(
            matrix_columns_err
                .to_string()
                .contains("Total Pixel Matrix Columns exceeds UL range"),
            "unexpected error: {matrix_columns_err}"
        );

        let matrix_rows_err =
            sample_object_with_dimensions(512, 512, 512, u64::from(u32::MAX) + 1).unwrap_err();
        assert!(
            matrix_rows_err
                .to_string()
                .contains("Total Pixel Matrix Rows exceeds UL range"),
            "unexpected error: {matrix_rows_err}"
        );
    }

    fn sample_object_with_dimensions(
        frame_columns: u32,
        frame_rows: u32,
        matrix_columns: u64,
        matrix_rows: u64,
    ) -> Result<InMemDicomObject, Error> {
        let icc_profile = synthetic_srgb_icc_profile().unwrap();
        let metadata = DicomMetadata::research_placeholder();
        let mut params = sample_dicom_object_params(&metadata, Some(&icc_profile));
        params.frame_grid = super::FrameGrid {
            frame_columns,
            frame_rows,
            matrix_columns,
            matrix_rows,
        };
        params.frame_count = 1;
        params.pixel_data_offsets = super::PixelDataOffsetTables {
            offsets: vec![0],
            lengths: vec![128],
        };
        super::build_dicom_object(params)
    }

    #[test]
    fn vl_wsi_rectangular_frames_write_rows_columns_and_positions() {
        let icc_profile = synthetic_srgb_icc_profile().unwrap();
        let metadata = DicomMetadata::research_placeholder();
        let mut params = sample_dicom_object_params(&metadata, Some(&icc_profile));
        params.frame_grid = super::FrameGrid {
            frame_columns: 64,
            frame_rows: 8,
            matrix_columns: 130,
            matrix_rows: 31,
        };
        params.frame_count = 12;
        params.profile = PixelProfile {
            components: 3,
            bits_allocated: 8,
            photometric_interpretation: "YBR_FULL_422",
        };
        params.pixel_spacing_mm = Some((0.0005, 0.00025));
        params.pixel_data_offsets = super::PixelDataOffsetTables {
            offsets: vec![0; 12],
            lengths: vec![128; 12],
        };
        let object = super::build_dicom_object(params).unwrap();

        assert_eq!(
            object.element(tags::ROWS).unwrap().to_int::<u16>().unwrap(),
            8
        );
        assert_eq!(
            object
                .element(tags::COLUMNS)
                .unwrap()
                .to_int::<u16>()
                .unwrap(),
            64
        );
        let per_frame = sequence_items(&object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE);
        assert_eq!(per_frame.len(), 12);
        let frame_4_position = sequence_items(&per_frame[4], tags::PLANE_POSITION_SLIDE_SEQUENCE);
        assert_eq!(
            frame_4_position[0]
                .element(tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX)
                .unwrap()
                .to_int::<i32>()
                .unwrap(),
            65
        );
        assert_eq!(
            frame_4_position[0]
                .element(tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX)
                .unwrap()
                .to_int::<i32>()
                .unwrap(),
            9
        );
    }

    #[test]
    fn vl_wsi_rejects_per_frame_positions_outside_sl_range() {
        let metadata = DicomMetadata::research_placeholder();
        let mut params = sample_dicom_object_params(&metadata, None);
        params.frame_grid = super::FrameGrid {
            frame_columns: u16::MAX as u32,
            frame_rows: 1,
            matrix_columns: 2_147_516_416,
            matrix_rows: 1,
        };
        params.frame_count = 32_770;
        params.profile = PixelProfile {
            components: 3,
            bits_allocated: 8,
            photometric_interpretation: "YBR_FULL_422",
        };
        params.pixel_spacing_mm = Some((0.0005, 0.00025));
        params.pixel_data_offsets = super::PixelDataOffsetTables {
            offsets: vec![0; 32_770],
            lengths: vec![128; 32_770],
        };
        let err = super::build_dicom_object(params).unwrap_err();

        assert!(
            err.to_string().contains("position exceeds SL range"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn per_frame_items_reject_invalid_frame_grid_values() {
        let err = super::per_frame_items(
            1,
            super::FrameGrid {
                frame_columns: 0,
                frame_rows: 512,
                matrix_columns: 512,
                matrix_rows: 512,
            },
            0.00025,
            0.00025,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("non-zero frame dimensions"),
            "unexpected error: {err}"
        );

        let err = super::checked_dimension_index_value(u64::from(u32::MAX), "column")
            .expect_err("one-based dimension index should exceed UL range");
        assert!(
            err.to_string()
                .contains("column dimension index exceeds UL range"),
            "unexpected error: {err}"
        );
    }

    fn sample_dicom_object_params<'a>(
        metadata: &'a DicomMetadata,
        icc_profile: Option<&'a [u8]>,
    ) -> super::DicomObjectParams<'a> {
        super::DicomObjectParams {
            metadata,
            identifiers: super::DicomObjectIdentifiers {
                study_uid: "1.2.826.0.1.3680043.10.999.1",
                series_uid: "1.2.826.0.1.3680043.10.999.2",
                sop_instance_uid: "1.2.826.0.1.3680043.10.999.3",
                frame_of_reference_uid: "1.2.826.0.1.3680043.10.999.4",
                pyramid_uid: "1.2.826.0.1.3680043.10.999.5",
                dimension_organization_uid: "1.2.826.0.1.3680043.10.999.6",
                pyramid_label: "WSI pyramid s0 ser0 z0 c0 t0",
            },
            series_number: 7,
            instance_number: 42,
            level_idx: 0,
            frame_grid: super::FrameGrid {
                frame_columns: 512,
                frame_rows: 512,
                matrix_columns: 1024,
                matrix_rows: 1536,
            },
            frame_count: 6,
            profile: PixelProfile {
                components: 3,
                bits_allocated: 8,
                photometric_interpretation: "RGB",
            },
            pixel_spacing_mm: Some((0.0005, 0.0005)),
            pixel_data_offsets: super::PixelDataOffsetTables {
                offsets: vec![0; 6],
                lengths: vec![128; 6],
            },
            icc_profile,
            lossy_compression: None,
        }
    }

    fn sequence_items(object: &InMemDicomObject, tag: Tag) -> &[InMemDicomObject] {
        object
            .element(tag)
            .unwrap_or_else(|err| panic!("missing sequence {tag:?}: {err}"))
            .items()
            .unwrap_or_else(|| panic!("element {tag:?} is not a sequence"))
    }

    fn tag_str(object: &InMemDicomObject, tag: Tag) -> String {
        object
            .element(tag)
            .unwrap_or_else(|err| panic!("missing element {tag:?}: {err}"))
            .to_str()
            .unwrap_or_else(|err| panic!("element {tag:?} is not a string: {err}"))
            .into_owned()
    }

    fn assert_code_item(
        item: &InMemDicomObject,
        code_value: &str,
        coding_scheme: &str,
        code_meaning: &str,
    ) {
        assert_eq!(tag_str(item, tags::CODE_VALUE), code_value);
        assert_eq!(tag_str(item, tags::CODING_SCHEME_DESIGNATOR), coding_scheme);
        assert_eq!(tag_str(item, tags::CODE_MEANING), code_meaning);
    }

    fn assert_dimension_index_item(item: &InMemDicomObject, indexed_tag: Tag, dimension_uid: &str) {
        assert_eq!(
            item.element(tags::DIMENSION_INDEX_POINTER)
                .unwrap()
                .value()
                .to_tag()
                .unwrap(),
            indexed_tag
        );
        assert_eq!(
            item.element(tags::FUNCTIONAL_GROUP_POINTER)
                .unwrap()
                .value()
                .to_tag()
                .unwrap(),
            tags::PLANE_POSITION_SLIDE_SEQUENCE
        );
        assert_eq!(
            item.element(tags::DIMENSION_ORGANIZATION_UID)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            dimension_uid
        );
    }

    struct SeekCountingReader {
        inner: std::io::Cursor<Vec<u8>>,
        seek_count: usize,
    }

    struct MaxReadLenReader {
        bytes: Vec<u8>,
        position: usize,
        max_read_len: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl Read for MaxReadLenReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.max_read_len
                .set(self.max_read_len.get().max(buf.len()));
            if self.position >= self.bytes.len() {
                return Ok(0);
            }
            let len = buf.len().min(self.bytes.len() - self.position);
            buf[..len].copy_from_slice(&self.bytes[self.position..self.position + len]);
            self.position += len;
            Ok(len)
        }
    }

    impl SeekCountingReader {
        fn new(data: Vec<u8>) -> Self {
            Self {
                inner: std::io::Cursor::new(data),
                seek_count: 0,
            }
        }
    }

    impl Read for SeekCountingReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.inner.read(buf)
        }
    }

    impl Seek for SeekCountingReader {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.seek_count += 1;
            self.inner.seek(pos)
        }
    }
}
