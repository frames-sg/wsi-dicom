//! Unit tests for the requirement engine; integration tests that validate
//! exported files live in src/validation/tests/iod.rs.

use dicom_core::value::DataSetSequence;
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::tags;
use dicom_object::mem::InMemElement;
use dicom_object::InMemDicomObject;
use sha2::{Digest, Sha256};

use super::conditions::evaluate;
use super::model::{Placement, Usage, ValueMultiplicity};
use super::scope::{Frame, Requirement, Scope};
use super::tables::{ConditionId as C, EDITION, IODS, SOURCE_SHA256};
use super::values::{check_value, entry};

const EXPLICIT_VR_LITTLE_ENDIAN: &str = "1.2.840.10008.1.2.1";

fn text(tag: Tag, vr: VR, values: &[&str]) -> InMemElement {
    DataElement::new(
        tag,
        vr,
        PrimitiveValue::Strs(values.iter().map(|value| value.to_string()).collect()),
    )
}

fn unsigned(tag: Tag, value: u16) -> InMemElement {
    DataElement::new(tag, VR::US, PrimitiveValue::from(value))
}

fn sequence(tag: Tag, items: Vec<InMemDicomObject>) -> InMemElement {
    DataElement::new(tag, VR::SQ, DataSetSequence::from(items))
}

fn data_set(elements: Vec<InMemElement>) -> InMemDicomObject {
    InMemDicomObject::from_element_iter(elements)
}

fn image(flavor: &str) -> InMemDicomObject {
    data_set(vec![text(
        tags::IMAGE_TYPE,
        VR::CS,
        &["ORIGINAL", "PRIMARY", flavor, "NONE"],
    )])
}

fn at_root(id: C, root: &InMemDicomObject) -> Requirement {
    evaluate(id, &Scope::top(root, EXPLICIT_VR_LITTLE_ENDIAN))
}

fn in_item(id: C, root: &InMemDicomObject, item: &InMemDicomObject) -> Requirement {
    let top = Scope::top(root, EXPLICIT_VR_LITTLE_ENDIAN);
    evaluate(id, &top.child(item))
}

#[track_caller]
fn expect(id: C, root: &InMemDicomObject, expected: Requirement) {
    assert_eq!(at_root(id, root), expected, "{id:?}");
}

#[track_caller]
fn expect_in_item(id: C, root: &InMemDicomObject, item: &InMemDicomObject, expected: Requirement) {
    assert_eq!(in_item(id, root, item), expected, "{id:?}");
}

fn photometric(value: &str) -> InMemDicomObject {
    data_set(vec![text(
        tags::PHOTOMETRIC_INTERPRETATION,
        VR::CS,
        &[value],
    )])
}

#[test]
fn image_flavor_conditions_follow_image_type_value_three() {
    let volume = image("VOLUME");
    let label = image("LABEL");
    let overview = image("OVERVIEW");
    expect(C::ImagedVolumeWidth_96eff9, &volume, Requirement::Required);
    expect(C::ImagedVolumeWidth_96eff9, &label, Requirement::Optional);
    expect(C::ModuleSlideLabel_82fe14, &label, Requirement::Required);
    expect(C::ModuleSlideLabel_82fe14, &volume, Requirement::Optional);
    expect(
        C::ModuleFrameOfReference_10a326,
        &image("THUMBNAIL"),
        Requirement::Required,
    );
    expect(
        C::ModuleFrameOfReference_10a326,
        &overview,
        Requirement::Optional,
    );
    expect(
        C::ModuleMultiResolutionPyramid_c66402,
        &overview,
        Requirement::Forbidden,
    );
    expect(
        C::ModuleMultiResolutionPyramid_c66402,
        &volume,
        Requirement::Optional,
    );
    // A conditional Enumerated Values list "applies" (Required) or not.
    expect(C::NumberOfFrames_8113e0, &label, Requirement::Required);
    expect(C::NumberOfFrames_8113e0, &volume, Requirement::Optional);
}

#[test]
fn conditions_without_an_otherwise_clause_forbid_presence() {
    let color = data_set(vec![unsigned(tags::SAMPLES_PER_PIXEL, 3)]);
    let gray = data_set(vec![unsigned(tags::SAMPLES_PER_PIXEL, 1)]);
    expect(C::PlanarConfiguration_290ffd, &color, Requirement::Required);
    expect(C::PlanarConfiguration_290ffd, &gray, Requirement::Forbidden);

    let lossy = data_set(vec![text(tags::LOSSY_IMAGE_COMPRESSION, VR::CS, &["01"])]);
    let lossless = data_set(vec![text(tags::LOSSY_IMAGE_COMPRESSION, VR::CS, &["00"])]);
    expect(
        C::LossyImageCompressionRatio_84e963,
        &lossy,
        Requirement::Required,
    );
    expect(
        C::LossyImageCompressionRatio_84e963,
        &lossless,
        Requirement::Forbidden,
    );

    let mono = photometric("MONOCHROME2");
    let rgb = photometric("RGB");
    expect(C::PresentationLUTShape_dde08b, &mono, Requirement::Required);
    expect(C::PresentationLUTShape_dde08b, &rgb, Requirement::Forbidden);
}

