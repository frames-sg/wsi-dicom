//! The Key Object Selection Document and SR document IODs (PS3.3 A.35): a
//! document of each, built here like the ones highdicom writes about a VL
//! WSI image, passes every IOD check, each Content Item is held to the
//! Content Item Macro of its Value Type, and breaking one Attribute fails
//! the check of its family. The same documents, written by
//! `write_structured_report_fixtures`, have no dciodvfy errors.

use super::*;

mod rules;

const EXPLICIT_VR_LITTLE_ENDIAN: &str = "1.2.840.10008.1.2.1";
const KOS_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.88.59";
const BASIC_TEXT_SR_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.88.11";
const ENHANCED_SR_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.88.22";
const COMPREHENSIVE_SR_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.88.33";
const COMPREHENSIVE_3D_SR_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.88.34";
const STUDY: &str = "1.2.826.0.1.3680043.10.999.50.1";
const IMAGE: &str = "1.2.826.0.1.3680043.10.999.50.2";
const IMAGE_SERIES: &str = "1.2.826.0.1.3680043.10.999.50.3";
/// Directory `write_structured_report_fixtures` writes the documents to.
const FIXTURE_DIR_VARIABLE: &str = "WSI_DICOM_IOD_FIXTURES";

/// A document's file name, SOP Class, IOD and the function building its
/// Data Set.
type Document = (
    &'static str,
    &'static str,
    &'static str,
    fn(&'static str) -> InMemDicomObject,
);

/// The KOS, and the TID 1500 Measurement Report in each SR IOD that holds
/// it; the Basic Text SR, whose Value Types exclude NUM and SCOORD, a text
/// report.
const DOCUMENTS: [Document; 5] = [
    ("kos", KOS_STORAGE, "A.35.4", key_object_selection),
    ("basic-text-sr", BASIC_TEXT_SR_STORAGE, "A.35.1", basic_text),
    (
        "enhanced-sr",
        ENHANCED_SR_STORAGE,
        "A.35.2",
        measurement_report,
    ),
    (
        "comprehensive-sr",
        COMPREHENSIVE_SR_STORAGE,
        "A.35.3",
        measurement_report,
    ),
    (
        "comprehensive-3d-sr",
        COMPREHENSIVE_3D_SR_STORAGE,
        "A.35.13",
        measurement_report,
    ),
];

fn content_item(
    relationship: &str,
    value_type: &str,
    concept: InMemDicomObject,
    mut elements: Vec<InMemElement>,
) -> InMemDicomObject {
    elements.extend([
        text(tags::RELATIONSHIP_TYPE, VR::CS, relationship),
        text(tags::VALUE_TYPE, VR::CS, value_type),
        sq(tags::CONCEPT_NAME_CODE_SEQUENCE, vec![concept]),
    ]);
    item(elements)
}

fn image_reference() -> InMemDicomObject {
    item(vec![
        text(
            tags::REFERENCED_SOP_CLASS_UID,
            VR::UI,
            crate::VL_WSI_SOP_CLASS_UID,
        ),
        text(tags::REFERENCED_SOP_INSTANCE_UID, VR::UI, IMAGE),
    ])
}

/// The Patient, General Study, Equipment and SOP Common Attributes of a
/// document of `sop_class`, with its Series and the root CONTAINER titled
/// `title`.
fn document(sop_class: &str, modality: &str, title: InMemDicomObject) -> InMemDicomObject {
    item(vec![
        text(tags::SOP_CLASS_UID, VR::UI, sop_class),
        text(
            tags::SOP_INSTANCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.50.10",
        ),
        text(tags::STUDY_DATE, VR::DA, "19700101"),
        text(tags::CONTENT_DATE, VR::DA, "20261024"),
        text(tags::STUDY_TIME, VR::TM, "000000"),
        text(tags::CONTENT_TIME, VR::TM, "120000"),
        text(tags::ACCESSION_NUMBER, VR::SH, "RESEARCH"),
        text(tags::MODALITY, VR::CS, modality),
        text(tags::MANUFACTURER, VR::LO, "WSI-DICOM test"),
        text(tags::REFERRING_PHYSICIAN_NAME, VR::PN, ""),
        sq(tags::REFERENCED_PERFORMED_PROCEDURE_STEP_SEQUENCE, vec![]),
        text(tags::PATIENT_NAME, VR::PN, "RESEARCH^PLACEHOLDER"),
        text(tags::PATIENT_ID, VR::LO, "RESEARCH"),
        text(tags::PATIENT_BIRTH_DATE, VR::DA, ""),
        text(tags::PATIENT_SEX, VR::CS, ""),
        text(tags::STUDY_INSTANCE_UID, VR::UI, STUDY),
        text(
            tags::SERIES_INSTANCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.50.11",
        ),
        text(tags::STUDY_ID, VR::SH, "1"),
        text(tags::SERIES_NUMBER, VR::IS, "98"),
        text(tags::INSTANCE_NUMBER, VR::IS, "1"),
        text(tags::VALUE_TYPE, VR::CS, "CONTAINER"),
        sq(tags::CONCEPT_NAME_CODE_SEQUENCE, vec![title]),
        text(tags::CONTINUITY_OF_CONTENT, VR::CS, "CONTINUOUS"),
        sq(
            tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE,
            vec![evidence(STUDY)],
        ),
    ])
}

/// A Hierarchical SOP Instance Reference Macro Item naming the image.
fn evidence(study: &str) -> InMemDicomObject {
    item(vec![
        text(tags::STUDY_INSTANCE_UID, VR::UI, study),
        sq(
            tags::REFERENCED_SERIES_SEQUENCE,
            vec![item(vec![
                text(tags::SERIES_INSTANCE_UID, VR::UI, IMAGE_SERIES),
                sq(tags::REFERENCED_SOP_SEQUENCE, vec![image_reference()]),
            ])],
        ),
    ])
}

/// The Observer Context (TID 1002) of a pathologist.
fn observer() -> Vec<InMemDicomObject> {
    vec![
        content_item(
            "HAS OBS CONTEXT",
            "CODE",
            code("DCM", "121005", "Observer Type"),
            vec![sq(
                tags::CONCEPT_CODE_SEQUENCE,
                vec![code("DCM", "121006", "Person")],
            )],
        ),
        content_item(
            "HAS OBS CONTEXT",
            "PNAME",
            code("DCM", "121008", "Person Observer Name"),
            vec![text(tags::PERSON_NAME, VR::PN, "Pathologist^Test")],
        ),
    ]
}

/// TID 2010 "Of Interest" flagging the image (PS3.3 A.35.4).
fn key_object_selection(sop_class: &'static str) -> InMemDicomObject {
    let mut object = document(sop_class, "KO", code("DCM", "113000", "Of Interest"));
    let mut content = observer();
    content.push(content_item(
        "CONTAINS",
        "TEXT",
        code("DCM", "113012", "Key Object Description"),
        vec![text(tags::TEXT_VALUE, VR::UT, "Scan accepted")],
    ));
    content.push(content_item(
        "CONTAINS",
        "IMAGE",
        code("SCT", "260753009", "Source"),
        vec![sq(tags::REFERENCED_SOP_SEQUENCE, vec![image_reference()])],
    ));
    object.put(sq(tags::CONTENT_SEQUENCE, content));
    object
}

/// The SR Document General Module Attributes of a completed, unverified
/// report.
fn report(sop_class: &str, title: InMemDicomObject) -> InMemDicomObject {
    let mut object = document(sop_class, "SR", title);
    object.put(text(tags::COMPLETION_FLAG, VR::CS, "COMPLETE"));
    object.put(text(tags::VERIFICATION_FLAG, VR::CS, "UNVERIFIED"));
    object.put(sq(tags::PERFORMED_PROCEDURE_CODE_SEQUENCE, vec![]));
    object
}

fn basic_text(sop_class: &'static str) -> InMemDicomObject {
    let mut object = report(
        sop_class,
        code("DCM", "126000", "Imaging Measurement Report"),
    );
    let mut content = observer();
    content.push(content_item(
        "CONTAINS",
        "TEXT",
        code("DCM", "121071", "Finding"),
        vec![text(tags::TEXT_VALUE, VR::UT, "Adequate tissue")],
    ));
    object.put(sq(tags::CONTENT_SEQUENCE, content));
    object
}

/// A TID 1500 Measurement Report: the area of a region of the slide image.
fn measurement_report(sop_class: &'static str) -> InMemDicomObject {
    let mut object = report(
        sop_class,
        code("DCM", "126000", "Imaging Measurement Report"),
    );
    let area = content_item(
        "CONTAINS",
        "NUM",
        code("SCT", "42798000", "Area"),
        vec![sq(
            tags::MEASURED_VALUE_SEQUENCE,
            vec![item(vec![
                sq(
                    tags::MEASUREMENT_UNITS_CODE_SEQUENCE,
                    vec![code("UCUM", "um2", "square micrometer")],
                ),
                text(tags::NUMERIC_VALUE, VR::DS, "600"),
            ])],
        )],
    );
    let region = content_item(
        "CONTAINS",
        "SCOORD",
        code("DCM", "111030", "Image Region"),
        vec![
            text(tags::GRAPHIC_TYPE, VR::CS, "POLYLINE"),
            DataElement::new(
                tags::GRAPHIC_DATA,
                VR::FL,
                PrimitiveValue::F32(
                    [10.0f32, 10.0, 50.0, 10.0, 50.0, 40.0, 10.0, 10.0]
                        .into_iter()
                        .collect(),
                ),
            ),
            text(tags::PIXEL_ORIGIN_INTERPRETATION, VR::CS, "FRAME"),
            sq(
                tags::CONTENT_SEQUENCE,
                vec![item(vec![
                    text(tags::RELATIONSHIP_TYPE, VR::CS, "SELECTED FROM"),
                    text(tags::VALUE_TYPE, VR::CS, "IMAGE"),
                    sq(tags::REFERENCED_SOP_SEQUENCE, vec![image_reference()]),
                ])],
            ),
        ],
    );
    let group = content_item(
        "CONTAINS",
        "CONTAINER",
        code("DCM", "125007", "Measurement Group"),
        vec![
            text(tags::CONTINUITY_OF_CONTENT, VR::CS, "SEPARATE"),
            sq(tags::CONTENT_SEQUENCE, vec![region, area]),
        ],
    );
    let measurements = content_item(
        "CONTAINS",
        "CONTAINER",
        code("DCM", "126010", "Imaging Measurements"),
        vec![
            text(tags::CONTINUITY_OF_CONTENT, VR::CS, "SEPARATE"),
            sq(tags::CONTENT_SEQUENCE, vec![group]),
        ],
    );
    let mut content = observer();
    content.push(measurements);
    object.put(sq(tags::CONTENT_SEQUENCE, content));
    object
}

/// Writes `object` as `name`.dcm with File Meta Information from its SOP
/// Class and Instance UIDs, then applies `edit` to the Data Set.
fn write(
    dir: &Path,
    name: &str,
    object: InMemDicomObject,
    edit: impl FnOnce(&mut InMemDicomObject),
) -> PathBuf {
    let mut file = object
        .with_meta(FileMetaTableBuilder::new().transfer_syntax(EXPLICIT_VR_LITTLE_ENDIAN))
        .expect("file meta");
    edit(&mut file);
    let path = dir.join(format!("{name}.dcm"));
    file.write_to_file(&path).expect("write document");
    path
}

/// Writes every document into `dir`, so that each IOD's checks run there.
pub(super) fn write_documents(dir: &Path) {
    for (name, sop_class, _, document) in DOCUMENTS {
        write(dir, name, document(sop_class), |_| {});
    }
}

#[test]
fn a_document_of_each_structured_reporting_iod_passes_every_iod_check() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, sop_class, _, document) in DOCUMENTS {
        let path = write(tmp.path(), name, document(sop_class), |_| {});
        let report = iod_report(&path, ValidationProfile::General);
        for check in IOD_CHECKS {
            let found = named(&report, check);
            assert_eq!(
                found.status,
                ValidationStatus::Passed,
                "{name} {check}: {}",
                found.message
            );
        }
        assert!(named(&report, "intrinsic-wsi-dicom-iod-modules")
            .message
            .contains(" A.35."));
    }
}

