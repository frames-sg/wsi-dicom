//! Unit tests of the de-identification rule on minimal in-memory objects:
//! each starts from a conformant de-identified object and breaks one
//! requirement of PS3.15 Annex E.

mod fixtures;

use std::path::Path;

use dicom_core::value::DataSetSequence;
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::mem::InMemElement;
use dicom_object::{DefaultDicomObject, FileMetaTableBuilder, InMemDicomObject};

use super::run_deidentification_check;
use crate::validation::{ValidationCheck, ValidationStatus};

const BASIC: &str = "113100";
const CLEAN_PIXEL: &str = "113101";
const CLEAN_FEATURES: &str = "113102";
const CLEAN_STRUCTURED: &str = "113104";
const CLEAN_DESCRIPTORS: &str = "113105";
const FULL_DATES: &str = "113106";
const MODIFIED_DATES: &str = "113107";
const RETAIN_UIDS: &str = "113110";
const RETAIN_SAFE_PRIVATE: &str = "113111";

fn text(tag: Tag, vr: VR, value: &str) -> InMemElement {
    DataElement::new(tag, vr, PrimitiveValue::from(value))
}

fn values(tag: Tag, vr: VR, values: &[&str]) -> InMemElement {
    DataElement::new(
        tag,
        vr,
        PrimitiveValue::Strs(values.iter().map(|value| value.to_string()).collect()),
    )
}

fn empty(tag: Tag, vr: VR) -> InMemElement {
    DataElement::new(tag, vr, PrimitiveValue::Empty)
}

fn sequence(tag: Tag, items: Vec<InMemDicomObject>) -> InMemElement {
    DataElement::new(tag, VR::SQ, DataSetSequence::from(items))
}

fn item(elements: Vec<InMemElement>) -> InMemDicomObject {
    InMemDicomObject::from_element_iter(elements)
}

fn code(value: &str, scheme: &str, meaning: &str) -> InMemDicomObject {
    item(vec![
        text(tags::CODE_VALUE, VR::SH, value),
        text(tags::CODING_SCHEME_DESIGNATOR, VR::SH, scheme),
        text(tags::CODE_MEANING, VR::LO, meaning),
    ])
}

fn methods(codes: &[&str]) -> InMemElement {
    sequence(
        tags::DEIDENTIFICATION_METHOD_CODE_SEQUENCE,
        codes
            .iter()
            .map(|value| code(value, "DCM", "De-identification Method"))
            .collect(),
    )
}

/// A VL WSI object de-identified with the Basic Profile and the Options
/// `codes` name, conformant to them.
fn deidentified(codes: &[&str]) -> InMemDicomObject {
    let temporal = if codes.contains(&FULL_DATES) {
        "UNMODIFIED"
    } else if codes.contains(&MODIFIED_DATES) {
        "MODIFIED"
    } else {
        "REMOVED"
    };
    item(vec![
        text(
            tags::SOP_CLASS_UID,
            VR::UI,
            uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE,
        ),
        text(tags::SOP_INSTANCE_UID, VR::UI, "2.25.1001"),
        values(
            tags::IMAGE_TYPE,
            VR::CS,
            &["DERIVED", "PRIMARY", "VOLUME", "NONE"],
        ),
        empty(tags::PATIENT_NAME, VR::PN),
        text(tags::PATIENT_ID, VR::LO, "PSEUDO-1"),
        text(tags::STUDY_INSTANCE_UID, VR::UI, "2.25.2001"),
        text(tags::FRAME_OF_REFERENCE_UID, VR::UI, "2.25.3001"),
        text(tags::PATIENT_IDENTITY_REMOVED, VR::CS, "YES"),
        methods(codes),
        text(tags::BURNED_IN_ANNOTATION, VR::CS, "NO"),
        text(tags::RECOGNIZABLE_VISUAL_FEATURES, VR::CS, "NO"),
        text(
            tags::LONGITUDINAL_TEMPORAL_INFORMATION_MODIFIED,
            VR::CS,
            temporal,
        ),
        text(tags::CONTAINER_IDENTIFIER, VR::LO, "DUMMY-SLIDE"),
        DataElement::new(
            tags::PIXEL_DATA,
            VR::OB,
            PrimitiveValue::U8(vec![0; 12].into()),
        ),
        sequence(
            tags::SPECIMEN_DESCRIPTION_SEQUENCE,
            vec![item(vec![
                text(tags::SPECIMEN_IDENTIFIER, VR::LO, "DUMMY-SPECIMEN"),
                text(tags::SPECIMEN_UID, VR::UI, "2.25.4001"),
            ])],
        ),
    ])
}

