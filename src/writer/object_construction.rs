use dicom_core::value::DataSetSequence;
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::encoding::format_ds;
use super::functional_groups::FrameGrid;
use crate::lossy::LossyCompressionHistory;
use crate::metadata::ValidatedDicomMetadata;
use crate::options::ConformanceLevel;
use crate::report::ImageFlavor;
use crate::tile::PixelProfile;
use crate::{DicomMetadata, Error, VL_WSI_SOP_CLASS_UID};

const DEFAULT_DATE: &str = "19700101";
const DEFAULT_TIME: &str = "000000";
const DEFAULT_DATE_TIME: &str = "19700101000000";
const DEFAULT_POSITION_REFERENCE: &str = "SLIDE_CORNER";
const DEFAULT_MANUFACTURER: &str = "wsi-dicom";
const DEFAULT_DEVICE_SERIAL_NUMBER: &str = "RESEARCH";
const DEFAULT_CONTAINER_IDENTIFIER: &str = "RESEARCH-CONTAINER";
const DEFAULT_SPECIMEN_DESCRIPTION: &str = "Research placeholder specimen";
const DEFAULT_FOCUS_METHOD: &str = "AUTO";
pub(crate) struct DicomObjectIdentifiers<'a> {
    pub(crate) study_uid: &'a str,
    pub(crate) specimen_uid: &'a str,
    pub(crate) series_uid: &'a str,
    pub(crate) sop_instance_uid: &'a str,
    pub(crate) frame_of_reference_uid: &'a str,
    /// Pyramid UID and Pyramid Label of a pyramid level; LABEL, OVERVIEW and
    /// THUMBNAIL instances stay outside the Multi-Resolution Pyramid module.
    pub(crate) pyramid: Option<(&'a str, &'a str)>,
    pub(crate) dimension_organization_uid: &'a str,
    /// Level-zero SOP Instance a resampled level was derived from, when that
    /// instance is part of the same export.
    pub(crate) derivation_source_sop_instance_uid: Option<&'a str>,
}

pub(crate) struct DicomObjectParams<'a> {
    pub(crate) metadata: &'a DicomMetadata,
    pub(crate) identifiers: DicomObjectIdentifiers<'a>,
    pub(crate) series_number: u32,
    pub(crate) instance_number: u32,
    /// Image Type Value 3; it decides which flavour-conditional modules and
    /// attributes the object carries (PS3.3 A.32.8, C.8.12.4).
    pub(crate) flavor: ImageFlavor,
    pub(crate) level_idx: u32,
    pub(crate) frame_grid: FrameGrid,
    pub(crate) frame_count: u32,
    pub(crate) profile: PixelProfile,
    pub(crate) pixel_spacing_mm: Option<(f64, f64)>,
    pub(crate) icc_profile: Option<&'a [u8]>,
    /// Objective lens power the source reports (`openslide.objective-power`).
    pub(crate) objective_lens_power: Option<f64>,
    pub(crate) lossy_compression: LossyCompressionHistory,
    /// `Optional` also writes the optional attributes this writer can fill
    /// truthfully; `Required` leaves every optional descriptive one out.
    pub(crate) conformance: ConformanceLevel,
    /// The frames are a subset of the tile grid, positioned per frame
    /// (TILED_SPARSE); otherwise they are the whole grid (TILED_FULL).
    pub(crate) sparse: bool,
}

struct DicomObjectBuildContext<'a> {
    params: DicomObjectParams<'a>,
    metadata: ValidatedDicomMetadata<'a>,
    image_type: &'static str,
    dicom_frame_rows: u16,
    dicom_frame_columns: u16,
    dicom_matrix_columns: u32,
    dicom_matrix_rows: u32,
}

