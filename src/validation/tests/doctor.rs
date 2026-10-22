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
    assert!(report.tools.iter().any(|tool| {
        tool.name == "validate_iods" && tool.required && tool.status == DoctorStatus::Failed
    }));
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
            return_code: Some(1),
            elapsed_millis: 0,
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
fn doctor_probes_no_codec_tools() {
    let report =
        doctor_dicom_environment_with_runner(&DoctorOptions::default(), &FakeRunner::default());

    let names = report
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["dciodvfy", "dcentvfy", "validate_iods", "dcmvalidate"]
    );
}

#[test]
fn staged_dicom3tools_probe_requires_debug_build_or_explicit_env() {
    assert!(!super::staged_dicom3tools_probe_enabled_from(false, false));
    assert!(super::staged_dicom3tools_probe_enabled_from(true, false));
    assert!(super::staged_dicom3tools_probe_enabled_from(false, true));
}
