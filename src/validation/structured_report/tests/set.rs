//! The set checks: what a document references compared with the Instances
//! the validated set holds, and Identical Documents with each other.

use std::path::Path;

use dicom_core::VR;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::fixtures::*;
use crate::validation::structured_report::{
    run_structured_report_set_checks, StructuredReportCorpus,
};
use crate::validation::{ValidationCheck, ValidationStatus};

const DUPLICATE: &str = "1.2.826.0.1.3680043.10.999.60.70";

/// The slide image: a tiled VL WSI Instance of 4 Frames of 64 x 64 in a
/// 128 x 128 Total Pixel Matrix.
fn slide() -> InMemDicomObject {
    item(vec![
        text(tags::SOP_CLASS_UID, VR::UI, WSI),
        text(tags::SOP_INSTANCE_UID, VR::UI, IMAGE),
        text(tags::STUDY_INSTANCE_UID, VR::UI, STUDY),
        text(tags::NUMBER_OF_FRAMES, VR::IS, "4"),
        text(tags::ROWS, VR::US, "64"),
        text(tags::COLUMNS, VR::US, "64"),
        text(tags::TOTAL_PIXEL_MATRIX_ROWS, VR::UL, "128"),
        text(tags::TOTAL_PIXEL_MATRIX_COLUMNS, VR::UL, "128"),
    ])
}

fn set_checks(objects: Vec<InMemDicomObject>) -> Vec<ValidationCheck> {
    let mut corpus = StructuredReportCorpus::default();
    for (index, object) in objects.into_iter().enumerate() {
        corpus.observe(Path::new(&format!("object-{index}.dcm")), &file(object));
    }
    run_structured_report_set_checks(&corpus)
}

#[track_caller]
fn only(checks: &[ValidationCheck], rule: &str) -> ValidationCheck {
    let found: Vec<&ValidationCheck> = checks.iter().filter(|check| check.name == rule).collect();
    assert_eq!(found.len(), 1, "{checks:?}");
    found[0].clone()
}

#[track_caller]
fn assert_set_fails(objects: Vec<InMemDicomObject>, rule: &str, expected: &str) {
    let check = only(&set_checks(objects), rule);
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
    assert!(
        check.message.contains(expected),
        "lacks {expected:?}: {}",
        check.message
    );
}

const SR_RULE: &str = "intrinsic-wsi-dicom-structured-report";
const KOS_RULE: &str = "intrinsic-wsi-dicom-key-object-selection";

#[test]
fn each_rule_reports_once_and_only_with_its_documents() {
    assert!(set_checks(vec![slide()]).is_empty());
    let checks = set_checks(vec![
        key_object_selection(),
        measurement_report(COMPREHENSIVE_SR),
        slide(),
    ]);
    assert_eq!(
        only(&checks, KOS_RULE).status,
        ValidationStatus::Passed,
        "{checks:?}"
    );
    assert_eq!(
        only(&checks, SR_RULE).status,
        ValidationStatus::Passed,
        "{checks:?}"
    );
    assert!(checks.iter().all(|check| check.path.is_none()));
    let alone = only(&set_checks(vec![key_object_selection()]), KOS_RULE);
    assert_eq!(alone.status, ValidationStatus::Skipped);
    assert!(
        alone
            .message
            .contains("no Instance a document references is in the validated set"),
        "{}",
        alone.message
    );
}

#[test]
fn a_reference_names_the_referenced_instances_sop_class() {
    let mut image = slide();
    image.put(text(
        tags::SOP_CLASS_UID,
        VR::UI,
        "1.2.840.10008.5.1.4.1.1.66.4",
    ));
    assert_set_fails(
        vec![key_object_selection(), image],
        KOS_RULE,
        &format!("object-0.dcm Content Item 1.2 references SOP Instance {IMAGE} with Referenced SOP Class UID {WSI}, but object-1.dcm has SOP Class UID 1.2.840.10008.5.1.4.1.1.66.4; Referenced SOP Class UID uniquely identifies the referenced SOP Class (PS3.3 2026d Table 10-11)"),
    );
}

#[test]
fn referenced_frames_are_frames_of_the_image() {
    let with_frames = |frames: &str| {
        let mut object = key_object_selection();
        edit_first(&mut object, &[1], tags::REFERENCED_SOP_SEQUENCE, |sop| {
            sop.put(text(tags::REFERENCED_FRAME_NUMBER, VR::IS, frames));
        });
        object
    };
    let check = only(&set_checks(vec![with_frames("1\\4"), slide()]), KOS_RULE);
    assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);
    assert_set_fails(
        vec![with_frames("2\\5"), slide()],
        KOS_RULE,
        "object-0.dcm Content Item 1.2 references Frame 5 of object-1.dcm, which has 4 Frames; Referenced Frame Number identifies the Frame numbers within the Referenced SOP Instance (PS3.3 2026d Table C.18.4-1)",
    );
}

