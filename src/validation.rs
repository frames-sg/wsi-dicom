use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use dicom_dictionary_std::tags;
use serde::{Deserialize, Serialize};

use crate::{Error, TransferSyntax};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandOutcome {
    pub(crate) success: bool,
    pub(crate) timed_out: bool,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) stdout_truncated: bool,
    pub(crate) stderr_truncated: bool,
}

pub(crate) trait ValidationCommandRunner {
    fn find_command(&self, name: &str) -> Option<PathBuf>;
    fn run(
        &self,
        program: &Path,
        args: &[OsString],
        timeout: Duration,
        max_output_bytes: usize,
    ) -> Result<CommandOutcome, std::io::Error>;
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

struct SystemCommandRunner;

impl ValidationCommandRunner for SystemCommandRunner {
    fn find_command(&self, name: &str) -> Option<PathBuf> {
        let command = Path::new(name);
        if command.is_absolute() {
            return command.is_file().then(|| command.to_path_buf());
        }
        std::env::var_os("PATH")
            .and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|path| path.join(name))
                    .find(|path| path.is_file())
            })
            .or_else(|| staged_dicom3tools_command(name))
    }

    fn run(
        &self,
        program: &Path,
        args: &[OsString],
        timeout: Duration,
        max_output_bytes: usize,
    ) -> Result<CommandOutcome, std::io::Error> {
        let mut child = Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("child stdout pipe was not captured"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("child stderr pipe was not captured"))?;
        let stdout_reader = read_child_pipe(stdout, max_output_bytes);
        let stderr_reader = read_child_pipe(stderr, max_output_bytes);
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let (stdout, stderr, stdout_truncated, stderr_truncated) =
                        collect_child_pipes(stdout_reader, stderr_reader)?;
                    return Ok(CommandOutcome {
                        success: status.success(),
                        timed_out: false,
                        stdout,
                        stderr,
                        stdout_truncated,
                        stderr_truncated,
                    });
                }
                Ok(None) => {}
                Err(err) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = collect_child_pipes(stdout_reader, stderr_reader);
                    return Err(err);
                }
            }
            if started.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait()?;
                let (stdout, stderr, stdout_truncated, stderr_truncated) =
                    collect_child_pipes(stdout_reader, stderr_reader)?;
                return Ok(CommandOutcome {
                    success: false,
                    timed_out: true,
                    stdout,
                    stderr,
                    stdout_truncated,
                    stderr_truncated,
                });
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn read_child_pipe(
    mut pipe: impl Read + Send + 'static,
    max_output_bytes: usize,
) -> JoinHandle<io::Result<CapturedOutput>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut limited = pipe.by_ref().take(
            u64::try_from(max_output_bytes)
                .unwrap_or(u64::MAX)
                .saturating_add(1),
        );
        limited.read_to_end(&mut bytes)?;
        let truncated = bytes.len() > max_output_bytes;
        if truncated {
            bytes.truncate(max_output_bytes);
            io::copy(&mut pipe, &mut io::sink())?;
        }
        Ok(CapturedOutput { bytes, truncated })
    })
}

fn collect_child_pipes(
    stdout_reader: JoinHandle<io::Result<CapturedOutput>>,
    stderr_reader: JoinHandle<io::Result<CapturedOutput>>,
) -> io::Result<(String, String, bool, bool)> {
    let stdout = collect_child_pipe(stdout_reader)?;
    let stderr = collect_child_pipe(stderr_reader)?;
    Ok((
        String::from_utf8_lossy(&stdout.bytes).into_owned(),
        String::from_utf8_lossy(&stderr.bytes).into_owned(),
        stdout.truncated,
        stderr.truncated,
    ))
}

fn collect_child_pipe(
    reader: JoinHandle<io::Result<CapturedOutput>>,
) -> io::Result<CapturedOutput> {
    reader
        .join()
        .map_err(|_| io::Error::other("child output reader thread panicked"))?
}

/// Validate a DICOM file or recursively discovered DICOM directory.
pub fn validate_dicom_path(
    path: impl AsRef<Path>,
    options: &ValidationOptions,
) -> Result<ValidationReport, Error> {
    validate_dicom_path_with_runner(path.as_ref(), options, &SystemCommandRunner)
}

/// Check local external DICOM validator availability.
pub fn doctor_dicom_environment(options: &DoctorOptions) -> DoctorReport {
    doctor_dicom_environment_with_runner(options, &SystemCommandRunner)
}

pub(crate) fn doctor_dicom_environment_with_runner(
    options: &DoctorOptions,
    runner: &impl ValidationCommandRunner,
) -> DoctorReport {
    let mut tools = VALIDATOR_DOCTOR_TOOLS
        .iter()
        .map(|tool| doctor_command_tool(runner, tool, options.strict))
        .collect::<Vec<_>>();

    tools.push(match &options.dcmvalidate_iod {
        Some(_iod) => doctor_command_tool(runner, &DCMVALIDATE_TOOL, options.strict),
        None => skipped_doctor_tool(
            "dcmvalidate",
            false,
            "dcmvalidate IOD path is not configured".to_string(),
        ),
    });
    tools.push(doctor_htj2k_decoder_tool(runner, options));

    DoctorReport { tools }
}

