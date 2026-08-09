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

pub(super) fn format_timeout(timeout: Duration) -> String {
    if timeout.as_millis() > 0 && timeout.as_millis() < 1000 {
        format!("{}ms", timeout.as_millis())
    } else if timeout.subsec_millis() == 0 {
        format!("{}s", timeout.as_secs())
    } else {
        format!("{}ms", timeout.as_millis())
    }
}