pub(crate) fn build_dicom_object(params: DicomObjectParams<'_>) -> Result<InMemDicomObject, Error> {
    let metadata = params.metadata.validated_for_writer()?;
    let frame_grid = params.frame_grid;
    frame_grid.validate()?;
    let flavor = params.flavor;
    if params.pixel_spacing_mm.is_none() && flavor_is_slide_registered(flavor) {
        return Err(Error::Metadata {
            reason: format!(
                "VL WSI {} export requires pixel spacing metadata",
                flavor.as_dicom()
            ),
        });
    }
    let dicom_frame_rows = checked_u16_attribute(frame_grid.frame_rows, "Rows")?;
    let dicom_frame_columns = checked_u16_attribute(frame_grid.frame_columns, "Columns")?;
    let dicom_matrix_columns =
        checked_u32_attribute(frame_grid.matrix_columns, "Total Pixel Matrix Columns")?;
    let dicom_matrix_rows =
        checked_u32_attribute(frame_grid.matrix_rows, "Total Pixel Matrix Rows")?;
    let image_type = image_type(flavor, params.level_idx);
    let context = DicomObjectBuildContext {
        params,
        metadata,
        image_type,
        dicom_frame_rows,
        dicom_frame_columns,
        dicom_matrix_columns,
        dicom_matrix_rows,
    };
    let mut object = InMemDicomObject::new_empty();

    put_sop_common_and_identification_modules(&mut object, &context);
    put_patient_module(&mut object, &context);
    put_study_and_frame_of_reference_modules(&mut object, &context);
    put_equipment_and_content_modules(&mut object, &context);
    put_image_pixel_module(&mut object, &context);
    put_lossy_compression_module(&mut object, &context);
    put_wsi_image_module(&mut object, &context);
    put_wsi_sequences(&mut object, &context);
    if flavor == ImageFlavor::Label {
        put_slide_label_module(&mut object);
    }

    Ok(object)
}

fn image_type(flavor: ImageFlavor, level_idx: u32) -> &'static str {
    match flavor {
        ImageFlavor::Volume if level_idx == 0 => "ORIGINAL\\PRIMARY\\VOLUME\\NONE",
        ImageFlavor::Volume => "DERIVED\\PRIMARY\\VOLUME\\RESAMPLED",
        ImageFlavor::Label => "ORIGINAL\\PRIMARY\\LABEL\\NONE",
        ImageFlavor::Overview => "ORIGINAL\\PRIMARY\\OVERVIEW\\NONE",
        ImageFlavor::Thumbnail => "DERIVED\\PRIMARY\\THUMBNAIL\\RESAMPLED",
    }
}

/// VOLUME and THUMBNAIL images are registered to the slide coordinates: they
/// carry the Frame of Reference (PS3.3 A.32.8) and Pixel Spacing (C.7.6.16.2.1).
/// LABEL and OVERVIEW images require neither.
fn flavor_is_slide_registered(flavor: ImageFlavor) -> bool {
    matches!(flavor, ImageFlavor::Volume | ImageFlavor::Thumbnail)
}

/// Slide Label module (PS3.3 C.8.12.8), required for LABEL images. Barcode
/// Value and Label Text are Type 2; they are left empty because the source's
/// label content is carried only in the pixels.
fn put_slide_label_module(object: &mut InMemDicomObject) {
    put_str(object, tags::BARCODE_VALUE, VR::LT, "");
    put_str(object, tags::LABEL_TEXT, VR::UT, "");
}

fn put_sop_common_and_identification_modules(
    object: &mut InMemDicomObject,
    context: &DicomObjectBuildContext<'_>,
) {
    let params = &context.params;
    let metadata = &context.metadata;
    let identifiers = &params.identifiers;

    if metadata.requires_utf8() {
        put_str(object, tags::SPECIFIC_CHARACTER_SET, VR::CS, "ISO_IR 192");
    }
    put_str(object, tags::SOP_CLASS_UID, VR::UI, VL_WSI_SOP_CLASS_UID);
    put_str(
        object,
        tags::SOP_INSTANCE_UID,
        VR::UI,
        identifiers.sop_instance_uid,
    );
    put_str(
        object,
        tags::STUDY_INSTANCE_UID,
        VR::UI,
        identifiers.study_uid,
    );
    put_str(
        object,
        tags::SERIES_INSTANCE_UID,
        VR::UI,
        identifiers.series_uid,
    );
    if flavor_is_slide_registered(params.flavor) {
        put_str(
            object,
            tags::FRAME_OF_REFERENCE_UID,
            VR::UI,
            identifiers.frame_of_reference_uid,
        );
    }
    let optional = params.conformance.is_optional();
    // The Multi-Resolution Pyramid module may be present for pyramid levels.
    if let Some((pyramid_uid, pyramid_label)) = identifiers.pyramid.filter(|_| optional) {
        put_str(object, tags::PYRAMID_UID, VR::UI, pyramid_uid);
        put_str(object, tags::PYRAMID_LABEL, VR::LO, pyramid_label);
    }
    put_str(object, tags::MODALITY, VR::CS, "SM");
    if optional {
        let date = metadata.content_date.as_deref().unwrap_or(DEFAULT_DATE);
        let time = metadata.content_time.as_deref().unwrap_or(DEFAULT_TIME);
        for (tag, vr, value) in [
            (tags::SERIES_DATE, VR::DA, date),
            (tags::SERIES_TIME, VR::TM, time),
            (tags::ACQUISITION_DATE, VR::DA, date),
            (tags::ACQUISITION_TIME, VR::TM, time),
        ] {
            put_str(object, tag, vr, value);
        }
    }
    put_str(object, tags::IMAGE_TYPE, VR::CS, context.image_type);
    put_str(object, tags::LOSSY_IMAGE_COMPRESSION, VR::CS, "00");
}

