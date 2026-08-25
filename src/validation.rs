use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::Error;

mod command_checks;
mod discovery;
mod doctor;
mod pixel_decode;
mod pixel_structure;
mod process;
mod wsi_conformance;

use command_checks::*;
use discovery::discover_dicom_files;
pub use doctor::doctor_dicom_environment;
#[cfg(test)]
use doctor::doctor_dicom_environment_with_runner;
pub(crate) use pixel_decode::htj2k_decoder_command;
use pixel_decode::{auto_htj2k_decoder_template, run_pixel_decode_checks, ValidationTempDir};
#[cfg(test)]
use pixel_decode::{
    inspect_pnm_output, write_private_validation_file, DecodedFrameExpectation,
    AUTO_HTJ2K_DECODER_COMMAND,
};
#[cfg(test)]
use pixel_structure::assemble_encapsulated_frames;
#[cfg(any(test, feature = "bench-internals"))]
pub(crate) use pixel_structure::fragment_payload_without_padding;
use pixel_structure::run_intrinsic_pixel_structure_check;
use process::SystemCommandRunner;
pub(crate) use process::{CommandOutcome, ValidationCommandRunner};
use wsi_conformance::{run_intrinsic_wsi_conformance_checks, run_specimen_uid_set_check};

/// Options for validating generated DICOM files with external tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct ValidationOptions {
    /// Treat missing required validators or pixel decoders as failures.
    pub strict: bool,
    /// Optional dcm4che IOD XML file used by `dcmvalidate`.
    pub dcmvalidate_iod: Option<PathBuf>,
    /// Optional HTJ2K decoder command template using `{input}` and `{output}` placeholders.
    pub htj2k_decoder: Option<String>,
    /// Maximum number of pixel frames to decode per transfer syntax; zero disables pixel checks.
    pub max_pixel_frames: usize,
    /// Timeout in seconds applied to each external validator command.
    pub command_timeout_secs: u64,
    /// Maximum DICOM files discovered under a directory input.
    pub max_files: usize,
    /// Maximum directory depth walked under a directory input.
    pub max_depth: usize,
    /// Maximum captured stdout or stderr bytes per child process.
    pub max_child_output_bytes: usize,
    /// Maximum encoded bytes assembled for one compressed pixel frame.
    pub max_pixel_frame_bytes: usize,
}

impl Default for ValidationOptions {
    fn default() -> Self {
        Self {
            strict: false,
            dcmvalidate_iod: None,
            htj2k_decoder: None,
            max_pixel_frames: 1,
            command_timeout_secs: 60,
            max_files: 100_000,
            max_depth: 64,
            max_child_output_bytes: 4 * 1024 * 1024,
            max_pixel_frame_bytes: 512 * 1024 * 1024,
        }
    }
}

impl ValidationOptions {
    /// Timeout applied to each external validator command.
    pub fn command_timeout(&self) -> Duration {
        Duration::from_secs(self.command_timeout_secs)
    }
}

/// Options for checking validator tool availability.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct DoctorOptions {
    /// Treat missing required tools as failures.
    pub strict: bool,
    /// Optional dcm4che IOD XML file used to decide whether `dcmvalidate` is configured.
    pub dcmvalidate_iod: Option<PathBuf>,
    /// Optional HTJ2K decoder command template to check.
    pub htj2k_decoder: Option<String>,
}

/// Validator environment report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct DoctorReport {
    /// Tools checked for availability and configuration.
    pub tools: Vec<DoctorTool>,
}

impl DoctorReport {
    /// Whether any configured tool failed its doctor check.
    pub fn has_failures(&self) -> bool {
        self.tools
            .iter()
            .any(|tool| tool.status == DoctorStatus::Failed)
    }

    /// Count tools that are available.
    pub fn available_tools(&self) -> usize {
        self.tools
            .iter()
            .filter(|tool| tool.status == DoctorStatus::Available)
            .count()
    }

    /// Count tools that were found but failed their doctor check.
    pub fn failed_tools(&self) -> usize {
        self.tools
            .iter()
            .filter(|tool| tool.status == DoctorStatus::Failed)
            .count()
    }

    /// Count tools that were required but missing.
    pub fn missing_tools(&self) -> usize {
        self.tools
            .iter()
            .filter(|tool| tool.status == DoctorStatus::Missing)
            .count()
    }

    /// Count optional or unconfigured tools skipped by doctor.
    pub fn skipped_tools(&self) -> usize {
        self.tools
            .iter()
            .filter(|tool| tool.status == DoctorStatus::Skipped)
            .count()
    }
}