fn doctor_command_tool(
    runner: &impl ValidationCommandRunner,
    tool: &ValidatorToolSpec,
    strict: bool,
) -> DoctorTool {
    let args = tool
        .doctor_args
        .iter()
        .map(|arg| OsString::from(*arg))
        .collect::<Vec<_>>();
    let command = std::iter::once(tool.name.to_string())
        .chain(tool.doctor_args.iter().map(|arg| (*arg).to_string()))
        .collect::<Vec<_>>();
    match runner.find_command(tool.name) {
        Some(path) => match runner.run(&path, &args, DOCTOR_PROBE_TIMEOUT, 4 * 1024 * 1024) {
            Ok(outcome) if doctor_probe_passed(tool, &outcome) => DoctorTool {
                name: tool.name.to_string(),
                required: tool.required,
                status: DoctorStatus::Available,
                command,
                path: Some(path),
                message: format!("{} probe passed", tool.name),
            },
            Ok(outcome) => {
                let message = if outcome.timed_out {
                    format!(
                        "{} probe timed out after {}",
                        tool.name,
                        format_timeout(DOCTOR_PROBE_TIMEOUT)
                    )
                } else {
                    format!("{} probe failed", tool.name)
                };
                DoctorTool {
                    name: tool.name.to_string(),
                    required: tool.required,
                    status: DoctorStatus::Failed,
                    command,
                    path: Some(path),
                    message,
                }
            }
            Err(source) => DoctorTool {
                name: tool.name.to_string(),
                required: tool.required,
                status: DoctorStatus::Failed,
                command,
                path: Some(path),
                message: format!("failed to start {}: {source}", tool.name),
            },
        },
        None => {
            let status = if strict && tool.required {
                DoctorStatus::Failed
            } else {
                DoctorStatus::Missing
            };
            DoctorTool {
                name: tool.name.to_string(),
                required: tool.required,
                status,
                command,
                path: None,
                message: format!("{} not found", tool.name),
            }
        }
    }
}

fn doctor_probe_passed(tool: &ValidatorToolSpec, outcome: &CommandOutcome) -> bool {
    !outcome.timed_out
        && (outcome.success
            || tool
                .nonzero_success_output
                .is_some_and(|needle| output_contains_probe_needle(outcome, needle)))
}

fn output_contains_probe_needle(outcome: &CommandOutcome, needle: &str) -> bool {
    outcome.stdout.contains(needle) || outcome.stderr.contains(needle)
}

fn doctor_htj2k_decoder_tool(
    runner: &impl ValidationCommandRunner,
    options: &DoctorOptions,
) -> DoctorTool {
    let Some(template) = &options.htj2k_decoder else {
        return skipped_doctor_tool(
            "htj2k_decoder",
            false,
            "HTJ2K decoder command is not configured".to_string(),
        );
    };
    let (name, args) =
        match htj2k_decoder_command(template, Path::new("input.jhc"), Path::new("output.ppm")) {
            Ok(command) => command,
            Err(message) => {
                return DoctorTool {
                    name: "htj2k_decoder".to_string(),
                    required: true,
                    status: DoctorStatus::Failed,
                    command: Vec::new(),
                    path: None,
                    message,
                };
            }
        };
    let command = std::iter::once(name.clone())
        .chain(args.iter().map(|arg| arg.to_string_lossy().into_owned()))
        .collect::<Vec<_>>();
    match runner.find_command(&name) {
        Some(path) => DoctorTool {
            name: "htj2k_decoder".to_string(),
            required: true,
            status: DoctorStatus::Available,
            command,
            path: Some(path),
            message: format!("{name} found"),
        },
        None => DoctorTool {
            name: "htj2k_decoder".to_string(),
            required: true,
            status: DoctorStatus::Failed,
            command,
            path: None,
            message: format!("{name} not found"),
        },
    }
}

fn skipped_doctor_tool(name: &str, required: bool, message: String) -> DoctorTool {
    DoctorTool {
        name: name.to_string(),
        required,
        status: DoctorStatus::Skipped,
        command: Vec::new(),
        path: None,
        message,
    }
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

fn discover_dicom_files(input: &Path, options: &ValidationOptions) -> Result<Vec<PathBuf>, Error> {
    let metadata = std::fs::symlink_metadata(input).map_err(|source| Error::Io {
        path: input.to_path_buf(),
        source,
    })?;
    let mut files = Vec::new();
    if metadata.file_type().is_symlink() {
        return Err(Error::Validation {
            reason: format!("refusing to validate symlink path {}", input.display()),
        });
    } else if metadata.is_file() {
        files.push(input.to_path_buf());
    } else if metadata.is_dir() {
        collect_dicom_files(input, options, &mut files)?;
        files.sort();
    } else {
        return Err(Error::Validation {
            reason: format!("{} is not a regular file or directory", input.display()),
        });
    }
    if files.is_empty() {
        return Err(Error::Validation {
            reason: format!("no .dcm files found under {}", input.display()),
        });
    }
    Ok(files)
}

fn collect_dicom_files(
    root: &Path,
    options: &ValidationOptions,
    files: &mut Vec<PathBuf>,
) -> Result<(), Error> {
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = pending.pop() {
        if depth > options.max_depth {
            return Err(Error::Validation {
                reason: format!(
                    "DICOM validation directory depth exceeds max_depth={} at {}",
                    options.max_depth,
                    dir.display()
                ),
            });
        }
        let entries = std::fs::read_dir(&dir).map_err(|source| Error::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| Error::Io {
                path: dir.clone(),
                source,
            })?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            if file_type.is_symlink() {
                return Err(Error::Validation {
                    reason: format!("refusing to traverse symlink {}", path.display()),
                });
            } else if file_type.is_dir() {
                pending.push((path, depth + 1));
            } else if file_type.is_file() && has_dcm_extension(&path) {
                files.push(path);
                if files.len() > options.max_files {
                    return Err(Error::Validation {
                        reason: format!(
                            "DICOM validation found more than max_files={} files",
                            options.max_files
                        ),
                    });
                }
            }
        }
    }
    Ok(())
}

fn has_dcm_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("dcm"))
}

struct CommandCheckRequest<'a> {
    check_name: &'a str,
    command_name: &'a str,
    args: Vec<OsString>,
    path: Option<&'a PathBuf>,
    required: bool,
    error_line_is_failure: bool,
    timeout: Duration,
    max_output_bytes: usize,
}

struct SetLevelCommandCheckRequest<'a> {
    check_name: &'a str,
    command_name: &'a str,
    required: bool,
    error_line_is_failure: bool,
    timeout: Duration,
    max_output_bytes: usize,
    chunk_size: usize,
}

