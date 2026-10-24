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
        ["A.32.8", "A.87", "A.51", "A.75", "A.35.4", "A.35.1", "A.35.2", "A.35.3", "A.35.13"]
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
    // The Key Object Selection Document and the SR documents (PS3.4 B.5).
    for (section, uid) in [
        ("A.35.4", "1.2.840.10008.5.1.4.1.1.88.59"),
        ("A.35.1", "1.2.840.10008.5.1.4.1.1.88.11"),
        ("A.35.2", "1.2.840.10008.5.1.4.1.1.88.22"),
        ("A.35.3", "1.2.840.10008.5.1.4.1.1.88.33"),
        ("A.35.13", "1.2.840.10008.5.1.4.1.1.88.34"),
    ] {
        assert_eq!(uids(section), [uid]);
        assert_eq!(iod(section).functional_group_table, None);
    }
    // Height Map Segmentation (A.91) is not a pathology-slide IOD.
    assert!(IODS.iter().all(|iod| iod
        .sop_classes
        .iter()
        .all(|class| class.uid != "1.2.840.10008.5.1.4.1.1.66.8")));
    // VL Microscopic, VL Slide-Coordinates Microscopic and the Confocal
    // Microscopy IODs are not among them.
    for uid in [
        "1.2.840.10008.5.1.4.1.1.77.1.2",
        "1.2.840.10008.5.1.4.1.1.77.1.3",
        "1.2.840.10008.5.1.4.1.1.77.1.8",
        "1.2.840.10008.5.1.4.1.1.77.1.9",
    ] {
        assert!(super::iod_of(uid).is_none(), "{uid}");
    }
    assert_eq!(iod("A.87").functional_group_table, None);
    assert!(iod("A.87").functional_groups.is_empty());
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
        // PS3.5 6.2: "Embedded spaces are not allowed" (DS, IS); "Leading and
        // embedded spaces are not allowed" (DT, TM); "Leading spaces are not
        // allowed" (UR).
        (VR::IS, "1 2"),
        (VR::DS, "1 5"),
        (VR::TM, " 120000"),
        (VR::TM, "12 0000"),
        (VR::DT, " 20240101"),
        (VR::DT, "2024 0101"),
        (VR::UR, " https://example.org/a"),
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
            advisories: &mut super::report::Advisories::default(),
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

#[test]
fn every_table_e_1_1_row_is_found_by_its_tag() {
    use super::deidentification::{
        confidentiality_attribute, confidentiality_attributes, ConfidentialityOption as O,
        DeidentificationAction as A, TagPattern,
    };
    let accession = confidentiality_attribute(tags::ACCESSION_NUMBER).unwrap();
    assert_eq!(
        (accession.name, accession.basic),
        ("Accession Number", A::Z)
    );
    assert!(accession.options.is_empty() && accession.in_standard_iod && !accession.retired);
    let date = confidentiality_attribute(tags::ACQUISITION_DATE).unwrap();
    assert_eq!(date.basic, A::XZ);
    assert_eq!(
        date.action(O::RetainLongitudinalTemporalInformationFullDates),
        Some(A::K)
    );
    assert_eq!(
        date.action(O::RetainLongitudinalTemporalInformationModifiedDates),
        Some(A::C)
    );
    assert_eq!(date.action(O::RetainSafePrivate), None);
    let referenced = confidentiality_attribute(tags::REFERENCED_IMAGE_SEQUENCE).unwrap();
    assert_eq!(
        (referenced.basic.code(), referenced.action(O::RetainUIDs)),
        ("X/Z/U*", Some(A::K))
    );
    // Repeating groups and Private Data Elements.
    let comments = confidentiality_attribute(Tag(0x6002, 0x4000)).unwrap();
    assert_eq!(comments.name, "Overlay Comments");
    let curve = confidentiality_attribute(Tag(0x501E, 0x3000)).unwrap();
    assert_eq!(curve.name, "Curve Data");
    let private = confidentiality_attribute(Tag(0x0009, 0x1001)).unwrap();
    assert_eq!((private.tag, private.basic), (TagPattern::Private, A::X));
    assert_eq!(private.action(O::RetainSafePrivate), Some(A::C));
    // Rows is not listed: the Profile leaves it as it is.
    assert!(confidentiality_attribute(tags::ROWS).is_none());
    // Every row is found again by the tag it names.
    for row in confidentiality_attributes() {
        let tag = match row.tag {
            TagPattern::Tag(tag) | TagPattern::Repeating { tag, .. } => tag,
            TagPattern::Private => 0x0009_1001,
        };
        let found = confidentiality_attribute(Tag((tag >> 16) as u16, (tag & 0xFFFF) as u16));
        assert!(
            found.is_some_and(|found| std::ptr::eq(found, row)),
            "{}",
            row.name
        );
    }
    assert!(confidentiality_attributes().count() > 600);
}

