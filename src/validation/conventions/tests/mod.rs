//! The convention profiles on a conformant slide scan, and on broken variants
//! of it that each break one requirement and fail exactly the checks that
//! state it.
//!
//! Set `WSI_DICOM_CONVENTION_FIXTURES` to a directory to keep every variant
//! there, e.g. to run `dciodvfy -profile WG26SP2025` on them.

mod disclosure;
mod fixture;
mod ihe_dpia;
mod ihe_dpia_draft;
mod interop;
mod json;
mod profiles;
mod wg26;

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use dicom_object::DefaultDicomObject;

use super::ConventionProfile;
use crate::validation::{
    validate_dicom_path_with_runner, CommandOutcome, ValidationCommandRunner, ValidationOptions,
    ValidationReport, ValidationStatus,
};
use fixture::Slide;

/// No external validator is installed.
struct NoTools;

impl ValidationCommandRunner for NoTools {
    fn find_command(&self, _name: &str) -> Option<PathBuf> {
        None
    }

    fn run(
        &self,
        _program: &Path,
        _args: &[OsString],
        _timeout: Duration,
        _max_output_bytes: usize,
    ) -> Result<CommandOutcome, std::io::Error> {
        Err(std::io::Error::other(
            "no external validator in convention tests",
        ))
    }
}

/// The objects of one scan, by file name.
struct Scan {
    slide: Slide,
    objects: Vec<(String, DefaultDicomObject)>,
}

const VOLUME_1: &str = "volume-1";
const VOLUME_2: &str = "volume-2";
const VOLUME_3: &str = "volume-3";
const LABEL: &str = "label";
const OVERVIEW: &str = "overview";
const ANNOTATION: &str = "annotation";
const COMPLETION: &str = "completion";
const WSI: [&str; 5] = [VOLUME_1, VOLUME_2, VOLUME_3, LABEL, OVERVIEW];

impl Scan {
    /// The conformant scan of slide `suffix`.
    fn conformant(suffix: u32) -> Self {
        let slide = Slide::new(suffix);
        let images = fixture::images();
        let mut objects: Vec<(String, DefaultDicomObject)> = WSI
            .iter()
            .zip(&images)
            .map(|(name, image)| (name.to_string(), image.object(&slide)))
            .collect();
        objects.push((ANNOTATION.into(), fixture::annotation(&slide)));
        objects.push((COMPLETION.into(), fixture::completion(&slide, &images)));
        Self { slide, objects }
    }

    fn object(&mut self, name: &str) -> &mut DefaultDicomObject {
        &mut self
            .objects
            .iter_mut()
            .find(|(own, _)| own == name)
            .unwrap_or_else(|| panic!("no object {name}"))
            .1
    }

    /// Applies `edit` to object `name`.
    fn edit(mut self, name: &str, edit: impl FnOnce(&mut DefaultDicomObject)) -> Self {
        edit(self.object(name));
        self
    }

    /// Applies `edit` to every VL WSI image.
    fn edit_images(mut self, edit: impl Fn(&mut DefaultDicomObject)) -> Self {
        for name in WSI {
            if self.objects.iter().any(|(own, _)| own == name) {
                edit(self.object(name));
            }
        }
        self
    }

    /// Puts `object` in place of `name`, or adds it.
    fn with(mut self, name: &str, object: DefaultDicomObject) -> Self {
        self.objects.retain(|(own, _)| own != name);
        self.objects.push((name.into(), object));
        self
    }

    fn without(mut self, name: &str) -> Self {
        self.objects.retain(|(own, _)| own != name);
        self
    }

    /// The objects of another scan added to this set.
    fn and(mut self, other: Scan) -> Self {
        let prefix = other.slide.series.clone();
        self.objects.extend(
            other
                .objects
                .into_iter()
                .map(|(name, object)| (format!("{prefix}-{name}"), object)),
        );
        self
    }

    fn write(&self, dir: &Path) -> PathBuf {
        for (name, object) in &self.objects {
            fixture::write(object, dir, name);
        }
        dir.to_path_buf()
    }
}

/// Where a variant is written: kept under `WSI_DICOM_CONVENTION_FIXTURES`
/// when it is set, otherwise in a temporary directory.
struct FixtureDir {
    _temporary: Option<tempfile::TempDir>,
    path: PathBuf,
}