fn file(data_set: InMemDicomObject) -> DefaultDicomObject {
    data_set
        .with_meta(
            FileMetaTableBuilder::new()
                .media_storage_sop_class_uid(uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE)
                .media_storage_sop_instance_uid("2.25.1001")
                .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN),
        )
        .expect("file meta")
}

fn check(data_set: InMemDicomObject) -> ValidationCheck {
    run_deidentification_check(Path::new("deid.dcm"), &file(data_set))
}

fn with(mut data_set: InMemDicomObject, element: InMemElement) -> InMemDicomObject {
    data_set.put(element);
    data_set
}

fn without(mut data_set: InMemDicomObject, tag: Tag) -> InMemDicomObject {
    data_set.remove_element(tag);
    data_set
}

fn assert_fails(check: &ValidationCheck, needle: &str) {
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
    assert!(check.message.contains(needle), "{}", check.message);
}

fn assert_passes(check: &ValidationCheck) {
    assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);
}

#[test]
fn an_object_claiming_no_deidentification_passes_saying_so() {
    let data_set = without(
        without(deidentified(&[BASIC]), tags::PATIENT_IDENTITY_REMOVED),
        tags::DEIDENTIFICATION_METHOD_CODE_SEQUENCE,
    );
    let result = check(data_set);
    assert_passes(&result);
    assert!(
        result
            .message
            .contains("Patient Identity Removed is not YES; no de-identification is claimed"),
        "{}",
        result.message
    );
}

#[test]
fn each_conformant_option_combination_passes() {
    for codes in [
        &[BASIC][..],
        &[BASIC, CLEAN_PIXEL, CLEAN_FEATURES],
        &[BASIC, FULL_DATES, RETAIN_UIDS],
        &[BASIC, MODIFIED_DATES, CLEAN_DESCRIPTORS, CLEAN_STRUCTURED],
    ] {
        assert_passes(&check(deidentified(codes)));
    }
}

#[test]
fn patient_identity_removed_must_be_yes_when_a_method_is_named() {
    let data_set = deidentified(&[BASIC]);
    assert_fails(
        &check(without(data_set.clone(), tags::PATIENT_IDENTITY_REMOVED)),
        "Patient Identity Removed (0012,0062) is absent",
    );
    assert_fails(
        &check(with(
            data_set,
            text(tags::PATIENT_IDENTITY_REMOVED, VR::CS, "NO"),
        )),
        "E.1.2",
    );
}

#[test]
fn a_method_must_be_named_by_code_or_text() {
    let bare = without(
        deidentified(&[BASIC]),
        tags::DEIDENTIFICATION_METHOD_CODE_SEQUENCE,
    );
    assert_fails(&check(bare.clone()), "E.1.1 requires one or both");
    let described = with(
        bare,
        text(
            tags::DEIDENTIFICATION_METHOD,
            VR::LO,
            "PS3.15 2026d Basic Profile",
        ),
    );
    let result = check(described);
    assert_passes(&result);
    assert!(result.message.contains("text alone"), "{}", result.message);
}

#[test]
fn a_code_outside_cid_7050_alone_names_no_method() {
    let data_set = with(
        deidentified(&[BASIC]),
        sequence(
            tags::DEIDENTIFICATION_METHOD_CODE_SEQUENCE,
            vec![code("LOCAL-1", "99LOCAL", "Site method")],
        ),
    );
    assert_fails(&check(data_set), "E.1.1 requires one or both");
}

#[test]
fn clean_pixel_data_requires_burned_in_annotation_no() {
    let data_set = with(
        deidentified(&[BASIC, CLEAN_PIXEL]),
        text(tags::BURNED_IN_ANNOTATION, VR::CS, "YES"),
    );
    assert_fails(
        &check(data_set),
        "Burned In Annotation (0028,0301) is \"YES\"",
    );
}

