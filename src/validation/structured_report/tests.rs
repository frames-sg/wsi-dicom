//! Unit tests of the KOS and SR document rules on in-memory documents: each
//! starts from a conformant document (`fixtures`) and breaks one requirement.

mod document;
mod fixtures;
mod items;
mod relations;
mod set;
mod templates;

use std::path::Path;

use dicom_object::InMemDicomObject;

use super::{run_key_object_selection_check, run_structured_report_check};
use crate::validation::{ValidationCheck, ValidationStatus};
use fixtures::file;

/// The rule's check of `object`, a KOS or an SR document.
fn check(object: InMemDicomObject) -> ValidationCheck {
    let object = file(object);
    run_key_object_selection_check(Path::new("document.dcm"), &object)
        .or_else(|| run_structured_report_check(Path::new("document.dcm"), &object))
        .expect("a KOS or SR document")
}

#[track_caller]
fn assert_passes(object: InMemDicomObject) -> ValidationCheck {
    let check = check(object);
    assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);
    check
}

#[track_caller]
fn assert_fails(object: InMemDicomObject, expected: &str) -> ValidationCheck {
    let check = check(object);
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
    assert!(
        check.message.contains(expected),
        "lacks {expected:?}: {}",
        check.message
    );
    check
}

/// The conformant documents and one broken document per kind of departure
/// the oracles also report, by file name.
fn oracle_documents() -> Vec<(&'static str, InMemDicomObject)> {
    use dicom_dictionary_std::tags;
    use fixtures::*;
    let mut kos_code = key_object_selection();
    push_child(
        &mut kos_code,
        &[],
        content(
            "CONTAINS",
            "CODE",
            Some(code("DCM", "121071", "Finding")),
            vec![sq(
                tags::CONCEPT_CODE_SEQUENCE,
                vec![code("SCT", "85756007", "Tissue")],
            )],
        ),
    );
    let mut by_reference_contains = measurement_report(COMPREHENSIVE_SR);
    push_child(
        &mut by_reference_contains,
        &GROUP,
        by_reference("CONTAINS", &[1, 4, 1, 4]),
    );
    let mut ancestor = measurement_report(COMPREHENSIVE_SR);
    push_child(
        &mut ancestor,
        &AREA,
        by_reference("INFERRED FROM", &[1, 4, 1]),
    );
    let mut unselected = measurement_report(COMPREHENSIVE_SR);
    set_children(&mut unselected, &REGION, vec![]);
    let mut inferred = measurement_report(COMPREHENSIVE_SR);
    push_child(
        &mut inferred,
        &AREA,
        by_reference("INFERRED FROM", &[1, 4, 1, 4]),
    );
    let mut untitled_template = key_object_selection();
    untitled_template.remove_element(tags::CONTENT_TEMPLATE_SEQUENCE);
    vec![
        ("kos", key_object_selection()),
        ("basic-text-sr", text_report()),
        ("enhanced-sr", measurement_report(ENHANCED_SR)),
        ("comprehensive-sr", measurement_report(COMPREHENSIVE_SR)),
        (
            "comprehensive-3d-sr",
            measurement_report(COMPREHENSIVE_3D_SR),
        ),
        ("by-reference-sr", inferred),
        ("kos-contains-code", kos_code),
        ("sr-contains-by-reference", by_reference_contains),
        ("sr-ancestor-reference", ancestor),
        ("sr-scoord-unselected", unselected),
        ("kos-no-template", untitled_template),
    ]
}

/// Writes `oracle_documents` for dsrdump, dciodvfy and highdicom:
/// `WSI_DICOM_SR_FIXTURES=<dir> cargo test write_structured_report_rule_fixtures -- --ignored`.
#[test]
#[ignore = "writes the documents to $WSI_DICOM_SR_FIXTURES for the oracles"]
fn write_structured_report_rule_fixtures() {
    let dir = std::env::var_os("WSI_DICOM_SR_FIXTURES")
        .map(std::path::PathBuf::from)
        .expect("set WSI_DICOM_SR_FIXTURES to the output directory");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, object) in oracle_documents() {
        file(object)
            .write_to_file(dir.join(format!("{name}.dcm")))
            .unwrap();
    }
}

/// Each oracle document passes or fails the rule as its name says.
#[test]
fn the_oracle_documents_pass_or_fail_as_named() {
    for (name, object) in oracle_documents() {
        let check = check(object);
        let broken = name.contains('-') && !name.ends_with("-sr");
        let expected = if broken {
            ValidationStatus::Failed
        } else {
            ValidationStatus::Passed
        };
        assert_eq!(check.status, expected, "{name}: {}", check.message);
    }
}