#[test]
fn clean_structured_content_rows_are_found_by_concept_and_value_type() {
    use super::deidentification::{
        structured_content_concept, structured_content_concepts, ConfidentialityOption as O,
        DeidentificationAction as A,
    };
    let accession = structured_content_concept("DCM", "121022", "TEXT").unwrap();
    assert_eq!(
        (accession.code.meaning, accession.basic),
        ("Accession Number", A::X)
    );
    // One concept, one row per Value Type.
    let report = structured_content_concept("SCT", "371524004", "TEXT").unwrap();
    assert_eq!(report.action(O::CleanDescriptors), Some(A::C));
    let composite = structured_content_concept("SCT", "371524004", "COMPOSITE").unwrap();
    assert_eq!(composite.action(O::RetainUIDs), Some(A::K));
    assert!(structured_content_concept("SCT", "371524004", "CODE").is_none());
    // A Coding Scheme Designator with a version, "NCDR [2.0b]".
    let admission = structured_content_concept("NCDR", "15", "DATETIME").unwrap();
    assert_eq!(
        (admission.code.scheme, admission.scheme_version),
        ("NCDR", "2.0b")
    );
    for row in structured_content_concepts() {
        assert!(
            structured_content_concept(row.code.scheme, row.code.value, row.value_type)
                .is_some_and(|found| std::ptr::eq(found, row))
        );
    }
}

#[test]
fn every_confidentiality_option_has_its_cid_7050_code() {
    use super::deidentification::{
        deidentification_methods, ConfidentialityOption, DeidentificationAction, BASIC_PROFILE,
        BASIC_PROFILE_SECTION,
    };
    let methods = deidentification_methods();
    assert_eq!(methods.len(), ConfidentialityOption::ALL.len() + 1);
    assert!(methods.contains(&BASIC_PROFILE));
    assert_eq!(BASIC_PROFILE_SECTION, "E.2");
    for option in ConfidentialityOption::ALL {
        let code = option.code();
        assert!(methods.contains(&code), "{option:?}");
        assert_eq!(
            ConfidentialityOption::from_code(code.scheme, code.value),
            Some(*option)
        );
        assert!(option.section().starts_with("E.3."));
    }
    assert_eq!(ConfidentialityOption::from_code("DCM", "113100"), None);
    assert_eq!(
        ConfidentialityOption::from_code("DCM", "113111"),
        Some(ConfidentialityOption::RetainSafePrivate)
    );
    let codes: Vec<&str> = DeidentificationAction::ALL
        .iter()
        .map(|action| action.code())
        .collect();
    assert_eq!(
        codes,
        ["D", "Z", "X", "K", "C", "U", "Z/D", "X/Z", "X/D", "X/Z/D", "X/Z/U*"]
    );
}

