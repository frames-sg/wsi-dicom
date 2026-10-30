//! A slide scan that meets the DICOM standard and every convention profile:
//! a three-layer TILED_FULL pyramid in baseline JPEG, a LABEL and an OVERVIEW
//! image, a 2D Microscopy Bulk Simple Annotations object and a completion
//! Key Object Selection, each built as a DICOM object that the tests edit.

use std::path::{Path, PathBuf};

use dicom_core::value::{DataSetSequence, PixelFragmentSequence};
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::mem::InMemElement;
use dicom_object::{DefaultDicomObject, FileMetaTableBuilder, InMemDicomObject};
use j2k::{J2kLosslessEncodeOptions, J2kLosslessSamples, ReversibleTransform};
use j2k_jpeg::{JpegBackend, JpegEncodeOptions, JpegSamples, JpegSubsampling};

const ROOT: &str = "1.2.826.0.1.3680043.10.999.8";
pub(super) const CONTAINER: &str = "SLIDE-1";
pub(super) const SPECIMEN: &str = "SPECIMEN-1";
pub(super) const SPECIMEN_ISSUER: &str = "FIXTURE-LAB";
pub(super) const ACCESSION: &str = "A1";

/// The UIDs of one scan of one slide.
#[derive(Clone, Debug)]
pub(super) struct Slide {
    pub(super) study: String,
    pub(super) series: String,
    pub(super) frame_of_reference: String,
    pub(super) pyramid: String,
    pub(super) specimen: String,
    pub(super) acquisition: String,
}

impl Slide {
    pub(super) fn new(suffix: u32) -> Self {
        let uid = |role: u32| format!("{ROOT}.{suffix}.{role}");
        Self {
            study: uid(1),
            series: uid(2),
            frame_of_reference: uid(3),
            pyramid: uid(4),
            specimen: uid(5),
            acquisition: uid(6),
        }
    }

    pub(super) fn instance_uid(&self, instance: u32) -> String {
        format!("{}.{}", self.study, 100 + instance)
    }
}

/// Image Type Value 3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Flavor {
    Volume,
    Label,
    Overview,
}

impl Flavor {
    fn term(self) -> &'static str {
        match self {
            Self::Volume => "VOLUME",
            Self::Label => "LABEL",
            Self::Overview => "OVERVIEW",
        }
    }
}

/// One VL WSI image of the scan.
#[derive(Clone, Debug)]
pub(super) struct Image {
    pub(super) instance: u32,
    pub(super) flavor: Flavor,
    /// Total Pixel Matrix Rows and Columns.
    pub(super) matrix: u32,
    /// Rows and Columns of a Frame.
    pub(super) tile: u16,
    pub(super) spacing_mm: f64,
    pub(super) syntax: &'static str,
}

impl Image {
    /// A JPEG baseline VOLUME layer of `matrix` pixels in 8x8 tiles.
    pub(super) fn volume(instance: u32, matrix: u32, spacing_mm: f64) -> Self {
        Self {
            instance,
            flavor: Flavor::Volume,
            matrix,
            tile: 8,
            spacing_mm,
            syntax: uids::JPEG_BASELINE8_BIT,
        }
    }

    /// An uncompressed single-frame LABEL or OVERVIEW image.
    pub(super) fn associated(instance: u32, flavor: Flavor) -> Self {
        Self {
            instance,
            flavor,
            matrix: 8,
            tile: 8,
            spacing_mm: 0.05,
            syntax: uids::EXPLICIT_VR_LITTLE_ENDIAN,
        }
    }