impl FixtureDir {
    fn new(name: &str) -> Self {
        match std::env::var_os("WSI_DICOM_CONVENTION_FIXTURES") {
            Some(root) => {
                let path = PathBuf::from(root).join(name);
                // A kept variant is never overwritten: stale files would join it.
                assert!(
                    !path.exists(),
                    "{} exists; point WSI_DICOM_CONVENTION_FIXTURES at an empty directory",
                    path.display()
                );
                std::fs::create_dir_all(&path).expect("fixture directory");
                Self {
                    _temporary: None,
                    path,
                }
            }
            None => {
                let temporary = tempfile::tempdir().expect("tempdir");
                let path = temporary.path().to_path_buf();
                Self {
                    _temporary: Some(temporary),
                    path,
                }
            }
        }
    }
}

fn report(dir: &Path, profiles: &[ConventionProfile]) -> ValidationReport {
    let options = ValidationOptions {
        conventions: profiles.iter().copied().collect(),
        ..ValidationOptions::default()
    };
    validate_dicom_path_with_runner(dir, &options, &NoTools).expect("validation report")
}

fn names_with(report: &ValidationReport, status: ValidationStatus) -> BTreeSet<String> {
    report
        .conventions
        .checks
        .iter()
        .filter(|check| check.status == status)
        .map(|check| check.name.clone())
        .collect()
}

fn advised(report: &ValidationReport) -> BTreeSet<String> {
    report
        .conventions
        .advisories
        .iter()
        .map(|advisory| advisory.name.clone())
        .collect()
}

/// The DICOM checks of `report` that failed, for failure messages.
fn dicom_failures(report: &ValidationReport) -> Vec<String> {
    report
        .checks
        .iter()
        .filter(|check| check.status == ValidationStatus::Failed)
        .map(|check| format!("{} {:?}: {}", check.name, check.path, check.message))
        .collect()
}

/// Every convention check and advisory of `report`, for failure messages.
fn describe(report: &ValidationReport) -> String {
    let mut lines: Vec<String> = report
        .conventions
        .checks
        .iter()
        .filter(|check| check.status != ValidationStatus::Passed)
        .map(|check| format!("{:?} {}: {}", check.status, check.name, check.message))
        .collect();
    lines.extend(
        report
            .conventions
            .advisories
            .iter()
            .map(|advisory| format!("advisory {}: {}", advisory.name, advisory.message)),
    );
    lines.join("\n")
}

/// Writes `scan` as variant `name` and asserts that, under `profiles`,
/// exactly the checks `failing` fail and exactly the advisories `advising`
/// apply. Returns the report for further assertions.
#[track_caller]
fn assert_outcome(
    name: &str,
    scan: &Scan,
    profiles: &[ConventionProfile],
    failing: &[&str],
    advising: &[&str],
) -> ValidationReport {
    let dir = FixtureDir::new(name);
    let report = report(&scan.write(&dir.path), profiles);
    let expected: BTreeSet<String> = failing.iter().map(|name| name.to_string()).collect();
    let advisories: BTreeSet<String> = advising.iter().map(|name| name.to_string()).collect();
    assert_eq!(
        names_with(&report, ValidationStatus::Failed),
        expected,
        "{name}: failing convention checks differ:\n{}",
        describe(&report)
    );
    assert_eq!(
        advised(&report),
        advisories,
        "{name}: advisories differ:\n{}",
        describe(&report)
    );
    report
}

/// The message of the first failed check `check`.
#[track_caller]
fn failure_message(report: &ValidationReport, check: &str) -> String {
    report
        .conventions
        .checks
        .iter()
        .find(|result| result.name == check && result.status == ValidationStatus::Failed)
        .unwrap_or_else(|| panic!("{check} did not fail"))
        .message
        .clone()
}

/// The message of the first advisory `name`.
#[track_caller]
fn advisory_message(report: &ValidationReport, name: &str) -> String {
    report
        .conventions
        .advisories
        .iter()
        .find(|advisory| advisory.name == name)
        .unwrap_or_else(|| panic!("no advisory {name}"))
        .message
        .clone()
}