fn run_set_level_command_checks(
    runner: &impl ValidationCommandRunner,
    files: &[PathBuf],
    request: SetLevelCommandCheckRequest<'_>,
) -> Vec<ValidationCheck> {
    let chunk_size = request.chunk_size.max(1);
    files
        .chunks(chunk_size)
        .map(|chunk| {
            run_named_command_check(
                runner,
                CommandCheckRequest {
                    check_name: request.check_name,
                    command_name: request.command_name,
                    args: chunk
                        .iter()
                        .map(|file| file.as_os_str().to_os_string())
                        .collect(),
                    path: None,
                    required: request.required,
                    error_line_is_failure: request.error_line_is_failure,
                    timeout: request.timeout,
                    max_output_bytes: request.max_output_bytes,
                },
            )
        })
        .collect()
}

fn run_named_command_check(
    runner: &impl ValidationCommandRunner,
    request: CommandCheckRequest<'_>,
) -> ValidationCheck {
    let CommandCheckRequest {
        check_name,
        command_name,
        args,
        path,
        required,
        error_line_is_failure,
        timeout,
        max_output_bytes,
    } = request;
    let command = std::iter::once(command_name.to_string())
        .chain(args.iter().map(|arg| arg.to_string_lossy().into_owned()))
        .collect::<Vec<_>>();
    let Some(program) = runner.find_command(command_name) else {
        let status = if required {
            ValidationStatus::Failed
        } else {
            ValidationStatus::Skipped
        };
        return ValidationCheck {
            name: check_name.to_string(),
            path: path.cloned(),
            status,
            command,
            message: format!("{command_name} not found"),
            stdout: String::new(),
            stderr: String::new(),
        };
    };

    match runner.run(&program, &args, timeout, max_output_bytes) {
        Ok(outcome) => {
            if outcome.stdout_truncated || outcome.stderr_truncated {
                return ValidationCheck {
                    name: check_name.to_string(),
                    path: path.cloned(),
                    status: ValidationStatus::Failed,
                    command,
                    message: format!(
                        "{command_name} output exceeded {} byte capture limit",
                        max_output_bytes
                    ),
                    stdout: outcome.stdout,
                    stderr: outcome.stderr,
                };
            }
            if outcome.timed_out {
                return ValidationCheck {
                    name: check_name.to_string(),
                    path: path.cloned(),
                    status: ValidationStatus::Failed,
                    command,
                    message: format!("{command_name} timed out after {}", format_timeout(timeout)),
                    stdout: outcome.stdout,
                    stderr: outcome.stderr,
                };
            }
            let output_has_error = error_line_is_failure
                && outcome
                    .stdout
                    .lines()
                    .chain(outcome.stderr.lines())
                    .any(|line| line.trim_start().starts_with("Error"));
            let status = if outcome.success && !output_has_error {
                ValidationStatus::Passed
            } else {
                ValidationStatus::Failed
            };
            ValidationCheck {
                name: check_name.to_string(),
                path: path.cloned(),
                status,
                command,
                message: if status == ValidationStatus::Passed {
                    format!("{command_name} passed")
                } else {
                    format!("{command_name} failed")
                },
                stdout: outcome.stdout,
                stderr: outcome.stderr,
            }
        }
        Err(source) => ValidationCheck {
            name: check_name.to_string(),
            path: path.cloned(),
            status: ValidationStatus::Failed,
            command,
            message: format!("failed to start {command_name}: {source}"),
            stdout: String::new(),
            stderr: String::new(),
        },
    }
}

fn format_timeout(timeout: Duration) -> String {
    if timeout.as_millis() > 0 && timeout.as_millis() < 1000 {
        format!("{}ms", timeout.as_millis())
    } else if timeout.subsec_millis() == 0 {
        format!("{}s", timeout.as_secs())
    } else {
        format!("{}ms", timeout.as_millis())
    }
}

fn run_pixel_decode_checks(
    file_idx: usize,
    file: &PathBuf,
    options: &ValidationOptions,
    runner: &impl ValidationCommandRunner,
    temp_dir: &Path,
) -> Vec<ValidationCheck> {
    let object = match dicom_object::open_file(file) {
        Ok(object) => object,
        Err(err) => {
            return vec![failed_check(
                "pixel-decode",
                Some(file),
                format!("failed to read DICOM file for pixel decode: {err}"),
            )];
        }
    };
    let transfer_syntax = object.meta().transfer_syntax.trim_end_matches('\0');
    let Some(decoder) = pixel_decoder_for_transfer_syntax(transfer_syntax, options) else {
        return vec![skipped_check(
            "pixel-decode",
            Some(file),
            format!("pixel decode not needed for transfer syntax {transfer_syntax}"),
        )];
    };
    if let PixelDecoder::Htj2kUnconfigured = decoder {
        let status = if options.strict {
            ValidationStatus::Failed
        } else {
            ValidationStatus::Skipped
        };
        return vec![ValidationCheck {
            name: "pixel-htj2k".to_string(),
            path: Some(file.clone()),
            status,
            command: Vec::new(),
            message: "HTJ2K decoder command is not configured".to_string(),
            stdout: String::new(),
            stderr: String::new(),
        }];
    }

    let pixel_data = match object.element(tags::PIXEL_DATA) {
        Ok(pixel_data) => pixel_data,
        Err(err) => {
            return vec![failed_check(
                "pixel-decode",
                Some(file),
                format!("failed to read Pixel Data: {err}"),
            )];
        }
    };
    let Some(fragments) = pixel_data.value().fragments() else {
        return vec![skipped_check(
            "pixel-decode",
            Some(file),
            "Pixel Data is not encapsulated".to_string(),
        )];
    };
    if fragments.is_empty() {
        return vec![skipped_check(
            "pixel-decode",
            Some(file),
            "Pixel Data has no fragments".to_string(),
        )];
    }

    let mut checks = Vec::new();
    for (frame_idx, fragment) in fragments.iter().take(options.max_pixel_frames).enumerate() {
        checks.push(run_pixel_decoder_for_fragment(
            &decoder,
            PixelFragmentDecode {
                file_idx,
                frame_idx,
                fragment: fragment_payload_without_padding(fragment),
                file,
                runner,
                temp_dir,
                strict: options.strict,
                timeout: options.command_timeout(),
                max_output_bytes: options.max_child_output_bytes,
            },
        ));
    }
    checks
}