    fn photometric(&self) -> &'static str {
        if self.syntax == uids::JPEG_BASELINE8_BIT {
            "YBR_FULL_422"
        } else {
            "RGB"
        }
    }

    fn frames(&self) -> u32 {
        let across = self.matrix.div_ceil(u32::from(self.tile));
        across * across
    }

    pub(super) fn object(&self, slide: &Slide) -> DefaultDicomObject {
        let mut object = InMemDicomObject::new_empty();
        self.identification(&mut object, slide);
        patient_study(&mut object, slide, &slide.series, "SM");
        request_attributes(&mut object, slide);
        self.frame_of_reference(&mut object, slide);
        equipment(&mut object, "Fixture Scanner Co", "F1");
        put(
            &mut object,
            tags::ACQUISITION_DATE_TIME,
            VR::DT,
            "20260102101500",
        );
        self.image(&mut object);
        specimen(&mut object, slide);
        self.tile_organization(&mut object);
        self.optical_path(&mut object);
        self.functional_groups(&mut object);
        self.pixel_data(&mut object);
        with_meta(object, crate::VL_WSI_SOP_CLASS_UID, self.syntax)
    }

    fn identification(&self, object: &mut InMemDicomObject, slide: &Slide) {
        put(
            object,
            tags::IMAGE_TYPE,
            VR::CS,
            &format!("ORIGINAL\\PRIMARY\\{}\\NONE", self.flavor.term()),
        );
        put(
            object,
            tags::SOP_CLASS_UID,
            VR::UI,
            crate::VL_WSI_SOP_CLASS_UID,
        );
        put(
            object,
            tags::SOP_INSTANCE_UID,
            VR::UI,
            &slide.instance_uid(self.instance),
        );
        put(
            object,
            tags::INSTANCE_NUMBER,
            VR::IS,
            &self.instance.to_string(),
        );
        put(object, tags::ACQUISITION_UID, VR::UI, &slide.acquisition);
        if self.flavor == Flavor::Volume {
            put(object, tags::PYRAMID_UID, VR::UI, &slide.pyramid);
        }
    }

    fn frame_of_reference(&self, object: &mut InMemDicomObject, slide: &Slide) {
        put(
            object,
            tags::FRAME_OF_REFERENCE_UID,
            VR::UI,
            &slide.frame_of_reference,
        );
        put(
            object,
            tags::POSITION_REFERENCE_INDICATOR,
            VR::LO,
            "SLIDE_CORNER",
        );
    }

    fn image(&self, object: &mut InMemDicomObject) {
        object.put(unsigned(tags::SAMPLES_PER_PIXEL, 3));
        put(
            object,
            tags::PHOTOMETRIC_INTERPRETATION,
            VR::CS,
            self.photometric(),
        );
        object.put(unsigned(tags::PLANAR_CONFIGURATION, 0));
        put(
            object,
            tags::NUMBER_OF_FRAMES,
            VR::IS,
            &self.frames().to_string(),
        );
        object.put(unsigned(tags::ROWS, self.tile));
        object.put(unsigned(tags::COLUMNS, self.tile));
        object.put(unsigned(tags::BITS_ALLOCATED, 8));
        object.put(unsigned(tags::BITS_STORED, 8));
        object.put(unsigned(tags::HIGH_BIT, 7));
        object.put(unsigned(tags::PIXEL_REPRESENTATION, 0));
        let label = self.flavor != Flavor::Volume;
        let yes_no = |value: bool| if value { "YES" } else { "NO" };
        for (tag, value) in [
            (tags::VOLUMETRIC_PROPERTIES, "VOLUME"),
            (tags::SPECIMEN_LABEL_IN_IMAGE, yes_no(label)),
            (tags::BURNED_IN_ANNOTATION, yes_no(label)),
            (tags::FOCUS_METHOD, "AUTO"),
            (tags::EXTENDED_DEPTH_OF_FIELD, "NO"),
        ] {
            put(object, tag, VR::CS, value);
        }
        if self.flavor == Flavor::Label {
            put(object, tags::BARCODE_VALUE, VR::LT, CONTAINER);
            put(object, tags::LABEL_TEXT, VR::UT, CONTAINER);
        }
        if self.syntax == uids::JPEG_BASELINE8_BIT {
            put(object, tags::LOSSY_IMAGE_COMPRESSION, VR::CS, "01");
            put(object, tags::LOSSY_IMAGE_COMPRESSION_RATIO, VR::DS, "10");
            put(
                object,
                tags::LOSSY_IMAGE_COMPRESSION_METHOD,
                VR::CS,
                "ISO_10918_1",
            );
        } else {
            put(object, tags::LOSSY_IMAGE_COMPRESSION, VR::CS, "00");
        }
    }

    fn tile_organization(&self, object: &mut InMemDicomObject) {
        object.put(long(tags::TOTAL_PIXEL_MATRIX_COLUMNS, self.matrix));
        object.put(long(tags::TOTAL_PIXEL_MATRIX_ROWS, self.matrix));
        object.put(long(tags::TOTAL_PIXEL_MATRIX_FOCAL_PLANES, 1));
        object.put(sq(
            tags::TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE,
            vec![item(vec![
                text(tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, VR::DS, "20"),
                text(tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, VR::DS, "10"),
            ])],
        ));
        put(
            object,
            tags::IMAGE_ORIENTATION_SLIDE,
            VR::DS,
            "0\\-1\\0\\-1\\0\\0",
        );
        put(object, tags::TILES_OVERLAP, VR::CS, "NONE");
        if self.flavor == Flavor::Volume {
            let extent = (f64::from(self.matrix) * self.spacing_mm) as f32;
            object.put(float(tags::IMAGED_VOLUME_WIDTH, extent));
            object.put(float(tags::IMAGED_VOLUME_HEIGHT, extent));
            object.put(float(tags::IMAGED_VOLUME_DEPTH, 1.0));
        }
        object.put(sq(
            tags::DIMENSION_ORGANIZATION_SEQUENCE,
            vec![item(vec![text(
                tags::DIMENSION_ORGANIZATION_UID,
                VR::UI,
                &format!("{}.{}.7", ROOT, self.instance),
            )])],
        ));
        put(
            object,
            tags::DIMENSION_ORGANIZATION_TYPE,
            VR::CS,
            "TILED_FULL",
        );
    }

    fn optical_path(&self, object: &mut InMemDicomObject) {
        object.put(long(tags::NUMBER_OF_OPTICAL_PATHS, 1));
        let profile = crate::icc::synthetic_srgb_icc_profile().expect("sRGB profile");
        object.put(sq(
            tags::OPTICAL_PATH_SEQUENCE,
            vec![item(vec![
                text(tags::OPTICAL_PATH_IDENTIFIER, VR::SH, "1"),
                sq(
                    tags::ILLUMINATION_TYPE_CODE_SEQUENCE,
                    vec![code("DCM", "111744", "Brightfield illumination")],
                ),
                DataElement::new(tags::ICC_PROFILE, VR::OB, PrimitiveValue::from(profile)),
                sq(
                    tags::ILLUMINATION_COLOR_CODE_SEQUENCE,
                    vec![code("SCT", "371251000", "White")],
                ),
                text(tags::OBJECTIVE_LENS_POWER, VR::DS, "20"),
            ])],
        ));
    }

    fn functional_groups(&self, object: &mut InMemDicomObject) {
        let mut measures = Vec::new();
        if self.flavor != Flavor::Label {
            measures.push(text(
                tags::PIXEL_SPACING,
                VR::DS,
                &format!("{}\\{}", self.spacing_mm, self.spacing_mm),
            ));
        }
        if self.flavor == Flavor::Volume {
            measures.push(text(tags::SLICE_THICKNESS, VR::DS, "0.001"));
        }
        let image_type = format!("ORIGINAL\\PRIMARY\\{}\\NONE", self.flavor.term());
        object.put(sq(
            tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
            vec![item(vec![
                sq(tags::PIXEL_MEASURES_SEQUENCE, vec![item(measures)]),
                sq(
                    tags::WHOLE_SLIDE_MICROSCOPY_IMAGE_FRAME_TYPE_SEQUENCE,
                    vec![item(vec![text(tags::FRAME_TYPE, VR::CS, &image_type)])],
                ),
                sq(
                    tags::OPTICAL_PATH_IDENTIFICATION_SEQUENCE,
                    vec![item(vec![text(tags::OPTICAL_PATH_IDENTIFIER, VR::SH, "1")])],
                ),
            ])],
        ));
    }

    /// Deterministic RGB samples of Frame `frame`.
    fn samples(&self, frame: u32) -> Vec<u8> {
        let size = usize::from(self.tile);
        let mut bytes = Vec::with_capacity(size * size * 3);
        for y in 0..size {
            for x in 0..size {
                for c in 0..3 {
                    bytes.push((x * 7 + y * 13 + c * 61 + frame as usize * 29 + (x ^ y)) as u8);
                }
            }
        }
        bytes
    }

    fn pixel_data(&self, object: &mut InMemDicomObject) {
        let frames: Vec<Vec<u8>> = (0..self.frames())
            .map(|frame| self.samples(frame))
            .collect();
        let size = u32::from(self.tile);
        let fragments: Vec<Vec<u8>> = match self.syntax {
            uids::JPEG_BASELINE8_BIT => frames
                .iter()
                .map(|samples| {
                    j2k_jpeg::encode_jpeg_baseline(
                        JpegSamples::Rgb8 {
                            data: samples,
                            width: size,
                            height: size,
                        },
                        JpegEncodeOptions {
                            quality: 90,
                            subsampling: JpegSubsampling::Ybr422,
                            restart_interval: None,
                            backend: JpegBackend::Cpu,
                        },
                    )
                    .expect("JPEG baseline")
                    .data
                })
                .collect(),
            uids::JPEG2000_LOSSLESS => frames
                .iter()
                .map(|samples| {
                    let mut options = J2kLosslessEncodeOptions::default();
                    options.reversible_transform = ReversibleTransform::None53;
                    let input = J2kLosslessSamples::new(samples, size, size, 3, 8, false)
                        .expect("lossless samples");
                    j2k::encode_j2k_lossless(input, &options.with_cpu_only_backend())
                        .expect("lossless JPEG 2000")
                        .codestream
                })
                .collect(),
            _ => {
                let bytes: Vec<u8> = frames.into_iter().flatten().collect();
                object.put(DataElement::new(
                    tags::PIXEL_DATA,
                    VR::OB,
                    PrimitiveValue::U8(even(bytes).into()),
                ));
                return;
            }
        };
        let fragments: Vec<Vec<u8>> = fragments.into_iter().map(even).collect();
        let mut offsets = Vec::with_capacity(fragments.len());
        let mut offset = 0u32;
        for fragment in &fragments {
            offsets.push(offset);
            offset += 8 + fragment.len() as u32;
        }
        object.put(DataElement::new(
            tags::PIXEL_DATA,
            VR::OB,
            PixelFragmentSequence::new(offsets, fragments),
        ));
    }
}

