//! Constraints between Attributes, each failed by an edited export.

use super::*;

const RELATIONS: &str = "intrinsic-wsi-dicom-attribute-relations";
const SERIES_SET: &str = "intrinsic-wsi-dicom-series-set";
const PARENT: &str = "1.2.826.0.1.3680043.10.999.922";

fn texts(tag: Tag, vr: VR, values: &[&str]) -> InMemElement {
    DataElement::new(
        tag,
        vr,
        PrimitiveValue::Strs(values.iter().map(|value| value.to_string()).collect()),
    )
}

fn pointers(tag: Tag, values: &[Tag]) -> InMemElement {
    DataElement::new(
        tag,
        VR::AT,
        PrimitiveValue::Tags(values.iter().copied().collect()),
    )
}

fn us(tag: Tag, values: &[u16]) -> InMemElement {
    DataElement::new(
        tag,
        VR::US,
        PrimitiveValue::U16(values.iter().copied().collect()),
    )
}

/// An Original Attributes Sequence Item recording `nonconforming` with
/// `modified` as the prior Values.
fn original(modified: Vec<InMemElement>, nonconforming: Vec<InMemElement>) -> InMemElement {
    sq(
        tags::ORIGINAL_ATTRIBUTES_SEQUENCE,
        vec![item(vec![
            sq(tags::MODIFIED_ATTRIBUTES_SEQUENCE, vec![item(modified)]),
            sq(
                tags::NONCONFORMING_MODIFIED_ATTRIBUTES_SEQUENCE,
                vec![item(nonconforming)],
            ),
        ])],
    )
}

fn empty_body_part() -> InMemElement {
    DataElement::new(tags::BODY_PART_EXAMINED, VR::CS, PrimitiveValue::Empty)
}

fn preparation_step(datetime: &str) -> InMemDicomObject {
    item(vec![sq(
        tags::SPECIMEN_PREPARATION_STEP_CONTENT_ITEM_SEQUENCE,
        vec![item(vec![
            text(tags::VALUE_TYPE, VR::CS, "DATETIME"),
            sq(
                tags::CONCEPT_NAME_CODE_SEQUENCE,
                vec![code("DCM", "111702", "DateTime of processing")],
            ),
            text(tags::DATE_TIME, VR::DT, datetime),
        ])],
    )])
}

#[test]
fn exported_wsi_passes_the_attribute_relations_rule() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_for_validation(tmp.path(), "consistent");
    let (status, message) = rule_result(&path, RELATIONS);
    assert_eq!(status, ValidationStatus::Passed, "{message}");
}

#[test]
fn conformant_relations_pass() {
    let cases: [(&str, Edit); 4] = [
        ("quadruped-orientation", |object| {
            object.put(text(tags::ANATOMICAL_ORIENTATION_TYPE, VR::CS, "QUADRUPED"));
            object.put(texts(tags::PATIENT_ORIENTATION, VR::CS, &["CRDIPA", "LE"]));
        }),
        ("nonconforming-recorded", |object| {
            object.put(original(
                vec![empty_body_part()],
                vec![
                    pointers(tags::SELECTOR_ATTRIBUTE, &[tags::BODY_PART_EXAMINED]),
                    us(tags::SELECTOR_VALUE_NUMBER, &[1]),
                ],
            ));
        }),
        ("calibration-pairs", |object| {
            object.put(texts(
                tags::DATE_OF_LAST_CALIBRATION,
                VR::DA,
                &["20240101", "20250101"],
            ));
            object.put(texts(
                tags::TIME_OF_LAST_CALIBRATION,
                VR::TM,
                &["120000", "080000"],
            ));
        }),
        ("preparation-precision", |object| {
            first_item(object, tags::SPECIMEN_DESCRIPTION_SEQUENCE, |specimen| {
                specimen.put(sq(
                    tags::SPECIMEN_PREPARATION_SEQUENCE,
                    vec![preparation_step("20250101"), preparation_step("2025010112")],
                ));
            });
        }),
    ];
    let tmp = tempfile::tempdir().unwrap();
    for (name, edit) in cases {
        let path = edited_wsi(tmp.path(), name, edit);
        let (status, message) = rule_result(&path, RELATIONS);
        assert_eq!(status, ValidationStatus::Passed, "{name}: {message}");
    }
}