enum PixelDecoder {
    Djpeg,
    OpenJpeg,
    Htj2kUnconfigured,
    Htj2k { template: String },
}

fn pixel_decoder_for_transfer_syntax(
    transfer_syntax_uid: &str,
    options: &ValidationOptions,
) -> Option<PixelDecoder> {
    match transfer_syntax_uid {
        uid if uid == TransferSyntax::JpegBaseline8Bit.uid() => Some(PixelDecoder::Djpeg),
        uid if uid == TransferSyntax::Jpeg2000.uid()
            || uid == TransferSyntax::Jpeg2000Lossless.uid() =>
        {
            Some(PixelDecoder::OpenJpeg)
        }
        uid if uid == TransferSyntax::Htj2k.uid()
            || uid == TransferSyntax::Htj2kLossless.uid()
            || uid == TransferSyntax::Htj2kLosslessRpcl.uid() =>
        {
            Some(
                options
                    .htj2k_decoder
                    .as_ref()
                    .map(|template| PixelDecoder::Htj2k {
                        template: template.clone(),
                    })
                    .unwrap_or(PixelDecoder::Htj2kUnconfigured),
            )
        }
        _ => None,
    }
}

struct PixelFragmentDecode<'a, R: ValidationCommandRunner> {
    file_idx: usize,
    frame_idx: usize,
    fragment: &'a [u8],
    file: &'a PathBuf,
    runner: &'a R,
    temp_dir: &'a Path,
    strict: bool,
    timeout: Duration,
    max_output_bytes: usize,
}

fn run_pixel_decoder_for_fragment<R: ValidationCommandRunner>(
    decoder: &PixelDecoder,
    request: PixelFragmentDecode<'_, R>,
) -> ValidationCheck {
    let input = request.temp_dir.join(format!(
        "file-{:04}-frame-{:06}.codestream",
        request.file_idx, request.frame_idx
    ));
    let output = request.temp_dir.join(format!(
        "file-{:04}-frame-{:06}.ppm",
        request.file_idx, request.frame_idx
    ));
    if let Err(err) = write_private_validation_file(&input, request.fragment) {
        return failed_check(
            "pixel-decode",
            Some(request.file),
            format!("failed to write temporary codestream: {err}"),
        );
    }

    match decoder {
        PixelDecoder::Djpeg => run_named_command_check(
            request.runner,
            CommandCheckRequest {
                check_name: "pixel-djpeg",
                command_name: "djpeg",
                args: vec![
                    OsString::from("-outfile"),
                    output.as_os_str().to_os_string(),
                    input.as_os_str().to_os_string(),
                ],
                path: Some(request.file),
                required: request.strict,
                error_line_is_failure: false,
                timeout: request.timeout,
                max_output_bytes: request.max_output_bytes,
            },
        ),
        PixelDecoder::OpenJpeg => run_named_command_check(
            request.runner,
            CommandCheckRequest {
                check_name: "pixel-opj-decompress",
                command_name: "opj_decompress",
                args: vec![
                    OsString::from("-i"),
                    input.as_os_str().to_os_string(),
                    OsString::from("-o"),
                    output.as_os_str().to_os_string(),
                ],
                path: Some(request.file),
                required: request.strict,
                error_line_is_failure: false,
                timeout: request.timeout,
                max_output_bytes: request.max_output_bytes,
            },
        ),
        PixelDecoder::Htj2k { template } => {
            let (command, args) = match htj2k_decoder_command(template, &input, &output) {
                Ok(command) => command,
                Err(message) => {
                    return failed_check("pixel-htj2k", Some(request.file), message);
                }
            };
            run_named_command_check(
                request.runner,
                CommandCheckRequest {
                    check_name: "pixel-htj2k",
                    command_name: &command,
                    args,
                    path: Some(request.file),
                    required: request.strict,
                    error_line_is_failure: false,
                    timeout: request.timeout,
                    max_output_bytes: request.max_output_bytes,
                },
            )
        }
        PixelDecoder::Htj2kUnconfigured => skipped_check(
            "pixel-htj2k",
            Some(request.file),
            "HTJ2K decoder command is not configured".to_string(),
        ),
    }
}

pub(crate) fn htj2k_decoder_command(
    template: &str,
    input: &Path,
    output: &Path,
) -> Result<(String, Vec<OsString>), String> {
    let mut parts = shlex::split(template)
        .ok_or_else(|| "HTJ2K decoder command has invalid quoting".to_string())?;
    if parts.is_empty() {
        return Err("HTJ2K decoder command is empty".to_string());
    }
    let command = parts.remove(0);
    if command.trim().is_empty() {
        return Err("HTJ2K decoder command is empty".to_string());
    }
    if !Path::new(&command).is_absolute() {
        return Err(
            "HTJ2K decoder command must start with an absolute executable path".to_string(),
        );
    }
    let mut saw_placeholder = false;
    let mut args = parts
        .into_iter()
        .map(|part| {
            let replaced = part
                .replace("{input}", &input.to_string_lossy())
                .replace("{output}", &output.to_string_lossy());
            if replaced != part {
                saw_placeholder = true;
            }
            OsString::from(replaced)
        })
        .collect::<Vec<_>>();
    if !saw_placeholder {
        args.push(input.as_os_str().to_os_string());
    }
    Ok((command, args))
}