/// The three VOLUME layers, the LABEL and the OVERVIEW image of a scan.
pub(super) fn images() -> Vec<Image> {
    vec![
        Image::volume(1, 32, 0.0005),
        Image::volume(2, 16, 0.001),
        Image::volume(3, 8, 0.002),
        Image::associated(4, Flavor::Label),
        Image::associated(5, Flavor::Overview),
    ]
}

pub(super) fn patient_study(
    object: &mut InMemDicomObject,
    slide: &Slide,
    series: &str,
    modality: &str,
) {
    for (tag, vr, value) in [
        (tags::PATIENT_NAME, VR::PN, "FIXTURE^SLIDE"),
        (tags::PATIENT_ID, VR::LO, "FIXTURE"),
        (tags::PATIENT_BIRTH_DATE, VR::DA, ""),
        (tags::PATIENT_SEX, VR::CS, ""),
        (tags::STUDY_INSTANCE_UID, VR::UI, &slide.study),
        (tags::STUDY_DATE, VR::DA, "20260102"),
        (tags::STUDY_TIME, VR::TM, "101500"),
        (tags::REFERRING_PHYSICIAN_NAME, VR::PN, ""),
        (tags::STUDY_ID, VR::SH, "1"),
        (tags::ACCESSION_NUMBER, VR::SH, ACCESSION),
        (tags::MODALITY, VR::CS, modality),
        (tags::SERIES_INSTANCE_UID, VR::UI, series),
        (tags::SERIES_NUMBER, VR::IS, "1"),
    ] {
        put(object, tag, vr, value);
    }
    object.put(accession_issuer());
}

