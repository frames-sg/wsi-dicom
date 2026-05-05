use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use dicom_core::value::DataSetSequence;
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::tags;
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};

use crate::tile::PixelProfile;
use crate::uid::uid_from_seed;
use crate::{DicomMetadata, WsiDicomError, VL_WSI_SOP_CLASS_UID};

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

pub(crate) struct LossyCompressionMetadata {
    pub(crate) method: &'static str,
    pub(crate) ratio: Option<f64>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_dicom_object(
    metadata: &DicomMetadata,
    study_uid: &str,
    series_uid: &str,
    sop_instance_uid: &str,
    frame_of_reference_uid: &str,
    pyramid_uid: &str,
    dimension_organization_uid: &str,
    pyramid_label: &str,
    series_number: u32,
    instance_number: u32,
    level_idx: u32,
    frame_columns: u32,
    frame_rows: u32,
    matrix_columns: u64,
    matrix_rows: u64,
    frame_count: u32,
    profile: PixelProfile,
    pixel_spacing_mm: Option<(f64, f64)>,
    offsets: Vec<u64>,
    lengths: Vec<u64>,
    lossy_compression: Option<LossyCompressionMetadata>,
) -> Result<InMemDicomObject, WsiDicomError> {
    let mut object = InMemDicomObject::new_empty();
    let (row_spacing_mm, column_spacing_mm) =
        pixel_spacing_mm.ok_or_else(|| WsiDicomError::Metadata {
            reason: "VL WSI VOLUME export requires pixel spacing metadata".into(),
        })?;
    let image_type = if level_idx == 0 {
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
        sop_instance_uid,
    );
    put_str(&mut object, tags::STUDY_INSTANCE_UID, VR::UI, study_uid);
    put_str(&mut object, tags::SERIES_INSTANCE_UID, VR::UI, series_uid);
    put_str(
        &mut object,
        tags::FRAME_OF_REFERENCE_UID,
        VR::UI,
        frame_of_reference_uid,
    );
    put_str(&mut object, tags::PYRAMID_UID, VR::UI, pyramid_uid);
    put_str(&mut object, tags::PYRAMID_LABEL, VR::LO, pyramid_label);
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
    put_u16(&mut object, tags::ROWS, frame_rows as u16);
    put_u16(&mut object, tags::COLUMNS, frame_columns as u16);
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
    put_fl(
        &mut object,
        tags::IMAGED_VOLUME_WIDTH,
        matrix_columns as f64 * column_spacing_mm,
    );
    put_fl(
        &mut object,
        tags::IMAGED_VOLUME_HEIGHT,
        matrix_rows as f64 * row_spacing_mm,
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
    if let Some(lossy) = lossy_compression {
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
    put_is(&mut object, tags::SERIES_NUMBER, series_number);
    put_is(&mut object, tags::INSTANCE_NUMBER, instance_number);
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
        DataSetSequence::from(vec![optical_path_item()?]),
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
        DataSetSequence::from(vec![specimen_description_item(metadata)]),
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
            dimension_organization_uid,
        )]),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::DIMENSION_INDEX_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(dimension_index_items(dimension_organization_uid)),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(per_frame_items(
            frame_count,
            frame_columns,
            frame_rows,
            matrix_columns,
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

fn optical_path_item() -> Result<InMemDicomObject, WsiDicomError> {
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
    item.put(DataElement::new(
        tags::ICC_PROFILE,
        VR::OB,
        PrimitiveValue::from(default_srgb_icc_profile()?),
    ));
    Ok(item)
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

fn default_srgb_icc_profile() -> Result<Vec<u8>, WsiDicomError> {
    moxcms::ColorProfile::new_srgb()
        .encode()
        .map_err(|err| WsiDicomError::Metadata {
            reason: format!("failed to generate default sRGB ICC profile: {err}"),
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
    frame_columns: u32,
    frame_rows: u32,
    matrix_columns: u64,
    row_spacing_mm: f64,
    column_spacing_mm: f64,
) -> Result<Vec<InMemDicomObject>, WsiDicomError> {
    let tiles_across = matrix_columns.div_ceil(u64::from(frame_columns));
    let mut items = Vec::with_capacity(frame_count as usize);
    for frame in 0..frame_count {
        let row = u64::from(frame) / tiles_across;
        let col = u64::from(frame) % tiles_across;
        let column_position = checked_slide_matrix_position(col, frame_columns, "column")?;
        let row_position = checked_slide_matrix_position(row, frame_rows, "row")?;
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
            col as f64 * f64::from(frame_columns) * column_spacing_mm,
        );
        put_ds(
            &mut position,
            tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
            row as f64 * f64::from(frame_rows) * row_spacing_mm,
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
            PrimitiveValue::U32(vec![col as u32 + 1, row as u32 + 1].into()),
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
) -> Result<i32, WsiDicomError> {
    let position = index
        .checked_mul(u64::from(frame_extent))
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: format!("DICOM {axis} position overflow"),
        })?;
    i32::try_from(position).map_err(|_| WsiDicomError::Unsupported {
        reason: format!("DICOM {axis} position exceeds SL range: {position}"),
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
    use super::{format_ds, write_encapsulated_pixel_data_from_spool, SpooledPixelDataFragment};
    use crate::{tile::PixelProfile, DicomMetadata};
    use dicom_core::Tag;
    use dicom_dictionary_std::tags;
    use dicom_object::InMemDicomObject;
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
        let err = super::build_dicom_object(
            &DicomMetadata::default(),
            "1.2.826.0.1.3680043.10.999.1",
            "1.2.826.0.1.3680043.10.999.2",
            "1.2.826.0.1.3680043.10.999.3",
            "1.2.826.0.1.3680043.10.999.4",
            "1.2.826.0.1.3680043.10.999.5",
            "1.2.826.0.1.3680043.10.999.6",
            "WSI pyramid s0 ser0 z0 c0 t0",
            7,
            42,
            0,
            512,
            512,
            1024,
            1536,
            6,
            PixelProfile {
                components: 3,
                bits_allocated: 8,
                photometric_interpretation: "RGB",
            },
            None,
            vec![0; 6],
            vec![128; 6],
            None,
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("pixel spacing"),
            "unexpected error: {err}"
        );
    }

    fn sample_object(level_idx: u32) -> InMemDicomObject {
        sample_object_with_metadata_level(DicomMetadata::default(), level_idx)
    }

    fn sample_object_with_metadata(metadata: DicomMetadata) -> InMemDicomObject {
        sample_object_with_metadata_level(metadata, 0)
    }

    fn sample_object_with_metadata_level(
        metadata: DicomMetadata,
        level_idx: u32,
    ) -> InMemDicomObject {
        super::build_dicom_object(
            &metadata,
            "1.2.826.0.1.3680043.10.999.1",
            "1.2.826.0.1.3680043.10.999.2",
            "1.2.826.0.1.3680043.10.999.3",
            "1.2.826.0.1.3680043.10.999.4",
            "1.2.826.0.1.3680043.10.999.5",
            "1.2.826.0.1.3680043.10.999.6",
            "WSI pyramid s0 ser0 z0 c0 t0",
            7,
            42,
            level_idx,
            512,
            512,
            1024,
            1536,
            6,
            PixelProfile {
                components: 3,
                bits_allocated: 8,
                photometric_interpretation: "RGB",
            },
            Some((0.0005, 0.0005)),
            vec![0; 6],
            vec![128; 6],
            None,
        )
        .unwrap()
    }

    #[test]
    fn vl_wsi_rectangular_frames_write_rows_columns_and_positions() {
        let object = super::build_dicom_object(
            &DicomMetadata::default(),
            "1.2.826.0.1.3680043.10.999.1",
            "1.2.826.0.1.3680043.10.999.2",
            "1.2.826.0.1.3680043.10.999.3",
            "1.2.826.0.1.3680043.10.999.4",
            "1.2.826.0.1.3680043.10.999.5",
            "1.2.826.0.1.3680043.10.999.6",
            "WSI pyramid s0 ser0 z0 c0 t0",
            7,
            42,
            0,
            64,
            8,
            130,
            31,
            12,
            PixelProfile {
                components: 3,
                bits_allocated: 8,
                photometric_interpretation: "YBR_FULL_422",
            },
            Some((0.0005, 0.00025)),
            vec![0; 12],
            vec![128; 12],
            None,
        )
        .unwrap();

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
        let err = super::build_dicom_object(
            &DicomMetadata::default(),
            "1.2.826.0.1.3680043.10.999.1",
            "1.2.826.0.1.3680043.10.999.2",
            "1.2.826.0.1.3680043.10.999.3",
            "1.2.826.0.1.3680043.10.999.4",
            "1.2.826.0.1.3680043.10.999.5",
            "1.2.826.0.1.3680043.10.999.6",
            "WSI pyramid s0 ser0 z0 c0 t0",
            7,
            42,
            0,
            u16::MAX as u32,
            1,
            2_147_516_416,
            1,
            32_770,
            PixelProfile {
                components: 3,
                bits_allocated: 8,
                photometric_interpretation: "YBR_FULL_422",
            },
            Some((0.0005, 0.00025)),
            vec![0; 32_770],
            vec![128; 32_770],
            None,
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("position exceeds SL range"),
            "unexpected error: {err}"
        );
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
}
