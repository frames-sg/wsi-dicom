//! What holds across profiles: the conformant scan passes every check and
//! triggers only the CP-LAB-281 conflict note, every catalogued check reports
//! a result on it,
//! selecting profiles leaves the DICOM checks unchanged, and the catalog
//! names and cites every convention.

use super::*;
use crate::validation::conventions::{convention_catalog, ConventionKind};

const ALL: [ConventionProfile; 5] = [
    ConventionProfile::Wg26,
    ConventionProfile::IheDpia,
    ConventionProfile::IheDpiaDraft,
    ConventionProfile::IheDpiaDraftJpeg,
    ConventionProfile::Interop,
];

#[test]
fn the_conformant_scan_meets_dicom_and_every_profile() {
    let scan = Scan::conformant(1);
    // Every completion KOS carries the CP-LAB-281 conflict note.
    let report = assert_outcome(
        "conformant",
        &scan,
        &ALL,
        &[],
        &[super::ihe_dpia_draft::KOS_NOTE],
    );
    assert_eq!(
        names_with(&report, ValidationStatus::Skipped),
        BTreeSet::new(),
        "{}",
        describe(&report)
    );
    // The VL WSI images also pass every intrinsic DICOM check.
    let failures: Vec<String> = report
        .checks
        .iter()
        .filter(|check| check.name.starts_with("intrinsic-"))
        .filter(|check| {
            check.path.as_ref().is_none_or(|path| {
                WSI.iter()
                    .any(|name| path.file_stem().is_some_and(|stem| stem == *name))
            })
        })
        .filter(|check| check.status != ValidationStatus::Passed)
        .map(|check| format!("{} {:?}: {}", check.name, check.path, check.message))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_catalogued_check_reports_a_result_on_the_conformant_scan() {
    let dir = FixtureDir::new("catalog-coverage");
    let report = report(&Scan::conformant(2).write(&dir.path), &ALL);
    let reported: BTreeSet<&str> = report
        .conventions
        .checks
        .iter()
        .map(|check| check.name.as_str())
        .collect();
    let missing: Vec<&str> = convention_catalog()
        .into_iter()
        .filter(|entry| entry.kind == ConventionKind::Check)
        .filter(|entry| !entry.name.ends_with("-evaluable"))
        .map(|entry| entry.name)
        .filter(|name| !reported.contains(name))
        .collect();
    assert!(missing.is_empty(), "never reported: {missing:?}");
    assert!(
        report
            .conventions
            .checks
            .iter()
            .all(|check| check.status == ValidationStatus::Passed),
        "{}",
        describe(&report)
    );
}

#[test]
fn conventions_do_not_change_dicom_results() {
    let dir = FixtureDir::new("dicom-unchanged");
    // A scan that fails conventions of every profile.
    let scan = Scan::conformant(3).edit_images(|object| {
        object.remove_element(dicom_dictionary_std::tags::SPECIMEN_DESCRIPTION_SEQUENCE);
        object.put(fixture::text(
            dicom_dictionary_std::tags::MODALITY,
            dicom_core::VR::CS,
            "OT",
        ));
    });
    let path = scan.write(&dir.path);
    let without = report(&path, &[]);
    let with = report(&path, &ALL);
    assert!(with.conventions.has_failures());
    assert!(!with.conventions.advisories.is_empty());
    assert_eq!(without.checks, with.checks);
    assert_eq!(without.optional, with.optional);
    assert_eq!(without.has_failures(), with.has_failures());
    assert!(without.conventions.is_empty());
    let json = serde_json::to_value(&without).expect("report JSON");
    assert!(json.get("conventions").is_none(), "{json}");
    let json = serde_json::to_value(&with).expect("report JSON");
    assert_eq!(json["conventions"]["profiles"][0], "wg26");
}

#[test]
fn only_selected_profiles_report() {
    let dir = FixtureDir::new("selected-only");
    let report = report(
        &Scan::conformant(4).write(&dir.path),
        &[ConventionProfile::IheDpia],
    );
    assert!(!report.conventions.checks.is_empty());
    assert!(report
        .conventions
        .checks
        .iter()
        .all(|check| check.profile == ConventionProfile::IheDpia
            && check.name.starts_with("convention-ihe-dpia-")));
}

#[test]
fn an_unparsable_file_is_reported_not_evaluated_by_each_profile() {
    let dir = FixtureDir::new("unparsable");
    let path = Scan::conformant(5).write(&dir.path);
    std::fs::write(path.join("broken.dcm"), b"not DICOM").expect("write broken file");
    let report = report(
        &path,
        &[ConventionProfile::Wg26, ConventionProfile::Interop],
    );
    let skipped: Vec<&str> = report
        .conventions
        .checks
        .iter()
        .filter(|check| {
            check.status == ValidationStatus::Skipped
                && check
                    .path
                    .as_ref()
                    .is_some_and(|path| path.ends_with("broken.dcm"))
        })
        .map(|check| check.name.as_str())
        .collect();
    assert_eq!(
        skipped,
        ["convention-wg26-evaluable", "convention-interop-evaluable"]
    );
}

#[test]
fn an_image_without_a_series_is_reported_not_evaluated() {
    let scan = Scan::conformant(6).edit(VOLUME_3, |object| {
        object.remove_element(dicom_dictionary_std::tags::SERIES_INSTANCE_UID);
    });
    let dir = FixtureDir::new("no-series");
    let report = report(&scan.write(&dir.path), &[ConventionProfile::IheDpiaDraft]);
    let skipped: Vec<&super::super::ConventionCheck> = report
        .conventions
        .checks
        .iter()
        .filter(|check| check.name == "convention-ihe-dpia-draft-evaluable")
        .collect();
    assert_eq!(skipped.len(), 1);
    assert!(skipped[0].message.contains("no Series Instance UID"));
}

#[test]
fn the_catalog_names_and_cites_every_convention() {
    let catalog = convention_catalog();
    let names: BTreeSet<&str> = catalog.iter().map(|entry| entry.name).collect();
    assert_eq!(names.len(), catalog.len(), "duplicate convention names");
    for entry in &catalog {
        let prefix = format!("convention-{}-", entry.profile.name());
        assert!(
            entry.name.starts_with(&prefix),
            "{} lacks {prefix}",
            entry.name
        );
        assert!(!entry.source.is_empty() && !entry.requirement.is_empty());
        let draft = matches!(
            entry.profile,
            ConventionProfile::IheDpiaDraft | ConventionProfile::IheDpiaDraftJpeg
        ) && !entry.name.ends_with("-evaluable");
        assert_eq!(
            entry.source.contains("draft (CP under ballot)"),
            draft,
            "{}: {}",
            entry.name,
            entry.source
        );
    }
    for profile in ALL {
        assert!(catalog.iter().any(|entry| entry.profile == profile));
    }
    assert!(catalog
        .iter()
        .filter(|entry| entry.profile == ConventionProfile::Interop)
        .all(|entry| entry.kind == ConventionKind::Advisory || entry.name.ends_with("-evaluable")));
}

#[test]
fn draft_results_say_draft_in_their_messages() {
    let scan = Scan::conformant(7).edit(VOLUME_1, |object| {
        object.put(fixture::text(
            dicom_dictionary_std::tags::DIMENSION_ORGANIZATION_TYPE,
            dicom_core::VR::CS,
            "TILED_SPARSE",
        ));
    });
    let report = assert_outcome(
        "draft-message",
        &scan,
        &[ConventionProfile::IheDpiaDraft],
        &["convention-ihe-dpia-draft-tiled-full"],
        &[super::ihe_dpia_draft::KOS_NOTE],
    );
    let message = failure_message(&report, "convention-ihe-dpia-draft-tiled-full");
    assert!(message.contains("draft (CP under ballot)"), "{message}");
    assert!(message.contains("TILED_SPARSE"), "{message}");
}

#[test]
fn the_jpeg_option_alone_evaluates_only_its_own_check() {
    let dir = FixtureDir::new("jpeg-only");
    let report = report(
        &Scan::conformant(8).write(&dir.path),
        &[ConventionProfile::IheDpiaDraftJpeg],
    );
    let names: BTreeSet<&str> = report
        .conventions
        .checks
        .iter()
        .map(|check| check.name.as_str())
        .collect();
    assert_eq!(names, ["convention-ihe-dpia-draft-jpeg-baseline"].into());
    // One result per VL WSI image, and no advisory.
    assert_eq!(report.conventions.checks.len(), WSI.len());
    assert!(report.conventions.advisories.is_empty());
}

#[test]
fn a_result_shows_twenty_violations_and_counts_the_rest_unformatted() {
    let mut found = super::super::Violations::default();
    let mut formatted = 0;
    for violation in 0..25 {
        found.push(|| {
            formatted += 1;
            format!("violation {violation}")
        });
    }
    let message = found.result().expect_err("violations");
    assert_eq!(formatted, 20);
    assert!(
        message.starts_with("violation 0; violation 1; "),
        "{message}"
    );
    assert!(message.ends_with("violation 19; and 5 more"), "{message}");
    assert_eq!(super::super::Violations::default().result(), Ok(()));
}