#[test]
fn every_jpip_referenced_syntax_replaces_pixel_data_with_its_url() {
    // PS3.3 Table C.7-11a names only JPIP Referenced (Deflate); PS3.5 A.11
    // and A.12 encode the JPIP HTJ2K Referenced syntaxes the same way.
    let url = data_set(vec![text(
        tags::PIXEL_DATA_PROVIDER_URL,
        VR::UR,
        &["https://example.org/jpip?target=slide.jph"],
    )]);
    let in_syntax = |id, root, syntax| evaluate(id, &Scope::top(root, syntax));
    for syntax in [
        "1.2.840.10008.1.2.4.94",
        "1.2.840.10008.1.2.4.95",
        "1.2.840.10008.1.2.4.204",
        "1.2.840.10008.1.2.4.205",
    ] {
        assert_eq!(
            in_syntax(C::PixelDataProviderURL_83d79f, &url, syntax),
            Requirement::Required,
            "{syntax}"
        );
        assert_eq!(
            in_syntax(C::PixelData_f3a5dd, &url, syntax),
            Requirement::Forbidden,
            "{syntax}"
        );
    }
    for syntax in [EXPLICIT_VR_LITTLE_ENDIAN, "1.2.840.10008.1.2.4.203"] {
        assert_eq!(
            in_syntax(C::PixelDataProviderURL_83d79f, &url, syntax),
            Requirement::Forbidden,
            "{syntax}"
        );
    }
}

#[test]
fn may_be_present_otherwise_conditions_never_forbid() {
    let sparse = data_set(vec![text(
        tags::DIMENSION_ORGANIZATION_TYPE,
        VR::CS,
        &["TILED_SPARSE"],
    )]);
    let full = data_set(vec![text(
        tags::DIMENSION_ORGANIZATION_TYPE,
        VR::CS,
        &["TILED_FULL"],
    )]);
    expect(
        C::FgPlanePositionSlide_dc3941,
        &sparse,
        Requirement::Required,
    );
    expect(C::FgPlanePositionSlide_dc3941, &full, Requirement::Optional);
    expect(
        C::TotalPixelMatrixFocalPlanes_d443dd,
        &full,
        Requirement::Required,
    );
    expect(
        C::TotalPixelMatrixFocalPlanes_d443dd,
        &sparse,
        Requirement::Optional,
    );
    expect(
        C::DimensionIndexSequence_aaaa80,
        &data_set(vec![]),
        Requirement::Required,
    );
    expect(
        C::DimensionIndexSequence_aaaa80,
        &full,
        Requirement::Optional,
    );
    // Image Orientation (Slide) is required for TILED_FULL, so Patient
    // Orientation is not (see the predicate's reading of the condition).
    expect(C::PatientOrientation_964c1d, &full, Requirement::Optional);

    let root = data_set(vec![]);
    let with_color = data_set(vec![sequence(
        tags::ILLUMINATION_COLOR_CODE_SEQUENCE,
        vec![data_set(vec![])],
    )]);
    expect_in_item(
        C::IlluminationWaveLength_66d7d2,
        &root,
        &data_set(vec![]),
        Requirement::Required,
    );
    expect_in_item(
        C::IlluminationWaveLength_66d7d2,
        &root,
        &with_color,
        Requirement::Optional,
    );
}

#[test]
fn facts_the_object_cannot_carry_are_undeterminable() {
    let root = data_set(vec![]);
    expect(
        C::PatientSpeciesDescription_6364e6,
        &root,
        Requirement::Undeterminable,
    );
    let with_code = data_set(vec![sequence(
        tags::PATIENT_SPECIES_CODE_SEQUENCE,
        vec![data_set(vec![])],
    )]);
    expect(
        C::PatientSpeciesDescription_6364e6,
        &with_code,
        Requirement::Optional,
    );
    expect(
        C::ModuleFrameExtraction_221a2c,
        &root,
        Requirement::Undeterminable,
    );
    expect_in_item(
        C::ChannelDescriptionCodeSequence_5684e2,
        &root,
        &data_set(vec![]),
        Requirement::Undeterminable,
    );
}

