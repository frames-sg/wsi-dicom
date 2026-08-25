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

    let direct = tmp.path().join("one.dcm");
    let err = validate_dicom_path_with_runner(
        &direct,
        &ValidationOptions {
            max_pixel_frames: 0,
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
    write_primitive_pixel_dicom(&file, TransferSyntax::ExplicitVrLittleEndian.uid(), &[1]);

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
    write_primitive_pixel_dicom(&file, TransferSyntax::ExplicitVrLittleEndian.uid(), &[1]);

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
    assert!(report
        .checks
        .iter()
        .any(|check| check.name == "validate_iods" && check.status == ValidationStatus::Skipped));
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

    assert!(report
        .checks
        .iter()
        .any(|check| { check.name == "pixel-djpeg" && check.status == ValidationStatus::Passed }));
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
fn htj2k_pixel_decode_uses_auto_grok_decoder_when_available() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("htj2k.dcm");
    write_encapsulated_dicom(&file, "1.2.840.10008.1.2.4.202", &[0xFF, 0x4F, 0xFF, 0x51]);

    let report = validate_dicom_path_with_runner(
        &file,
        &ValidationOptions::default(),
        &FakeRunner::default()
            .with_command_path(super::AUTO_HTJ2K_DECODER_COMMAND, ABSOLUTE_GROK_DECODER),
    )
    .expect("validation report");

    assert!(report.checks.iter().any(|check| {
        check.name == "pixel-htj2k"
            && check.status == ValidationStatus::Passed
            && check
                .command
                .first()
                .is_some_and(|command| command == ABSOLUTE_GROK_DECODER)
    }));
}

#[test]
fn htj2k_pixel_decode_skips_without_configured_or_auto_decoder() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("htj2k.dcm");
    write_encapsulated_dicom(&file, "1.2.840.10008.1.2.4.202", &[0xFF, 0x4F, 0xFF, 0x51]);

    let report = validate_dicom_path_with_runner(
        &file,
        &ValidationOptions::default(),
        &FakeRunner::default(),
    )
    .expect("validation report");

    assert!(report
        .checks
        .iter()
        .any(|check| { check.name == "pixel-htj2k" && check.status == ValidationStatus::Skipped }));
}

#[test]
fn strict_htj2k_pixel_decode_fails_without_configured_or_auto_decoder() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("htj2k.dcm");
    write_encapsulated_dicom(&file, "1.2.840.10008.1.2.4.202", &[0xFF, 0x4F, 0xFF, 0x51]);

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
        .any(|check| { check.name == "pixel-htj2k" && check.status == ValidationStatus::Failed }));
    assert!(report.has_failures());
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

    assert!(report
        .checks
        .iter()
        .any(|check| { check.name == "pixel-djpeg" && check.status == ValidationStatus::Failed }));
}

#[test]
fn zero_pixel_frame_limit_disables_optional_decode_but_keeps_intrinsic_checks() {
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

    assert!(report.checks.iter().any(|check| {
        check.name == "intrinsic-pixel-structure" && check.status == ValidationStatus::Passed
    }));
    assert!(!report
        .checks
        .iter()
        .any(|check| check.name == "pixel-djpeg"));
}