#[test]
fn attribute_relations_fail_their_violations() {
    let cases: [(&str, Edit, &str); 28] = [
        (
            "selector-private",
            |object| {
                object.put(original(
                    Vec::new(),
                    vec![pointers(tags::SELECTOR_ATTRIBUTE, &[Tag(0x0019, 0x1010)])],
                ));
            },
            "where the Value shall be (0019,0010)",
        ),
        (
            "selector-value-number",
            |object| {
                object.put(original(
                    vec![empty_body_part()],
                    vec![
                        pointers(tags::SELECTOR_ATTRIBUTE, &[tags::BODY_PART_EXAMINED]),
                        us(tags::SELECTOR_VALUE_NUMBER, &[2]),
                    ],
                ));
            },
            "Selector Value Number (0072,0028) is 2",
        ),
        (
            "selector-pointer-sequence",
            |object| {
                object.put(original(
                    Vec::new(),
                    vec![
                        pointers(tags::SELECTOR_ATTRIBUTE, &[tags::BODY_PART_EXAMINED]),
                        pointers(tags::SELECTOR_SEQUENCE_POINTER, &[tags::PATIENT_ID]),
                    ],
                ));
            },
            "which is not a Sequence",
        ),
        (
            "selector-pointer-counts",
            |object| {
                object.put(original(
                    Vec::new(),
                    vec![
                        pointers(tags::SELECTOR_ATTRIBUTE, &[tags::SPECIMEN_UID]),
                        pointers(
                            tags::SELECTOR_SEQUENCE_POINTER,
                            &[tags::SPECIMEN_DESCRIPTION_SEQUENCE],
                        ),
                        us(tags::SELECTOR_SEQUENCE_POINTER_ITEMS, &[1, 1]),
                    ],
                ));
            },
            "Selector Sequence Pointer Items (0074,1057) has 2 Values",
        ),
        (
            "selector-public-creator",
            |object| {
                object.put(original(
                    Vec::new(),
                    vec![
                        pointers(tags::SELECTOR_ATTRIBUTE, &[tags::SPECIMEN_UID]),
                        pointers(
                            tags::SELECTOR_SEQUENCE_POINTER,
                            &[tags::SPECIMEN_DESCRIPTION_SEQUENCE],
                        ),
                        text(
                            tags::SELECTOR_SEQUENCE_POINTER_PRIVATE_CREATOR,
                            VR::LO,
                            "ACME",
                        ),
                    ],
                ));
            },
            "it shall be empty",
        ),
        (
            "nonconforming-selector",
            |object| {
                object.put(original(
                    vec![empty_body_part()],
                    vec![us(tags::SELECTOR_VALUE_NUMBER, &[1])],
                ));
            },
            "has no Selector Attribute (0072,0026)",
        ),
        (
            "nonconforming-not-empty",
            |object| {
                object.put(original(
                    vec![text(tags::BODY_PART_EXAMINED, VR::CS, "CHEST")],
                    vec![pointers(
                        tags::SELECTOR_ATTRIBUTE,
                        &[tags::BODY_PART_EXAMINED],
                    )],
                ));
            },
            "shall hold with a zero length Value",
        ),
        (
            "context-identifier",
            |object| {
                let mut region = code("SCT", "818981001", "Abdomen");
                region.put(text(tags::CONTEXT_IDENTIFIER, VR::CS, "0004"));
                region.put(text(tags::MAPPING_RESOURCE, VR::CS, "DCMR"));
                object.put(sq(tags::ANATOMIC_REGION_SEQUENCE, vec![region]));
            },
            "without leading zeros",
        ),
        (
            "referenced-frame",
            |object| {
                object.put(sq(
                    tags::SOURCE_IMAGE_SEQUENCE,
                    vec![item(vec![
                        text(
                            tags::REFERENCED_SOP_CLASS_UID,
                            VR::UI,
                            "1.2.840.10008.5.1.4.1.1.77.1.6",
                        ),
                        text(
                            tags::REFERENCED_SOP_INSTANCE_UID,
                            VR::UI,
                            "1.2.826.0.1.3680043.10.999.910",
                        ),
                        text(tags::REFERENCED_FRAME_NUMBER, VR::IS, "0"),
                    ])],
                ));
            },
            "Referenced Frame Number (0008,1160) 0 is below 1",
        ),
        (
            "identification-count",
            |object| {
                object.put(texts(
                    tags::PERFORMING_PHYSICIAN_NAME,
                    VR::PN,
                    &["A^B", "C^D", "E^F"],
                ));
                object.put(sq(
                    tags::PERFORMING_PHYSICIAN_IDENTIFICATION_SEQUENCE,
                    vec![item(Vec::new()), item(Vec::new())],
                ));
            },
            "has 2 Items for the 3 Values of PerformingPhysicianName (0008,1050)",
        ),
        (
            "contributing-operator",
            |object| {
                object.put(sq(
                    tags::CONTRIBUTING_EQUIPMENT_SEQUENCE,
                    vec![item(vec![
                        texts(tags::OPERATORS_NAME, VR::PN, &["A^B", "C^D"]),
                        sq(
                            tags::OPERATOR_IDENTIFICATION_SEQUENCE,
                            vec![item(Vec::new())],
                        ),
                    ])],
                ));
            },
            "(PS3.3 2026d Table C.12-1)",
        ),
        (
            "calibration-pairs",
            |object| {
                object.put(texts(
                    tags::DATE_OF_LAST_CALIBRATION,
                    VR::DA,
                    &["20240101", "20250101"],
                ));
                object.put(texts(tags::TIME_OF_LAST_CALIBRATION, VR::TM, &["120000"]));
            },
            "when both are present they are pairs",
        ),
        (
            "calibration-order",
            |object| {
                object.put(texts(
                    tags::DATE_OF_LAST_CALIBRATION,
                    VR::DA,
                    &["20250101", "20240101"],
                ));
            },
            "not ordered from the oldest to the most recent",
        ),
        (
            "subject-position",
            |object| {
                object.put(sq(
                    tags::GROUP_OF_PATIENTS_IDENTIFICATION_SEQUENCE,
                    vec![item(vec![
                        text(tags::PATIENT_ID, VR::LO, "M1"),
                        us(tags::SUBJECT_RELATIVE_POSITION_IN_IMAGE, &[0, 1, 1]),
                    ])],
                ));
            },
            "has a Value below 1",
        ),
        (
            "color-space",
            |object| {
                first_item(object, tags::OPTICAL_PATH_SEQUENCE, |path| {
                    path.put(text(tags::COLOR_SPACE, VR::CS, "ADOBERGB"));
                });
            },
            "are not those of ADOBERGB",
        ),
        (
            "preparation-order",
            |object| {
                first_item(object, tags::SPECIMEN_DESCRIPTION_SEQUENCE, |specimen| {
                    specimen.put(sq(
                        tags::SPECIMEN_PREPARATION_SEQUENCE,
                        vec![preparation_step("20250102"), preparation_step("20250101")],
                    ));
                });
            },
            "Items shall be in ascending chronological order",
        ),
        (
            "signed-order",
            |object| {
                object.put(sq(
                    tags::MAC_PARAMETERS_SEQUENCE,
                    vec![item(vec![pointers(
                        tags::DATA_ELEMENTS_SIGNED,
                        &[tags::SOP_INSTANCE_UID, tags::SOP_CLASS_UID],
                    )])],
                ));
            },
            "does not list its Tags in the order they appear",
        ),
        (
            "signed-excluded",
            |object| {
                object.put(sq(
                    tags::MAC_PARAMETERS_SEQUENCE,
                    vec![item(vec![pointers(
                        tags::DATA_ELEMENTS_SIGNED,
                        &[tags::TRANSFER_SYNTAX_UID],
                    )])],
                ));
            },
            "a Tag with a group below 0008 shall not be signed",
        ),
        (
            "signed-level",
            |object| {
                object.remove_element(tags::DATE_OF_LAST_CALIBRATION);
                object.put(sq(
                    tags::MAC_PARAMETERS_SEQUENCE,
                    vec![item(vec![pointers(
                        tags::DATA_ELEMENTS_SIGNED,
                        &[tags::DATE_OF_LAST_CALIBRATION],
                    )])],
                ));
            },
            "which is not at the level of the MAC Parameters Sequence",
        ),
        (
            "measured-value",
            |object| {
                object.put(sq(
                    tags::ACQUISITION_CONTEXT_SEQUENCE,
                    vec![item(vec![sq(
                        tags::MEASURED_VALUE_SEQUENCE,
                        vec![item(vec![
                            texts(tags::NUMERIC_VALUE, VR::DS, &["1", "2"]),
                            DataElement::new(
                                tags::FLOATING_POINT_VALUE,
                                VR::FD,
                                PrimitiveValue::F64([1.0].into_iter().collect()),
                            ),
                        ])],
                    )])],
                ));
            },
            "only a single Value shall be present",
        ),
        (
            "content-item-numeric",
            |object| {
                object.put(sq(
                    tags::ACQUISITION_CONTEXT_SEQUENCE,
                    vec![item(vec![
                        text(tags::VALUE_TYPE, VR::CS, "NUMERIC"),
                        sq(
                            tags::CONCEPT_NAME_CODE_SEQUENCE,
                            vec![code("SCT", "246205007", "Quantity")],
                        ),
                        text(tags::NUMERIC_VALUE, VR::DS, "1"),
                        DataElement::new(
                            tags::FLOATING_POINT_VALUE,
                            VR::FD,
                            PrimitiveValue::F64([1.0, 2.0].into_iter().collect()),
                        ),
                        sq(
                            tags::MEASUREMENT_UNITS_CODE_SEQUENCE,
                            vec![code("UCUM", "mm", "millimeter")],
                        ),
                    ])],
                ));
            },
            "Floating Point Value (0040,A161) has 2 Values where Numeric Value (0040,A30A) has 1",
        ),
        (
            "person-code",
            |object| {
                object.put(sq(
                    tags::PERFORMING_PHYSICIAN_IDENTIFICATION_SEQUENCE,
                    vec![item(vec![sq(
                        tags::PERSON_IDENTIFICATION_CODE_SEQUENCE,
                        vec![code("99LOCAL", "P1", "A^B^C^D^E^F")],
                    )])],
                ));
            },
            "is encoded as a person name but is not a valid PN Value",
        ),
        (
            "biped-orientation",
            |object| {
                object.put(texts(tags::PATIENT_ORIENTATION, VR::CS, &["A", "FX"]));
            },
            "Value \"FX\" is not one to three of the abbreviations",
        ),
        (
            "quadruped-orientation",
            |object| {
                object.put(text(tags::ANATOMICAL_ORIENTATION_TYPE, VR::CS, "QUADRUPED"));
                object.put(texts(tags::PATIENT_ORIENTATION, VR::CS, &["LE", "CRDIPAL"]));
            },
            "Value \"CRDIPAL\" is not one to three",
        ),
        (
            "laterality",
            |object| {
                object.put(text(tags::IMAGE_LATERALITY, VR::CS, "R"));
                object.put(text(tags::LATERALITY, VR::CS, "L"));
            },
            "Image Laterality (0020,0062) is R but Laterality (0020,0060) is L",
        ),
        (
            "laterality-modifier",
            |object| {
                object.put(text(tags::IMAGE_LATERALITY, VR::CS, "L"));
                let mut structure = code("SCT", "76752008", "Breast");
                structure.put(sq(
                    tags::PRIMARY_ANATOMIC_STRUCTURE_MODIFIER_SEQUENCE,
                    vec![code("SCT", "24028007", "Right")],
                ));
                object.put(sq(
                    tags::PRIMARY_ANATOMIC_STRUCTURE_SEQUENCE,
                    vec![structure],
                ));
            },
            "a Primary Anatomic Structure Modifier is R",
        ),
        (
            "patient-photo",
            |object| {
                object.put(sq(
                    tags::REFERENCED_PATIENT_PHOTO_SEQUENCE,
                    vec![item(vec![sq(
                        tags::REFERENCED_SERIES_SEQUENCE,
                        vec![item(vec![sq(
                            tags::REFERENCED_SOP_SEQUENCE,
                            vec![item(vec![text(
                                tags::REFERENCED_SOP_CLASS_UID,
                                VR::UI,
                                "1.2.840.10008.5.1.4.1.1.77.1.6",
                            )])],
                        )])],
                    )])],
                ));
            },
            "VL Photographic or Secondary Capture Image Storage",
        ),
        (
            "hl7-document",
            |object| {
                object.put(sq(
                    tags::HL7_STRUCTURED_DOCUMENT_REFERENCE_SEQUENCE,
                    vec![item(vec![text(
                        tags::REFERENCED_SOP_CLASS_UID,
                        VR::UI,
                        "1.2.840.10008.5.1.4.1.1.88.11",
                    )])],
                ));
            },
            "is not the HL7 Object Identifier of a Hierarchical Message Description",
        ),
    ];
    let tmp = tempfile::tempdir().unwrap();
    for (name, edit, expected) in cases {
        let path = edited_wsi(tmp.path(), name, edit);
        let (status, message) = rule_result(&path, RELATIONS);
        assert_eq!(status, ValidationStatus::Failed, "{name}: {message}");
        assert!(message.contains(expected), "{name}: {message}");
    }
}