/// Status for one validator tool check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct DoctorTool {
    /// Tool name.
    pub name: String,
    /// Whether strict mode treats this tool as required.
    pub required: bool,
    /// Availability or configuration status.
    pub status: DoctorStatus,
    /// Command used for the doctor probe.
    pub command: Vec<String>,
    /// Resolved tool path when available.
    pub path: Option<PathBuf>,
    /// Human-readable status message.
    pub message: String,
}

/// Availability status for a validator tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DoctorStatus {
    /// Tool was found and accepted.
    Available,
    /// Required tool was not found.
    Missing,
    /// Tool was found but failed its probe.
    Failed,
    /// Tool was optional or not configured.
    Skipped,
}

/// Report from validating one file or directory of DICOM files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ValidationReport {
    /// Input path passed to validation.
    pub input: PathBuf,
    /// DICOM files discovered and checked.
    pub files: Vec<PathBuf>,
    /// Individual validation checks.
    pub checks: Vec<ValidationCheck>,
}

impl ValidationReport {
    /// Whether any validation check failed.
    pub fn has_failures(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.status == ValidationStatus::Failed)
    }

    /// Count validation checks that passed.
    pub fn passed_checks(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.status == ValidationStatus::Passed)
            .count()
    }

    /// Count validation checks that failed.
    pub fn failed_checks(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.status == ValidationStatus::Failed)
            .count()
    }

    /// Count validation checks that were skipped.
    pub fn skipped_checks(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.status == ValidationStatus::Skipped)
            .count()
    }
}

/// Result of one external validator or pixel decode check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ValidationCheck {
    /// Check name.
    pub name: String,
    /// File path associated with this check, when file-specific.
    pub path: Option<PathBuf>,
    /// Check status.
    pub status: ValidationStatus,
    /// Command used for the check.
    pub command: Vec<String>,
    /// Human-readable result message.
    pub message: String,
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
}

/// Status for one DICOM validation check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ValidationStatus {
    /// Check completed successfully.
    Passed,
    /// Check completed and reported a failure.
    Failed,
    /// Check was intentionally skipped, usually because a tool was unavailable.
    Skipped,
}

const DOCTOR_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ValidatorToolSpec {
    name: &'static str,
    required: bool,
    doctor_args: &'static [&'static str],
    nonzero_success_output: Option<&'static str>,
}

const DCIODVFY_TOOL: ValidatorToolSpec = ValidatorToolSpec {
    name: "dciodvfy",
    required: true,
    doctor_args: &["-version"],
    nonzero_success_output: None,
};
const DCENTVFY_TOOL: ValidatorToolSpec = ValidatorToolSpec {
    name: "dcentvfy",
    required: true,
    doctor_args: &["-version"],
    nonzero_success_output: None,
};
const VALIDATE_IODS_TOOL: ValidatorToolSpec = ValidatorToolSpec {
    name: "validate_iods",
    required: false,
    doctor_args: &["-h"],
    nonzero_success_output: None,
};
const DJPEG_TOOL: ValidatorToolSpec = ValidatorToolSpec {
    name: "djpeg",
    required: false,
    doctor_args: &["-version"],
    nonzero_success_output: None,
};
const OPJ_DECOMPRESS_TOOL: ValidatorToolSpec = ValidatorToolSpec {
    name: "opj_decompress",
    required: false,
    doctor_args: &["-h"],
    nonzero_success_output: Some("OpenJPEG"),
};
const DCMVALIDATE_TOOL: ValidatorToolSpec = ValidatorToolSpec {
    name: "dcmvalidate",
    required: true,
    doctor_args: &["--help"],
    nonzero_success_output: None,
};
const VALIDATOR_DOCTOR_TOOLS: &[ValidatorToolSpec] = &[
    DCIODVFY_TOOL,
    DCENTVFY_TOOL,
    VALIDATE_IODS_TOOL,
    DJPEG_TOOL,
    OPJ_DECOMPRESS_TOOL,
];
const VALIDATOR_SET_FILE_CHUNK_SIZE: usize = 512;

fn staged_dicom3tools_command(name: &str) -> Option<PathBuf> {
    if !staged_dicom3tools_probe_enabled() {
        return None;
    }
    let staged = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("dicom3tools-mac")
        .join(name);
    staged.is_file().then_some(staged)
}

