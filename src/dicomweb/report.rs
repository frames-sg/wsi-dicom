//! What a DICOMweb conformance run reports.

use serde::Serialize;

use crate::ValidationReport;

/// Shown violations per check; the rest are counted.
const MAX_SHOWN: usize = 24;

/// Report of a DICOMweb origin server conformance run.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct DicomWebReport {
    /// The Base URI of the Studies Service checked.
    pub base_url: String,
    /// The Study, Series and Instance the transactions targeted.
    pub study: Option<String>,
    /// See `study`.
    pub series: Option<String>,
    /// See `study`.
    pub instance: Option<String>,
    /// One result per PS3.18 requirement family exercised.
    pub checks: Vec<DicomWebCheck>,
    /// The intrinsic checks of what was retrieved: the Metadata as DICOM JSON
    /// and as Data Sets, the Frames' codestreams and a retrieved Instance.
    pub objects: ValidationReport,
    /// Every request sent, without credentials.
    pub exchanges: Vec<DicomWebExchange>,
}

impl DicomWebReport {
    /// Whether a required behavior failed, here or in a retrieved object.
    pub fn has_failures(&self) -> bool {
        self.failed_checks() > 0 || self.objects.has_failures()
    }

    /// Count checks with `status`.
    pub fn count(&self, status: DicomWebStatus) -> usize {
        self.checks
            .iter()
            .filter(|check| check.status == status)
            .count()
    }

    /// Count failed checks.
    pub fn failed_checks(&self) -> usize {
        self.count(DicomWebStatus::Failed)
    }
}

/// The outcome of one DICOMweb check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DicomWebStatus {
    /// The origin server behaves as PS3.18 requires.
    Passed,
    /// A required behavior is missing or broken.
    Failed,
    /// An optional capability the origin server does not offer; not a failure.
    Unsupported,
    /// Not exercised: skipped by request, opt-in not given, or a prerequisite
    /// (such as an Instance to target) was not available.
    NotRun,
}

/// One DICOMweb check, citing the PS3.18 sections it holds the server to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct DicomWebCheck {
    /// Check name, `dicomweb-<family>`.
    pub name: String,
    /// The PS3.18 sections and tables, with the pinned edition.
    pub section: String,
    /// The outcome.
    pub status: DicomWebStatus,
    /// The requests the check sent, as "METHOD target".
    pub requests: Vec<String>,
    /// What was found and what is required.
    pub message: String,
}

/// One request and what came back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct DicomWebExchange {
    /// HTTP method.
    pub method: String,
    /// The URL requested.
    pub url: String,
    /// The status code, if a response arrived.
    pub status: Option<u16>,
    /// The response Content-Type.
    pub content_type: Option<String>,
    /// Response body bytes read.
    pub body_bytes: u64,
    /// Wall time in milliseconds.
    pub elapsed_millis: u64,
    /// Why no usable response arrived: timeout, size limit, connection.
    pub error: Option<String>,
}

/// Violations and notes of one check while it runs.
#[derive(Debug, Default)]
pub(crate) struct Verdict {
    violations: Vec<String>,
    notes: Vec<String>,
    requests: Vec<String>,
    unsupported: Option<String>,
    not_run: Option<String>,
}

impl Verdict {
    /// A required behavior is broken.
    pub(crate) fn fail(&mut self, message: impl Into<String>) {
        self.violations.push(message.into());
    }

    /// Context shown with the outcome; not a failure.
    pub(crate) fn note(&mut self, message: impl Into<String>) {
        self.notes.push(message.into());
    }

    /// Records a request the check sent.
    pub(crate) fn request(&mut self, line: impl Into<String>) {
        self.requests.push(line.into());
    }

    /// The optional capability is not offered.
    pub(crate) fn unsupported(&mut self, message: impl Into<String>) {
        self.unsupported = Some(message.into());
    }

    /// The check could not be exercised.
    pub(crate) fn not_run(&mut self, message: impl Into<String>) {
        self.not_run = Some(message.into());
    }

    /// Whether a violation was recorded.
    pub(crate) fn failed(&self) -> bool {
        !self.violations.is_empty()
    }

    /// The check, `passed` describing a conforming outcome.
    pub(crate) fn finish(self, name: &str, section: String, passed: &str) -> DicomWebCheck {
        let notes = if self.notes.is_empty() {
            String::new()
        } else {
            format!("; {}", self.notes.join("; "))
        };
        let (status, message) = if !self.violations.is_empty() {
            let shown: Vec<&str> = self
                .violations
                .iter()
                .take(MAX_SHOWN)
                .map(String::as_str)
                .collect();
            let hidden = self.violations.len() - shown.len();
            let more = if hidden > 0 {
                format!("; and {hidden} more")
            } else {
                String::new()
            };
            (
                DicomWebStatus::Failed,
                format!(
                    "{} violation{}: {}{more}{notes}",
                    self.violations.len(),
                    if self.violations.len() == 1 { "" } else { "s" },
                    shown.join("; ")
                ),
            )
        } else if let Some(reason) = self.not_run {
            (DicomWebStatus::NotRun, format!("{reason}{notes}"))
        } else if let Some(reason) = self.unsupported {
            (DicomWebStatus::Unsupported, format!("{reason}{notes}"))
        } else {
            (DicomWebStatus::Passed, format!("{passed}{notes}"))
        };
        DicomWebCheck {
            name: name.to_string(),
            section,
            status,
            requests: self.requests,
            message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DicomWebStatus, Verdict};

    #[test]
    fn a_violation_outranks_unsupported_and_not_run() {
        let mut verdict = Verdict::default();
        verdict.unsupported("optional");
        verdict.fail("broken");
        let check = verdict.finish("dicomweb-x", "PS3.18".into(), "fine");
        assert_eq!(check.status, DicomWebStatus::Failed);
        assert!(check.message.starts_with("1 violation: broken"));

        let mut verdict = Verdict::default();
        verdict.unsupported("thumbnails are not offered");
        verdict.note("404");
        let check = verdict.finish("dicomweb-x", "PS3.18".into(), "fine");
        assert_eq!(check.status, DicomWebStatus::Unsupported);
        assert_eq!(check.message, "thumbnails are not offered; 404");
    }
}
