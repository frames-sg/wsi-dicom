use super::*;

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
                stdout: "This is the opj_decompress utility from the OpenJPEG project.".to_string(),
                stderr: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
            },
        );

    let report = doctor_dicom_environment_with_runner(&DoctorOptions::default(), &runner);

    assert!(report
        .tools
        .iter()
        .any(|tool| { tool.name == "opj_decompress" && tool.status == DoctorStatus::Available }));
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
fn doctor_auto_detects_grok_for_htj2k_decoder() {
    let report = doctor_dicom_environment_with_runner(
        &DoctorOptions::default(),
        &FakeRunner::default()
            .with_command_path(super::AUTO_HTJ2K_DECODER_COMMAND, ABSOLUTE_GROK_DECODER),
    );

    assert!(report.tools.iter().any(|tool| {
        tool.name == "htj2k_decoder"
            && tool.status == DoctorStatus::Available
            && !tool.required
            && tool.path.as_deref() == Some(Path::new(ABSOLUTE_GROK_DECODER))
            && tool.message.contains("auto-detected")
    }));
}

#[test]
fn doctor_strict_fails_when_htj2k_decoder_is_not_configured_or_auto_detected() {
    let report = doctor_dicom_environment_with_runner(
        &DoctorOptions {
            strict: true,
            ..DoctorOptions::default()
        },
        &FakeRunner::default(),
    );

    assert!(report.tools.iter().any(|tool| {
        tool.name == "htj2k_decoder" && tool.required && tool.status == DoctorStatus::Failed
    }));
    assert!(report.has_failures());
}

#[test]
fn staged_dicom3tools_probe_requires_debug_build_or_explicit_env() {
    assert!(!super::staged_dicom3tools_probe_enabled_from(false, false));
    assert!(super::staged_dicom3tools_probe_enabled_from(true, false));
    assert!(super::staged_dicom3tools_probe_enabled_from(false, true));
}