/// A Parametric Map is an Image Storage SOP Instance whose pixels are its
/// Float Pixel Data: E.3.1 binds them as it binds Pixel Data (7FE0,0010).
#[test]
fn clean_pixel_data_binds_a_float_parametric_map() {
    let mut data_set = without(
        without(deidentified(&[BASIC, CLEAN_PIXEL]), tags::PIXEL_DATA),
        tags::BURNED_IN_ANNOTATION,
    );
    data_set.put(text(
        tags::SOP_CLASS_UID,
        VR::UI,
        uids::PARAMETRIC_MAP_STORAGE,
    ));
    data_set.put(DataElement::new(
        tags::FLOAT_PIXEL_DATA,
        VR::OF,
        PrimitiveValue::F32(vec![0.5; 4].into()),
    ));
    assert_fails(
        &check(data_set),
        "Burned In Annotation (0028,0301) is absent; E.3.1 Clean Pixel Data Option adds it with the Value NO",
    );
}

#[test]
fn clean_recognizable_visual_features_requires_no() {
    let data_set = without(
        deidentified(&[BASIC, CLEAN_FEATURES]),
        tags::RECOGNIZABLE_VISUAL_FEATURES,
    );
    assert_fails(
        &check(data_set),
        "Recognizable Visual Features (0028,0302) is absent",
    );
}

#[test]
fn longitudinal_temporal_information_follows_the_date_options() {
    let removed = with(
        deidentified(&[BASIC]),
        text(
            tags::LONGITUDINAL_TEMPORAL_INFORMATION_MODIFIED,
            VR::CS,
            "MODIFIED",
        ),
    );
    assert_fails(&check(removed), "the Value REMOVED");
    let full = with(
        deidentified(&[BASIC, FULL_DATES]),
        text(
            tags::LONGITUDINAL_TEMPORAL_INFORMATION_MODIFIED,
            VR::CS,
            "REMOVED",
        ),
    );
    assert_fails(&check(full), "the Value UNMODIFIED");
    let modified = without(
        deidentified(&[BASIC, MODIFIED_DATES]),
        tags::LONGITUDINAL_TEMPORAL_INFORMATION_MODIFIED,
    );
    assert_fails(&check(modified), "the Value MODIFIED");
    let both = deidentified(&[BASIC, FULL_DATES, MODIFIED_DATES]);
    assert_fails(&check(both), "mutually exclusive");
}

#[test]
fn group_0004_is_removed() {
    let data_set = with(
        deidentified(&[BASIC]),
        text(Tag(0x0004, 0x1130), VR::CS, "FILESET"),
    );
    assert_fails(&check(data_set), "group 0004");
}

#[test]
fn private_attributes_are_removed_unless_retain_safe_private() {
    let private = |data_set| {
        with(
            with(data_set, text(Tag(0x0009, 0x0010), VR::LO, "SCANNER")),
            text(Tag(0x0009, 0x1001), VR::LO, "slide 12"),
        )
    };
    assert_fails(
        &check(private(deidentified(&[BASIC]))),
        "without the Retain Safe Private Option",
    );
    assert_passes(&check(private(deidentified(&[BASIC, RETAIN_SAFE_PRIVATE]))));
    let orphan = with(
        deidentified(&[BASIC, RETAIN_SAFE_PRIVATE]),
        text(Tag(0x0009, 0x1001), VR::LO, "slide 12"),
    );
    assert_fails(&check(orphan), "without its Private Creator (0009,0010)");
}

#[test]
fn an_attribute_whose_action_is_x_is_absent() {
    // Table E.1-1: Other Patient IDs Sequence X, Original Attributes Sequence X.
    let data_set = with(
        deidentified(&[BASIC]),
        sequence(tags::ORIGINAL_ATTRIBUTES_SEQUENCE, Vec::new()),
    );
    assert_fails(
        &check(data_set),
        "OriginalAttributesSequence (0400,0561) is present; Table E.1-1 action X",
    );
}

#[test]
fn a_nested_attribute_is_held_to_its_action() {
    // Table E.1-1: Specimen Short Description X, C under Clean Descriptors.
    let specimen = |codes: &[&str]| {
        with(
            deidentified(codes),
            sequence(
                tags::SPECIMEN_DESCRIPTION_SEQUENCE,
                vec![item(vec![
                    text(tags::SPECIMEN_IDENTIFIER, VR::LO, "DUMMY-SPECIMEN"),
                    text(tags::SPECIMEN_UID, VR::UI, "2.25.4001"),
                    text(tags::SPECIMEN_SHORT_DESCRIPTION, VR::LO, "left breast core"),
                ])],
            ),
        )
    };
    assert_fails(
        &check(specimen(&[BASIC])),
        "SpecimenDescriptionSequence (0040,0560)[1] > SpecimenShortDescription (0040,0600) is present",
    );
    assert_passes(&check(specimen(&[BASIC, CLEAN_DESCRIPTORS])));
}

