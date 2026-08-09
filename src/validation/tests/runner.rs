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
    assert!(started.elapsed() < Duration::from_secs(3));
}
