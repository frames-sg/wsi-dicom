use dicom_core::value::DataSetSequence;
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::encoding::format_ds;
#[cfg(test)]
use super::functional_groups::per_frame_items;
use super::functional_groups::FrameGrid;
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
const DEFAULT_FOCUS_METHOD: &str = "AUTO";
pub(crate) struct LossyCompressionMetadata {
    pub(crate) method: &'static str,
    pub(crate) ratio: Option<f64>,
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
    if metadata.requires_utf8() {
        put_str(
            &mut object,
            tags::SPECIFIC_CHARACTER_SET,
            VR::CS,
            "ISO_IR 192",
        );
    }
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
        f64::from(metadata.imaged_volume_depth_um()),
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
            metadata.slice_thickness_mm_ds(),
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
    #[cfg(test)]
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
    let mut profile = moxcms::ColorProfile::new_srgb()
        .encode()
        .map_err(|err| Error::Metadata {
            reason: format!("failed to generate synthetic sRGB ICC profile: {err}"),
        })?;
    stabilize_synthetic_icc_profile(&mut profile);
    Ok(profile)
}

pub(crate) fn synthetic_display_p3_icc_profile() -> Result<Vec<u8>, Error> {
    let mut profile = moxcms::ColorProfile::new_display_p3()
        .encode()
        .map_err(|err| Error::Metadata {
            reason: format!("failed to generate synthetic Display P3 ICC profile: {err}"),
        })?;
    stabilize_synthetic_icc_profile(&mut profile);
    Ok(profile)
}

fn stabilize_synthetic_icc_profile(profile: &mut [u8]) {
    const ICC_CREATION_DATETIME: std::ops::Range<usize> = 24..36;
    const FIXED_CREATION_DATETIME: [u8; 12] = [
        0x07, 0xE8, // 2024
        0x00, 0x01, // January
        0x00, 0x01, // Day 1
        0x00, 0x00, // Hour 0
        0x00, 0x00, // Minute 0
        0x00, 0x00, // Second 0
    ];
    if let Some(created_at) = profile.get_mut(ICC_CREATION_DATETIME) {
        created_at.copy_from_slice(&FIXED_CREATION_DATETIME);
    }
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
    slice_thickness_mm_ds: &str,
) -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    item.put(DataElement::<InMemDicomObject>::new(
        tags::PIXEL_MEASURES_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![pixel_measures_item(
            row_spacing_mm,
            column_spacing_mm,
            slice_thickness_mm_ds,
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
    slice_thickness_mm_ds: &str,
) -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    put_ds_pair(
        &mut item,
        tags::PIXEL_SPACING,
        row_spacing_mm,
        column_spacing_mm,
    );
    put_str(
        &mut item,
        tags::SLICE_THICKNESS,
        VR::DS,
        slice_thickness_mm_ds,
    );
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