#[test]
fn a_coded_species_decides_whether_the_patient_is_non_human() {
    let species = |scheme: &str, value: &str| {
        data_set(vec![sequence(
            tags::PATIENT_SPECIES_CODE_SEQUENCE,
            vec![data_set(vec![
                text(tags::CODE_VALUE, VR::SH, &[value]),
                text(tags::CODING_SCHEME_DESIGNATOR, VR::SH, &[scheme]),
            ])],
        )])
    };
    let mouse = species("SCT", "447612001");
    let human = species("SCT", "337915000");
    let legacy_human = species("SRT", "L-85B00");
    expect(
        C::PatientBreedCodeSequence_878016,
        &mouse,
        Requirement::Required,
    );
    expect(
        C::ResponsibleOrganization_a81ee5,
        &mouse,
        Requirement::Required,
    );
    for human in [&human, &legacy_human] {
        expect(
            C::PatientBreedCodeSequence_878016,
            human,
            Requirement::Forbidden,
        );
        expect(
            C::ResponsibleOrganization_a81ee5,
            human,
            Requirement::Optional,
        );
    }
}

#[test]
fn code_sequence_identifier_conditions_partition_code_values() {
    let root = data_set(vec![]);
    let short_long = data_set(vec![text(tags::LONG_CODE_VALUE, VR::UC, &["12345"])]);
    let long_long = data_set(vec![text(
        tags::LONG_CODE_VALUE,
        VR::UC,
        &["12345678901234567890"],
    )]);
    let urn = data_set(vec![text(tags::URN_CODE_VALUE, VR::UR, &["urn:oid:1.2.3"])]);
    let code_value = data_set(vec![text(tags::CODE_VALUE, VR::SH, &["433466003"])]);
    let empty = data_set(vec![]);
    // A short, non-URN identifier belongs in Code Value.
    expect_in_item(
        C::CodeValue_c75d2f,
        &root,
        &short_long,
        Requirement::Required,
    );
    expect_in_item(
        C::CodeValue_c75d2f,
        &root,
        &long_long,
        Requirement::Forbidden,
    );
    expect_in_item(C::CodeValue_c75d2f, &root, &empty, Requirement::Required);
    expect_in_item(
        C::LongCodeValue_318ae8,
        &root,
        &code_value,
        Requirement::Forbidden,
    );
    expect_in_item(
        C::LongCodeValue_318ae8,
        &root,
        &long_long,
        Requirement::Required,
    );
    expect_in_item(C::URNCodeValue_3efa2e, &root, &urn, Requirement::Required);
    expect_in_item(
        C::URNCodeValue_3efa2e,
        &root,
        &long_long,
        Requirement::Forbidden,
    );
    expect_in_item(
        C::CodingSchemeDesignator_117155,
        &root,
        &code_value,
        Requirement::Required,
    );
}

#[test]
fn item_conditions_read_root_and_item_attributes() {
    let rgb = photometric("RGB");
    let mono = photometric("MONOCHROME2");
    let palette = data_set(vec![sequence(
        tags::PALETTE_COLOR_LOOKUP_TABLE_SEQUENCE,
        vec![data_set(vec![])],
    )]);
    let plain = data_set(vec![]);
    expect_in_item(C::ICCProfile_3b45af, &rgb, &plain, Requirement::Required);
    expect_in_item(C::ICCProfile_3b45af, &mono, &plain, Requirement::Forbidden);
    expect_in_item(C::ICCProfile_3b45af, &mono, &palette, Requirement::Required);

    let with_pixels = data_set(vec![
        DataElement::new(tags::PIXEL_DATA, VR::OB, PrimitiveValue::from(vec![0u8; 2])),
        unsigned(tags::PIXEL_PADDING_RANGE_LIMIT, 0),
    ]);
    expect(
        C::PixelPaddingValue_894c69,
        &with_pixels,
        Requirement::Required,
    );
    expect(C::PixelPaddingValue_894c69, &plain, Requirement::Forbidden);

    let meta = data_set(vec![text(
        tags::PRIVATE_INFORMATION_CREATOR_UID,
        VR::UI,
        &["1.2.3"],
    )]);
    expect(C::PrivateInformation_5c0ac1, &meta, Requirement::Required);
    expect(C::PrivateInformation_5c0ac1, &plain, Requirement::Forbidden);
}