fn put_patient_module(object: &mut InMemDicomObject, context: &DicomObjectBuildContext<'_>) {
    let metadata = &context.metadata;

    put_str(
        object,
        tags::PATIENT_NAME,
        VR::PN,
        metadata.patient_name.as_deref().unwrap_or_default(),
    );
    put_str(
        object,
        tags::PATIENT_ID,
        VR::LO,
        metadata.patient_id.as_deref().unwrap_or_default(),
    );
    put_str(
        object,
        tags::PATIENT_BIRTH_DATE,
        VR::DA,
        metadata.patient_birth_date.as_deref().unwrap_or_default(),
    );
    put_str(
        object,
        tags::PATIENT_SEX,
        VR::CS,
        metadata.patient_sex.as_deref().unwrap_or_default(),
    );
}

fn put_study_and_frame_of_reference_modules(
    object: &mut InMemDicomObject,
    context: &DicomObjectBuildContext<'_>,
) {
    let metadata = &context.metadata;

    put_str(
        object,
        tags::ACCESSION_NUMBER,
        VR::SH,
        metadata.accession_number.as_deref().unwrap_or_default(),
    );
    put_str(
        object,
        tags::STUDY_DATE,
        VR::DA,
        metadata.study_date.as_deref().unwrap_or(DEFAULT_DATE),
    );
    put_str(
        object,
        tags::STUDY_TIME,
        VR::TM,
        metadata.study_time.as_deref().unwrap_or(DEFAULT_TIME),
    );
    put_str(
        object,
        tags::STUDY_ID,
        VR::SH,
        metadata.study_id.as_deref().unwrap_or("1"),
    );
    if let Some(description) = non_empty(metadata.study_description.as_deref())
        .filter(|_| context.params.conformance.is_optional())
    {
        put_str(object, tags::STUDY_DESCRIPTION, VR::LO, description);
    }
    put_str(
        object,
        tags::REFERRING_PHYSICIAN_NAME,
        VR::PN,
        metadata
            .referring_physician_name
            .as_deref()
            .unwrap_or_default(),
    );
    if let Some(laterality) = non_empty(metadata.laterality.as_deref()) {
        put_str(object, tags::LATERALITY, VR::CS, laterality);
    }
    if flavor_is_slide_registered(context.params.flavor) {
        put_str(
            object,
            tags::POSITION_REFERENCE_INDICATOR,
            VR::LO,
            DEFAULT_POSITION_REFERENCE,
        );
    }
}

fn put_equipment_and_content_modules(
    object: &mut InMemDicomObject,
    context: &DicomObjectBuildContext<'_>,
) {
    let metadata = &context.metadata;

    put_str(
        object,
        tags::MANUFACTURER,
        VR::LO,
        metadata
            .manufacturer
            .as_deref()
            .unwrap_or(DEFAULT_MANUFACTURER),
    );
    put_str(
        object,
        tags::MANUFACTURER_MODEL_NAME,
        VR::LO,
        metadata
            .manufacturer_model_name
            .as_deref()
            .unwrap_or(DEFAULT_MANUFACTURER),
    );
    put_str(
        object,
        tags::DEVICE_SERIAL_NUMBER,
        VR::LO,
        metadata
            .device_serial_number
            .as_deref()
            .unwrap_or(DEFAULT_DEVICE_SERIAL_NUMBER),
    );
    put_str(
        object,
        tags::SOFTWARE_VERSIONS,
        VR::LO,
        metadata
            .software_versions
            .as_deref()
            .unwrap_or(env!("CARGO_PKG_VERSION")),
    );
    put_str(
        object,
        tags::CONTENT_DATE,
        VR::DA,
        metadata.content_date.as_deref().unwrap_or(DEFAULT_DATE),
    );
    put_str(
        object,
        tags::CONTENT_TIME,
        VR::TM,
        metadata.content_time.as_deref().unwrap_or(DEFAULT_TIME),
    );
    put_str(
        object,
        tags::ACQUISITION_DATE_TIME,
        VR::DT,
        metadata
            .acquisition_date_time
            .as_deref()
            .unwrap_or(DEFAULT_DATE_TIME),
    );
    put_str(
        object,
        tags::CONTAINER_IDENTIFIER,
        VR::LO,
        metadata
            .container_identifier
            .as_deref()
            .unwrap_or(DEFAULT_CONTAINER_IDENTIFIER),
    );
}