/// The findings of a Specimen Preparation step holding its Processing type
/// alone, missing TID 8001 Row 1 (121041, DCM, "Specimen Identifier"), held
/// to TID 8001 as a Defined Template in an object whose De-identification
/// Method Code Sequence lists `methods`.
fn unidentified_step(methods: &[(&str, &str)]) -> super::report::Findings {
    let coded = |scheme: &str, value: &str| {
        data_set(vec![
            text(tags::CODE_VALUE, VR::SH, &[value]),
            text(tags::CODING_SCHEME_DESIGNATOR, VR::SH, &[scheme]),
            text(tags::CODE_MEANING, VR::LO, &["meaning"]),
        ])
    };
    let step = data_set(vec![
        text(tags::VALUE_TYPE, VR::CS, &["CODE"]),
        sequence(
            tags::CONCEPT_NAME_CODE_SEQUENCE,
            vec![coded("DCM", "111701")],
        ),
        sequence(tags::CONCEPT_CODE_SEQUENCE, vec![coded("SCT", "9265001")]),
    ]);
    let object = data_set(vec![sequence(
        tags::DEIDENTIFICATION_METHOD_CODE_SEQUENCE,
        methods
            .iter()
            .map(|(scheme, value)| coded(scheme, value))
            .collect(),
    )]);
    let mut findings = super::report::Findings::default();
    super::templates::check_items(
        &mut findings,
        "Steps",
        super::model::Strength::Defined,
        8001,
        &[step],
        &object,
        crate::validation::disclosure::Nesting::default(),
    );
    findings
}

#[test]
fn a_required_item_clean_structured_content_removes_is_advised_citing_both_parts() {
    use crate::validation::ValidationStatus;
    let missing =
        "TID 8001 \"Specimen Preparation\" Row 1 TEXT (121041, DCM, \"Specimen Identifier\") missing";
    let status = |findings: &super::report::Findings| {
        findings
            .value_sets
            .check(std::path::Path::new("step.dcm"), "value-sets", "citation")
    };
    // Under the Basic Profile alone the Template's requirement stands.
    let basic = unidentified_step(&[("DCM", "113100")]);
    let check = status(&basic);
    assert_eq!(check.status, ValidationStatus::Failed);
    assert!(
        check
            .message
            .contains(&format!("Steps: {missing} (PS3.16 {EDITION})")),
        "{}",
        check.message
    );
    assert!(basic.advisories.describe().is_empty());
    // Table E.3.4-1 removes the Item under Clean Structured Content, with or
    // without an Option naming no other action for it.
    for methods in [
        &[("DCM", "113100"), ("DCM", "113104")][..],
        &[("DCM", "113104"), ("DCM", "113105")],
    ] {
        let cleaned = unidentified_step(methods);
        assert_eq!(
            status(&cleaned).status,
            ValidationStatus::Passed,
            "{methods:?}"
        );
        assert_eq!(
            cleaned.advisories.describe(),
            [format!(
                "Steps: {missing}, a Content Item PS3.15 {EDITION} Table E.3.4-1 removes under the Clean Structured Content Option the De-identification Method Code Sequence (0012,0064) declares (PS3.15 {EDITION} E.3.4; PS3.16 {EDITION} TID 8001)"
            )]
        );
    }
}

