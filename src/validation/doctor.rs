use super::*;

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
                probe_stdout: outcome.stdout.clone(),
                probe_stderr: outcome.stderr.clone(),
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
                    probe_stdout: outcome.stdout.clone(),
                    probe_stderr: outcome.stderr.clone(),
                    name: tool.name.to_string(),
                    required: tool.required,
                    status: DoctorStatus::Failed,
                    command,
                    path: Some(path),
                    message,
                }
            }
            Err(source) => DoctorTool {
                probe_stdout: String::new(),
                probe_stderr: String::new(),
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
                probe_stdout: String::new(),
                probe_stderr: String::new(),
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
    let configured = options.htj2k_decoder.is_some();
    let template = match options
        .htj2k_decoder
        .clone()
        .or_else(|| auto_htj2k_decoder_template(runner))
    {
        Some(template) => template,
        None => {
            let status = if options.strict {
                DoctorStatus::Failed
            } else {
                DoctorStatus::Skipped
            };
            return DoctorTool {
                probe_stdout: String::new(),
                probe_stderr: String::new(),
                name: "htj2k_decoder".to_string(),
                required: options.strict,
                status,
                command: Vec::new(),
                path: None,
                message: "HTJ2K decoder command is not configured and grk_decompress was not found"
                    .to_string(),
            };
        }
    };
    let (name, args) =
        match htj2k_decoder_command(&template, Path::new("input.jhc"), Path::new("output.ppm")) {
            Ok(command) => command,
            Err(message) => {
                return DoctorTool {
                    probe_stdout: String::new(),
                    probe_stderr: String::new(),
                    name: "htj2k_decoder".to_string(),
                    required: options.strict || configured,
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
            probe_stdout: String::new(),
            probe_stderr: String::new(),
            name: "htj2k_decoder".to_string(),
            required: options.strict || configured,
            status: DoctorStatus::Available,
            command,
            path: Some(path),
            message: if configured {
                format!("{name} found")
            } else {
                format!("{name} auto-detected")
            },
        },
        None => DoctorTool {
            probe_stdout: String::new(),
            probe_stderr: String::new(),
            name: "htj2k_decoder".to_string(),
            required: options.strict || configured,
            status: DoctorStatus::Failed,
            command,
            path: None,
            message: format!("{name} not found"),
        },
    }
}

fn skipped_doctor_tool(name: &str, required: bool, message: String) -> DoctorTool {
    DoctorTool {
        probe_stdout: String::new(),
        probe_stderr: String::new(),
        name: name.to_string(),
        required,
        status: DoctorStatus::Skipped,
        command: Vec::new(),
        path: None,
        message,
    }
}