fn accession_issuer() -> InMemElement {
    sq(
        tags::ISSUER_OF_ACCESSION_NUMBER_SEQUENCE,
        vec![item(vec![text(
            tags::LOCAL_NAMESPACE_ENTITY_ID,
            VR::UT,
            "FIXTURE-LIS",
        )])],
    )
}

/// The DPIA Appendix B Request Attributes Sequence of an image.
fn request_attributes(object: &mut InMemDicomObject, slide: &Slide) {
    object.put(sq(
        tags::REQUEST_ATTRIBUTES_SEQUENCE,
        vec![item(vec![
            text(tags::ACCESSION_NUMBER, VR::SH, ACCESSION),
            accession_issuer(),
            text(tags::STUDY_INSTANCE_UID, VR::UI, &slide.study),
        ])],
    ));
}

fn equipment(object: &mut InMemDicomObject, manufacturer: &str, model: &str) {
    for (tag, vr, value) in [
        (tags::MANUFACTURER, VR::LO, manufacturer),
        (tags::MANUFACTURER_MODEL_NAME, VR::LO, model),
        (tags::DEVICE_SERIAL_NUMBER, VR::LO, "SN1"),
        (tags::SOFTWARE_VERSIONS, VR::LO, "1.0"),
        (tags::CONTENT_DATE, VR::DA, "20260102"),
        (tags::CONTENT_TIME, VR::TM, "101500"),
    ] {
        put(object, tag, vr, value);
    }
}