#[test]
fn structured_reporting_iods_expose_their_content_constraints_and_templates() {
    use super::structured_report::{
        document_constraints, document_constraints_of_sop_class, template, templates,
    };
    // Table A.35.4-2: a KOS CONTAINER CONTAINS IMAGE, and nothing CONTAINS
    // a NUM; HAS ACQ CONTEXT has IMAGE among its Sources.
    let kos = document_constraints_of_sop_class("1.2.840.10008.5.1.4.1.1.88.59").unwrap();
    assert_eq!(
        (kos.iod, kos.table, kos.root_template),
        ("A.35.4", "A.35.4-2", Some(2010))
    );
    assert!(kos.admits("CONTAINER", "CONTAINS", "IMAGE"));
    assert!(!kos.admits("CONTAINER", "CONTAINS", "NUM"));
    assert!(kos.admits("IMAGE", "HAS ACQ CONTEXT", "DATETIME"));
    assert!(!kos.admits("TEXT", "HAS ACQ CONTEXT", "DATETIME"));
    // Table A.35.3-2: "any type" HAS CONCEPT MOD TEXT or CODE; a Comprehensive
    // SR permits SCOORD3D nowhere, the Comprehensive 3D SR does.
    let comprehensive = document_constraints("A.35.3").unwrap();
    assert!(comprehensive.admits("SCOORD", "HAS CONCEPT MOD", "CODE"));
    assert!(!comprehensive.admits("SCOORD", "HAS CONCEPT MOD", "NUM"));
    assert!(comprehensive.admits("SCOORD", "SELECTED FROM", "IMAGE"));
    assert!(!comprehensive.value_types.contains(&"SCOORD3D"));
    assert!(document_constraints("A.35.13")
        .unwrap()
        .value_types
        .contains(&"SCOORD3D"));
    // The Basic Text SR has no NUM Content Items (A.35.1.3.1.1).
    assert!(!document_constraints("A.35.1")
        .unwrap()
        .value_types
        .contains(&"NUM"));
    assert!(document_constraints("A.32.8").is_none());
    assert!(document_constraints_of_sop_class(crate::VL_WSI_SOP_CLASS_UID).is_none());
    // TID 2010 Row 1 is the CONTAINER; its children are the rows one level
    // down, Row 8 IMAGE among them, and none of the root's own level.
    let kos_template = template(2010).unwrap();
    assert_eq!(kos_template.rows[0].value_type, "CONTAINER");
    let children: Vec<&str> = kos_template.children(0).map(|row| row.row).collect();
    assert_eq!(
        children,
        ["2", "3", "4", "4b", "5", "6", "7", "8", "9", "10", "11"]
    );
    assert!(kos_template.children(1).next().is_none());
    // TID 1500 and the Templates it includes are reachable, in ID order.
    assert!(template(1500).is_some() && template(1501).is_some());
    assert!(templates().windows(2).all(|pair| pair[0].id < pair[1].id));
    assert!(template(1).is_none());
}

#[test]
fn template_rows_and_context_groups_judge_content_items_for_the_content_rule() {
    use super::model::Strength;
    use super::structured_report::{code_finding, context_group_name, template, CodeFinding};
    let coded = |scheme: &str, value: &str, flag: Option<&str>| {
        let mut elements = vec![
            text(tags::CODE_VALUE, VR::SH, &[value]),
            text(tags::CODING_SCHEME_DESIGNATOR, VR::SH, &[scheme]),
            text(tags::CODE_MEANING, VR::LO, &["meaning"]),
        ];
        elements.extend(flag.map(|flag| text(tags::CONTEXT_GROUP_EXTENSION_FLAG, VR::CS, &[flag])));
        data_set(elements)
    };
    let content = |value_type: &str, scheme: &str, value: &str| {
        data_set(vec![
            text(tags::VALUE_TYPE, VR::CS, &[value_type]),
            sequence(
                tags::CONCEPT_NAME_CODE_SEQUENCE,
                vec![coded(scheme, value, None)],
            ),
        ])
    };
    // TID 2010 Row 1: a CONTAINER titled from CID 7010; Row 7: TEXT
    // (113012, DCM, "Key Object Description"); Row 8: IMAGE of any name.
    let rows = template(2010).unwrap().rows;
    assert!(rows[0].admits(&content("CONTAINER", "DCM", "113000")));
    assert!(!rows[0].admits(&content("TEXT", "DCM", "113000")));
    assert!(!rows[0].admits(&content("CONTAINER", "99LOCAL", "t1")));
    assert_eq!(rows[0].concept_text(), "CID 7010");
    let description = rows.iter().find(|row| row.row == "7").unwrap();
    assert!(description.admits(&content("TEXT", "DCM", "113012")));
    assert!(!description.admits(&content("TEXT", "DCM", "113011")));
    assert_eq!(
        description.concept_text(),
        "(113012, DCM, \"Key Object Description\")"
    );
    let image = rows.iter().find(|row| row.row == "8").unwrap();
    assert!(image.admits(&content("IMAGE", "SCT", "260753009")));
    // A member, a declared extension and a non-coded Item pass; a code
    // outside an extensible Defined group is advised, outside a
    // non-extensible one (CID 82) a violation.
    assert_eq!(
        code_finding(Strength::Defined, &[7010], &coded("DCM", "113000", None)),
        None
    );
    assert_eq!(
        code_finding(
            Strength::Defined,
            &[7010],
            &coded("99LOCAL", "t1", Some("Y"))
        ),
        None
    );
    assert_eq!(
        code_finding(Strength::Defined, &[7010], &data_set(vec![])),
        None
    );
    assert!(matches!(
        code_finding(Strength::Defined, &[7010], &coded("99LOCAL", "t1", None)),
        Some(CodeFinding::Advisory(message)) if message.starts_with("(t1, 99LOCAL, \"meaning\") is not in Defined CID 7010 \"Key Object Selection Document Title\"")
    ));
    assert!(matches!(
        code_finding(Strength::Baseline, &[7010], &coded("99LOCAL", "t1", None)),
        Some(CodeFinding::Advisory(message)) if message.contains("Baseline CID 7010")
    ));
    assert!(matches!(
        code_finding(Strength::Defined, &[82], &coded("99LOCAL", "mm", None)),
        Some(CodeFinding::Violation(message)) if message.contains("Defined CID 82 \"Measurement Unit\"")
    ));
    assert_eq!(
        context_group_name(7010),
        Some("Key Object Selection Document Title")
    );
    assert_eq!(context_group_name(1), None);
}

