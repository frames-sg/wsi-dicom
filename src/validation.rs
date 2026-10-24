use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::Error;

mod codestream;
mod command_checks;
mod dicom_json;
mod digital_signatures;
mod discovery;
mod doctor;
mod encoding;
mod iod;
mod pixel_structure;
mod process;
mod transfer_syntaxes;
mod wsi_conformance;

pub(crate) use codestream::check_retrieved_frames;
use command_checks::*;
pub(crate) use dicom_json::{
    bulk_references, json_text_error, read_json_data_sets, validate_json_bytes, JsonRun,
    Source as JsonSource,
};
pub use dicom_json::{validate_dicom_json_path, DicomJsonOptions};
use discovery::discover_dicom_files;
pub use doctor::doctor_dicom_environment;
#[cfg(test)]
use doctor::doctor_dicom_environment_with_runner;
pub(crate) use iod::{check_value_format, is_image_storage_sop_class, registered};
#[cfg(test)]
use pixel_structure::assemble_encapsulated_frames;
#[cfg(any(test, feature = "bench-internals"))]
pub(crate) use pixel_structure::fragment_payload_without_padding;
use pixel_structure::run_intrinsic_pixel_structure_check;
use process::SystemCommandRunner;
pub(crate) use process::{CommandOutcome, ValidationCommandRunner};
use wsi_conformance::{
    run_clinical_identity_set_check, run_identity_set_check, run_intrinsic_wsi_conformance_checks,
    run_pyramid_geometry_set_check, run_series_set_check, run_slide_coordinate_set_check,
    run_source_relationship_set_check, run_specimen_uid_set_check, WsiConformanceCorpus,
};

/// The DICOM edition validation holds objects to, e.g. `2026d`.
pub const DICOM_EDITION: &str = iod::EDITION;

/// Intrinsic validation contract; general checks remain compatible with legacy catalogs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ValidationProfile {
    /// Selected reusable WSI checks, without the restricted core scope.
    #[default]
    General,
    /// Single-path, single-plane, non-concatenated TILED_FULL VOLUME profile
    /// of the pinned DICOM edition ([`DICOM_EDITION`]).
    Core,
}

/// Options for validating generated DICOM files with external tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct ValidationOptions {
    /// Select the intrinsic rule set explicitly. Defaults to general.
    pub profile: ValidationProfile,
    /// Maximum encoded file bytes admitted before parsing (default 1 GiB).
    /// This bounds input, not the parser's heap amplification.
    pub max_input_bytes: u64,
    /// Treat missing required validators as failures.
    pub strict: bool,
    /// Optional dcm4che IOD XML file used by `dcmvalidate`.
    pub dcmvalidate_iod: Option<PathBuf>,
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
    /// `Optional` (the default) also lists the optional VL WSI modules and
    /// attributes each object lacks; they are not failures.
    pub conformance: crate::ConformanceLevel,
}