#[test]
fn an_attribute_whose_action_is_d_has_a_value() {
    let data_set = with(
        deidentified(&[BASIC]),
        empty(tags::CONTAINER_IDENTIFIER, VR::LO),
    );
    assert_fails(
        &check(data_set),
        "ContainerIdentifier (0040,0512) has no Value; Table E.1-1 action D",
    );
}

#[test]
fn a_uid_whose_action_is_u_has_a_value_unless_retained() {
    let data_set = |codes: &[&str]| with(deidentified(codes), empty(tags::PYRAMID_UID, VR::UI));
    assert_fails(
        &check(data_set(&[BASIC])),
        "PyramidUID (0008,0019) has no Value",
    );
    // Retain UIDs makes the action K, which any Value satisfies.
    assert_passes(&check(data_set(&[BASIC, RETAIN_UIDS])));
}

#[test]
fn barcode_and_label_text_may_hold_dummies() {
    // Table E.1-1 X/Z: Type 2 in the Slide Label Module, so Z, which admits
    // a dummy Value; the file cannot show the Value is original.
    let data_set = with(
        with(
            deidentified(&[BASIC]),
            text(tags::BARCODE_VALUE, VR::LT, "DUMMY-BARCODE"),
        ),
        text(tags::LABEL_TEXT, VR::UT, "DUMMY"),
    );
    assert_passes(&check(data_set));
}

fn preparation_step(content: Vec<InMemDicomObject>) -> InMemDicomObject {
    item(vec![sequence(
        tags::SPECIMEN_PREPARATION_STEP_CONTENT_ITEM_SEQUENCE,
        content,
    )])
}

fn content_item(
    value: &str,
    meaning: &str,
    value_type: &str,
    element: InMemElement,
) -> InMemDicomObject {
    item(vec![
        text(tags::VALUE_TYPE, VR::CS, value_type),
        sequence(
            tags::CONCEPT_NAME_CODE_SEQUENCE,
            vec![code(value, "DCM", meaning)],
        ),
        element,
    ])
}

fn with_preparation(codes: &[&str], steps: Vec<InMemDicomObject>) -> InMemDicomObject {
    with(
        deidentified(codes),
        sequence(
            tags::SPECIMEN_DESCRIPTION_SEQUENCE,
            vec![item(vec![
                text(tags::SPECIMEN_IDENTIFIER, VR::LO, "DUMMY-SPECIMEN"),
                text(tags::SPECIMEN_UID, VR::UI, "2.25.4001"),
                sequence(tags::SPECIMEN_PREPARATION_SEQUENCE, steps),
            ])],
        ),
    )
}

#[test]
fn clean_structured_content_removes_specimen_identifier_items() {
    let identifier = content_item(
        "121041",
        "Specimen Identifier",
        "TEXT",
        text(tags::TEXT_VALUE, VR::UT, "S26-1234 A1"),
    );
    let data_set = with_preparation(
        &[BASIC, CLEAN_STRUCTURED],
        vec![preparation_step(vec![identifier])],
    );
    assert_fails(
        &check(data_set),
        "Content Item (121041, DCM, \"Specimen Identifier\") TEXT is present; Table E.3.4-1 action X",
    );
}

#[test]
fn the_tid_8001_conflict_is_an_advisory_citing_both() {
    let processing = content_item(
        "111703",
        "Processing step description",
        "TEXT",
        text(tags::TEXT_VALUE, VR::UT, "cleaned"),
    );
    let data_set = with_preparation(
        &[BASIC, CLEAN_STRUCTURED, CLEAN_DESCRIPTORS],
        vec![preparation_step(vec![processing])],
    );
    let result = check(data_set);
    assert_passes(&result);
    assert!(
        result.message.contains("Table E.3.4-1") && result.message.contains("TID 8001"),
        "{}",
        result.message
    );
}

#[test]
fn label_pixels_without_clean_pixel_data_are_an_advisory() {
    let label = with(
        deidentified(&[BASIC]),
        values(
            tags::IMAGE_TYPE,
            VR::CS,
            &["ORIGINAL", "PRIMARY", "LABEL", "NONE"],
        ),
    );
    let result = check(label.clone());
    assert_passes(&result);
    assert!(
        result
            .message
            .contains("the label pixels are not covered by the declared options"),
        "{}",
        result.message
    );
    let cleaned = with(label, methods(&[BASIC, CLEAN_PIXEL]));
    let result = check(cleaned);
    assert!(
        !result.message.contains("label pixels"),
        "{}",
        result.message
    );
}