#[test]
fn frame_conditions_read_this_frame_and_the_sop_class() {
    let frame_type = |value: &str| {
        data_set(vec![sequence(
            tags::WHOLE_SLIDE_MICROSCOPY_IMAGE_FRAME_TYPE_SEQUENCE,
            vec![data_set(vec![text(
                tags::FRAME_TYPE,
                VR::CS,
                &[value, "PRIMARY", "VOLUME", "NONE"],
            )])],
        )])
    };
    let root_of = |sop_class: &str| {
        data_set(vec![
            text(tags::SOP_CLASS_UID, VR::UI, &[sop_class]),
            text(tags::DIMENSION_ORGANIZATION_TYPE, VR::CS, &["TILED_SPARSE"]),
        ])
    };
    let evaluate_in_frame = |root: &InMemDicomObject, shared: &InMemDicomObject| {
        let top = Scope::top(root, EXPLICIT_VR_LITTLE_ENDIAN);
        let item = data_set(vec![]);
        let scope = top.frame_item(
            &item,
            Frame {
                shared: Some(shared),
                per_frame: None,
            },
        );
        evaluate(C::FrameReferenceDateTime_b677d6, &scope)
    };
    let original = frame_type("ORIGINAL");
    let derived = frame_type("DERIVED");
    // VL Whole Slide Microscopy is excluded by the condition itself.
    let wsi = root_of("1.2.840.10008.5.1.4.1.1.77.1.6");
    assert_eq!(evaluate_in_frame(&wsi, &original), Requirement::Optional);
    let other = root_of("1.2.840.10008.5.1.4.1.1.2.1");
    assert_eq!(evaluate_in_frame(&other, &original), Requirement::Required);
    assert_eq!(evaluate_in_frame(&other, &derived), Requirement::Optional);
}

fn fixtures() -> Vec<InMemDicomObject> {
    vec![
        data_set(vec![]),
        data_set(vec![
            text(
                tags::SOP_CLASS_UID,
                VR::UI,
                &["1.2.840.10008.5.1.4.1.1.77.1.6"],
            ),
            text(
                tags::IMAGE_TYPE,
                VR::CS,
                &["ORIGINAL", "PRIMARY", "VOLUME", "NONE"],
            ),
            text(tags::PHOTOMETRIC_INTERPRETATION, VR::CS, &["RGB"]),
            unsigned(tags::SAMPLES_PER_PIXEL, 3),
            text(tags::DIMENSION_ORGANIZATION_TYPE, VR::CS, &["TILED_FULL"]),
            text(tags::LOSSY_IMAGE_COMPRESSION, VR::CS, &["00"]),
        ]),
        data_set(vec![
            text(
                tags::IMAGE_TYPE,
                VR::CS,
                &["DERIVED", "PRIMARY", "LABEL", "NONE"],
            ),
            text(tags::PHOTOMETRIC_INTERPRETATION, VR::CS, &["MONOCHROME2"]),
            unsigned(tags::SAMPLES_PER_PIXEL, 1),
            text(tags::DIMENSION_ORGANIZATION_TYPE, VR::CS, &["TILED_SPARSE"]),
        ]),
    ]
}

#[test]
fn every_condition_evaluates_and_honours_may_be_present_otherwise() {
    for id in C::ALL {
        let text = id.text().to_ascii_lowercase();
        let unconditional_otherwise = text.contains("may be present otherwise")
            && !text.contains("only if")
            && !text.contains("shall not be present")
            && !text.contains("otherwise, if")
            && !text.contains("otherwise if");
        for root in fixtures() {
            for requirement in [at_root(*id, &root), in_item(*id, &root, &data_set(vec![]))] {
                if unconditional_otherwise {
                    assert_ne!(
                        requirement,
                        Requirement::Forbidden,
                        "{id:?} says \"may be present otherwise\": {}",
                        id.text()
                    );
                }
            }
        }
    }
}

#[test]
fn condition_variants_are_named_by_a_digest_of_their_text() {
    let mut seen = std::collections::BTreeSet::new();
    for id in C::ALL {
        assert!(seen.insert(id.text()), "duplicate condition text {id:?}");
        let digest = Sha256::digest(id.text().as_bytes());
        let suffix: String = digest[..3]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let name = format!("{id:?}");
        assert!(
            name.ends_with(&format!("_{suffix}")),
            "{name} does not end with the digest of its text"
        );
    }
}

fn iod(section: &str) -> &'static super::model::Iod {
    IODS.iter()
        .find(|iod| iod.section == section)
        .unwrap_or_else(|| panic!("no IOD {section}"))
}

#[test]
fn tables_cover_the_iod_module_and_functional_group_tables() {
    assert_eq!(EDITION, "2026d");
    let wsi = iod("A.32.8");
    assert_eq!(wsi.modules.len(), 27);
    assert_eq!(wsi.functional_groups.len(), 9);
    let slide_label = wsi
        .modules
        .iter()
        .find(|module| module.name == "Slide Label")
        .unwrap();
    assert_eq!(slide_label.usage, Usage::Conditional);
    assert_eq!(slide_label.condition, Some(C::ModuleSlideLabel_82fe14));
    let pixel_measures = wsi
        .functional_groups
        .iter()
        .find(|group| group.name == "Pixel Measures")
        .unwrap();
    assert_eq!(pixel_measures.usage, Usage::Mandatory);
    assert_eq!(pixel_measures.placement, Placement::SharedOnly);
    assert_eq!(pixel_measures.sequences, &[0x0028_9110]);
    assert!(SOURCE_SHA256
        .iter()
        .all(|(name, digest)| name.ends_with(".xml") && digest.len() == 64));
    assert!(SOURCE_SHA256.iter().any(|(name, _)| *name == "part04.xml"));
}

