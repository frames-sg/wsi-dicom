//! Convention profiles: what the DICOM WG-26 connectathon, the IHE PaLM DPIA
//! profile and its draft CPs, and the viewers, archives and converters of
//! whole-slide images require beyond the DICOM standard.
//!
//! They are opt-in (`ValidationOptions::conventions`), each result names the
//! profile and the source that states it, and they are reported apart from
//! the DICOM checks: no convention result or advisory makes validation fail.
//! Where a source conflicts with DICOM normative text, the normative text is
//! followed and the conflict is reported as an advisory of the profile.

mod facts;
mod ihe_dpia;
mod ihe_dpia_draft;
mod interop;
mod specimen;
#[cfg(test)]
mod tests;
mod wg26;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use dicom_object::DefaultDicomObject;
use serde::{Deserialize, Serialize};

use super::ValidationStatus;
use facts::{Facts, SetIndex};

/// A convention profile: requirements a community, an IHE profile or the
/// tools that read whole-slide images add to the DICOM standard. Selecting
/// one never changes the DICOM checks or makes them fail.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ConventionProfile {
    /// DICOM WG-26 Digital Pathology Connectathon Technical Requirements
    /// 2026, and the nine checks dicom3tools `dciodvfy -profile WG26SP2025`
    /// adds to the IODs.
    Wg26,
    /// IHE PaLM DPIA Rev. 1.4 (Trial Implementation): the Appendix B mapping
    /// rules an object or set shows.
    IheDpia,
    /// IHE PaLM CP-LAB-279 True Color Tiled Full Single Z Plane Pyramid
    /// Option and CP-LAB-281 Acquisition Complete Option: draft, CPs under
    /// ballot.
    IheDpiaDraft,
    /// IHE PaLM CP-LAB-279 JPEG Storage Option: draft, CP under ballot.
    IheDpiaDraftJpeg,
    /// Configurations that named viewers, archives, converters and
    /// validators do not handle. Advisories only.
    Interop,
}

impl ConventionProfile {
    /// The profile's name on the command line and in check names.
    pub fn name(self) -> &'static str {
        match self {
            Self::Wg26 => "wg26",
            Self::IheDpia => "ihe-dpia",
            Self::IheDpiaDraft => "ihe-dpia-draft",
            Self::IheDpiaDraftJpeg => "ihe-dpia-draft-jpeg",
            Self::Interop => "interop",
        }
    }
}

/// Whether a convention is a check, with a pass or fail result for each
/// object or set it applies to, or an advisory, listed only where it applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ConventionKind {
    /// A requirement of the profile's source.
    Check,
    /// A warning: what a named tool does with such objects, a source's
    /// expectation, or a conflict between a source and DICOM.
    Advisory,
}

/// One convention of a profile, with the source that states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ConventionEntry {
    /// `convention-<profile>-<topic>`.
    pub name: &'static str,
    /// The profile that selects it.
    pub profile: ConventionProfile,
    /// Check or advisory.
    pub kind: ConventionKind,
    /// The document, version and section, requirement number or source line.
    pub source: &'static str,
    /// What it requires, or the configuration it warns about.
    pub requirement: &'static str,
}

/// The results of the selected convention profiles. None of them is a DICOM
/// failure.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ConventionReport {
    /// The profiles that ran.
    pub profiles: Vec<ConventionProfile>,
    /// One result per check and object or set it applies to.
    pub checks: Vec<ConventionCheck>,
    /// The advisories that apply, each with its source.
    pub advisories: Vec<ConventionAdvisory>,
}

impl ConventionReport {
    /// Whether no profile ran.
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty() && self.checks.is_empty() && self.advisories.is_empty()
    }

    /// Whether any convention check failed.
    pub fn has_failures(&self) -> bool {
        self.count(ValidationStatus::Failed) > 0
    }

    /// Count convention checks with `status`.
    pub fn count(&self, status: ValidationStatus) -> usize {
        self.checks
            .iter()
            .filter(|check| check.status == status)
            .count()
    }
}

/// The result of one convention check for one object or set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ConventionCheck {
    /// `convention-<profile>-<topic>`.
    pub name: String,
    /// The profile that selected it.
    pub profile: ConventionProfile,
    /// The checked file; absent for a check of a Series or of the set.
    pub path: Option<PathBuf>,
    /// The Series Instance UID of a check of one Series.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series: Option<String>,
    /// Passed, failed, or skipped when the object lacks what the check reads.
    pub status: ValidationStatus,
    /// The source of the requirement.
    pub source: String,
    /// The source, then what was found and what is required.
    pub message: String,
}