/// One TID 8001 Content Item.
pub(super) fn content(
    value_type: &str,
    concept: InMemDicomObject,
    value: InMemElement,
) -> InMemDicomObject {
    item(vec![
        text(tags::VALUE_TYPE, VR::CS, value_type),
        sq(tags::CONCEPT_NAME_CODE_SEQUENCE, vec![concept]),
        value,
    ])
}

pub(super) fn coded(concept: InMemDicomObject, value: InMemDicomObject) -> InMemDicomObject {
    content(
        "CODE",
        concept,
        sq(tags::CONCEPT_CODE_SEQUENCE, vec![value]),
    )
}

/// A preparation step of Processing type `processing` with `items` after
/// the Specimen Identifier, its Issuer and the Processing type.
pub(super) fn step(processing: InMemDicomObject, items: Vec<InMemDicomObject>) -> InMemDicomObject {
    let mut content_items = vec![
        content(
            "TEXT",
            code("DCM", "121041", "Specimen Identifier"),
            text(tags::TEXT_VALUE, VR::UT, SPECIMEN),
        ),
        content(
            "TEXT",
            code("DCM", "111724", "Issuer of Specimen Identifier"),
            text(tags::TEXT_VALUE, VR::UT, SPECIMEN_ISSUER),
        ),
        coded(code("DCM", "111701", "Processing type"), processing),
    ];
    content_items.extend(items);
    item(vec![sq(
        tags::SPECIMEN_PREPARATION_STEP_CONTENT_ITEM_SEQUENCE,
        content_items,
    )])
}

/// Collection, fixation, embedding and H&E staining, as DPIA Table B.2-1.
pub(super) fn preparation() -> Vec<InMemDicomObject> {
    let processing = || code("SCT", "9265001", "Specimen processing");
    let staining = |value: &str, meaning: &str| {
        step(
            code("SCT", "127790008", "Staining"),
            vec![coded(
                code("SCT", "424361007", "Using substance"),
                code("SCT", value, meaning),
            )],
        )
    };
    vec![
        step(
            code("SCT", "17636008", "Specimen collection"),
            vec![
                content(
                    "DATETIME",
                    code("DCM", "111702", "DateTime of processing"),
                    text(tags::DATE_TIME, VR::DT, "20260101093000"),
                ),
                coded(
                    code("SCT", "17636008", "Specimen Collection"),
                    code("SCT", "65801008", "Excision"),
                ),
            ],
        ),
        step(
            processing(),
            vec![coded(
                code("SCT", "430864009", "Tissue Fixative"),
                code("SCT", "431510009", "Formalin"),
            )],
        ),
        step(
            processing(),
            vec![coded(
                code("SCT", "430863003", "Embedding medium"),
                code("SCT", "311731000", "Paraffin wax"),
            )],
        ),
        staining("12710003", "Hematoxylin stain"),
        staining("36879007", "Water soluble eosin stain"),
    ]
}