#[test]
fn directory_records_follow_table_f_4_1() {
    use super::file_set::{
        basic_directory_section, basic_directory_sop_class, directory_record, directory_records,
        root_directory_records,
    };
    assert_eq!(basic_directory_sop_class(), "1.2.840.10008.1.3.10");
    assert_eq!(basic_directory_section(), "F.3");
    assert!(root_directory_records().contains(&"PATIENT"));
    assert!(!root_directory_records().contains(&"STUDY"));
    let patient = directory_record("PATIENT").unwrap();
    assert_eq!(
        (patient.section, patient.keys_table()),
        ("F.5.1", Some("F.5-1"))
    );
    assert!(patient.admits_lower_level("STUDY") && !patient.admits_lower_level("IMAGE"));
    let keys = patient.selection_keys();
    assert!(keys
        .iter()
        .any(|key| key.tag == 0x0010_0020 && key.kind == "1"));
    assert!(keys
        .iter()
        .any(|key| key.tag == 0x0008_0005 && key.kind == "1C"));
    let series = directory_record("SERIES").unwrap();
    assert!(series.admits_lower_level("IMAGE") && series.admits_lower_level("ANNOTATION"));
    let private = directory_record("PRIVATE").unwrap();
    assert_eq!((private.section, private.keys_table()), ("F.6.1", None));
    assert!(private.selection_keys().is_empty());
    assert!(private.any_private_lower_level && !patient.any_private_lower_level);
    assert!(directory_record("OVERLAY").is_none());
    // Every record type the records admit is defined.
    for record in directory_records() {
        for lower in record.lower_level {
            assert!(directory_record(lower).is_some(), "{lower}");
        }
    }
}