/// Every rule the catalog credits with a statement in a KOS or SR IOD
/// emits a check on that IOD's document: Checked names a check that runs.
#[test]
fn every_rule_the_catalog_credits_in_a_document_iod_runs_on_its_documents() {
    use crate::validation::{requirement_catalog, RequirementDisposition};

    let tmp = tempfile::tempdir().unwrap();
    let catalog = requirement_catalog();
    for (name, sop_class, iod, document) in DOCUMENTS {
        let path = write(tmp.path(), name, document(sop_class), |_| {});
        let emitted: BTreeSet<String> = iod_report(&path, ValidationProfile::General)
            .checks
            .into_iter()
            .map(|check| check.name)
            .collect();
        let mut credited = 0;
        for entry in catalog.iter().filter(|entry| entry.iods.contains(&iod)) {
            let disposition = entry
                .iod_dispositions
                .get(iod)
                .copied()
                .unwrap_or(entry.disposition);
            if let RequirementDisposition::Checked(rule) = disposition {
                credited += 1;
                assert!(
                    emitted.contains(rule),
                    "{name}: {rule} is credited in {iod} but does not run: {entry:?}"
                );
            }
        }
        assert!(credited > 0, "{name}: no statement of {iod} is Checked");
    }
}

/// Writes the documents for dciodvfy:
/// `WSI_DICOM_IOD_FIXTURES=<dir> cargo test write_structured_report_fixtures -- --ignored`.
#[test]
#[ignore = "writes the documents to $WSI_DICOM_IOD_FIXTURES for dciodvfy"]
fn write_structured_report_fixtures() {
    let dir = std::env::var_os(FIXTURE_DIR_VARIABLE)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("set {FIXTURE_DIR_VARIABLE} to the output directory"));
    std::fs::create_dir_all(&dir).unwrap();
    write_documents(&dir);
}