/// An advisory that applies to one object or set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ConventionAdvisory {
    /// `convention-<profile>-<topic>`.
    pub name: String,
    /// The profile that selected it.
    pub profile: ConventionProfile,
    /// The file; absent for a Series or the set.
    pub path: Option<PathBuf>,
    /// The Series Instance UID of an advisory about one Series.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series: Option<String>,
    /// The tool, document or conflict it comes from.
    pub source: String,
    /// The source, then what was found and what follows from it.
    pub message: String,
}

/// Every convention of every profile, with its source.
pub fn convention_catalog() -> Vec<ConventionEntry> {
    [
        wg26::ENTRIES,
        ihe_dpia::ENTRIES,
        ihe_dpia_draft::ENTRIES,
        interop::ENTRIES,
        EVALUABLE,
    ]
    .into_iter()
    .flatten()
    .map(|entry| **entry)
    .collect()
}

const fn evaluable(profile: ConventionProfile, name: &'static str) -> ConventionEntry {
    check(
        profile,
        name,
        "wsi-dicom",
        "The file parses as DICOM and a VL WSI image names its Series Instance UID, so the profile's file and Series conventions can be evaluated for it; reported skipped otherwise.",
    )
}

const EVALUABLE_WG26: ConventionEntry =
    evaluable(ConventionProfile::Wg26, "convention-wg26-evaluable");
const EVALUABLE_IHE_DPIA: ConventionEntry =
    evaluable(ConventionProfile::IheDpia, "convention-ihe-dpia-evaluable");
const EVALUABLE_IHE_DPIA_DRAFT: ConventionEntry = evaluable(
    ConventionProfile::IheDpiaDraft,
    "convention-ihe-dpia-draft-evaluable",
);
const EVALUABLE_IHE_DPIA_DRAFT_JPEG: ConventionEntry = evaluable(
    ConventionProfile::IheDpiaDraftJpeg,
    "convention-ihe-dpia-draft-jpeg-evaluable",
);
const EVALUABLE_INTEROP: ConventionEntry =
    evaluable(ConventionProfile::Interop, "convention-interop-evaluable");
const EVALUABLE: &[&ConventionEntry] = &[
    &EVALUABLE_WG26,
    &EVALUABLE_IHE_DPIA,
    &EVALUABLE_IHE_DPIA_DRAFT,
    &EVALUABLE_IHE_DPIA_DRAFT_JPEG,
    &EVALUABLE_INTEROP,
];

/// The convention profiles of one validation run: file results as each
/// object is read, set results at the end.
pub(in crate::validation) struct ConventionRun {
    out: Out,
    facts: Vec<Facts>,
}

impl ConventionRun {
    pub(in crate::validation) fn new(profiles: &BTreeSet<ConventionProfile>) -> Self {
        Self {
            out: Out {
                report: ConventionReport {
                    profiles: profiles.iter().copied().collect(),
                    ..ConventionReport::default()
                },
            },
            facts: Vec::new(),
        }
    }

    /// The file results of `object`, and the facts the set results need.
    pub(in crate::validation) fn observe(&mut self, path: &Path, object: &DefaultDicomObject) {
        if self.out.report.profiles.is_empty() {
            return;
        }
        let facts = Facts::of(path, object);
        let out = &mut self.out;
        if out.selected(ConventionProfile::Wg26) {
            wg26::file_checks(object, &facts, out);
        }
        if out.selected(ConventionProfile::IheDpia) {
            ihe_dpia::file_checks(object, &facts, out);
        }
        if out.selected(ConventionProfile::IheDpiaDraft) {
            ihe_dpia_draft::file_checks(object, &facts, out);
        }
        if out.selected(ConventionProfile::IheDpiaDraftJpeg) {
            ihe_dpia_draft::jpeg_file_checks(&facts, out);
        }
        if out.selected(ConventionProfile::Interop) {
            interop::file_advisories(object, &facts, out);
        }
        self.facts.push(facts);
    }

    /// A file the parser could not read: each profile reports it skipped.
    pub(in crate::validation) fn unreadable(&mut self, path: &Path) {
        self.out.not_evaluable(
            path,
            "the file could not be parsed as DICOM (intrinsic-pixel-structure reports why), so no convention of the profile was evaluated for it",
        );
    }

    pub(in crate::validation) fn finish(mut self) -> ConventionReport {
        let out = &mut self.out;
        for fact in self.facts.iter().filter(|fact| fact.is_wsi()) {
            if fact.series_uid.is_none() {
                out.not_evaluable(
                    &fact.path,
                    "the VL WSI image has no Series Instance UID (0020,000E), so the profile's Series conventions were not evaluated for it",
                );
            }
        }
        let index = SetIndex::new(&self.facts);
        if out.selected(ConventionProfile::Wg26) {
            wg26::set_checks(&index, &self.facts, out);
        }
        if out.selected(ConventionProfile::IheDpia) {
            ihe_dpia::set_checks(&self.facts, out);
        }
        // The JPEG Storage Option has no Series or set rules.
        if out.selected(ConventionProfile::IheDpiaDraft) {
            ihe_dpia_draft::set_checks(&index, &self.facts, out);
        }
        if out.selected(ConventionProfile::Interop) {
            interop::set_advisories(&index, out);
        }
        self.out.report
    }
}