#[test]
fn every_pathology_slide_iod_is_dispatched_by_its_ps3_4_sop_classes() {
    let sections: Vec<&str> = IODS.iter().map(|iod| iod.section).collect();
    assert_eq!(
        sections,
        ["A.32.8", "A.32.2", "A.32.3", "A.90.1", "A.90.2", "A.87", "A.51", "A.75"]
    );
    let uids = |section: &str| {
        iod(section)
            .sop_classes
            .iter()
            .map(|class| class.uid)
            .collect::<Vec<_>>()
    };
    assert_eq!(uids("A.32.8"), [crate::VL_WSI_SOP_CLASS_UID]);
    assert_eq!(
        uids("A.51"),
        [
            "1.2.840.10008.5.1.4.1.1.66.4",
            "1.2.840.10008.5.1.4.1.1.66.7"
        ]
    );
    assert_eq!(uids("A.87"), ["1.2.840.10008.5.1.4.1.1.91.1"]);
    // Height Map Segmentation (A.91) is not a pathology-slide IOD.
    assert!(IODS.iter().all(|iod| iod
        .sop_classes
        .iter()
        .all(|class| class.uid != "1.2.840.10008.5.1.4.1.1.66.8")));
    assert_eq!(iod("A.32.2").functional_group_table, None);
    assert!(iod("A.32.2").functional_groups.is_empty());
    let forbidden: Vec<&str> = iod("A.51")
        .forbidden_modules
        .iter()
        .map(|module| module.name)
        .collect();
    assert_eq!(forbidden, ["VOI LUT", "Modality LUT", "Overlay Plane"]);
    assert_eq!(
        iod("A.75").exclusive_modules[0].modules,
        [
            "Image Pixel",
            "Floating Point Image Pixel",
            "Double Floating Point Image Pixel"
        ]
    );
}

fn segmentation_root(segmentation_type: &str, extra: Vec<InMemElement>) -> InMemDicomObject {
    let mut root = data_set(vec![
        text(
            tags::SOP_CLASS_UID,
            VR::UI,
            &["1.2.840.10008.5.1.4.1.1.66.4"],
        ),
        text(tags::SEGMENTATION_TYPE, VR::CS, &[segmentation_type]),
    ]);
    for element in extra {
        root.put(element);
    }
    root
}

#[test]
fn segmentation_conditions_follow_the_segmentation_type_and_sop_class() {
    let binary = segmentation_root("BINARY", vec![]);
    let label_map = segmentation_root("LABELMAP", vec![]);
    expect(C::SegmentationType_c84858, &binary, Requirement::Required);
    expect(C::SegmentationType_fd2ec6, &binary, Requirement::Optional);
    expect(
        C::PixelPaddingRangeLimit_ae4d69,
        &label_map,
        Requirement::Forbidden,
    );
    // Pixel Padding Value is forbidden unless LABELMAP, and then follows the
    // General Equipment Module condition (permitted with Pixel Data).
    let with_pixels = segmentation_root(
        "LABELMAP",
        vec![DataElement::new(
            tags::PIXEL_DATA,
            VR::OB,
            PrimitiveValue::from(vec![0u8; 2]),
        )],
    );
    expect(C::PixelPaddingValue_d4f36e, &binary, Requirement::Forbidden);
    expect(
        C::PixelPaddingValue_d4f36e,
        &with_pixels,
        Requirement::Optional,
    );
    expect(
        C::SegmentationFractionalType_63bbc3,
        &binary,
        Requirement::Forbidden,
    );
    expect(
        C::SegmentationFractionalType_63bbc3,
        &segmentation_root("FRACTIONAL", vec![]),
        Requirement::Required,
    );
    expect(C::BitsAllocated_c43232, &binary, Requirement::Required);
    expect(C::BitsAllocated_c43232, &label_map, Requirement::Optional);
    expect(C::FgSegmentation_99fd82, &binary, Requirement::Required);
    expect(C::FgSegmentation_99fd82, &label_map, Requirement::Forbidden);
}

fn shared_groups(groups: Vec<InMemElement>, extra: Vec<InMemElement>) -> InMemDicomObject {
    let mut root = data_set(vec![sequence(
        tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
        vec![data_set(groups)],
    )]);
    for element in extra {
        root.put(element);
    }
    root
}

fn frame_of_reference() -> InMemElement {
    text(tags::FRAME_OF_REFERENCE_UID, VR::UI, &["1.2.3"])
}