fn level(
    sop_instance_uid: &str,
    codes: Option<&[&str]>,
    changes: Vec<InMemElement>,
) -> DefaultDicomObject {
    let mut data_set = deidentified(codes.unwrap_or(&[BASIC]));
    if codes.is_none() {
        data_set.remove_element(tags::PATIENT_IDENTITY_REMOVED);
        data_set.remove_element(tags::DEIDENTIFICATION_METHOD_CODE_SEQUENCE);
    }
    data_set.put(text(tags::SOP_INSTANCE_UID, VR::UI, sop_instance_uid));
    data_set.put(text(tags::PYRAMID_UID, VR::UI, "2.25.5001"));
    data_set.put(text(tags::SERIES_INSTANCE_UID, VR::UI, "2.25.6001"));
    for change in changes {
        data_set.put(change);
    }
    file(data_set)
}

fn set_check(
    objects: &[DefaultDicomObject],
) -> Option<(ValidationCheck, Vec<(std::path::PathBuf, String)>)> {
    let mut corpus = super::DeidentificationCorpus::default();
    for (index, object) in objects.iter().enumerate() {
        corpus.observe(Path::new(&format!("level-{index}.dcm")), object);
    }
    super::run_deidentification_set_check(&corpus)
}

#[test]
fn a_set_claiming_no_deidentification_has_no_set_check() {
    let set = [
        level("2.25.1", None, Vec::new()),
        level("2.25.2", None, Vec::new()),
    ];
    assert!(set_check(&set).is_none());
}

#[test]
fn a_consistently_deidentified_pyramid_passes_the_set_check() {
    let set = [
        level("2.25.1", Some(&[BASIC]), Vec::new()),
        level("2.25.2", Some(&[BASIC]), Vec::new()),
    ];
    let (check, advisories) = set_check(&set).expect("set check");
    assert_passes(&check);
    assert!(advisories.is_empty());
}

#[test]
fn dummy_values_stay_consistent_across_a_pyramid() {
    for (change, name) in [
        (
            text(tags::FRAME_OF_REFERENCE_UID, VR::UI, "2.25.3999"),
            "Frame of Reference UID (0020,0052)",
        ),
        (
            text(tags::CONTAINER_IDENTIFIER, VR::LO, "OTHER-DUMMY"),
            "Container Identifier (0040,0512)",
        ),
        (
            sequence(
                tags::SPECIMEN_DESCRIPTION_SEQUENCE,
                vec![item(vec![
                    text(tags::SPECIMEN_IDENTIFIER, VR::LO, "DUMMY-SPECIMEN"),
                    text(tags::SPECIMEN_UID, VR::UI, "2.25.4999"),
                ])],
            ),
            "the Specimen UIDs (0040,0554)",
        ),
        (
            text(tags::PATIENT_ID, VR::LO, "PSEUDO-2"),
            "Patient ID (0010,0020)",
        ),
    ] {
        let set = [
            level("2.25.1", Some(&[BASIC]), Vec::new()),
            level("2.25.2", Some(&[BASIC]), vec![change]),
        ];
        let (check, _) = set_check(&set).expect("set check");
        assert_fails(
            &check,
            &format!("Pyramid UID (0008,0019) 2.25.5001 disagree on {name}"),
        );
    }
}

#[test]
fn a_sop_instance_uid_shared_with_an_original_was_not_replaced() {
    let set = [
        level("2.25.7", None, Vec::new()),
        level("2.25.7", Some(&[BASIC]), Vec::new()),
    ];
    let (check, advisories) = set_check(&set).expect("set check");
    assert_fails(&check, "so action U did not replace it");
    assert_eq!(advisories.len(), 1, "{advisories:?}");
    assert!(advisories[0].1.contains("claims no de-identification"));
    let retained = [
        level("2.25.7", None, Vec::new()),
        level("2.25.7", Some(&[BASIC, RETAIN_UIDS]), Vec::new()),
    ];
    let (check, _) = set_check(&retained).expect("set check");
    assert!(
        !check.message.contains("did not replace"),
        "{}",
        check.message
    );
}