fn staged_dicom3tools_probe_enabled() -> bool {
    staged_dicom3tools_probe_enabled_from(
        cfg!(debug_assertions),
        std::env::var_os("WSI_DICOM_VALIDATOR_STAGED_TOOLS").is_some(),
    )
}

fn staged_dicom3tools_probe_enabled_from(debug_assertions: bool, env_present: bool) -> bool {
    debug_assertions || env_present
}

/// Validate a DICOM file or recursively discovered DICOM directory.
pub fn validate_dicom_path(
    path: impl AsRef<Path>,
    options: &ValidationOptions,
) -> Result<ValidationReport, Error> {
    validate_dicom_path_with_runner(path.as_ref(), options, &SystemCommandRunner)
}

pub(crate) fn validate_dicom_path_with_runner(
    path: impl AsRef<Path>,
    options: &ValidationOptions,
    runner: &impl ValidationCommandRunner,
) -> Result<ValidationReport, Error> {
    let input = path.as_ref().to_path_buf();
    let files = discover_dicom_files(&input, options)?;
    let mut checks = Vec::new();

    for file in &files {
        checks.push(run_intrinsic_pixel_structure_check(
            file,
            options.max_pixel_frame_bytes,
        ));
        checks.extend(run_intrinsic_wsi_conformance_checks(file));
    }
    if let Some(check) = run_specimen_uid_set_check(&files) {
        checks.push(check);
    }

    for file in &files {
        checks.push(run_named_command_check(
            runner,
            CommandCheckRequest {
                check_name: DCIODVFY_TOOL.name,
                command_name: DCIODVFY_TOOL.name,
                args: vec![OsString::from("-new"), file.as_os_str().to_os_string()],
                path: Some(file),
                required: options.strict,
                error_line_is_failure: true,
                timeout: options.command_timeout(),
                max_output_bytes: options.max_child_output_bytes,
            },
        ));
    }

    checks.extend(run_set_level_command_checks(
        runner,
        &files,
        SetLevelCommandCheckRequest {
            check_name: DCENTVFY_TOOL.name,
            command_name: DCENTVFY_TOOL.name,
            required: options.strict,
            error_line_is_failure: true,
            timeout: options.command_timeout(),
            max_output_bytes: options.max_child_output_bytes,
            chunk_size: VALIDATOR_SET_FILE_CHUNK_SIZE,
        },
    ));

    checks.extend(run_set_level_command_checks(
        runner,
        &files,
        SetLevelCommandCheckRequest {
            check_name: VALIDATE_IODS_TOOL.name,
            command_name: VALIDATE_IODS_TOOL.name,
            required: options.strict,
            error_line_is_failure: false,
            timeout: options.command_timeout(),
            max_output_bytes: options.max_child_output_bytes,
            chunk_size: VALIDATOR_SET_FILE_CHUNK_SIZE,
        },
    ));

    if let Some(iod) = &options.dcmvalidate_iod {
        for file in &files {
            checks.push(run_named_command_check(
                runner,
                CommandCheckRequest {
                    check_name: DCMVALIDATE_TOOL.name,
                    command_name: DCMVALIDATE_TOOL.name,
                    args: vec![
                        OsString::from("--iod"),
                        iod.as_os_str().to_os_string(),
                        file.as_os_str().to_os_string(),
                    ],
                    path: Some(file),
                    required: true,
                    error_line_is_failure: false,
                    timeout: options.command_timeout(),
                    max_output_bytes: options.max_child_output_bytes,
                },
            ));
        }
    }

    if options.max_pixel_frames > 0 {
        let temp_dir = ValidationTempDir::create()?;
        for (file_idx, file) in files.iter().enumerate() {
            checks.extend(run_pixel_decode_checks(
                file_idx,
                file,
                options,
                runner,
                temp_dir.path(),
            ));
        }
    }

    Ok(ValidationReport {
        input,
        files,
        checks,
    })
}

fn failed_check(name: &str, path: Option<&PathBuf>, message: String) -> ValidationCheck {
    ValidationCheck {
        name: name.to_string(),
        path: path.cloned(),
        status: ValidationStatus::Failed,
        command: Vec::new(),
        message,
        stdout: String::new(),
        stderr: String::new(),
    }
}

fn skipped_check(name: &str, path: Option<&PathBuf>, message: String) -> ValidationCheck {
    ValidationCheck {
        name: name.to_string(),
        path: path.cloned(),
        status: ValidationStatus::Skipped,
        command: Vec::new(),
        message,
        stdout: String::new(),
        stderr: String::new(),
    }
}

#[cfg(test)]
mod tests;