/// One broken document and the failure it causes.
struct Breakage {
    case: &'static str,
    check: &'static str,
    expected: &'static str,
    edit: fn(&mut InMemDicomObject),
}

/// The first Content Item of the root's Content Sequence that is not
/// Observer Context: the KOS's description, the report's measurements.
fn first_content(object: &mut InMemDicomObject, edit: impl FnOnce(&mut InMemDicomObject)) {
    edit_items(object, tags::CONTENT_SEQUENCE, |items| edit(&mut items[2]));
}

const BREAKAGES: [Breakage; 10] = [
    Breakage {
        case: "no-series-module",
        check: "intrinsic-wsi-dicom-iod-modules",
        expected: "Module (PS3.3 C.17",
        edit: |object| {
            for tag in [
                tags::MODALITY,
                tags::SERIES_INSTANCE_UID,
                tags::SERIES_NUMBER,
                tags::REFERENCED_PERFORMED_PROCEDURE_STEP_SEQUENCE,
            ] {
                object.remove_element(tag);
            }
        },
    },
    // "This root Content Item shall have a heading in the Concept Name Code
    // Sequence (0040,A043)" (C.17.3.1).
    Breakage {
        case: "untitled",
        check: "intrinsic-wsi-dicom-iod-attribute-presence",
        expected: "Concept Name Code Sequence (0040,A043): Type 1C missing (PS3.3 C.17.3.3 Table C.17-5",
        edit: |object| {
            object.remove_element(tags::CONCEPT_NAME_CODE_SEQUENCE);
        },
    },
    // A TEXT Content Item includes the Text Value of Table C.17-5.
    Breakage {
        case: "text-without-value",
        check: "intrinsic-wsi-dicom-iod-attribute-presence",
        expected: "Content Sequence[3] > Text Value (0040,A160): Type 1C missing",
        edit: |object| {
            edit_items(object, tags::CONTENT_SEQUENCE, |items| {
                if let Some(text) = items.iter_mut().find(|item| {
                    item.get(tags::VALUE_TYPE)
                        .and_then(|element| element.to_str().ok())
                        .is_some_and(|value| value == "TEXT")
                }) {
                    text.remove_element(tags::TEXT_VALUE);
                }
            });
        },
    },
    // A by-value Content Item names no Referenced Content Item Identifier.
    Breakage {
        case: "by-value-and-by-reference",
        check: "intrinsic-wsi-dicom-iod-attribute-presence",
        expected: "Content Sequence[3] > Referenced Content Item Identifier (0040,DB73): Type 1C present although its condition is not met",
        edit: |object| {
            first_content(object, |item| {
                item.put(DataElement::new(
                    tags::REFERENCED_CONTENT_ITEM_IDENTIFIER,
                    VR::UL,
                    PrimitiveValue::U32([1u32, 1].into_iter().collect()),
                ));
            });
        },
    },
    Breakage {
        case: "unknown-relationship",
        check: "intrinsic-wsi-dicom-iod-enumerated-values",
        expected: "Content Sequence[3] > Relationship Type (0040,A010) \"OWNS\" is not an Enumerated Value",
        edit: |object| {
            first_content(object, |item| {
                item.put(text(tags::RELATIONSHIP_TYPE, VR::CS, "OWNS"));
            });
        },
    },
    Breakage {
        case: "two-modalities",
        check: "intrinsic-wsi-dicom-iod-vr-vm",
        expected: "(0008,0060)",
        edit: |object| {
            object.put(text(tags::MODALITY, VR::CS, "KO\\SR"));
        },
    },
    Breakage {
        case: "content-date-format",
        check: "intrinsic-wsi-dicom-iod-value-format",
        expected: "(0008,0023)",
        edit: |object| {
            object.put(text(tags::CONTENT_DATE, VR::DA, "2026-10-24"));
        },
    },
    Breakage {
        case: "two-concept-names",
        check: "intrinsic-wsi-dicom-iod-sequence-items",
        expected: "Content Sequence[3] > Concept Name Code Sequence (0040,A043): 2 Items where exactly 1 is required",
        edit: |object| {
            first_content(object, |item| {
                let name = || code("DCM", "121071", "Finding");
                item.put(sq(tags::CONCEPT_NAME_CODE_SEQUENCE, vec![name(), name()]));
            });
        },
    },
    // Current Requested Procedure Evidence Sequence names Studies by the
    // Hierarchical SOP Instance Reference Macro, whose Series reference is
    // Type 1.
    Breakage {
        case: "evidence-without-series",
        check: "intrinsic-wsi-dicom-iod-attribute-presence",
        expected: "Current Requested Procedure Evidence Sequence[1] > Referenced Series Sequence (0008,1115): Type 1 missing",
        edit: |object| {
            first_item(
                object,
                tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE,
                |study| {
                    study.remove_element(tags::REFERENCED_SERIES_SEQUENCE);
                },
            );
        },
    },
    Breakage {
        case: "instance-mismatch",
        check: "intrinsic-wsi-dicom-file-meta",
        expected: "does not match SOP Instance UID (0008,0018)",
        edit: |object| {
            object.put(text(
                tags::SOP_INSTANCE_UID,
                VR::UI,
                "1.2.826.0.1.3680043.10.999.50.99",
            ));
        },
    },
];