pub(super) fn specimen_description(slide: &Slide) -> InMemDicomObject {
    item(vec![
        text(tags::SPECIMEN_IDENTIFIER, VR::LO, SPECIMEN),
        text(tags::SPECIMEN_UID, VR::UI, &slide.specimen),
        sq(
            tags::ISSUER_OF_THE_SPECIMEN_IDENTIFIER_SEQUENCE,
            vec![item(vec![text(
                tags::LOCAL_NAMESPACE_ENTITY_ID,
                VR::UT,
                SPECIMEN_ISSUER,
            )])],
        ),
        sq(
            tags::SPECIMEN_TYPE_CODE_SEQUENCE,
            vec![code("SCT", "119376003", "Tissue specimen")],
        ),
        sq(tags::SPECIMEN_PREPARATION_SEQUENCE, preparation()),
        sq(
            tags::PRIMARY_ANATOMIC_STRUCTURE_SEQUENCE,
            vec![code("SCT", "71854001", "Colon")],
        ),
    ])
}

fn specimen(object: &mut InMemDicomObject, slide: &Slide) {
    put(object, tags::CONTAINER_IDENTIFIER, VR::LO, CONTAINER);
    object.put(sq(
        tags::ISSUER_OF_THE_CONTAINER_IDENTIFIER_SEQUENCE,
        Vec::new(),
    ));
    object.put(sq(
        tags::CONTAINER_TYPE_CODE_SEQUENCE,
        vec![code("SCT", "433466003", "Microscope slide")],
    ));
    object.put(sq(tags::ACQUISITION_CONTEXT_SEQUENCE, Vec::new()));
    object.put(sq(
        tags::SPECIMEN_DESCRIPTION_SEQUENCE,
        vec![specimen_description(slide)],
    ));
}

/// A 2D annotation of the base layer by another manufacturer's tool.
pub(super) fn annotation(slide: &Slide) -> DefaultDicomObject {
    let mut object = InMemDicomObject::new_empty();
    put(
        &mut object,
        tags::SOP_CLASS_UID,
        VR::UI,
        uids::MICROSCOPY_BULK_SIMPLE_ANNOTATIONS_STORAGE,
    );
    put(
        &mut object,
        tags::SOP_INSTANCE_UID,
        VR::UI,
        &slide.instance_uid(20),
    );
    patient_study(&mut object, slide, &format!("{}.20", slide.series), "ANN");
    put(&mut object, tags::LATERALITY, VR::CS, "");
    equipment(&mut object, "Fixture Annotator Co", "A1");
    for (tag, vr, value) in [
        (tags::INSTANCE_NUMBER, VR::IS, "1"),
        (tags::CONTENT_LABEL, VR::CS, "NUCLEI"),
        (tags::CONTENT_DESCRIPTION, VR::LO, ""),
        (tags::CONTENT_CREATOR_NAME, VR::PN, ""),
        (tags::ANNOTATION_COORDINATE_TYPE, VR::CS, "2D"),
        (tags::PIXEL_ORIGIN_INTERPRETATION, VR::CS, "VOLUME"),
    ] {
        put(&mut object, tag, vr, value);
    }
    let base = || {
        item(vec![
            text(
                tags::REFERENCED_SOP_CLASS_UID,
                VR::UI,
                crate::VL_WSI_SOP_CLASS_UID,
            ),
            text(
                tags::REFERENCED_SOP_INSTANCE_UID,
                VR::UI,
                &slide.instance_uid(1),
            ),
        ])
    };
    object.put(sq(tags::REFERENCED_IMAGE_SEQUENCE, vec![base()]));
    object.put(sq(
        tags::REFERENCED_SERIES_SEQUENCE,
        vec![item(vec![
            text(tags::SERIES_INSTANCE_UID, VR::UI, &slide.series),
            sq(tags::REFERENCED_INSTANCE_SEQUENCE, vec![base()]),
        ])],
    ));
    object.put(sq(
        tags::ANNOTATION_GROUP_SEQUENCE,
        vec![item(vec![
            DataElement::new(
                tags::ANNOTATION_GROUP_NUMBER,
                VR::US,
                PrimitiveValue::from(1u16),
            ),
            text(
                tags::ANNOTATION_GROUP_UID,
                VR::UI,
                &format!("{}.21", slide.study),
            ),
            text(tags::ANNOTATION_GROUP_LABEL, VR::LO, "nucleus"),
            text(tags::ANNOTATION_GROUP_GENERATION_TYPE, VR::CS, "MANUAL"),
            sq(
                tags::ANNOTATION_PROPERTY_CATEGORY_CODE_SEQUENCE,
                vec![code("SCT", "91723000", "Anatomical structure")],
            ),
            sq(
                tags::ANNOTATION_PROPERTY_TYPE_CODE_SEQUENCE,
                vec![code("SCT", "84640000", "Nucleus")],
            ),
            long(tags::NUMBER_OF_ANNOTATIONS, 1),
            text(tags::GRAPHIC_TYPE, VR::CS, "POINT"),
            text(tags::ANNOTATION_APPLIES_TO_ALL_OPTICAL_PATHS, VR::CS, "YES"),
            DataElement::new(
                tags::POINT_COORDINATES_DATA,
                VR::OF,
                PrimitiveValue::F32([4.0f32, 4.0].into_iter().collect()),
            ),
        ])],
    ));
    with_meta(
        object,
        uids::MICROSCOPY_BULK_SIMPLE_ANNOTATIONS_STORAGE,
        uids::EXPLICIT_VR_LITTLE_ENDIAN,
    )
}