#[test]
fn source_instances_are_not_images() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited_wsi(tmp.path(), "source-instance", |object| {
        object.put(sq(
            tags::SOURCE_INSTANCE_SEQUENCE,
            vec![item(vec![text(
                tags::REFERENCED_SOP_CLASS_UID,
                VR::UI,
                "1.2.840.10008.5.1.4.1.1.77.1.6",
            )])],
        ));
    });
    let (status, message) = rule_result(&path, RELATIONS);
    assert_eq!(status, ValidationStatus::Failed, "{message}");
    assert!(
        message.contains("images are referenced by Source Image Sequence (0008,2112)"),
        "{message}"
    );
}

/// A directory with an export and a copy that is another SOP Instance, with
/// `edit` applied to the copy and `original` to the export.
fn pair(root: &Path, name: &str, original: Edit, edit: Edit) -> PathBuf {
    let directory = root.join(name);
    std::fs::create_dir_all(&directory).unwrap();
    let export = export_valid_color_wsi_for_validation(root, name);
    let first = directory.join("a.dcm");
    std::fs::copy(&export, &first).unwrap();
    edited(first, original);
    let copy = directory.join("b.dcm");
    std::fs::copy(&export, &copy).unwrap();
    edited(copy, |object| {
        let uid = "1.2.826.0.1.3680043.10.999.920";
        object.put(text(tags::SOP_INSTANCE_UID, VR::UI, uid));
        object.meta_mut().media_storage_sop_instance_uid = uid.to_string();
        object.put(text(tags::INSTANCE_NUMBER, VR::IS, "78"));
        edit(object);
    });
    directory
}

