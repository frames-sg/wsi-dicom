use super::*;

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

#[test]
fn encapsulated_frame_assembly_uses_offsets_and_extended_lengths() {
    let sequence = dicom_core::value::PixelFragmentSequence::new_fragments(vec![
        vec![1, 2],
        vec![3, 0],
        vec![4, 5],
    ]);
    let frames =
        super::assemble_encapsulated_frames(&sequence, 2, Some(&[0, 20]), Some(&[3, 2]), 2, 64)
            .unwrap();

    assert_eq!(frames, vec![vec![1, 2, 3], vec![4, 5]]);
}

#[test]
fn encapsulated_frame_assembly_rejects_ambiguous_fragment_mapping() {
    let sequence =
        dicom_core::value::PixelFragmentSequence::new_fragments(vec![vec![1], vec![2], vec![3]]);
    let error = super::assemble_encapsulated_frames(&sequence, 2, None, None, 2, 64)
        .expect_err("multiple frames without offsets must be unambiguous");
    assert!(error.contains("without an offset table"));
}

#[test]
fn decoded_output_must_exist_and_match_dicom_geometry() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("frame.ppm");
    let expected = super::DecodedFrameExpectation {
        columns: 2,
        rows: 1,
        samples_per_pixel: Some(3),
        bits_allocated: Some(8),
    };

    let missing =
        super::inspect_pnm_output(&output, expected, 1024).expect_err("missing output must fail");
    assert!(missing.contains("did not create readable output"));

    std::fs::write(&output, b"P6\n2 1\n255\n\x01\x02\x03\x04\x05\x06").unwrap();
    super::inspect_pnm_output(&output, expected, 1024).unwrap();

    std::fs::write(&output, b"P6\n1 1\n255\n\x01\x02\x03").unwrap();
    let wrong_geometry =
        super::inspect_pnm_output(&output, expected, 1024).expect_err("wrong dimensions must fail");
    assert!(wrong_geometry.contains("do not match DICOM"));
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
        max_pixel_frame_bytes: 512 * 1024 * 1024,
    };

    let json = serde_json::to_string(&options).expect("serialize validation options");
    assert!(json.contains("\"command_timeout_secs\":12"));

    let options: ValidationOptions =
        serde_json::from_str(&json).expect("deserialize validation options");
    assert!(options.strict);
    assert_eq!(options.command_timeout(), Duration::from_secs(12));
    assert_eq!(options.max_pixel_frames, 3);
}