#[test]
fn a_retained_private_sequence_is_parsed_and_its_attributes_held() {
    // E.3.10: a retained Private Sequence is walked, and each nested
    // Attribute handled on its own merits (Table E.1-1: Other Patient IDs
    // Sequence X).
    let nested = |inner: Vec<InMemElement>| {
        with(
            with(
                deidentified(&[BASIC, RETAIN_SAFE_PRIVATE]),
                text(Tag(0x0009, 0x0010), VR::LO, "SCANNER"),
            ),
            sequence(Tag(0x0009, 0x1002), vec![item(inner)]),
        )
    };
    assert_passes(&check(nested(vec![text(
        Tag(0x0009, 0x0010),
        VR::LO,
        "SCANNER",
    )])));
    assert_fails(
        &check(nested(vec![sequence(
            tags::OTHER_PATIENT_I_DS_SEQUENCE,
            vec![item(vec![text(tags::PATIENT_ID, VR::LO, "MRN-1")])],
        )])),
        "> OtherPatientIDsSequence (0010,1002) is present; Table E.1-1 action X",
    );
}

#[test]
fn retained_private_attributes_follow_their_declared_characteristics() {
    let characterized = |status: &str, action: Option<&str>| {
        let mut block = vec![
            DataElement::new(
                tags::PRIVATE_GROUP_REFERENCE,
                VR::US,
                PrimitiveValue::from(0x0009_u16),
            ),
            text(tags::PRIVATE_CREATOR_REFERENCE, VR::LO, "SCANNER"),
            text(tags::BLOCK_IDENTIFYING_INFORMATION_STATUS, VR::CS, status),
        ];
        if let Some(action) = action {
            block.push(sequence(
                tags::DEIDENTIFICATION_ACTION_SEQUENCE,
                vec![item(vec![
                    DataElement::new(
                        tags::IDENTIFYING_PRIVATE_ELEMENTS,
                        VR::US,
                        PrimitiveValue::from(0x0001_u16),
                    ),
                    text(tags::DEIDENTIFICATION_ACTION, VR::CS, action),
                ])],
            ));
        }
        with(
            with(
                with(
                    deidentified(&[BASIC, RETAIN_SAFE_PRIVATE]),
                    text(Tag(0x0009, 0x0010), VR::LO, "SCANNER"),
                ),
                text(Tag(0x0009, 0x1001), VR::LO, "slide 12"),
            ),
            sequence(
                tags::PRIVATE_DATA_ELEMENT_CHARACTERISTICS_SEQUENCE,
                vec![item(block)],
            ),
        )
    };
    assert_passes(&check(characterized("SAFE", None)));
    assert_passes(&check(characterized("UNSAFE", Some("Z"))));
    assert_fails(
        &check(characterized("UNSAFE", None)),
        "E.3.10 requires it removed",
    );
    assert_fails(
        &check(characterized("UNSAFE", Some("X"))),
        "Deidentification Action (0008,0307) is X",
    );
}

const AES_128_CBC: &str = "2.16.840.1.101.3.4.1.2";
const RSA_ENCRYPTION: &str = "1.2.840.113549.1.1.1";

/// CMS ContentInfo of EnvelopedData with one KeyTransRecipientInfo.
fn enveloped(content_algorithm: &str, key_algorithm: &str) -> Vec<u8> {
    use cms::cert::x509::ext::pkix::SubjectKeyIdentifier;
    use cms::cert::x509::spki::AlgorithmIdentifierOwned;
    use cms::content_info::{CmsVersion, ContentInfo};
    use cms::enveloped_data::{
        EncryptedContentInfo, EnvelopedData, KeyTransRecipientInfo, RecipientIdentifier,
        RecipientInfo, RecipientInfos,
    };
    use der::asn1::{ObjectIdentifier, OctetString, SetOfVec};
    use der::{Any, Encode};

    let algorithm = |oid: &str| AlgorithmIdentifierOwned {
        oid: ObjectIdentifier::new_unwrap(oid),
        parameters: None,
    };
    let octets = |bytes: Vec<u8>| OctetString::new(bytes).expect("octets");
    let recipient = RecipientInfo::Ktri(KeyTransRecipientInfo {
        version: CmsVersion::V2,
        rid: RecipientIdentifier::SubjectKeyIdentifier(SubjectKeyIdentifier(octets(vec![1, 2, 3]))),
        key_enc_alg: algorithm(key_algorithm),
        enc_key: octets(vec![7; 16]),
    });
    let data = EnvelopedData {
        version: CmsVersion::V2,
        originator_info: None,
        recip_infos: RecipientInfos(SetOfVec::try_from(vec![recipient]).expect("recipients")),
        encrypted_content: EncryptedContentInfo {
            content_type: ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.1"),
            content_enc_alg: algorithm(content_algorithm),
            encrypted_content: Some(octets(vec![9; 32])),
        },
        unprotected_attrs: None,
    };
    ContentInfo {
        content_type: ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.3"),
        content: Any::encode_from(&data).expect("enveloped data"),
    }
    .to_der()
    .expect("content info")
}