#[test]
fn breaking_an_attribute_of_each_family_fails_its_check_in_every_document() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, sop_class, _, document) in DOCUMENTS {
        for breakage in BREAKAGES {
            if breakage.case == "text-without-value" && name != "kos" && name != "basic-text-sr" {
                continue;
            }
            let path = write(
                tmp.path(),
                &format!("{name}-{}", breakage.case),
                document(sop_class),
                breakage.edit,
            );
            let check = iod_check(&path, ValidationProfile::General, breakage.check);
            assert_eq!(
                check.status,
                ValidationStatus::Failed,
                "{name} {}: {}",
                breakage.case,
                check.message
            );
            assert!(
                check.message.contains(breakage.expected),
                "{name} {} lacks {:?}: {}",
                breakage.case,
                breakage.expected,
                check.message
            );
        }
    }
}

#[test]
fn each_content_item_is_held_to_the_macro_of_its_value_type() {
    let tmp = tempfile::tempdir().unwrap();
    let measurement = |edit: fn(&mut InMemDicomObject)| {
        let mut object = measurement_report(COMPREHENSIVE_3D_SR_STORAGE);
        edit_items(&mut object, tags::CONTENT_SEQUENCE, |items| {
            edit_items(&mut items[2], tags::CONTENT_SEQUENCE, |groups| {
                edit_items(&mut groups[0], tags::CONTENT_SEQUENCE, |content| {
                    edit(&mut content[1]);
                });
            });
        });
        object
    };
    // A NUM Content Item includes the Numeric Measurement Macro (C.18.1).
    let path = write(
        tmp.path(),
        "num-without-value",
        measurement(|area| {
            area.remove_element(tags::MEASURED_VALUE_SEQUENCE);
        }),
        |_| {},
    );
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-iod-attribute-presence",
        "Content Sequence[3] > Content Sequence[1] > Content Sequence[2] > Measured Value Sequence (0040,A300): Type 2 missing (PS3.3 C.18.1 Table C.18.1-1)",
    );
    // A CODE Content Item does not: the same Item as CODE needs its Concept
    // Code Sequence and not a Measured Value Sequence.
    let path = write(
        tmp.path(),
        "num-as-code",
        measurement(|area| {
            area.put(text(tags::VALUE_TYPE, VR::CS, "CODE"));
        }),
        |_| {},
    );
    let check = iod_check(
        &path,
        ValidationProfile::General,
        "intrinsic-wsi-dicom-iod-attribute-presence",
    );
    assert!(
        check.message.contains(
            "Concept Code Sequence (0040,A168): Type 1 missing (PS3.3 C.18.2 Table C.18.2-1)"
        ),
        "{}",
        check.message
    );
    assert!(
        !check.message.contains("Measured Value Sequence"),
        "{}",
        check.message
    );
}