fn put_image_pixel_module(object: &mut InMemDicomObject, context: &DicomObjectBuildContext<'_>) {
    let params = &context.params;
    let metadata = &context.metadata;
    let frame_grid = params.frame_grid;

    put_u16(object, tags::ROWS, context.dicom_frame_rows);
    put_u16(object, tags::COLUMNS, context.dicom_frame_columns);
    put_u32(
        object,
        tags::TOTAL_PIXEL_MATRIX_COLUMNS,
        context.dicom_matrix_columns,
    );
    put_u32(
        object,
        tags::TOTAL_PIXEL_MATRIX_ROWS,
        context.dicom_matrix_rows,
    );
    // Imaged Volume Width, Height and Depth are required only for VOLUME.
    if let (ImageFlavor::Volume, Some((row_spacing_mm, column_spacing_mm))) =
        (params.flavor, params.pixel_spacing_mm)
    {
        put_fl(
            object,
            tags::IMAGED_VOLUME_WIDTH,
            frame_grid.matrix_columns as f64 * column_spacing_mm,
        );
        put_fl(
            object,
            tags::IMAGED_VOLUME_HEIGHT,
            frame_grid.matrix_rows as f64 * row_spacing_mm,
        );
        put_fl(
            object,
            tags::IMAGED_VOLUME_DEPTH,
            f64::from(metadata.imaged_volume_depth_um()),
        );
    }
    put_str(
        object,
        tags::NUMBER_OF_FRAMES,
        VR::IS,
        &params.frame_count.to_string(),
    );
    put_u16(
        object,
        tags::SAMPLES_PER_PIXEL,
        params.profile.components as u16,
    );
    put_str(
        object,
        tags::PHOTOMETRIC_INTERPRETATION,
        VR::CS,
        params.profile.photometric_interpretation,
    );
    if params.profile.photometric_interpretation == "MONOCHROME2" {
        put_str(object, tags::PRESENTATION_LUT_SHAPE, VR::CS, "IDENTITY");
        put_str(object, tags::RESCALE_INTERCEPT, VR::DS, "0");
        put_str(object, tags::RESCALE_SLOPE, VR::DS, "1");
    }
    if params.profile.components > 1 {
        put_u16(object, tags::PLANAR_CONFIGURATION, 0);
    }
    put_u16(object, tags::BITS_ALLOCATED, params.profile.bits_allocated);
    put_u16(object, tags::BITS_STORED, params.profile.bits_allocated);
    put_u16(object, tags::HIGH_BIT, params.profile.bits_allocated - 1);
    put_u16(object, tags::PIXEL_REPRESENTATION, 0);
}

fn put_lossy_compression_module(
    object: &mut InMemDicomObject,
    context: &DicomObjectBuildContext<'_>,
) {
    let lossy_compression = &context.params.lossy_compression;

    if !lossy_compression.is_empty() {
        put_str(object, tags::LOSSY_IMAGE_COMPRESSION, VR::CS, "01");
        let ratios = lossy_compression
            .stages()
            .iter()
            .map(|stage| format_ds(stage.ratio()))
            .collect::<Vec<_>>();
        put_multi_str(object, tags::LOSSY_IMAGE_COMPRESSION_RATIO, VR::DS, &ratios);
        let methods = lossy_compression
            .stages()
            .iter()
            .map(|stage| stage.method().to_string())
            .collect::<Vec<_>>();
        put_multi_str(
            object,
            tags::LOSSY_IMAGE_COMPRESSION_METHOD,
            VR::CS,
            &methods,
        );
    }
}

