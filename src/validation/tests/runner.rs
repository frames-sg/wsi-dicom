use super::*;

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

#[cfg(unix)]
#[test]
fn system_runner_timeout_terminates_descendants_and_returns_promptly() {
    let runner = SystemCommandRunner;
    let started = std::time::Instant::now();
    let outcome = runner
        .run(
            Path::new("/bin/sh"),
            &[OsString::from("-c"), OsString::from("sleep 30 & wait")],
            Duration::from_millis(100),
            1024,
        )
        .unwrap();

    assert!(outcome.timed_out);
    assert!(outcome.elapsed_millis >= 100);
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[cfg(unix)]
#[test]
fn external_execution_failure_is_distinct_from_a_defect() {
    let check = run_named_command_check(
        &SystemCommandRunner,
        CommandCheckRequest {
            check_name: "test-validator",
            command_name: "/bin/sh",
            args: vec![OsString::from("-c"), OsString::from("sleep 5")],
            path: None,
            required: true,
            error_line_is_failure: false,
            timeout: Duration::from_millis(50),
            max_output_bytes: 1024,
        },
    );
    assert_eq!(check.status, ValidationStatus::Failed);
    let evidence = check.execution.unwrap();
    assert_eq!(
        evidence.failure,
        Some(super::super::ExecutionFailure::Timeout)
    );
    assert!(evidence.elapsed_millis >= 50);
}

#[cfg(unix)]
#[test]
fn system_runner_timeout_still_applies_after_the_process_leader_exits() {
    let runner = SystemCommandRunner;
    let started = std::time::Instant::now();
    let outcome = runner
        .run(
            Path::new("/bin/sh"),
            &[OsString::from("-c"), OsString::from("sleep 30 & exit 0")],
            Duration::from_millis(100),
            1024,
        )
        .unwrap();

    assert!(outcome.timed_out);
    assert!(started.elapsed() < Duration::from_secs(3));
}