impl Default for ValidationOptions {
    fn default() -> Self {
        Self {
            profile: ValidationProfile::General,
            max_input_bytes: 1024 * 1024 * 1024,
            strict: false,
            dcmvalidate_iod: None,
            command_timeout_secs: 60,
            max_files: 100_000,
            max_depth: 64,
            max_child_output_bytes: 4 * 1024 * 1024,
            max_pixel_frame_bytes: 512 * 1024 * 1024,
            conformance: crate::ConformanceLevel::Optional,
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
    /// Standard output retained from the availability/version probe.
    pub probe_stdout: String,
    /// Standard error retained from the availability/version probe.
    pub probe_stderr: String,
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
    /// Intrinsic rule set used for this report.
    pub profile: ValidationProfile,
    /// Input path passed to validation.
    pub input: PathBuf,
    /// DICOM files discovered and checked.
    pub files: Vec<PathBuf>,
    /// Individual validation checks.
    pub checks: Vec<ValidationCheck>,
    /// Each VL WSI object's optional conformance, when validation runs at
    /// `ConformanceLevel::Optional`.
    pub optional: Vec<OptionalConformance>,
}

/// What one VL Whole Slide Microscopy object implements of the IOD's optional
/// content, and which suggestions of the standard it does not follow. None of
/// it is a failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct OptionalConformance {
    /// The validated file.
    pub path: PathBuf,
    /// Optional modules, functional groups and attributes the object encodes,
    /// each with its PS3.3 table.
    pub present: Vec<String>,
    /// Those it does not encode. An absent module or Sequence is listed once,
    /// without the rows it would contain.
    pub absent: Vec<String>,
    /// Codes outside a suggested (Baseline) PS3.16 Context Group, Content
    /// Items departing from a Baseline Template, and Context Group extensions
    /// not declared with Context Group Extension Flag "Y".
    pub advisories: Vec<String>,
}

/// One normative statement of the pinned DICOM edition in the scope of the
/// pathology-slide IODs (the PS3.3 IOD, Module, Macro and Attribute
/// descriptions each reaches; the PS3.4 specializations of their SOP Classes;
/// PS3.5 encoding; PS3.10 file format), and how validation holds their
/// objects to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct RequirementEntry {
    /// The statement's stable identifier, which an evaluation case can name;
    /// statements the generated tables enforce have none.
    pub id: Option<&'static str>,
    /// "PS3.3", "PS3.4", "PS3.5" or "PS3.10".
    pub part: &'static str,
    /// The section or table label that states it.
    pub section: &'static str,
    /// The Attribute whose table row states it, if any.
    pub keyword: Option<&'static str>,
    /// The sentence, as the standard words it.
    pub text: &'static str,
    /// How validation treats it: in VL Whole Slide Microscopy objects when
    /// that IOD is among `iods`, otherwise in the objects of each IOD in
    /// `iods` that `iod_dispositions` does not name.
    pub disposition: RequirementDisposition,
    /// The PS3.3 sections of the IODs whose scope holds it, e.g. "A.32.8".
    pub iods: Vec<&'static str>,
    /// How validation treats it in each IOD of `iods` where that differs from
    /// `disposition`.
    pub iod_dispositions: BTreeMap<&'static str, RequirementDisposition>,
}