/// A Key Object Selection titled (113034, DCM, "Complete Acquisition
/// Content") that lists `images`, in a new Series of the Study.
pub(super) fn completion(slide: &Slide, images: &[Image]) -> DefaultDicomObject {
    let mut object = InMemDicomObject::new_empty();
    put(
        &mut object,
        tags::SOP_CLASS_UID,
        VR::UI,
        uids::KEY_OBJECT_SELECTION_DOCUMENT_STORAGE,
    );
    put(
        &mut object,
        tags::SOP_INSTANCE_UID,
        VR::UI,
        &slide.instance_uid(30),
    );
    patient_study(&mut object, slide, &format!("{}.30", slide.series), "KO");
    object.put(sq(
        tags::REFERENCED_PERFORMED_PROCEDURE_STEP_SEQUENCE,
        Vec::new(),
    ));
    put(
        &mut object,
        tags::MANUFACTURER,
        VR::LO,
        "Fixture Scanner Co",
    );
    for (tag, vr, value) in [
        (tags::INSTANCE_NUMBER, VR::IS, "1"),
        (tags::CONTENT_DATE, VR::DA, "20260102"),
        (tags::CONTENT_TIME, VR::TM, "101600"),
        (tags::VALUE_TYPE, VR::CS, "CONTAINER"),
        (tags::CONTINUITY_OF_CONTENT, VR::CS, "SEPARATE"),
    ] {
        put(&mut object, tag, vr, value);
    }
    // The root is constructed from TID 2010 Key Object Selection.
    object.put(sq(
        tags::CONTENT_TEMPLATE_SEQUENCE,
        vec![item(vec![
            text(tags::MAPPING_RESOURCE, VR::CS, "DCMR"),
            text(tags::TEMPLATE_IDENTIFIER, VR::CS, "2010"),
        ])],
    ));
    object.put(sq(
        tags::CONCEPT_NAME_CODE_SEQUENCE,
        vec![code("DCM", "113034", "Complete Acquisition Content")],
    ));
    let reference = |image: &Image| {
        item(vec![
            text(
                tags::REFERENCED_SOP_CLASS_UID,
                VR::UI,
                crate::VL_WSI_SOP_CLASS_UID,
            ),
            text(
                tags::REFERENCED_SOP_INSTANCE_UID,
                VR::UI,
                &slide.instance_uid(image.instance),
            ),
        ])
    };
    object.put(sq(
        tags::CONTENT_SEQUENCE,
        images
            .iter()
            .map(|image| {
                item(vec![
                    text(tags::RELATIONSHIP_TYPE, VR::CS, "CONTAINS"),
                    text(tags::VALUE_TYPE, VR::CS, "IMAGE"),
                    sq(tags::REFERENCED_SOP_SEQUENCE, vec![reference(image)]),
                ])
            })
            .collect(),
    ));
    object.put(sq(
        tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE,
        vec![item(vec![
            text(tags::STUDY_INSTANCE_UID, VR::UI, &slide.study),
            sq(
                tags::REFERENCED_SERIES_SEQUENCE,
                vec![item(vec![
                    text(tags::SERIES_INSTANCE_UID, VR::UI, &slide.series),
                    sq(
                        tags::REFERENCED_SOP_SEQUENCE,
                        images.iter().map(reference).collect(),
                    ),
                ])],
            ),
        ])],
    ));
    put(&mut object, tags::CONTAINER_IDENTIFIER, VR::LO, CONTAINER);
    // The images' Specimen Description, complete as the Specimen Module
    // requires of it (Standard Extended in the KOS).
    object.put(sq(
        tags::SPECIMEN_DESCRIPTION_SEQUENCE,
        vec![specimen_description(slide)],
    ));
    with_meta(
        object,
        uids::KEY_OBJECT_SELECTION_DOCUMENT_STORAGE,
        uids::EXPLICIT_VR_LITTLE_ENDIAN,
    )
}