fn put_wsi_image_module(object: &mut InMemDicomObject, context: &DicomObjectBuildContext<'_>) {
    let params = &context.params;
    let metadata = &context.metadata;

    // Number of Optical Paths and Total Pixel Matrix Focal Planes are
    // required for TILED_FULL and may be present otherwise.
    let organization = if params.sparse {
        "TILED_SPARSE"
    } else {
        "TILED_FULL"
    };
    put_str(
        object,
        tags::DIMENSION_ORGANIZATION_TYPE,
        VR::CS,
        organization,
    );
    if params.sparse {
        // Viewers draw absent frames black, as partial frames are padded
        // (CIELab L*=0, a*=b*=0; PS3.3 C.10.7.1.1).
        object.put(DataElement::new(
            tags::RECOMMENDED_ABSENT_PIXEL_CIE_LAB_VALUE,
            VR::US,
            PrimitiveValue::U16([0x0000, 0x8080, 0x8080].as_slice().into()),
        ));
    }
    put_u32(object, tags::NUMBER_OF_OPTICAL_PATHS, 1);
    put_u32(object, tags::TOTAL_PIXEL_MATRIX_FOCAL_PLANES, 1);
    // Specimen Label in Image shall be YES for OVERVIEW and LABEL (PS3.3
    // C.8.12.4). A label can identify the patient, so those images also
    // declare burned-in annotation.
    let shows_label = matches!(params.flavor, ImageFlavor::Label | ImageFlavor::Overview);
    let yes_no = if shows_label { "YES" } else { "NO" };
    put_str(object, tags::SPECIMEN_LABEL_IN_IMAGE, VR::CS, yes_no);
    put_str(object, tags::BURNED_IN_ANNOTATION, VR::CS, yes_no);
    put_str(object, tags::VOLUMETRIC_PROPERTIES, VR::CS, "VOLUME");
    put_str(
        object,
        tags::FOCUS_METHOD,
        VR::CS,
        metadata
            .focus_method
            .as_deref()
            .unwrap_or(DEFAULT_FOCUS_METHOD),
    );
    put_str(object, tags::EXTENDED_DEPTH_OF_FIELD, VR::CS, "NO");
    put_is(object, tags::SERIES_NUMBER, params.series_number);
    put_is(object, tags::INSTANCE_NUMBER, params.instance_number);
    if params.conformance.is_optional() {
        put_u16(object, tags::REPRESENTATIVE_FRAME_NUMBER, 1);
        put_str(object, tags::TILES_OVERLAP, VR::CS, "NONE");
        put_str(object, tags::QUALITY_CONTROL_IMAGE, VR::CS, "NO");
        // A label can identify the patient; tissue cannot. Whether a source
        // thumbnail shows the label is unknown.
        let recognizable = match params.flavor {
            ImageFlavor::Label | ImageFlavor::Overview => Some("YES"),
            ImageFlavor::Volume => Some("NO"),
            ImageFlavor::Thumbnail => None,
        };
        if let Some(recognizable) = recognizable {
            put_str(
                object,
                tags::RECOGNIZABLE_VISUAL_FEATURES,
                VR::CS,
                recognizable,
            );
        }
    }
    put_str(
        object,
        tags::IMAGE_ORIENTATION_SLIDE,
        VR::DS,
        "1\\0\\0\\0\\1\\0",
    );
}

fn put_wsi_sequences(object: &mut InMemDicomObject, context: &DicomObjectBuildContext<'_>) {
    let params = &context.params;
    let metadata = &context.metadata;
    let identifiers = &params.identifiers;
    let optional = params.conformance.is_optional();

    object.put(DataElement::<InMemDicomObject>::new(
        tags::OPTICAL_PATH_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![optical_path_item(
            params.icc_profile,
            // LABEL and OVERVIEW images come from the macro camera, not the
            // scanning objective.
            params
                .objective_lens_power
                .filter(|_| flavor_is_slide_registered(params.flavor)),
            optional,
        )]),
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
        DataSetSequence::from(vec![specimen_description_item(
            metadata.as_metadata(),
            identifiers.specimen_uid,
            optional,
        )]),
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
            context.image_type,
            params.pixel_spacing_mm,
            flavor_is_slide_registered(params.flavor).then(|| metadata.slice_thickness_mm_ds()),
            identifiers.derivation_source_sop_instance_uid,
            optional,
        )]),
    ));
    if let Some(source_uid) = identifiers.derivation_source_sop_instance_uid {
        put_derivation_references(object, identifiers.series_uid, source_uid, optional);
    }
    object.put(DataElement::<InMemDicomObject>::new(
        tags::DIMENSION_ORGANIZATION_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![dimension_organization_item(
            identifiers.dimension_organization_uid,
        )]),
    ));
    // Dimension Index Sequence is 1C, not required for TILED_FULL; its
    // pointers name the per-frame Plane Position (Slide) that optional pyramid
    // levels carry. Single-frame LABEL, OVERVIEW and THUMBNAIL images carry
    // none.
    if optional && params.flavor == ImageFlavor::Volume {
        object.put(DataElement::<InMemDicomObject>::new(
            tags::DIMENSION_INDEX_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(dimension_index_items(
                identifiers.dimension_organization_uid,
            )),
        ));
    }
}