fn extraction(source: &str) -> InMemDicomObject {
    item(vec![
        text(tags::MULTI_FRAME_SOURCE_SOP_INSTANCE_UID, VR::UI, source),
        DataElement::new(
            tags::SIMPLE_FRAME_LIST,
            VR::UL,
            PrimitiveValue::U32([1].into_iter().collect()),
        ),
    ])
}

fn group(patient: &str, positions: [u16; 3]) -> InMemElement {
    sq(
        tags::GROUP_OF_PATIENTS_IDENTIFICATION_SEQUENCE,
        vec![item(vec![
            text(tags::PATIENT_ID, VR::LO, patient),
            us(tags::SUBJECT_RELATIVE_POSITION_IN_IMAGE, &positions),
        ])],
    )
}

#[test]
fn set_relations_fail_their_violations() {
    let cases: [(&str, Edit, Edit, &str); 2] = [
        (
            "frame-extraction",
            // The parent, itself extracted from an Instance outside the set.
            |object| {
                object.put(text(tags::SOP_INSTANCE_UID, VR::UI, PARENT));
                object.meta_mut().media_storage_sop_instance_uid = PARENT.to_string();
                object.put(sq(
                    tags::FRAME_EXTRACTION_SEQUENCE,
                    vec![extraction("1.2.826.0.1.3680043.10.999.921")],
                ));
            },
            // Extracted from the parent without carrying its Item.
            |object| {
                object.put(sq(tags::FRAME_EXTRACTION_SEQUENCE, vec![extraction(PARENT)]));
            },
            "shall hold all 1 Items of the parent's and one new Item, but has 1 Items without all of the parent's",
        ),
        (
            "group-arrangement",
            |object| {
                object.put(group("M1", [1, 1, 1]));
            },
            |object| {
                object.put(group("M1", [2, 1, 1]));
            },
            "names a group arranged differently",
        ),
    ];
    let tmp = tempfile::tempdir().unwrap();
    for (name, original, edit, expected) in cases {
        let (status, message) = rule_result(&pair(tmp.path(), name, original, edit), SERIES_SET);
        assert_eq!(status, ValidationStatus::Failed, "{name}: {message}");
        assert!(message.contains(expected), "{name}: {message}");
    }
}
