//! The rules that hold the pathology-slide objects to statements a Key
//! Object Selection Document or SR document shares also hold the documents:
//! each valid document passes them, no image or pixel rule runs on it, and
//! breaking a representative statement of each rule fails it in each
//! document IOD.

use super::*;

const RELATIONS: &str = "intrinsic-wsi-dicom-attribute-relations";
const IDENTITY_SET: &str = "intrinsic-wsi-dicom-identity-set";
const SERIES_SET: &str = "intrinsic-wsi-dicom-series-set";
/// The per-object rules a document is held to.
const DOCUMENT_RULES: [&str; 3] = ["intrinsic-wsi-dicom-icc-profile", RELATIONS, IDENTITY_SET];
/// The rules of images and their pixels, none of which runs on a document.
/// The core profile's scope check, which fails every object but a VL WSI
/// image, is not one of them.
const IMAGE_RULES: [&str; 12] = [
    "intrinsic-wsi-dicom-image-metadata",
    "intrinsic-wsi-dicom-wsi-image",
    "intrinsic-wsi-dicom-pixel-attributes",
    "intrinsic-wsi-dicom-codestream",
    "intrinsic-wsi-dicom-lossy-history",
    "intrinsic-wsi-dicom-dimension-order",
    "intrinsic-wsi-dicom-monochrome-presentation",
    "intrinsic-wsi-dicom-specimen-identity",
    "intrinsic-wsi-dicom-tile-geometry",
    "intrinsic-wsi-dicom-slide-coordinate-system",
    "intrinsic-wsi-dicom-optical-path-structure",
    "intrinsic-wsi-dicom-specimen-container",
];

/// The check `name` of the object at `path`, or of the set when `path` is
/// `None`.
fn check_of(report: &ValidationReport, name: &str, path: Option<&Path>) -> ValidationCheck {
    report
        .checks
        .iter()
        .find(|check| check.name == name && check.path.as_deref() == path)
        .cloned()
        .unwrap_or_else(|| panic!("no {name} for {path:?}: {:#?}", report.checks))
}

#[track_caller]
fn assert_check(check: &ValidationCheck, status: ValidationStatus, expected: &str) {
    assert_eq!(check.status, status, "{}: {}", check.name, check.message);
    assert!(
        check.message.contains(expected),
        "{} lacks {expected:?}: {}",
        check.name,
        check.message
    );
}

#[test]
fn each_valid_document_passes_the_document_rules_and_no_image_rule_runs() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, sop_class, iod, document) in DOCUMENTS {
        let dir = tmp.path().join(name);
        std::fs::create_dir(&dir).unwrap();
        let path = write(&dir, name, document(sop_class), |_| {});
        for profile in [ValidationProfile::General, ValidationProfile::Core] {
            let report = iod_report(&dir, profile);
            for rule in DOCUMENT_RULES {
                assert_check(
                    &check_of(&report, rule, Some(&path)),
                    ValidationStatus::Passed,
                    "conformant",
                );
            }
            for rule in [IDENTITY_SET, SERIES_SET] {
                assert_check(
                    &check_of(&report, rule, None),
                    ValidationStatus::Passed,
                    "conformant",
                );
            }
            for rule in IMAGE_RULES {
                assert!(
                    report
                        .checks
                        .iter()
                        .all(|check| check.name != rule || check.path.as_deref() != Some(&path)),
                    "{name} ({iod}): {rule} ran on a document"
                );
            }
        }
    }
}

#[test]
fn breaking_a_statement_of_each_document_rule_fails_it_in_each_document_iod() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, sop_class, _, document) in DOCUMENTS {
        let dir = tmp.path().join(name);
        std::fs::create_dir(&dir).unwrap();

        // PS3.3 8.6: a DCMR Context Identifier is its number, without
        // leading zeros.
        let path = write(&dir, "context", document(sop_class), |object| {
            first_item(object, tags::CONCEPT_NAME_CODE_SEQUENCE, |title| {
                title.put(text(tags::CONTEXT_IDENTIFIER, VR::CS, "07009"));
                title.put(text(tags::MAPPING_RESOURCE, VR::CS, "DCMR"));
            });
        });
        assert_check(
            &check_of(
                &iod_report(&path, ValidationProfile::General),
                RELATIONS,
                Some(&path),
            ),
            ValidationStatus::Failed,
            "Context Identifier (0008,010F) \"07009\" of a DCMR Context Group shall be its number",
        );

        // PS3.5 9: one UID names one entity, so a Series Instance UID is
        // not also the Study Instance UID.
        let path = write(&dir, "uid-roles", document(sop_class), |object| {
            object.put(text(tags::SERIES_INSTANCE_UID, VR::UI, STUDY));
        });
        assert_check(
            &check_of(
                &iod_report(&path, ValidationProfile::General),
                SERIES_SET,
                None,
            ),
            ValidationStatus::Failed,
            "UID (value withheld: identifying; 31 characters) is both a",
        );

        // PS3.5 9: two documents of one SOP Instance UID and SOP Class, but
        // different content, are copies whose Attributes differ.
        let copies = tmp.path().join(format!("{name}-copies"));
        std::fs::create_dir(&copies).unwrap();
        write(&copies, "first", document(sop_class), |_| {});
        write(&copies, "second", document(sop_class), |object| {
            object.put(text(tags::CONTENT_TIME, VR::TM, "130000"));
        });
        assert_check(
            &check_of(
                &iod_report(&copies, ValidationProfile::General),
                IDENTITY_SET,
                None,
            ),
            ValidationStatus::Passed,
            "ContentTime (0008,0033)",
        );
    }
}

#[test]
fn a_reference_to_frame_zero_fails_the_relations_rule_in_a_key_object_selection() {
    let tmp = tempfile::tempdir().unwrap();
    // PS3.3 Table 10-3: Frames are numbered from 1.
    let path = write(
        tmp.path(),
        "kos",
        key_object_selection(KOS_STORAGE),
        |object| {
            let Ok(content) = object.element(tags::CONTENT_SEQUENCE) else {
                panic!("KOS has no Content Sequence");
            };
            let mut items = content.items().unwrap().to_vec();
            let image = items
                .iter_mut()
                .find(|item| {
                    item.element(tags::VALUE_TYPE)
                        .ok()
                        .and_then(|value| value.to_str().ok())
                        .as_deref()
                        == Some("IMAGE")
                })
                .unwrap();
            let mut references = image
                .element(tags::REFERENCED_SOP_SEQUENCE)
                .unwrap()
                .items()
                .unwrap()
                .to_vec();
            references[0].put(DataElement::new(
                tags::REFERENCED_FRAME_NUMBER,
                VR::IS,
                PrimitiveValue::from("0"),
            ));
            image.put(sq(tags::REFERENCED_SOP_SEQUENCE, references));
            object.put(sq(tags::CONTENT_SEQUENCE, items));
        },
    );
    assert_check(
        &check_of(
            &iod_report(&path, ValidationProfile::General),
            RELATIONS,
            Some(&path),
        ),
        ValidationStatus::Failed,
        "Referenced Frame Number (0008,1160) 0 is below 1",
    );
}