fn optical_path_item(
    icc_profile: Option<&[u8]>,
    objective_lens_power: Option<f64>,
    optional: bool,
) -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    put_str(&mut item, tags::OPTICAL_PATH_IDENTIFIER, VR::SH, "0");
    if optional {
        put_str(
            &mut item,
            tags::OPTICAL_PATH_DESCRIPTION,
            VR::ST,
            "Default optical path",
        );
    }
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
    // Illumination Wave Length is required only without the color code.
    if optional {
        put_fl(&mut item, tags::ILLUMINATION_WAVE_LENGTH, 550.0);
    }
    if let Some(power) = objective_lens_power.filter(|_| optional) {
        put_ds(&mut item, tags::OBJECTIVE_LENS_POWER, power);
    }
    if let Some(icc_profile) = icc_profile {
        item.put(DataElement::new(
            tags::ICC_PROFILE,
            VR::OB,
            PrimitiveValue::from(icc_profile.to_vec()),
        ));
        // Color Space names the profile's space only when it is known to be
        // sRGB (PS3.3 C.11.15.1.4).
        if optional && crate::icc::is_synthetic_srgb_icc_profile(icc_profile) {
            put_str(&mut item, tags::COLOR_SPACE, VR::CS, "SRGB");
        }
    }
    item
}

fn specimen_description_item(
    metadata: &DicomMetadata,
    specimen_uid: &str,
    optional: bool,
) -> InMemDicomObject {
    let identifier = metadata.specimen_identifier_or_default();
    let description = metadata
        .specimen_description
        .as_deref()
        .unwrap_or(DEFAULT_SPECIMEN_DESCRIPTION);
    let mut item = InMemDicomObject::new_empty();
    put_str(&mut item, tags::SPECIMEN_IDENTIFIER, VR::LO, identifier);
    put_str(&mut item, tags::SPECIMEN_UID, VR::UI, specimen_uid);
    if optional {
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
    }
    match &metadata.specimen_identifier_issuer {
        Some(issuer) => {
            item.put(DataElement::<InMemDicomObject>::new(
                tags::ISSUER_OF_THE_SPECIMEN_IDENTIFIER_SEQUENCE,
                VR::SQ,
                DataSetSequence::from(vec![specimen_identifier_issuer_item(issuer)]),
            ));
        }
        None => put_empty_sequence(&mut item, tags::ISSUER_OF_THE_SPECIMEN_IDENTIFIER_SEQUENCE),
    }
    put_empty_sequence(&mut item, tags::SPECIMEN_PREPARATION_SEQUENCE);
    item
}

fn specimen_identifier_issuer_item(issuer: &crate::SpecimenIdentifierIssuer) -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    if let Some(local) = issuer.local_namespace_entity_id.as_deref() {
        put_str(&mut item, tags::LOCAL_NAMESPACE_ENTITY_ID, VR::UT, local);
    }
    if let Some(universal) = issuer.universal_entity_id.as_deref() {
        put_str(&mut item, tags::UNIVERSAL_ENTITY_ID, VR::UT, universal);
    }
    if let Some(kind) = issuer.universal_entity_id_type {
        put_str(
            &mut item,
            tags::UNIVERSAL_ENTITY_ID_TYPE,
            VR::CS,
            kind.as_dicom_code(),
        );
    }
    item
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