#[test]
fn directory_record_conditions_read_the_record_type_and_its_references() {
    let root = data_set(vec![]);
    let record = |record_type: &str, extra: Vec<InMemElement>| {
        let mut elements = vec![text(tags::DIRECTORY_RECORD_TYPE, VR::CS, &[record_type])];
        elements.extend(extra);
        data_set(elements)
    };
    let file_id = || text(tags::REFERENCED_FILE_ID, VR::CS, &["IM0001"]);
    let image = record("IMAGE", vec![]);
    let patient = record("PATIENT", vec![]);
    let private = record("PRIVATE", vec![]);
    // IMAGE records reference a SOP Instance; PATIENT records do not.
    expect_in_item(
        C::ReferencedFileID_3760d0,
        &root,
        &image,
        Requirement::Required,
    );
    expect_in_item(
        C::ReferencedFileID_3760d0,
        &root,
        &patient,
        Requirement::Forbidden,
    );
    let patient_file = record("PATIENT", vec![file_id()]);
    expect_in_item(
        C::ReferencedSOPClassUIDInFile_b5b28b,
        &root,
        &patient_file,
        Requirement::Forbidden,
    );
    // A PRIVATE record references one when it names one.
    let private_file = record("PRIVATE", vec![file_id()]);
    expect_in_item(
        C::ReferencedSOPClassUIDInFile_b5b28b,
        &root,
        &private_file,
        Requirement::Required,
    );
    expect_in_item(
        C::ReferencedFileID_3760d0,
        &root,
        &private,
        Requirement::Undeterminable,
    );
    expect_in_item(
        C::PrivateRecordUID_399277,
        &root,
        &private,
        Requirement::Required,
    );
    expect_in_item(
        C::PrivateRecordUID_399277,
        &root,
        &image,
        Requirement::Forbidden,
    );
    // Specific Character Set of a record: required by a key outside ISO 646.
    let named = |name: &str| record("PATIENT", vec![text(tags::PATIENT_NAME, VR::PN, &[name])]);
    expect_in_item(
        C::SpecificCharacterSet_ebd0ce,
        &root,
        &named("Muller^Anna"),
        Requirement::Forbidden,
    );
    expect_in_item(
        C::SpecificCharacterSet_ebd0ce,
        &root,
        &named("Müller^Anna"),
        Requirement::Required,
    );
}

#[test]
fn content_item_macros_follow_the_value_type() {
    let root = data_set(vec![text(tags::VALUE_TYPE, VR::CS, &["CONTAINER"])]);
    let content = |value_type: &str| data_set(vec![text(tags::VALUE_TYPE, VR::CS, &[value_type])]);
    // Table C.17-5 includes each Content Item Macro if and only if its Value Type.
    expect_in_item(
        C::IncludeC1811_b4d406,
        &root,
        &content("NUM"),
        Requirement::Required,
    );
    expect_in_item(
        C::IncludeC1811_b4d406,
        &root,
        &content("TEXT"),
        Requirement::Forbidden,
    );
    expect_in_item(
        C::IncludeC1841_08c4c7,
        &root,
        &content("IMAGE"),
        Requirement::Required,
    );
    expect_in_item(
        C::IncludeC1881_0f7a08,
        &root,
        &content("CONTAINER"),
        Requirement::Required,
    );
    // The root CONTAINER has a Concept Name; a nested one, or a reference, may not.
    expect(
        C::ConceptNameCodeSequence_abde30,
        &root,
        Requirement::Required,
    );
    for (value_type, expected) in [
        ("CONTAINER", Requirement::Optional),
        ("IMAGE", Requirement::Optional),
        ("CODE", Requirement::Required),
    ] {
        expect_in_item(
            C::ConceptNameCodeSequence_abde30,
            &root,
            &content(value_type),
            expected,
        );
    }
    // A table cell holds its Value in the Selector Attribute of its VR.
    let cell = |vr: &str, extra: Vec<InMemElement>| {
        let mut elements = vec![text(tags::SELECTOR_ATTRIBUTE_VR, VR::CS, &[vr])];
        elements.extend(extra);
        data_set(elements)
    };
    let fd = cell("FD", vec![]);
    expect_in_item(C::SelectorFDValue_3dcd6f, &root, &fd, Requirement::Required);
    expect_in_item(
        C::SelectorFDValue_3dcd6f,
        &root,
        &cell("DS", vec![]),
        Requirement::Forbidden,
    );
    let qualifier = sequence(
        tags::NUMERIC_VALUE_QUALIFIER_CODE_SEQUENCE,
        vec![data_set(vec![])],
    );
    let qualified = cell("FD", vec![qualifier]);
    expect_in_item(
        C::SelectorFDValue_3dcd6f,
        &root,
        &qualified,
        Requirement::Forbidden,
    );
    expect_in_item(
        C::NumericValueQualifierCodeSequence_21892a,
        &root,
        &fd,
        Requirement::Required,
    );
    expect_in_item(
        C::NumericValueQualifierCodeSequence_21892a,
        &root,
        &cell("UC", vec![]),
        Requirement::Forbidden,
    );
}
