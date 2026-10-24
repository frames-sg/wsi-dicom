//! The profiles on DICOM JSON: a scan's Metadata meets them as its files do,
//! and the conventions that read the Transfer Syntax or the encoded Pixel
//! Data, which Metadata does not hold, report that instead of a result.

use dicom_core::VR;
use dicom_dictionary_std::{tags, uids};

use super::fixture::text;
use super::wg26::edit_specimen;
use super::*;
use crate::dicomweb::tests::fixture::to_json;
use crate::validation::{validate_dicom_json_path, DicomJsonOptions};

const ALL: [ConventionProfile; 5] = [
    ConventionProfile::Wg26,
    ConventionProfile::IheDpia,
    ConventionProfile::IheDpiaDraft,
    ConventionProfile::IheDpiaDraftJpeg,
    ConventionProfile::Interop,
];

/// The checks that read the Transfer Syntax or the encoded Pixel Data.
const NEED_BYTES: [&str; 3] = [
    "convention-wg26-transfer-syntax",
    "convention-ihe-dpia-draft-jpeg-baseline",
    "convention-ihe-dpia-draft-offset-table",
];

/// The scan as one DICOM JSON document, its ICC Profiles and Pixel Data as
/// BulkDataURIs, written to `dir/scan.json`.
fn document(scan: &Scan, dir: &Path) -> PathBuf {
    let data_sets: Vec<String> = scan
        .objects
        .iter()
        .map(|(_, object)| {
            let json = to_json(object, &|tag, top| match tag {
                tags::ICC_PROFILE => Some("bulk/icc".to_string()),
                tags::PIXEL_DATA if top => Some("bulk/pixeldata".to_string()),
                _ => None,
            });
            json[1..json.len() - 1].to_string()
        })
        .collect();
    let path = dir.join("scan.json");
    std::fs::write(&path, format!("[{}]", data_sets.join(","))).expect("write document");
    path
}

fn json_report(
    path: &Path,
    profiles: &[ConventionProfile],
    configure: impl FnOnce(&mut DicomJsonOptions),
) -> ValidationReport {
    let mut options = DicomJsonOptions::default();
    options.validation.conventions = profiles.iter().copied().collect();
    configure(&mut options);
    validate_dicom_json_path(path, &options).expect("JSON validation report")
}

#[test]
fn a_scans_metadata_meets_every_profile_and_reports_what_it_lacks() {
    let dir = FixtureDir::new("json-conformant");
    let report = json_report(&document(&Scan::conformant(120), &dir.path), &ALL, |_| {});
    assert_eq!(
        names_with(&report, ValidationStatus::Failed),
        BTreeSet::new(),
        "{}",
        describe(&report)
    );
    assert_eq!(
        advised(&report),
        [super::ihe_dpia_draft::KOS_NOTE.to_string()].into()
    );
    let skipped: Vec<&super::super::ConventionCheck> = report
        .conventions
        .checks
        .iter()
        .filter(|check| check.status == ValidationStatus::Skipped)
        .collect();
    // Each VL WSI Data Set: the three checks, and the interop advisories
    // that read the same.
    let mut expected: BTreeSet<String> = NEED_BYTES.iter().map(|name| name.to_string()).collect();
    expected.insert("convention-interop-evaluable".into());
    assert_eq!(
        skipped
            .iter()
            .map(|check| check.name.clone())
            .collect::<BTreeSet<_>>(),
        expected
    );
    assert_eq!(skipped.len(), 4 * WSI.len(), "{}", describe(&report));
    for check in &skipped {
        let path = check.path.as_ref().expect("a Data Set");
        assert!(path.to_string_lossy().contains("scan.json#/"), "{path:?}");
        assert!(
            check.message.contains("needs the Transfer Syntax")
                || check.message.contains("needs the encoded Pixel Data"),
            "{}",
            check.message
        );
    }
    // Every other check passed on the Data Sets as it does on the files.
    assert!(report
        .conventions
        .checks
        .iter()
        .any(|check| check.name == "convention-wg26-pyramid"
            && check.status == ValidationStatus::Passed));
}

#[test]
fn a_known_transfer_syntax_lets_its_checks_run() {
    let dir = FixtureDir::new("json-with-syntax");
    let report = json_report(
        &document(&Scan::conformant(121), &dir.path),
        &[ConventionProfile::Wg26, ConventionProfile::IheDpiaDraftJpeg],
        |options| options.transfer_syntax = Some(uids::JPEG_BASELINE8_BIT.into()),
    );
    let skipped = names_with(&report, ValidationStatus::Skipped);
    assert!(
        NEED_BYTES[..2].iter().all(|name| !skipped.contains(*name)),
        "{}",
        describe(&report)
    );
    assert_eq!(
        names_with(&report, ValidationStatus::Failed),
        BTreeSet::new(),
        "{}",
        describe(&report)
    );
}

#[test]
fn a_broken_data_set_fails_its_convention_check() {
    let scan = Scan::conformant(122).edit_images(|object| {
        edit_specimen(object, |specimen| {
            specimen.remove_element(tags::PRIMARY_ANATOMIC_STRUCTURE_SEQUENCE);
        });
    });
    let dir = FixtureDir::new("json-broken");
    let report = json_report(
        &document(&scan, &dir.path),
        &[ConventionProfile::Wg26],
        |_| {},
    );
    assert_eq!(
        names_with(&report, ValidationStatus::Failed),
        ["convention-wg26-sp2025-primary-anatomic-structure".to_string()].into()
    );
}

#[test]
fn a_document_that_is_not_dicom_json_is_reported_not_evaluated() {
    let dir = FixtureDir::new("json-unreadable");
    let path = dir.path.join("broken.json");
    std::fs::write(&path, "{ not json").expect("write document");
    let report = json_report(&path, &[ConventionProfile::Interop], |_| {});
    assert_eq!(report.conventions.checks.len(), 1);
    let check = &report.conventions.checks[0];
    assert_eq!(check.name, "convention-interop-evaluable");
    assert_eq!(check.status, ValidationStatus::Skipped);
    assert!(check.message.contains("could not be read as DICOM JSON"));
}

#[test]
fn fail_on_conventions_gates_a_json_document_on_failed_checks_only() {
    // A Barcode Value that is not a DICOM requirement but a WG-26 one.
    let failing = Scan::conformant(123).edit(LABEL, |object| {
        object.put(text(tags::BARCODE_VALUE, VR::LT, ""));
    });
    let dir = FixtureDir::new("json-gate");
    let path = document(&failing, &dir.path);
    let ungated = json_report(&path, &[ConventionProfile::Wg26], |_| {});
    assert!(ungated.conventions.has_failures());
    assert!(!ungated.has_failures(), "{:#?}", dicom_failures(&ungated));
    let gated = json_report(&path, &[ConventionProfile::Wg26], |options| {
        options.validation.fail_on_conventions = true;
    });
    assert!(gated.has_failures());
    assert_eq!(gated.checks, ungated.checks);
}