/// General Reference Source Image Sequence (PS3.3 C.12.4) and Common
/// Instance Reference (C.12.2) for a level resampled from `source_uid`, which
/// is in the same Series.
/// The level-zero source of a resampled level, in the Common Instance
/// Reference module (required once the Derivation Image functional group
/// references it) and, as an optional General Reference, at the top level.
fn put_derivation_references(
    object: &mut InMemDicomObject,
    series_uid: &str,
    source_uid: &str,
    optional: bool,
) {
    if optional {
        object.put(DataElement::<InMemDicomObject>::new(
            tags::SOURCE_IMAGE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![referenced_instance_item(source_uid)]),
        ));
    }
    let mut series = InMemDicomObject::new_empty();
    put_str(&mut series, tags::SERIES_INSTANCE_UID, VR::UI, series_uid);
    series.put(DataElement::<InMemDicomObject>::new(
        tags::REFERENCED_INSTANCE_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![referenced_instance_item(source_uid)]),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::REFERENCED_SERIES_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![series]),
    ));
}

fn referenced_instance_item(sop_instance_uid: &str) -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    put_str(
        &mut item,
        tags::REFERENCED_SOP_CLASS_UID,
        VR::UI,
        VL_WSI_SOP_CLASS_UID,
    );
    put_str(
        &mut item,
        tags::REFERENCED_SOP_INSTANCE_UID,
        VR::UI,
        sop_instance_uid,
    );
    item
}

/// Derivation Image Macro item (PS3.3 C.7.6.16.2.6): the level was spatially
/// resampled (DCM 113085) from the referenced source (DCM 121322), keeping
/// slide coordinates.
fn derivation_image_item(source_uid: &str) -> InMemDicomObject {
    let mut source = referenced_instance_item(source_uid);
    source.put(DataElement::<InMemDicomObject>::new(
        tags::PURPOSE_OF_REFERENCE_CODE_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![code_item(
            "121322",
            "DCM",
            "Source image for image processing operation",
        )]),
    ));
    put_str(
        &mut source,
        tags::SPATIAL_LOCATIONS_PRESERVED,
        VR::CS,
        "YES",
    );
    let mut item = InMemDicomObject::new_empty();
    item.put(DataElement::<InMemDicomObject>::new(
        tags::DERIVATION_CODE_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![code_item("113085", "DCM", "Spatial resampling")]),
    ));
    item.put(DataElement::<InMemDicomObject>::new(
        tags::SOURCE_IMAGE_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![source]),
    ));
    item
}

fn shared_functional_groups_item(
    image_type: &str,
    pixel_spacing_mm: Option<(f64, f64)>,
    slice_thickness_mm_ds: Option<&str>,
    derivation_source_uid: Option<&str>,
    optional: bool,
) -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    if let Some(source_uid) = derivation_source_uid {
        item.put(DataElement::<InMemDicomObject>::new(
            tags::DERIVATION_IMAGE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![derivation_image_item(source_uid)]),
        ));
    }
    item.put(DataElement::<InMemDicomObject>::new(
        tags::PIXEL_MEASURES_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![pixel_measures_item(
            pixel_spacing_mm,
            slice_thickness_mm_ds,
        )]),
    ));
    item.put(DataElement::<InMemDicomObject>::new(
        tags::WHOLE_SLIDE_MICROSCOPY_IMAGE_FRAME_TYPE_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![frame_type_item(image_type)]),
    ));
    // Required only when Dimension Organization Type is not TILED_FULL, which
    // optional TILED_SPARSE instances are.
    if optional {
        item.put(DataElement::<InMemDicomObject>::new(
            tags::OPTICAL_PATH_IDENTIFICATION_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![optical_path_identification_item()]),
        ));
    }
    item
}

/// Pixel Measures item; Pixel Spacing and Slice Thickness are 1C and absent
/// when not required and not known (LABEL and OVERVIEW).
fn pixel_measures_item(
    pixel_spacing_mm: Option<(f64, f64)>,
    slice_thickness_mm_ds: Option<&str>,
) -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    if let Some((row_spacing_mm, column_spacing_mm)) = pixel_spacing_mm {
        put_ds_pair(
            &mut item,
            tags::PIXEL_SPACING,
            row_spacing_mm,
            column_spacing_mm,
        );
    }
    if let Some(slice_thickness_mm_ds) = slice_thickness_mm_ds {
        put_str(
            &mut item,
            tags::SLICE_THICKNESS,
            VR::DS,
            slice_thickness_mm_ds,
        );
    }
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
            tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
        ),
        dimension_index_item(
            dimension_organization_uid,
            tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
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

fn put_multi_str(object: &mut InMemDicomObject, tag: Tag, vr: VR, values: &[String]) {
    object.put(DataElement::new(
        tag,
        vr,
        PrimitiveValue::Strs(values.to_vec().into()),
    ));
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