pub(crate) fn fragment_payload_without_padding(fragment: &[u8]) -> &[u8] {
    fragment
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

fn write_private_validation_file(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.write_all(bytes).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.sync_all().map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

struct ValidationTempDir {
    inner: tempfile::TempDir,
}

impl ValidationTempDir {
    fn create() -> Result<Self, Error> {
        let inner = tempfile::Builder::new()
            .prefix("wsi-dicom-validation-")
            .tempdir()
            .map_err(|source| Error::Io {
                path: std::env::temp_dir(),
                source,
            })?;
        set_private_validation_dir_permissions(inner.path())?;
        Ok(Self { inner })
    }

    fn path(&self) -> &Path {
        self.inner.path()
    }
}

#[cfg(unix)]
fn set_private_validation_dir_permissions(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|source| {
        Error::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn set_private_validation_dir_permissions(_path: &Path) -> Result<(), Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        doctor_dicom_environment_with_runner, validate_dicom_path_with_runner, CommandOutcome,
        DoctorOptions, DoctorStatus, SystemCommandRunner, ValidationCommandRunner,
        ValidationOptions, ValidationStatus,
    };
    use dicom_core::{DataElement, PrimitiveValue, VR};
    use dicom_dictionary_std::tags;
    use dicom_object::{FileMetaTableBuilder, InMemDicomObject};
    use std::collections::{BTreeMap, BTreeSet};
    use std::ffi::OsString;
    use std::fs::File;
    use std::io::{BufWriter, Write};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    #[cfg(not(windows))]
    const ABSOLUTE_HTJ2K_DECODER: &str = "/usr/local/bin/ojph_expand";
    #[cfg(windows)]
    const ABSOLUTE_HTJ2K_DECODER: &str = "C:/Tools/ojph_expand.exe";

    #[derive(Default)]
    struct FakeRunner {
        commands: BTreeSet<String>,
        outcomes: BTreeMap<String, CommandOutcome>,
    }

    impl FakeRunner {
        fn with_command(mut self, name: &str) -> Self {
            self.commands.insert(name.to_string());
            self
        }

        fn with_outcome(mut self, command: &str, outcome: CommandOutcome) -> Self {
            self.outcomes.insert(command.to_string(), outcome);
            self
        }
    }

    impl ValidationCommandRunner for FakeRunner {
        fn find_command(&self, name: &str) -> Option<PathBuf> {
            self.commands.contains(name).then(|| PathBuf::from(name))
        }

        fn run(
            &self,
            program: &Path,
            args: &[OsString],
            _timeout: Duration,
            _max_output_bytes: usize,
        ) -> Result<CommandOutcome, std::io::Error> {
            let mut key = program.display().to_string();
            for arg in args {
                key.push(' ');
                key.push_str(&arg.to_string_lossy());
            }
            Ok(self.outcomes.get(&key).cloned().unwrap_or(CommandOutcome {
                success: true,
                timed_out: false,
                stdout: String::new(),
                stderr: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
            }))
        }
    }

    #[cfg(unix)]
    #[test]
    fn system_runner_drains_stdout_while_waiting_for_child_exit() {
        let runner = SystemCommandRunner;
        let outcome = runner
            .run(
                Path::new("/bin/sh"),
                &[
                    OsString::from("-c"),
                    OsString::from("yes validation-output | head -c 200000"),
                ],
                Duration::from_secs(5),
                4 * 1024 * 1024,
            )
            .unwrap();

        assert!(outcome.success);
        assert_eq!(outcome.stdout.len(), 200_000);
        assert!(!outcome.timed_out);
    }

    #[test]
    fn validation_discovers_dicom_files_recursively() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let nested = tmp.path().join("nested");
        std::fs::create_dir(&nested).expect("create nested");
        let first = tmp.path().join("one.dcm");
        let second = nested.join("two.DCM");
        std::fs::write(&first, b"not parsed without pixel checks").expect("write first");
        std::fs::write(&second, b"not parsed without pixel checks").expect("write second");
        std::fs::write(tmp.path().join("notes.txt"), b"ignore").expect("write ignored");

        let report = validate_dicom_path_with_runner(
            tmp.path(),
            &ValidationOptions {
                max_pixel_frames: 0,
                ..ValidationOptions::default()
            },
            &FakeRunner::default(),
        )
        .expect("validation report");

        assert_eq!(report.files, vec![second, first]);
    }

    #[test]
    fn validation_enforces_file_and_depth_limits() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let nested = tmp.path().join("nested");
        std::fs::create_dir(&nested).expect("create nested");
        std::fs::write(tmp.path().join("one.dcm"), b"one").expect("write one");
        std::fs::write(nested.join("two.dcm"), b"two").expect("write two");

        let err = validate_dicom_path_with_runner(
            tmp.path(),
            &ValidationOptions {
                max_pixel_frames: 0,
                max_files: 1,
                ..ValidationOptions::default()
            },
            &FakeRunner::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("max_files"));

        let err = validate_dicom_path_with_runner(
            tmp.path(),
            &ValidationOptions {
                max_pixel_frames: 0,
                max_depth: 0,
                ..ValidationOptions::default()
            },
            &FakeRunner::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("max_depth"));
    }

    #[cfg(unix)]
    #[test]
    fn validation_refuses_symlink_traversal() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).expect("create target");
        std::fs::write(target.join("one.dcm"), b"one").expect("write one");
        std::os::unix::fs::symlink(&target, tmp.path().join("link")).expect("symlink");

        let err = validate_dicom_path_with_runner(
            tmp.path(),
            &ValidationOptions {
                max_pixel_frames: 0,
                ..ValidationOptions::default()
            },
            &FakeRunner::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("symlink"));
    }

    #[test]
    fn missing_tools_are_skipped_by_default() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("one.dcm");
        std::fs::write(&file, b"not parsed without pixel checks").expect("write file");

        let report = validate_dicom_path_with_runner(
            &file,
            &ValidationOptions {
                max_pixel_frames: 0,
                ..ValidationOptions::default()
            },
            &FakeRunner::default(),
        )
        .expect("validation report");

        assert!(report
            .checks
            .iter()
            .any(|check| check.name == "dciodvfy" && check.status == ValidationStatus::Skipped));
        assert!(!report.has_failures());
    }

    #[test]
    fn strict_mode_fails_missing_required_tools() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("one.dcm");
        std::fs::write(&file, b"not parsed without pixel checks").expect("write file");

        let report = validate_dicom_path_with_runner(
            &file,
            &ValidationOptions {
                strict: true,
                max_pixel_frames: 0,
                ..ValidationOptions::default()
            },
            &FakeRunner::default(),
        )
        .expect("validation report");

        assert!(report
            .checks
            .iter()
            .any(|check| check.name == "dciodvfy" && check.status == ValidationStatus::Failed));
        assert!(report.has_failures());
    }

    #[test]
    fn dcentvfy_runs_once_for_the_output_set() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let first = tmp.path().join("one.dcm");
        let second = tmp.path().join("two.dcm");
        std::fs::write(&first, b"not parsed without pixel checks").expect("write first");
        std::fs::write(&second, b"not parsed without pixel checks").expect("write second");

        let report = validate_dicom_path_with_runner(
            tmp.path(),
            &ValidationOptions {
                max_pixel_frames: 0,
                ..ValidationOptions::default()
            },
            &FakeRunner::default().with_command("dcentvfy"),
        )
        .expect("validation report");

        let set_checks = report
            .checks
            .iter()
            .filter(|check| check.name == "dcentvfy")
            .count();

        assert_eq!(set_checks, 1);
    }

    #[test]
    fn set_level_validators_are_chunked_and_preserve_failures() {
        let tmp = tempfile::tempdir().expect("tempdir");
        for idx in 0..=super::VALIDATOR_SET_FILE_CHUNK_SIZE {
            std::fs::write(
                tmp.path().join(format!("file-{idx:04}.dcm")),
                b"not parsed without pixel checks",
            )
            .expect("write DICOM placeholder");
        }
        let failing_file = tmp.path().join(format!(
            "file-{:04}.dcm",
            super::VALIDATOR_SET_FILE_CHUNK_SIZE
        ));
        let failing_key = format!("dcentvfy {}", failing_file.display());
        let runner = FakeRunner::default().with_command("dcentvfy").with_outcome(
            &failing_key,
            CommandOutcome {
                success: false,
                timed_out: false,
                stdout: String::new(),
                stderr: "set check failed".to_string(),
                stdout_truncated: false,
                stderr_truncated: false,
            },
        );

        let report = validate_dicom_path_with_runner(
            tmp.path(),
            &ValidationOptions {
                max_pixel_frames: 0,
                ..ValidationOptions::default()
            },
            &runner,
        )
        .expect("validation report");

        let dcentvfy_checks = report
            .checks
            .iter()
            .filter(|check| check.name == "dcentvfy")
            .collect::<Vec<_>>();
        assert_eq!(dcentvfy_checks.len(), 2);
        assert!(dcentvfy_checks
            .iter()
            .all(|check| check.command.len() <= super::VALIDATOR_SET_FILE_CHUNK_SIZE + 1));
        assert!(dcentvfy_checks
            .iter()
            .any(|check| check.status == ValidationStatus::Failed));
        assert!(report.has_failures());
    }

    #[test]
    fn jpeg_baseline_pixel_decode_uses_djpeg() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("jpeg.dcm");
        write_encapsulated_dicom(&file, "1.2.840.10008.1.2.4.50", &[0xFF, 0xD8, 0xFF, 0xD9]);

        let report = validate_dicom_path_with_runner(
            &file,
            &ValidationOptions::default(),
            &FakeRunner::default().with_command("djpeg"),
        )
        .expect("validation report");

        assert!(report.checks.iter().any(|check| {
            check.name == "pixel-djpeg" && check.status == ValidationStatus::Passed
        }));
    }

    #[test]
    fn jpeg2000_pixel_decode_uses_openjpeg() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("j2k.dcm");
        write_encapsulated_dicom(&file, "1.2.840.10008.1.2.4.90", &[0xFF, 0x4F, 0xFF, 0x51]);

        let report = validate_dicom_path_with_runner(
            &file,
            &ValidationOptions::default(),
            &FakeRunner::default().with_command("opj_decompress"),
        )
        .expect("validation report");

        assert!(report.checks.iter().any(|check| {
            check.name == "pixel-opj-decompress" && check.status == ValidationStatus::Passed
        }));
    }

    #[test]
    fn htj2k_pixel_decode_skips_without_configured_decoder() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("htj2k.dcm");
        write_encapsulated_dicom(&file, "1.2.840.10008.1.2.4.202", &[0xFF, 0x4F, 0xFF, 0x51]);

        let report = validate_dicom_path_with_runner(
            &file,
            &ValidationOptions::default(),
            &FakeRunner::default(),
        )
        .expect("validation report");

        assert!(report.checks.iter().any(|check| {
            check.name == "pixel-htj2k" && check.status == ValidationStatus::Skipped
        }));
    }

    #[test]
    fn strict_mode_fails_missing_pixel_decoder_for_encountered_transfer_syntax() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("jpeg.dcm");
        write_encapsulated_dicom(&file, "1.2.840.10008.1.2.4.50", &[0xFF, 0xD8, 0xFF, 0xD9]);

        let report = validate_dicom_path_with_runner(
            &file,
            &ValidationOptions {
                strict: true,
                ..ValidationOptions::default()
            },
            &FakeRunner::default(),
        )
        .expect("validation report");

        assert!(report.checks.iter().any(|check| {
            check.name == "pixel-djpeg" && check.status == ValidationStatus::Failed
        }));
    }

    #[test]
    fn zero_pixel_frame_limit_disables_pixel_decode_checks() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("jpeg.dcm");
        write_encapsulated_dicom(&file, "1.2.840.10008.1.2.4.50", &[0xFF, 0xD8, 0xFF, 0xD9]);

        let report = validate_dicom_path_with_runner(
            &file,
            &ValidationOptions {
                max_pixel_frames: 0,
                ..ValidationOptions::default()
            },
            &FakeRunner::default().with_command("djpeg"),
        )
        .expect("validation report");

        assert!(!report
            .checks
            .iter()
            .any(|check| check.name.starts_with("pixel-")));
    }

    #[test]
    fn uncompressed_transfer_syntax_skips_pixel_decode() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("explicit.dcm");
        write_encapsulated_dicom(&file, "1.2.840.10008.1.2.1", &[1, 2, 3, 4]);

        let report = validate_dicom_path_with_runner(
            &file,
            &ValidationOptions::default(),
            &FakeRunner::default(),
        )
        .expect("validation report");

        assert!(report.checks.iter().any(|check| {
            check.name == "pixel-decode" && check.status == ValidationStatus::Skipped
        }));
    }

    #[test]
    fn htj2k_decoder_template_preserves_quoted_arguments() {
        let input = Path::new("/tmp/input codestream.j2k");
        let output = Path::new("/tmp/output pixels.ppm");
        let template =
            format!("{ABSOLUTE_HTJ2K_DECODER} --codec \"Open JPH\" -i {{input}} -o {{output}}");

        let (command, args) =
            super::htj2k_decoder_command(&template, input, output).expect("parse decoder command");

        assert_eq!(command, ABSOLUTE_HTJ2K_DECODER);
        assert_eq!(
            args,
            vec![
                OsString::from("--codec"),
                OsString::from("Open JPH"),
                OsString::from("-i"),
                input.as_os_str().to_os_string(),
                OsString::from("-o"),
                output.as_os_str().to_os_string(),
            ]
        );
    }

    #[test]
    fn htj2k_decoder_template_rejects_bare_command_name() {
        let err = super::htj2k_decoder_command(
            "ojph_expand -i {input} -o {output}",
            Path::new("/tmp/input.jhc"),
            Path::new("/tmp/output.ppm"),
        )
        .unwrap_err();

        assert!(err.contains("absolute executable path"));
    }

    #[test]
    fn empty_htj2k_decoder_template_is_reported_as_configuration_failure() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("htj2k.dcm");
        write_encapsulated_dicom(&file, "1.2.840.10008.1.2.4.202", &[0xFF, 0x4F, 0xFF, 0x51]);

        let report = validate_dicom_path_with_runner(
            &file,
            &ValidationOptions {
                htj2k_decoder: Some("   ".to_string()),
                ..ValidationOptions::default()
            },
            &FakeRunner::default(),
        )
        .expect("validation report");

        assert!(report.checks.iter().any(|check| {
            check.name == "pixel-htj2k"
                && check.status == ValidationStatus::Failed
                && check.message.contains("HTJ2K decoder command is empty")
        }));
    }

    #[test]
    fn bare_htj2k_decoder_template_is_reported_as_configuration_failure() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("htj2k.dcm");
        write_encapsulated_dicom(&file, "1.2.840.10008.1.2.4.202", &[0xFF, 0x4F, 0xFF, 0x51]);

        let report = validate_dicom_path_with_runner(
            &file,
            &ValidationOptions {
                htj2k_decoder: Some("ojph_expand -i {input} -o {output}".to_string()),
                ..ValidationOptions::default()
            },
            &FakeRunner::default(),
        )
        .expect("validation report");

        assert!(report.checks.iter().any(|check| {
            check.name == "pixel-htj2k"
                && check.status == ValidationStatus::Failed
                && check.message.contains("absolute executable path")
        }));
    }

    #[test]
    fn fragment_payload_ending_in_zero_is_preserved_for_validation() {
        assert_eq!(
            super::fragment_payload_without_padding(&[0xFF, 0x4F, 0x00]),
            &[0xFF, 0x4F, 0x00]
        );
        assert_eq!(
            super::fragment_payload_without_padding(&[0xFF, 0x4F, 0x00, 0x00]),
            &[0xFF, 0x4F, 0x00, 0x00]
        );
    }

    #[cfg(unix)]
    #[test]
    fn validation_temp_dir_and_codestream_files_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = super::ValidationTempDir::create().expect("validation temp dir");
        let dir_mode = std::fs::metadata(temp_dir.path())
            .expect("temp dir metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);

        let codestream = temp_dir.path().join("frame.codestream");
        super::write_private_validation_file(&codestream, b"codestream").expect("write codestream");
        let file_mode = std::fs::metadata(&codestream)
            .expect("codestream metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
    }

    #[test]
    fn command_timeout_is_reported_as_failed_check() {
        let runner = FakeRunner::default().with_command("dciodvfy").with_outcome(
            "dciodvfy -new one.dcm",
            CommandOutcome {
                success: false,
                timed_out: true,
                stdout: String::new(),
                stderr: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
            },
        );

        let check = super::run_named_command_check(
            &runner,
            super::CommandCheckRequest {
                check_name: "dciodvfy",
                command_name: "dciodvfy",
                args: vec![OsString::from("-new"), OsString::from("one.dcm")],
                path: None,
                required: true,
                error_line_is_failure: true,
                timeout: std::time::Duration::from_millis(25),
                max_output_bytes: 4 * 1024 * 1024,
            },
        );

        assert_eq!(check.status, ValidationStatus::Failed);
        assert!(check.message.contains("timed out after 25ms"));
    }

    #[test]
    fn command_output_limit_is_reported_as_failed_check() {
        let runner = FakeRunner::default().with_command("dciodvfy").with_outcome(
            "dciodvfy -new one.dcm",
            CommandOutcome {
                success: true,
                timed_out: false,
                stdout: "prefix".to_string(),
                stderr: String::new(),
                stdout_truncated: true,
                stderr_truncated: false,
            },
        );

        let check = super::run_named_command_check(
            &runner,
            super::CommandCheckRequest {
                check_name: "dciodvfy",
                command_name: "dciodvfy",
                args: vec![OsString::from("-new"), OsString::from("one.dcm")],
                path: None,
                required: true,
                error_line_is_failure: true,
                timeout: std::time::Duration::from_millis(25),
                max_output_bytes: 4,
            },
        );

        assert_eq!(check.status, ValidationStatus::Failed);
        assert!(check.message.contains("capture limit"));
    }

    #[test]
    fn validation_options_use_seconds_for_json_and_runtime_timeout() {
        let options = ValidationOptions {
            strict: true,
            dcmvalidate_iod: Some(PathBuf::from("iod.xml")),
            htj2k_decoder: Some("ojph_expand -i {input} -o {output}".to_string()),
            max_pixel_frames: 3,
            command_timeout_secs: 12,
            max_files: 100_000,
            max_depth: 64,
            max_child_output_bytes: 4 * 1024 * 1024,
        };

        let json = serde_json::to_string(&options).expect("serialize validation options");
        assert!(json.contains("\"command_timeout_secs\":12"));

        let options: ValidationOptions =
            serde_json::from_str(&json).expect("deserialize validation options");
        assert!(options.strict);
        assert_eq!(options.command_timeout(), Duration::from_secs(12));
        assert_eq!(options.max_pixel_frames, 3);
    }

    #[test]
    fn doctor_reports_missing_tools_without_failing_non_strict_runs() {
        let report =
            doctor_dicom_environment_with_runner(&DoctorOptions::default(), &FakeRunner::default());

        assert!(report
            .tools
            .iter()
            .any(|tool| { tool.name == "dciodvfy" && tool.status == DoctorStatus::Missing }));
        assert!(!report.has_failures());
    }

    #[test]
    fn doctor_fails_missing_baseline_tools_in_strict_mode() {
        let report = doctor_dicom_environment_with_runner(
            &DoctorOptions {
                strict: true,
                ..DoctorOptions::default()
            },
            &FakeRunner::default(),
        );

        assert!(report
            .tools
            .iter()
            .any(|tool| { tool.name == "dciodvfy" && tool.status == DoctorStatus::Failed }));
        assert!(report.has_failures());
    }

    #[test]
    fn doctor_runs_probe_for_found_commands() {
        let report = doctor_dicom_environment_with_runner(
            &DoctorOptions::default(),
            &FakeRunner::default().with_command("dciodvfy"),
        );

        let tool = report
            .tools
            .iter()
            .find(|tool| tool.name == "dciodvfy")
            .expect("dciodvfy doctor tool");
        assert_eq!(tool.status, DoctorStatus::Available);
        assert_eq!(tool.command, vec!["dciodvfy", "-version"]);
        assert!(tool.message.contains("probe passed"));
    }

    #[test]
    fn doctor_fails_found_command_when_probe_fails() {
        let runner = FakeRunner::default().with_command("dciodvfy").with_outcome(
            "dciodvfy -version",
            CommandOutcome {
                success: false,
                timed_out: false,
                stdout: String::new(),
                stderr: "bad probe".to_string(),
                stdout_truncated: false,
                stderr_truncated: false,
            },
        );

        let report = doctor_dicom_environment_with_runner(&DoctorOptions::default(), &runner);

        assert!(report.tools.iter().any(|tool| {
            tool.name == "dciodvfy"
                && tool.status == DoctorStatus::Failed
                && tool.message.contains("probe failed")
        }));
        assert!(report.has_failures());
    }

    #[test]
    fn doctor_accepts_openjpeg_help_output_when_it_exits_nonzero() {
        let runner = FakeRunner::default()
            .with_command("opj_decompress")
            .with_outcome(
                "opj_decompress -h",
                CommandOutcome {
                    success: false,
                    timed_out: false,
                    stdout: "This is the opj_decompress utility from the OpenJPEG project."
                        .to_string(),
                    stderr: String::new(),
                    stdout_truncated: false,
                    stderr_truncated: false,
                },
            );

        let report = doctor_dicom_environment_with_runner(&DoctorOptions::default(), &runner);

        assert!(report.tools.iter().any(|tool| {
            tool.name == "opj_decompress" && tool.status == DoctorStatus::Available
        }));
    }

    #[test]
    fn doctor_parses_configured_htj2k_decoder_template() {
        let report = doctor_dicom_environment_with_runner(
            &DoctorOptions {
                htj2k_decoder: Some(format!(
                    "{ABSOLUTE_HTJ2K_DECODER} -i {{input}} -o {{output}}"
                )),
                ..DoctorOptions::default()
            },
            &FakeRunner::default().with_command(ABSOLUTE_HTJ2K_DECODER),
        );

        assert!(report.tools.iter().any(|tool| {
            tool.name == "htj2k_decoder"
                && tool.status == DoctorStatus::Available
                && tool
                    .command
                    .first()
                    .is_some_and(|command| command == ABSOLUTE_HTJ2K_DECODER)
        }));
    }

    #[test]
    fn staged_dicom3tools_probe_requires_debug_build_or_explicit_env() {
        assert!(!super::staged_dicom3tools_probe_enabled_from(false, false));
        assert!(super::staged_dicom3tools_probe_enabled_from(true, false));
        assert!(super::staged_dicom3tools_probe_enabled_from(false, true));
    }

    fn write_encapsulated_dicom(path: &Path, transfer_syntax: &str, frame: &[u8]) {
        let mut object = InMemDicomObject::new_empty();
        object.put(DataElement::<InMemDicomObject>::new(
            tags::SOP_CLASS_UID,
            VR::UI,
            PrimitiveValue::from("1.2.840.10008.5.1.4.1.1.77.1.6"),
        ));
        object.put(DataElement::<InMemDicomObject>::new(
            tags::SOP_INSTANCE_UID,
            VR::UI,
            PrimitiveValue::from("1.2.826.0.1.3680043.10.999.200"),
        ));
        object.put(DataElement::<InMemDicomObject>::new(
            tags::ROWS,
            VR::US,
            PrimitiveValue::from(1u16),
        ));
        object.put(DataElement::<InMemDicomObject>::new(
            tags::COLUMNS,
            VR::US,
            PrimitiveValue::from(1u16),
        ));
        object.put(DataElement::<InMemDicomObject>::new(
            tags::NUMBER_OF_FRAMES,
            VR::IS,
            PrimitiveValue::from("1"),
        ));
        let meta = FileMetaTableBuilder::new()
            .media_storage_sop_class_uid("1.2.840.10008.5.1.4.1.1.77.1.6")
            .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.999.200")
            .transfer_syntax(transfer_syntax);
        let file = File::create(path).expect("create DICOM");
        let mut output = BufWriter::new(file);
        object
            .with_meta(meta)
            .expect("file meta")
            .write_all(&mut output)
            .expect("write DICOM object");
        crate::writer::write_encapsulated_pixel_data_from_frames(
            &mut output,
            &[u64::try_from(frame.len()).expect("frame len")],
            |_, output| output.write_all(frame),
        )
        .expect("write pixel data");
        output.flush().expect("flush DICOM");
    }
}