fn encrypted(syntax: &str, content: Vec<u8>) -> InMemElement {
    sequence(
        tags::ENCRYPTED_ATTRIBUTES_SEQUENCE,
        vec![item(vec![
            text(tags::ENCRYPTED_CONTENT_TRANSFER_SYNTAX_UID, VR::UI, syntax),
            DataElement::new(
                tags::ENCRYPTED_CONTENT,
                VR::OB,
                PrimitiveValue::U8(content.into()),
            ),
        ])],
    )
}

#[test]
fn encrypted_attributes_use_rsa_key_transport_and_des_or_aes() {
    let protected = |element| check(with(deidentified(&[BASIC]), element));
    let syntax = uids::EXPLICIT_VR_LITTLE_ENDIAN;
    assert_passes(&protected(encrypted(
        syntax,
        enveloped(AES_128_CBC, RSA_ENCRYPTION),
    )));
    assert_passes(&protected(encrypted(
        syntax,
        enveloped("1.2.840.113549.3.7", RSA_ENCRYPTION),
    )));
    // RSAES-OAEP is not the RFC 2313 key transport E.1.1 names.
    assert_fails(
        &protected(encrypted(
            syntax,
            enveloped(AES_128_CBC, "1.2.840.113549.1.1.7"),
        )),
        "requires RSA [RFC 2313] key transport",
    );
    // AES-GCM (RFC 5084) is not the RFC 3565 AES content encryption.
    assert_fails(
        &protected(encrypted(
            syntax,
            enveloped("2.16.840.1.101.3.4.1.6", RSA_ENCRYPTION),
        )),
        "requires Triple DES [RFC 3370] or AES [RFC 3565]",
    );
    assert_fails(
        &protected(encrypted(syntax, vec![1, 2, 3])),
        "is not a CMS ContentInfo",
    );
    assert_fails(
        &protected(encrypted("1.2.3.4", enveloped(AES_128_CBC, RSA_ENCRYPTION))),
        "with a DICOM Transfer Syntax",
    );
}

/// E.1.1 admits a DICOM Transfer Syntax, looked up exactly: not a UID
/// merely under the Transfer Syntax arc, nor a registered UID of another
/// kind, nor a syntax that encodes no Data Set.
#[test]
fn the_encrypted_content_transfer_syntax_is_an_exact_dicom_transfer_syntax() {
    let protected = |syntax: &str| {
        check(with(
            deidentified(&[BASIC]),
            encrypted(syntax, enveloped(AES_128_CBC, RSA_ENCRYPTION)),
        ))
    };
    for syntax in [
        uids::DEFLATED_EXPLICIT_VR_LITTLE_ENDIAN,
        uids::JPEG2000_LOSSLESS,
        // A DICOM syntax; that PS3.3 Table C.12-1 admits Explicit VR Little
        // Endian alone is the encoding check's finding, not this rule's.
        uids::IMPLICIT_VR_LITTLE_ENDIAN,
    ] {
        assert_passes(&protected(syntax));
    }
    for syntax in [
        // Under the arc, but PS3.6 Annex A registers no such UID.
        "1.2.840.10008.1.2.4.999",
        "1.2.840.10008.1.2.99",
        // Registered, as a SOP Class.
        uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE,
        // Registered Transfer Syntax that encodes RTV flows, not Data Sets.
        uids::SMPTEST211020_UNCOMPRESSED_PROGRESSIVE_ACTIVE_VIDEO,
    ] {
        assert_fails(
            &protected(syntax),
            "not a Transfer Syntax that PS3.6 Annex A registers and whose Data Set encoding PS3.5 Annex A defines",
        );
    }
}