/// How validation holds an object to one normative statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
#[non_exhaustive]
pub enum RequirementDisposition {
    /// Enforced by the IOD checks generated from the standard's tables
    /// (Module usage, Attribute presence and conditions, Enumerated Values,
    /// Sequence Item counts).
    Tables,
    /// Enforced by the named intrinsic check.
    Checked(&'static str),
    /// Decidable from the encoded objects but not yet checked; the check
    /// family it awaits.
    Unchecked(&'static str),
    /// Binds IODs, encodings or contexts the IOD's objects cannot carry.
    OutsideScope(&'static str),
    /// Binds applications or processes rather than the encoded objects.
    Behavior(&'static str),
    /// Binds the objects on facts their encoded bytes cannot show.
    Undecidable(&'static str),
}

/// Every normative statement of the pinned DICOM edition in the scope of the
/// pathology-slide IODs, with the IODs it applies to and how validation
/// treats it in each.
pub fn requirement_catalog() -> Vec<RequirementEntry> {
    iod::catalog()
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

/// Retained process facts for an external validation command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ValidationExecution {
    /// Exit code when the operating system supplied one.
    pub return_code: Option<i32>,
    /// Monotonic elapsed wall time in milliseconds.
    pub elapsed_millis: u64,
    /// Infrastructure failure; a completed nonzero validator verdict has no failure here.
    pub failure: Option<ExecutionFailure>,
}

/// An external check that could not produce a usable conformance verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExecutionFailure {
    /// Required executable was absent.
    Unavailable,
    /// Executable could not be started.
    Launch,
    /// Validator configuration was invalid.
    Configuration,
    /// Evidence staging or output could not be read or written.
    Io,
    /// Process exceeded its time budget.
    Timeout,
    /// Output exceeded the evidence capture budget.
    OutputLimit,
    /// Process terminated without an exit code.
    Terminated,
}

/// Result of one intrinsic or external validator check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ValidationCheck {
    /// Process outcome, separate from the DICOM verdict; absent for intrinsic checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<ValidationExecution>,
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
}

const DCIODVFY_TOOL: ValidatorToolSpec = ValidatorToolSpec {
    name: "dciodvfy",
    required: true,
    doctor_args: &["-version"],
};
const DCENTVFY_TOOL: ValidatorToolSpec = ValidatorToolSpec {
    name: "dcentvfy",
    required: true,
    doctor_args: &["-version"],
};
const VALIDATE_IODS_TOOL: ValidatorToolSpec = ValidatorToolSpec {
    name: "validate_iods",
    required: true,
    doctor_args: &["-h"],
};
const DCMVALIDATE_TOOL: ValidatorToolSpec = ValidatorToolSpec {
    name: "dcmvalidate",
    required: true,
    doctor_args: &["--help"],
};
const VALIDATOR_DOCTOR_TOOLS: &[ValidatorToolSpec] =
    &[DCIODVFY_TOOL, DCENTVFY_TOOL, VALIDATE_IODS_TOOL];
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
    let mut optional = Vec::new();
    let mut corpus = WsiConformanceCorpus::default();

    for file in &files {
        let input_file = std::fs::File::open(file).map_err(|err| Error::Validation {
            reason: format!("cannot open {}: {err}", file.display()),
        })?;
        let bytes = input_file
            .metadata()
            .map_err(|err| Error::Validation {
                reason: format!("cannot inspect {}: {err}", file.display()),
            })?
            .len();
        if bytes > options.max_input_bytes {
            return Err(Error::Validation {
                reason: format!(
                    "{} has {bytes} bytes, exceeding max_input_bytes={}",
                    file.display(),
                    options.max_input_bytes
                ),
            });
        }
        // Limiting the opened handle prevents reading beyond policy if the file grows.
        let object = match dicom_object::OpenFileOptions::new()
            .read_preamble(dicom_object::file::ReadPreamble::Auto)
            .from_reader(input_file.take(options.max_input_bytes))
        {
            Ok(object) => object,
            Err(err) => {
                checks.push(failed_check(
                    "intrinsic-pixel-structure",
                    Some(file),
                    format!("failed to read DICOM file: {err}"),
                ));
                checks.push(iod::run_unparsed_file_meta_check(file));
                checks.push(encoding::run_encoding_check(
                    file,
                    None,
                    options.max_input_bytes,
                ));
                continue;
            }
        };
        checks.push(run_intrinsic_pixel_structure_check(
            file,
            &object,
            options.max_pixel_frame_bytes,
        ));
        checks.push(encoding::run_encoding_check(
            file,
            Some(&object),
            options.max_input_bytes,
        ));
        checks.push(digital_signatures::run_digital_signatures_check(
            file, &object,
        ));
        let (wsi_checks, notes) = run_intrinsic_wsi_conformance_checks(
            file,
            &object,
            options.profile,
            options.conformance,
            options.max_pixel_frame_bytes,
        );
        checks.extend(wsi_checks);
        if let Some(notes) = notes {
            optional.push(OptionalConformance {
                path: file.clone(),
                present: notes.present,
                absent: notes.absent,
                advisories: notes.advisories,
            });
        }
        corpus.observe(file, &object);
    }
    if let Some(check) = run_specimen_uid_set_check(&corpus) {
        checks.push(check);
    }
    if let Some(check) = run_identity_set_check(&corpus) {
        checks.push(check);
    }
    if let Some(check) = run_series_set_check(&corpus) {
        checks.push(check);
    }
    if options.profile == ValidationProfile::Core {
        if let Some(check) = run_clinical_identity_set_check(&corpus) {
            checks.push(check);
        }
    }
    if let Some(check) = run_pyramid_geometry_set_check(&corpus) {
        checks.push(check);
    }
    if let Some(check) = run_slide_coordinate_set_check(&corpus) {
        checks.push(check);
    }
    if let Some(check) = run_source_relationship_set_check(&corpus) {
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
                required: options.strict && DCIODVFY_TOOL.required,
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
            prefix_args: Vec::new(),
            check_name: DCENTVFY_TOOL.name,
            command_name: DCENTVFY_TOOL.name,
            required: options.strict && DCENTVFY_TOOL.required,
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
            prefix_args: vec![OsString::from("--edition"), OsString::from(DICOM_EDITION)],
            check_name: VALIDATE_IODS_TOOL.name,
            command_name: VALIDATE_IODS_TOOL.name,
            required: options.strict && VALIDATE_IODS_TOOL.required,
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

    Ok(ValidationReport {
        profile: options.profile,
        input,
        files,
        checks,
        optional,
    })
}

fn failed_check(name: &str, path: Option<&PathBuf>, message: String) -> ValidationCheck {
    intrinsic_check(name, path.cloned(), ValidationStatus::Failed, message)
}

/// The result of a check this crate evaluates itself, without a command.
fn intrinsic_check(
    name: &str,
    path: Option<PathBuf>,
    status: ValidationStatus,
    message: String,
) -> ValidationCheck {
    ValidationCheck {
        execution: None,
        name: name.to_string(),
        path,
        status,
        command: Vec::new(),
        message,
        stdout: String::new(),
        stderr: String::new(),
    }
}

#[cfg(test)]
mod tests;
