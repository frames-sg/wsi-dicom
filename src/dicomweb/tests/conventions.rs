//! The convention profiles on what an origin server returns: they run on
//! the Series Metadata, with the Transfer Syntax the Frames response names,
//! and the retrieved Instance adds only its DICOM checks.

use std::sync::Arc;

use super::server::{run, slide};
use crate::{ConventionProfile, ValidationStatus};

#[test]
fn conventions_run_on_the_series_metadata_and_gate_only_when_asked() {
    let (_root, slide) = slide();
    let ungated = run(Arc::clone(&slide), &[], |options| {
        options.validation.conventions =
            [ConventionProfile::Wg26, ConventionProfile::IheDpiaDraft].into();
    });
    let conventions = &ungated.report.objects.conventions;
    assert_eq!(
        conventions.profiles,
        [ConventionProfile::Wg26, ConventionProfile::IheDpiaDraft]
    );
    // Each result is of a Data Set of the Metadata, or of its Series.
    assert!(conventions.checks.iter().all(|check| check
        .path
        .as_ref()
        .is_none_or(|path| path.to_string_lossy().contains("/metadata#/"))));
    let check = |name: &str| {
        conventions
            .checks
            .iter()
            .find(|check| check.name == name)
            .unwrap_or_else(|| panic!("no {name}: {:#?}", conventions.checks))
    };
    // The Frames response named the Instance's Transfer Syntax.
    let syntax = check("convention-wg26-transfer-syntax");
    assert_eq!(
        syntax.status,
        ValidationStatus::Passed,
        "{}",
        syntax.message
    );
    // The Metadata holds the Pixel Data as a BulkDataURI only.
    let offsets = check("convention-ihe-dpia-draft-offset-table");
    assert_eq!(offsets.status, ValidationStatus::Skipped);
    assert!(
        offsets.message.contains("BulkDataURI"),
        "{}",
        offsets.message
    );
    // The exported research slide records no specimen preparation.
    assert_eq!(
        check("convention-wg26-specimen-preparation").status,
        ValidationStatus::Failed
    );
    assert!(!ungated.report.has_failures());

    let gated = run(Arc::clone(&slide), &[], |options| {
        options.validation.conventions = [ConventionProfile::Wg26].into();
        options.validation.fail_on_conventions = true;
    });
    assert!(gated.report.objects.conventions.fails_validation());
    assert!(gated.report.has_failures());
    assert_eq!(gated.report.objects.failed_checks(), 0);
    assert_eq!(gated.report.failed_checks(), 0);
}
