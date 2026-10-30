//! The checks of each IOD family beside VL WSI: the fixture of each IOD
//! passes its family's check, and breaking one requirement fails it.

use super::*;
use dicom_core::value::{PixelFragmentSequence, Value};
use dicom_object::DefaultDicomObject;

mod bulk_annotations;
mod parametric_map;
mod segmentation;
mod set_checks;
mod synchronization;

const SEGMENTATION: &str = "intrinsic-wsi-dicom-segmentation";
const PARAMETRIC_MAP: &str = "intrinsic-wsi-dicom-parametric-map";
const BULK_ANNOTATIONS: &str = "intrinsic-wsi-dicom-bulk-annotations";
const SYNCHRONIZATION: &str = "intrinsic-wsi-dicom-synchronization";

/// The per-object check `rule` of fixture `name` after `edit`, validated
/// from a file.
fn family_check(
    name: &str,
    rule: &str,
    edit: impl FnOnce(&mut InMemDicomObject),
) -> ValidationCheck {
    let tmp = tempfile::tempdir().unwrap();
    let path = broken(tmp.path(), name, "family", edit);
    iod_report(&path, ValidationProfile::General)
        .checks
        .into_iter()
        .find(|check| check.name == rule && check.path.is_some())
        .unwrap_or_else(|| panic!("no {rule} check of {name}"))
}

#[track_caller]
fn assert_family_passes(name: &str, rule: &str, edit: impl FnOnce(&mut InMemDicomObject)) {
    let check = family_check(name, rule, edit);
    assert_eq!(
        check.status,
        ValidationStatus::Passed,
        "{name} {rule}: {}",
        check.message
    );
}

#[track_caller]
fn assert_family_fails(
    name: &str,
    rule: &str,
    expected: &str,
    edit: impl FnOnce(&mut InMemDicomObject),
) {
    let check = family_check(name, rule, edit);
    assert_eq!(
        check.status,
        ValidationStatus::Failed,
        "{name} {rule}: {}",
        check.message
    );
    assert!(
        check.message.contains(expected),
        "{name} {rule} message lacks {expected:?}: {}",
        check.message
    );
}

/// Fixture `name` after `edit`, with File Meta Information naming `syntax`.
fn in_syntax(
    name: &str,
    syntax: &str,
    edit: impl FnOnce(&mut InMemDicomObject),
) -> DefaultDicomObject {
    let mut object = fixture(name);
    edit(&mut object);
    object
        .with_meta(FileMetaTableBuilder::new().transfer_syntax(syntax))
        .expect("file meta")
}

/// Encapsulated Pixel Data of one Fragment per Frame.
fn encapsulated(frames: Vec<Vec<u8>>) -> InMemElement {
    DataElement::new(
        tags::PIXEL_DATA,
        VR::OB,
        Value::PixelSequence(PixelFragmentSequence::new(Vec::<u32>::new(), frames)),
    )
}

#[track_caller]
fn assert_status(check: Option<ValidationCheck>, status: ValidationStatus, expected: &str) {
    let check = check.expect("the family check runs on the object");
    assert_eq!(check.status, status, "{}", check.message);
    assert!(
        check.message.contains(expected),
        "message lacks {expected:?}: {}",
        check.message
    );
}

#[test]
fn each_family_check_runs_on_its_iods_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let families = [
        ("annotations", BULK_ANNOTATIONS),
        ("segmentation", SEGMENTATION),
        ("parametric-map", PARAMETRIC_MAP),
    ];
    for (name, rule) in families {
        let path = write(tmp.path(), name, fixture(name), |_| {});
        let report = iod_report(&path, ValidationProfile::General);
        for (_, other) in families {
            let ran: Vec<&ValidationCheck> = report
                .checks
                .iter()
                .filter(|check| check.name == other && check.path.is_some())
                .collect();
            if other == rule {
                assert_eq!(ran.len(), 1, "{name}: {other} {ran:?}");
                assert_eq!(
                    ran[0].status,
                    ValidationStatus::Passed,
                    "{name}: {}",
                    ran[0].message
                );
            } else {
                assert!(ran.is_empty(), "{other} ran on {name}");
            }
        }
    }
    let wsi = export_valid_color_wsi_for_validation(tmp.path(), "wsi");
    let report = iod_report(&wsi, ValidationProfile::General);
    for (_, rule) in families {
        assert!(
            report.checks.iter().all(|check| check.name != rule),
            "{rule} ran on VL WSI"
        );
    }
}