#[test]
fn a_kos_of_several_studies_lists_its_identical_documents() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(
        tmp.path(),
        "kos-two-studies",
        key_object_selection(KOS_STORAGE),
        |object| {
            object.put(sq(
                tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE,
                vec![
                    evidence(STUDY),
                    evidence("1.2.826.0.1.3680043.10.999.50.20"),
                ],
            ));
        },
    );
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-iod-attribute-presence",
        "Identical Documents Sequence (0040,A525): Type 1C missing",
    );
}

/// A predecessor report whose Purpose of Reference is `purpose`.
fn with_predecessor(dir: &Path, name: &str, purpose: InMemDicomObject) -> PathBuf {
    write(
        dir,
        name,
        measurement_report(COMPREHENSIVE_SR_STORAGE),
        |object| {
            let mut reference = evidence(STUDY);
            first_item(&mut reference, tags::REFERENCED_SERIES_SEQUENCE, |series| {
                first_item(series, tags::REFERENCED_SOP_SEQUENCE, |sop| {
                    sop.put(sq(tags::PURPOSE_OF_REFERENCE_CODE_SEQUENCE, vec![purpose]));
                });
            });
            object.put(sq(tags::PREDECESSOR_DOCUMENTS_SEQUENCE, vec![reference]));
        },
    )
}

/// The Predecessor Documents Sequence binds its Purpose of Reference to DCID
/// 7009 (Table C.17-2), an extensible group: a code outside it without a
/// declared extension is advised, one of its codes is not.
#[test]
fn predecessor_documents_purposes_of_reference_are_held_to_cid_7009() {
    let tmp = tempfile::tempdir().unwrap();
    let outside = "is not in Defined CID 7009";
    let local = with_predecessor(tmp.path(), "local", code("99LOCAL", "p1", "local purpose"));
    assert_iod_passes(&local, "intrinsic-wsi-dicom-iod-value-sets");
    assert_advises(&local, outside);
    let replaced = with_predecessor(
        tmp.path(),
        "replaced",
        code("DCM", "121360", "Replaced report"),
    );
    assert_iod_passes(&replaced, "intrinsic-wsi-dicom-iod-value-sets");
    assert_does_not_advise(&replaced, outside);
}
