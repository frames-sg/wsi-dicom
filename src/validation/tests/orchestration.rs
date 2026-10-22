use super::*;

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
        &ValidationOptions::default(),
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
            max_depth: 0,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("max_depth"));

    let direct = tmp.path().join("one.dcm");
    let err = validate_dicom_path_with_runner(
        &direct,
        &ValidationOptions {
            max_files: 0,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("max_files"));
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
        &ValidationOptions::default(),
        &FakeRunner::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("symlink"));
}

#[test]
fn missing_tools_are_skipped_by_default() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("one.dcm");
    write_primitive_pixel_dicom(&file, TransferSyntax::ExplicitVrLittleEndian.uid(), &[1]);

    let report = validate_dicom_path_with_runner(
        &file,
        &ValidationOptions::default(),
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
    write_primitive_pixel_dicom(&file, TransferSyntax::ExplicitVrLittleEndian.uid(), &[1]);

    let report = validate_dicom_path_with_runner(
        &file,
        &ValidationOptions {
            strict: true,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .expect("validation report");

    assert!(report
        .checks
        .iter()
        .any(|check| check.name == "dciodvfy" && check.status == ValidationStatus::Failed));
    assert!(report
        .checks
        .iter()
        .any(|check| check.name == "validate_iods" && check.status == ValidationStatus::Failed));
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
        &ValidationOptions::default(),
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
            return_code: Some(1),
            elapsed_millis: 0,
            success: false,
            timed_out: false,
            stdout: String::new(),
            stderr: "set check failed".to_string(),
            stdout_truncated: false,
            stderr_truncated: false,
        },
    );

    let report =
        validate_dicom_path_with_runner(tmp.path(), &ValidationOptions::default(), &runner)
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
fn validation_never_runs_external_pixel_decoders() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let decoders = [
        "djpeg",
        "opj_decompress",
        "grk_decompress",
        "kdu_expand",
        "ojph_expand",
    ];
    let mut runner = FakeRunner::default();
    for decoder in decoders {
        runner = runner.with_command(decoder);
    }
    for (name, transfer_syntax) in [
        ("jpeg.dcm", "1.2.840.10008.1.2.4.50"),
        ("jpeg-lossless.dcm", "1.2.840.10008.1.2.4.70"),
        ("j2k.dcm", "1.2.840.10008.1.2.4.90"),
        ("htj2k.dcm", "1.2.840.10008.1.2.4.202"),
    ] {
        let file = tmp.path().join(name);
        write_encapsulated_dicom(&file, transfer_syntax, &[0xFF, 0xD8, 0xFF, 0xD9]);

        let report = validate_dicom_path_with_runner(
            &file,
            &ValidationOptions {
                strict: true,
                ..ValidationOptions::default()
            },
            &runner,
        )
        .expect("validation report");

        assert!(report.checks.iter().any(|check| {
            check.name == "intrinsic-pixel-structure" && check.status == ValidationStatus::Passed
        }));
        assert!(
            report
                .checks
                .iter()
                .all(|check| !check.name.starts_with("pixel-")
                    && !check
                        .command
                        .iter()
                        .any(|arg| decoders.contains(&arg.as_str()))),
            "{transfer_syntax}: validation ran an external decoder"
        );
    }
}

#[test]
fn iod_validation_records_an_explicit_standard_edition() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_for_validation(tmp.path(), "edition");
    let report = validate_dicom_path_with_runner(
        &path,
        &ValidationOptions {
            ..Default::default()
        },
        &FakeRunner::default().with_command("validate_iods"),
    )
    .unwrap();
    let check = report
        .checks
        .iter()
        .find(|c| c.name == "validate_iods")
        .unwrap();
    assert!(check
        .command
        .windows(2)
        .any(|pair| pair == ["--edition", "2026c"]));
}

#[test]
fn command_timeout_is_reported_as_failed_check() {
    let runner = FakeRunner::default().with_command("dciodvfy").with_outcome(
        "dciodvfy -new one.dcm",
        CommandOutcome {
            return_code: Some(1),
            elapsed_millis: 0,
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
            return_code: Some(0),
            elapsed_millis: 0,
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
        profile: super::ValidationProfile::General,
        max_input_bytes: 1024 * 1024 * 1024,
        strict: true,
        dcmvalidate_iod: Some(PathBuf::from("iod.xml")),
        command_timeout_secs: 12,
        max_files: 100_000,
        max_depth: 64,
        max_child_output_bytes: 4 * 1024 * 1024,
        max_pixel_frame_bytes: 512 * 1024 * 1024,
    };

    let json = serde_json::to_string(&options).expect("serialize validation options");
    assert!(json.contains("\"command_timeout_secs\":12"));

    let options: ValidationOptions =
        serde_json::from_str(&json).expect("deserialize validation options");
    assert!(options.strict);
    assert_eq!(options.command_timeout(), Duration::from_secs(12));
}

#[test]
fn validation_options_from_before_decoder_removal_still_deserialize() {
    let options: ValidationOptions = serde_json::from_str(
        r#"{"strict":true,"htj2k_decoder":"grk_decompress -i {input} -o {output}","max_pixel_frames":3}"#,
    )
    .expect("removed external decoder fields are ignored");

    assert!(options.strict);
    assert_eq!(
        options,
        ValidationOptions {
            strict: true,
            ..ValidationOptions::default()
        }
    );
}