#[test]
fn frames_of_reference_are_slide_or_patient_by_the_positions_encoded() {
    let derivation = || sequence(tags::DERIVATION_IMAGE_SEQUENCE, vec![data_set(vec![])]);
    let patient = shared_groups(
        vec![sequence(
            tags::PLANE_ORIENTATION_SEQUENCE,
            vec![data_set(vec![])],
        )],
        vec![frame_of_reference()],
    );
    let slide = shared_groups(
        vec![sequence(
            tags::PLANE_POSITION_SLIDE_SEQUENCE,
            vec![data_set(vec![])],
        )],
        vec![frame_of_reference()],
    );
    let unplaced = shared_groups(vec![], vec![frame_of_reference()]);
    let derived = shared_groups(vec![derivation()], vec![]);
    // Pixel Measures, Plane Position and Orientation (Patient) of Table A.51-2.
    expect(C::FgPixelMeasures_3dce04, &patient, Requirement::Required);
    expect(C::FgPixelMeasures_3dce04, &derived, Requirement::Forbidden);
    expect(
        C::FgPixelMeasures_3dce04,
        &unplaced,
        Requirement::Undeterminable,
    );
    expect(
        C::FgPixelMeasures_3dce04,
        &slide,
        Requirement::Undeterminable,
    );
    // Derivation Image is required where the positions are not encoded.
    expect(C::FgDerivationImage_a322fc, &derived, Requirement::Required);
    expect(
        C::FgPlanePositionPatient_183b05,
        &patient,
        Requirement::Required,
    );
    expect(
        C::FgPlanePositionPatient_183b05,
        &slide,
        Requirement::Forbidden,
    );
    expect(
        C::FgPlanePositionSlide_7fb0bc,
        &slide,
        Requirement::Required,
    );
    expect(
        C::FgPlanePositionSlide_7fb0bc,
        &derived,
        Requirement::Forbidden,
    );
    let tiled = shared_groups(
        vec![sequence(
            tags::PLANE_POSITION_SLIDE_SEQUENCE,
            vec![data_set(vec![])],
        )],
        vec![
            frame_of_reference(),
            text(tags::DIMENSION_ORGANIZATION_TYPE, VR::CS, &["TILED_FULL"]),
        ],
    );
    expect(
        C::FgPlanePositionSlide_c1ecaa,
        &slide,
        Requirement::Required,
    );
    expect(
        C::FgPlanePositionSlide_c1ecaa,
        &tiled,
        Requirement::Optional,
    );
    expect(
        C::FgPlanePositionSlide_c1ecaa,
        &patient,
        Requirement::Forbidden,
    );
    // A patient Frame of Reference orients the image, so Patient Orientation
    // is not required.
    expect(
        C::PatientOrientation_964c1d,
        &patient,
        Requirement::Optional,
    );
    expect(
        C::PatientOrientation_964c1d,
        &derived,
        Requirement::Required,
    );
}

#[test]
fn bulk_annotation_conditions_read_the_annotation_group() {
    let coordinates = |kind: &str| {
        data_set(vec![text(
            tags::ANNOTATION_COORDINATE_TYPE,
            VR::CS,
            &[kind],
        )])
    };
    let planar = coordinates("2D");
    let volume = coordinates("3D");
    expect(
        C::ModuleFrameOfReference_9c03a6,
        &volume,
        Requirement::Required,
    );
    expect(
        C::ModuleFrameOfReference_9c03a6,
        &planar,
        Requirement::Forbidden,
    );
    expect(
        C::PixelOriginInterpretation_577198,
        &planar,
        Requirement::Required,
    );
    let points = |values: &[f32]| {
        data_set(vec![DataElement::new(
            tags::POINT_COORDINATES_DATA,
            VR::OF,
            PrimitiveValue::F32(values.iter().copied().collect()),
        )])
    };
    // All points at Z 5 need Common Z Coordinate Value; points at two Z do not.
    expect_in_item(
        C::CommonZCoordinateValue_a8e676,
        &volume,
        &points(&[1.0, 2.0, 5.0, 3.0, 4.0, 5.0]),
        Requirement::Required,
    );
    expect_in_item(
        C::CommonZCoordinateValue_a8e676,
        &volume,
        &points(&[1.0, 2.0, 5.0, 3.0, 4.0, 6.0]),
        Requirement::Forbidden,
    );
    let polygon = data_set(vec![text(tags::GRAPHIC_TYPE, VR::CS, &["POLYGON"])]);
    expect_in_item(
        C::LongPrimitivePointIndexList_bb9fb2,
        &planar,
        &polygon,
        Requirement::Required,
    );
    // Annotation Index List: fewer Floating Point Values than Annotations.
    let group = data_set(vec![DataElement::new(
        tags::NUMBER_OF_ANNOTATIONS,
        VR::UL,
        PrimitiveValue::from(3u32),
    )]);
    let measurements = data_set(vec![]);
    let values = |count: usize| {
        data_set(vec![DataElement::new(
            tags::FLOATING_POINT_VALUES,
            VR::OF,
            PrimitiveValue::F32(vec![1.0; count].into()),
        )])
    };
    let top = Scope::top(&planar, EXPLICIT_VR_LITTLE_ENDIAN);
    let group_scope = top.child(&group);
    let measurements_scope = group_scope.child(&measurements);
    let subset = values(2);
    let all = values(3);
    assert_eq!(
        evaluate(
            C::AnnotationIndexList_0474f0,
            &measurements_scope.child(&subset)
        ),
        Requirement::Required
    );
    assert_eq!(
        evaluate(
            C::AnnotationIndexList_0474f0,
            &measurements_scope.child(&all)
        ),
        Requirement::Forbidden
    );
}