/// What a result is about.
#[derive(Clone, Copy)]
enum Subject<'a> {
    File(&'a Path),
    Series(&'a str),
    /// The whole validated set.
    Set,
}

impl Subject<'_> {
    fn path(self) -> Option<PathBuf> {
        match self {
            Self::File(path) => Some(path.to_path_buf()),
            Self::Series(_) | Self::Set => None,
        }
    }

    fn series(self) -> Option<String> {
        match self {
            Self::Series(uid) => Some(uid.to_string()),
            Self::File(_) | Self::Set => None,
        }
    }
}

/// Collects results of the selected profiles only.
struct Out {
    report: ConventionReport,
}

impl Out {
    fn selected(&self, profile: ConventionProfile) -> bool {
        self.report.profiles.contains(&profile)
    }

    /// Each selected profile reports `path` skipped: `reason`.
    fn not_evaluable(&mut self, path: &Path, reason: &str) {
        for entry in EVALUABLE {
            self.skip(entry, Subject::File(path), reason.to_string());
        }
    }

    /// The pass or fail result of check `entry` for `subject`.
    fn check(&mut self, entry: &ConventionEntry, subject: Subject<'_>, result: Result<(), String>) {
        let (status, finding) = match result {
            Ok(()) => (ValidationStatus::Passed, "conformant".to_string()),
            Err(reason) => (ValidationStatus::Failed, reason),
        };
        self.push_check(entry, subject, status, finding);
    }

    /// Check `entry` applies to `subject` but cannot be evaluated: `reason`.
    fn skip(&mut self, entry: &ConventionEntry, subject: Subject<'_>, reason: String) {
        self.push_check(entry, subject, ValidationStatus::Skipped, reason);
    }

    fn push_check(
        &mut self,
        entry: &ConventionEntry,
        subject: Subject<'_>,
        status: ValidationStatus,
        finding: String,
    ) {
        debug_assert_eq!(entry.kind, ConventionKind::Check, "{}", entry.name);
        if !self.selected(entry.profile) {
            return;
        }
        self.report.checks.push(ConventionCheck {
            name: entry.name.to_string(),
            profile: entry.profile,
            path: subject.path(),
            series: subject.series(),
            status,
            source: entry.source.to_string(),
            message: format!("{}: {finding}", entry.source),
        });
    }

    /// Advisory `entry` applies to `subject`: `finding`.
    fn advise(&mut self, entry: &ConventionEntry, subject: Subject<'_>, finding: String) {
        debug_assert_eq!(entry.kind, ConventionKind::Advisory, "{}", entry.name);
        if !self.selected(entry.profile) {
            return;
        }
        self.report.advisories.push(ConventionAdvisory {
            name: entry.name.to_string(),
            profile: entry.profile,
            path: subject.path(),
            series: subject.series(),
            source: entry.source.to_string(),
            message: format!("{}: {finding}", entry.source),
        });
    }
}

/// Violations shown in one result; the rest are counted.
const MAX_SHOWN: usize = 20;

/// The violations of one result, collected as `iod::Family` collects them:
/// the first `MAX_SHOWN` are formatted, the rest only counted.
#[derive(Debug, Default)]
struct Violations {
    shown: Vec<String>,
    total: usize,
}

impl Violations {
    /// Counts a violation; `message` is formatted only while it will be shown.
    fn push(&mut self, message: impl FnOnce() -> String) {
        self.total += 1;
        if self.shown.len() < MAX_SHOWN {
            self.shown.push(message());
        }
    }

    /// Conformant when nothing was counted, else the shown violations and
    /// how many more there are.
    fn result(self) -> Result<(), String> {
        if self.total == 0 {
            return Ok(());
        }
        let mut message = self.shown.join("; ");
        let hidden = self.total - self.shown.len();
        if hidden > 0 {
            message.push_str(&format!("; and {hidden} more"));
        }
        Err(message)
    }
}

/// `const` builders of the entries of each profile.
const fn check(
    profile: ConventionProfile,
    name: &'static str,
    source: &'static str,
    requirement: &'static str,
) -> ConventionEntry {
    ConventionEntry {
        name,
        profile,
        kind: ConventionKind::Check,
        source,
        requirement,
    }
}

const fn advisory(
    profile: ConventionProfile,
    name: &'static str,
    source: &'static str,
    requirement: &'static str,
) -> ConventionEntry {
    ConventionEntry {
        name,
        profile,
        kind: ConventionKind::Advisory,
        source,
        requirement,
    }
}