/// The report whose SCOORD is `values` relative to `origin`.
fn outlined(values: &[f32], origin: Option<&str>) -> InMemDicomObject {
    let mut object = measurement_report(COMPREHENSIVE_SR);
    edit_at(&mut object, &REGION, |region| {
        region.put(floats(tags::GRAPHIC_DATA, values));
        match origin {
            Some(origin) => {
                region.put(text(tags::PIXEL_ORIGIN_INTERPRETATION, VR::CS, origin));
            }
            None => {
                region.remove_element(tags::PIXEL_ORIGIN_INTERPRETATION);
            }
        }
    });
    object
}

#[test]
fn graphic_data_lies_within_the_image() {
    let outline = [10.0, 10.0, 128.0, 10.0, 128.0, 100.0, 10.0, 10.0];
    let check = only(
        &set_checks(vec![outlined(&outline, Some("VOLUME")), slide()]),
        SR_RULE,
    );
    assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);
    assert_set_fails(
        vec![outlined(&outline, Some("FRAME")), slide()],
        SR_RULE,
        "object-0.dcm Content Item 1.4.1.4 has Graphic Data reaching column 128 and row 100 of object-1.dcm, whose Columns\\Rows are 64\\64; the values must be within the range 0\\0 to Columns\\Rows (PS3.3 2026d Table C.18.6-1)",
    );
    let mut wide = outline;
    wide[2] = 130.0;
    assert_set_fails(
        vec![outlined(&wide, Some("VOLUME")), slide()],
        SR_RULE,
        "whose Total Pixel Matrix Columns\\Rows are 128\\128",
    );
}

#[test]
fn an_scoord_on_a_tiled_image_has_a_pixel_origin_interpretation() {
    assert_set_fails(
        vec![outlined(&[10.0, 10.0, 20.0, 20.0], None), slide()],
        SR_RULE,
        "object-0.dcm Content Item 1.4.1.4 is an SCOORD selected from object-1.dcm, a tiled image (it has Total Pixel Matrix Columns and Rows), without Pixel Origin Interpretation (0048,0301); it is required if the referenced instance is tiled (PS3.3 2026d Table C.18.6-1)",
    );
}

/// A KOS and its duplicate in the other Study, each listing the other.
fn duplicates() -> (InMemDicomObject, InMemDicomObject) {
    let mut first = key_object_selection();
    first.put(sq(
        tags::IDENTICAL_DOCUMENTS_SEQUENCE,
        vec![evidence(
            OTHER_STUDY,
            "1.2.826.0.1.3680043.10.999.60.71",
            KOS,
            DUPLICATE,
        )],
    ));
    let mut second = key_object_selection();
    second.put(text(tags::SOP_INSTANCE_UID, VR::UI, DUPLICATE));
    second.put(text(tags::STUDY_INSTANCE_UID, VR::UI, OTHER_STUDY));
    second.put(sq(
        tags::IDENTICAL_DOCUMENTS_SEQUENCE,
        vec![evidence(
            STUDY,
            "1.2.826.0.1.3680043.10.999.60.11",
            KOS,
            DOCUMENT,
        )],
    ));
    (first, second)
}

#[test]
fn identical_documents_list_each_other() {
    let (first, second) = duplicates();
    let check = only(&set_checks(vec![first.clone(), second.clone()]), KOS_RULE);
    assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);
    let mut silent = second;
    silent.remove_element(tags::IDENTICAL_DOCUMENTS_SEQUENCE);
    assert_set_fails(
        vec![first, silent],
        KOS_RULE,
        &format!("object-0.dcm lists object-1.dcm in Identical Documents Sequence (0040,A525), but that duplicate's Identical Documents Sequence does not reference [\"{DOCUMENT}\"]; the Identical Documents Sequence in each SOP Instance shall contain references to all other duplicate SOP Instances (PS3.3 2026d C.17.2.2)"),
    );
}

#[test]
fn identical_documents_are_the_entire_document_in_another_study() {
    let (first, mut second) = duplicates();
    second.put(sq(
        tags::CONCEPT_NAME_CODE_SEQUENCE,
        vec![code("DCM", "113000", "Of Interest")],
    ));
    assert_set_fails(
        vec![first, second],
        KOS_RULE,
        "but their Document Titles or Content Sequences differ; the entire document shall be duplicated",
    );
    let (first, mut second) = duplicates();
    second.put(text(tags::STUDY_INSTANCE_UID, VR::UI, STUDY));
    assert_set_fails(
        vec![first, second],
        KOS_RULE,
        &format!("object-0.dcm and its duplicate object-1.dcm are both in Study {STUDY}; identical copies of a document are duplicated for inclusion into different Studies"),
    );
}