#[test]
fn parametric_map_modules_follow_the_pixel_data_attribute() {
    let float = data_set(vec![DataElement::new(
        tags::FLOAT_PIXEL_DATA,
        VR::OF,
        PrimitiveValue::F32(vec![0.0f32; 4].into()),
    )]);
    expect(
        C::ModuleFloatingPointImagePixel_202cb1,
        &float,
        Requirement::Required,
    );
    expect(C::ModuleImagePixel_f11529, &float, Requirement::Forbidden);
    expect(C::BitsAllocated_2af7e7, &float, Requirement::Required);
    expect(C::BitsAllocated_38792c, &float, Requirement::Optional);
    expect(C::BitsStored_44fd83, &float, Requirement::Forbidden);
    let color = data_set(vec![text(
        tags::PIXEL_PRESENTATION,
        VR::CS,
        &["COLOR_RANGE"],
    )]);
    expect(C::ICCProfile_875087, &color, Requirement::Required);
    expect(
        C::PaletteColorLookupTableUID_c285e7,
        &color,
        Requirement::Required,
    );
    expect(
        C::ModulePaletteColorLookupTable_501a20,
        &color,
        Requirement::Required,
    );
    expect(
        C::FgStoredValueColorRange_fd7048,
        &float,
        Requirement::Forbidden,
    );
}

#[test]
fn microscopy_module_usage_reads_the_slide_evidence() {
    let slide = data_set(vec![sequence(
        tags::CONTAINER_TYPE_CODE_SEQUENCE,
        vec![data_set(vec![
            text(tags::CODE_VALUE, VR::SH, &["433466003"]),
            text(tags::CODING_SCHEME_DESIGNATOR, VR::SH, &["SCT"]),
        ])],
    )]);
    let unknown = data_set(vec![]);
    expect(
        C::ModuleMicroscopeSlideLayerTileOrganization_031caa,
        &slide,
        Requirement::Required,
    );
    expect(
        C::ModuleMicroscopeSlideLayerTileOrganization_031caa,
        &unknown,
        Requirement::Undeterminable,
    );
    let icc = data_set(vec![DataElement::new(
        tags::ICC_PROFILE,
        VR::OB,
        PrimitiveValue::from(vec![0u8; 4]),
    )]);
    expect(C::ModuleOpticalPath_e06f79, &icc, Requirement::Forbidden);
    expect(C::ModuleOpticalPath_e06f79, &unknown, Requirement::Optional);
    expect(
        C::ModuleMultiResolutionPyramid_dfce04,
        &image("OVERVIEW"),
        Requirement::Forbidden,
    );
}

#[test]
fn dictionary_resolves_multi_vr_retired_and_repeating_entries() {
    let image_type = entry(tags::IMAGE_TYPE).unwrap();
    assert_eq!(image_type.vr, &[VR::CS]);
    assert_eq!(
        image_type.vm,
        ValueMultiplicity {
            min: 2,
            max: None,
            step: 1
        }
    );
    let smallest = entry(tags::SMALLEST_IMAGE_PIXEL_VALUE).unwrap();
    assert!(smallest.vr.contains(&VR::US) && smallest.vr.contains(&VR::SS));
    assert!(entry(Tag(0x0008, 0x0010)).unwrap().retired);
    let overlay = entry(Tag(0x6002, 0x3000)).unwrap();
    assert!(overlay.vr.contains(&VR::OB) && overlay.vr.contains(&VR::OW));
    assert!(entry(Tag(0x0002, 0x0010)).is_some());
    assert!(entry(Tag(0x0008, 0x0002)).is_none());
}

#[test]
fn value_multiplicity_admits_stepped_and_bounded_ranges() {
    let pairs = ValueMultiplicity {
        min: 2,
        max: None,
        step: 2,
    };
    assert!(pairs.admits(2) && pairs.admits(4));
    assert!(!pairs.admits(3) && !pairs.admits(1));
    let bounded = ValueMultiplicity {
        min: 1,
        max: Some(3),
        step: 1,
    };
    assert!(bounded.admits(3) && !bounded.admits(4));
    assert_eq!(pairs.describe(), "2-2n");
    assert_eq!(bounded.describe(), "1-3");
}