fn with_meta(object: InMemDicomObject, sop_class: &str, syntax: &str) -> DefaultDicomObject {
    let instance = object
        .element(tags::SOP_INSTANCE_UID)
        .expect("SOP Instance UID")
        .to_str()
        .expect("SOP Instance UID text")
        .into_owned();
    object
        .with_meta(
            FileMetaTableBuilder::new()
                .media_storage_sop_class_uid(sop_class)
                .media_storage_sop_instance_uid(instance)
                .transfer_syntax(syntax),
        )
        .expect("file meta")
}

/// Writes `object` to `dir/<name>.dcm`.
pub(super) fn write(object: &DefaultDicomObject, dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(format!("{name}.dcm"));
    object.write_to_file(&path).expect("write fixture");
    path
}

fn even(mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.len() % 2 == 1 {
        bytes.push(0);
    }
    bytes
}

pub(super) fn put(object: &mut InMemDicomObject, tag: Tag, vr: VR, value: &str) {
    object.put(text(tag, vr, value));
}

pub(super) fn text(tag: Tag, vr: VR, value: &str) -> InMemElement {
    if value.is_empty() {
        DataElement::new(tag, vr, PrimitiveValue::Empty)
    } else if matches!(vr, VR::LT | VR::UT | VR::ST) {
        DataElement::new(tag, vr, PrimitiveValue::from(value))
    } else {
        let values: Vec<String> = value.split('\\').map(str::to_string).collect();
        DataElement::new(tag, vr, PrimitiveValue::Strs(values.into()))
    }
}

pub(super) fn unsigned(tag: Tag, value: u16) -> InMemElement {
    DataElement::new(tag, VR::US, PrimitiveValue::from(value))
}

pub(super) fn long(tag: Tag, value: u32) -> InMemElement {
    DataElement::new(tag, VR::UL, PrimitiveValue::from(value))
}

fn float(tag: Tag, value: f32) -> InMemElement {
    DataElement::new(tag, VR::FL, PrimitiveValue::from(value))
}

pub(super) fn item(elements: Vec<InMemElement>) -> InMemDicomObject {
    InMemDicomObject::from_element_iter(elements)
}

pub(super) fn sq(tag: Tag, items: Vec<InMemDicomObject>) -> InMemElement {
    DataElement::new(tag, VR::SQ, DataSetSequence::from(items))
}

pub(super) fn code(scheme: &str, value: &str, meaning: &str) -> InMemDicomObject {
    item(vec![
        text(tags::CODE_VALUE, VR::SH, value),
        text(tags::CODING_SCHEME_DESIGNATOR, VR::SH, scheme),
        text(tags::CODE_MEANING, VR::LO, meaning),
    ])
}