#[test]
fn a_broken_encrypted_attributes_sequence_fails_without_a_claim() {
    let unclaimed = without(
        without(deidentified(&[BASIC]), tags::PATIENT_IDENTITY_REMOVED),
        tags::DEIDENTIFICATION_METHOD_CODE_SEQUENCE,
    );
    assert_fails(
        &check(with(unclaimed, encrypted("1.2.3.4", vec![1, 2, 3]))),
        "EncryptedAttributesSequence (0400,0500)[1] > ",
    );
}

/// The review's bound: however many violations an object holds, its
/// message shows the first twenty and counts the rest.
#[test]
fn a_heavily_broken_object_reports_a_bounded_message() {
    let mut data_set = deidentified(&[BASIC]);
    for element in 0x1000..0x1000 + 500u16 {
        data_set.put(text(Tag(0x0009, element), VR::LO, "IDENTIFYING"));
    }
    let result = check(data_set);
    assert_fails(&result, "500 violations: ");
    assert!(
        result.message.contains("; and 480 more"),
        "{}",
        result.message
    );
    assert_eq!(
        result
            .message
            .matches("without the Retain Safe Private Option")
            .count(),
        20,
        "{}",
        result.message
    );
}

/// Table E.1-1 Note 11: an Icon Image Sequence is removed unless the Clean
/// Pixel Data Option cleans or recreates its Pixel Data.
#[test]
fn an_icon_image_is_removed_unless_clean_pixel_data_cleans_it() {
    let icon = sequence(
        tags::ICON_IMAGE_SEQUENCE,
        vec![item(vec![text(
            tags::PHOTOMETRIC_INTERPRETATION,
            VR::CS,
            "MONOCHROME2",
        )])],
    );
    assert_fails(
        &check(with(deidentified(&[BASIC]), icon.clone())),
        "IconImageSequence (0088,0200) is present; Table E.1-1 action X",
    );
    assert_passes(&check(with(deidentified(&[BASIC, CLEAN_PIXEL]), icon)));
}

/// A DICOMDIR record describing several de-identified Files is held to the
/// union of their Options, which is sound while every Option only relaxes
/// an action: Tables E.1-1 and E.3.4-1 give an Option K or C alone.
#[test]
fn every_option_only_keeps_or_cleans() {
    use crate::validation::iod::deidentification::{
        confidentiality_attributes, structured_content_concepts, DeidentificationAction as A,
    };
    let overrides = confidentiality_attributes()
        .flat_map(|row| row.options.iter())
        .chain(
            structured_content_concepts()
                .iter()
                .flat_map(|row| row.options.iter()),
        );
    for (option, action) in overrides {
        assert!(matches!(action, A::K | A::C), "{option:?} gives {action:?}");
    }
}

/// A record describing de-identified Files holds no Value the Profile
/// removes; dcmmkdir's empty Type 2 Keys and the record's own group 0004
/// hold nothing to remove.
#[test]
fn a_directory_record_is_held_to_the_values_the_profile_removes() {
    use super::actions::{check_record, RecordAt};
    use crate::validation::iod::Family;
    let held = |record: &InMemDicomObject, codes: &[&str]| {
        let at = RecordAt {
            dicomdir: Path::new("ROOT/DICOMDIR"),
            record_type: "STUDY",
            offset: 412,
        };
        let mut violations = Family::default();
        check_record(
            record,
            at,
            &super::Declaration::of(&deidentified(codes)),
            &mut violations,
        );
        violations.check(Path::new(""), "deidentification", "citation")
    };
    let clean = item(vec![
        text(Tag(0x0004, 0x1430), VR::CS, "STUDY"),
        text(Tag(0x0004, 0x1400), VR::UL, "0"),
        text(tags::STUDY_INSTANCE_UID, VR::UI, "2.25.2001"),
        empty(tags::STUDY_DESCRIPTION, VR::LO),
    ]);
    assert_passes(&held(&clean, &[BASIC]));
    let described = with(
        clean,
        text(tags::STUDY_DESCRIPTION, VR::LO, "BIOPSY, LEFT BREAST"),
    );
    assert_fails(
        &held(&described, &[BASIC]),
        "ROOT/DICOMDIR STUDY record at byte 412 > StudyDescription (0008,1030) is present; Table E.1-1 action X",
    );
    // Clean Descriptors cleans it instead.
    assert_passes(&held(&described, &[BASIC, CLEAN_DESCRIPTORS]));
}