#[test]
fn value_format_follows_ps3_5_section_6_2() {
    let accepted: &[(VR, &str)] = &[
        (VR::DA, "20240229"),
        (VR::TM, "070907.0705"),
        (VR::TM, "1010"),
        (VR::DT, "2007-0500"),
        (VR::DT, "19530827111300.0"),
        (VR::DT, "20240101120000.123456+0100"),
        (VR::IS, " +12 "),
        (VR::DS, "-1.5e3"),
        (VR::DS, " .5 "),
        (VR::UI, "1.2.840.10008.5.1.4.1.1.77.1.6"),
        // Registered in PS3.6 Tables A-3 (Context Group) and A-4 (Template).
        (VR::UI, "1.2.840.10008.6.1.1"),
        (VR::UI, "1.2.840.10008.9.1"),
        (VR::UI, "1.0.2"),
        (VR::CS, "TILED_FULL"),
        (VR::PN, "Doe^John^^Dr^=^="),
        (VR::AS, "018M"),
        (VR::LO, "slide \u{1b}$B"),
        (VR::ST, "line one\r\nline two"),
        (VR::UR, "https://example.org/a%2Fb?c=d"),
    ];
    for (vr, value) in accepted {
        assert_eq!(check_value(*vr, value, false), Ok(()), "{vr:?} {value:?}");
    }
    let rejected: &[(VR, &str)] = &[
        (VR::DA, "20230229"),
        (VR::DA, "2024-01-01"),
        (VR::TM, "021"),
        (VR::TM, "240000"),
        (VR::DT, "20221007073017+429496728900"),
        (VR::DT, "2024-0000"),
        (VR::DT, "202401011200.5"),
        (VR::IS, "2147483648"),
        (VR::IS, "1 2"),
        (VR::DS, "1,5"),
        (VR::DS, "12345678901234567"),
        (VR::UI, "1.02.3"),
        // The DICOM root, but registered nowhere in PS3.6 Annex A (PS3.5 9.2).
        (VR::UI, "1.2.840.10008.999.1"),
        (VR::UI, "1.2 "),
        (VR::UI, &"1.2".repeat(30)),
        (VR::CS, "tiled"),
        (VR::CS, "ABCDEFGHIJKLMNOPQ"),
        (VR::PN, "a^b^c^d^e^f"),
        (VR::PN, "a=b=c=d"),
        (VR::AS, "18M"),
        (VR::AE, "    "),
        (VR::LO, "id\r\n"),
        (VR::SH, "12345678901234567"),
        (VR::UR, " http://example.org"),
        (VR::UR, "http://a b"),
        (VR::UR, "%zz"),
    ];
    for (vr, value) in rejected {
        assert!(check_value(*vr, value, false).is_err(), "{vr:?} {value:?}");
    }
    // Specific Character Set extends the repertoire of text VRs only.
    assert!(check_value(VR::LO, "Müller", false).is_err());
    assert_eq!(check_value(VR::LO, "Müller", true), Ok(()));
    assert!(check_value(VR::CS, "MÜLLER", true).is_err());
}

#[test]
fn unknown_vr_stands_in_for_a_short_vr_whose_value_exceeds_65534_bytes() {
    let vr_vm_status = |length: u32| {
        let object = data_set(vec![DataElement::new_with_len(
            tags::INSTITUTION_NAME,
            VR::UN,
            dicom_core::Length(length),
            PrimitiveValue::U8(vec![b'A'; length as usize].into()),
        )]);
        let (mut vr_vm, mut format) = (
            super::report::Family::default(),
            super::report::Family::default(),
        );
        super::values::ValueChecks {
            vr_vm: &mut vr_vm,
            format: &mut format,
            encoding: super::values::Encoding {
                explicit_vr: true,
                extended_characters: false,
            },
        }
        .data_set(&object, "", 0);
        vr_vm
            .check(std::path::Path::new(""), "vr-vm", "PS3.6")
            .status
    };
    // PS3.5 6.2.2: LO has a 16-bit Value Length Field.
    assert_eq!(
        vr_vm_status(65536),
        crate::validation::ValidationStatus::Passed
    );
    assert_eq!(vr_vm_status(8), crate::validation::ValidationStatus::Failed);
}

#[test]
fn code_extension_values_are_counted_by_the_encoding_check() {
    // Sixteen two-byte characters and two escape sequences, as decoded by
    // Value 1 alone: 38 characters here, 16 in the encoded bytes.
    let station = format!("\u{1b}$B{}\u{1b}(B", "0!".repeat(16));
    assert_eq!(check_value(VR::SH, &station, true), Ok(()));
    assert!(check_value(VR::SH, &"A".repeat(17), true).is_err());
    // "^" bytes inside two-byte characters are no component delimiters.
    let name = format!("A=\u{1b}$B{}\u{1b}(B", "0^".repeat(40));
    assert_eq!(check_value(VR::PN, &name, true), Ok(()));
}
