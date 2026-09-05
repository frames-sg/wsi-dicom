use super::*;

pub(super) struct CommandCheckRequest<'a> {
    pub(super) check_name: &'a str,
    pub(super) command_name: &'a str,
    pub(super) args: Vec<OsString>,
    pub(super) path: Option<&'a PathBuf>,
    pub(super) required: bool,
    pub(super) error_line_is_failure: bool,
    pub(super) timeout: Duration,
    pub(super) max_output_bytes: usize,
}

pub(super) struct SetLevelCommandCheckRequest<'a> {
    pub(super) prefix_args: Vec<OsString>,
    pub(super) check_name: &'a str,
    pub(super) command_name: &'a str,
    pub(super) required: bool,
    pub(super) error_line_is_failure: bool,
    pub(super) timeout: Duration,
    pub(super) max_output_bytes: usize,
    pub(super) chunk_size: usize,
}

pub(super) fn run_set_level_command_checks(
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
                    args: request
                        .prefix_args
                        .iter()
                        .cloned()
                        .chain(chunk.iter().map(|file| file.as_os_str().to_os_string()))
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

pub(super) fn run_named_command_check(
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
    let mut check = ValidationCheck {
        name: check_name.to_string(),
        path: path.cloned(),
        status: ValidationStatus::Failed,
        command,
        message: String::new(),
        stdout: String::new(),
        stderr: String::new(),
        execution: Some(ValidationExecution {
            return_code: None,
            elapsed_millis: 0,
            failure: None,
        }),
    };
    let execution = check
        .execution
        .as_mut()
        .expect("external check retains process facts");
    let Some(program) = runner.find_command(command_name) else {
        check.status = if required {
            ValidationStatus::Failed
        } else {
            ValidationStatus::Skipped
        };
        check.message = format!("{command_name} not found");
        execution.failure = Some(ExecutionFailure::Unavailable);
        return check;
    };
    match runner.run(&program, &args, timeout, max_output_bytes) {
        Ok(outcome) => {
            execution.return_code = outcome.return_code;
            execution.elapsed_millis = outcome.elapsed_millis;
            execution.failure = if outcome.timed_out {
                Some(ExecutionFailure::Timeout)
            } else if outcome.stdout_truncated || outcome.stderr_truncated {
                Some(ExecutionFailure::OutputLimit)
            } else if !outcome.success && outcome.return_code.is_none() {
                Some(ExecutionFailure::Terminated)
            } else {
                None
            };
            let output_has_error = error_line_is_failure
                && outcome
                    .stdout
                    .lines()
                    .chain(outcome.stderr.lines())
                    .any(|line| line.trim_start().starts_with("Error"));
            if execution.failure.is_none() && outcome.success && !output_has_error {
                check.status = ValidationStatus::Passed;
            }
            check.message = match execution.failure {
                Some(ExecutionFailure::Timeout) => {
                    format!("{command_name} timed out after {}", format_timeout(timeout))
                }
                Some(ExecutionFailure::OutputLimit) => {
                    format!("{command_name} output exceeded {max_output_bytes} byte capture limit")
                }
                Some(_) => format!("{command_name} did not complete"),
                None if check.status == ValidationStatus::Passed => {
                    format!("{command_name} passed")
                }
                None => format!("{command_name} failed"),
            };
            check.stdout = outcome.stdout;
            check.stderr = outcome.stderr;
        }
        Err(source) => {
            execution.failure = Some(ExecutionFailure::Launch);
            check.message = format!("failed to start {command_name}: {source}");
        }
    }
    check
}

pub(super) fn format_timeout(timeout: Duration) -> String {
    if timeout.as_millis() > 0 && timeout.as_millis() < 1000 {
        format!("{}ms", timeout.as_millis())
    } else if timeout.subsec_millis() == 0 {
        format!("{}s", timeout.as_secs())
    } else {
        format!("{}ms", timeout.as_millis())
    }
}
