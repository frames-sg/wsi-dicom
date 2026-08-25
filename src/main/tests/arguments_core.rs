use crate::cli_args::{resolve_export_transfer_syntax, Cli, Command, SelfTestArgs};
use crate::cli_output::cli_output_line;
use crate::cli_profile::effective_max_frames_per_level;
use clap::error::ErrorKind;
use clap::Parser;
use std::path::PathBuf;
use wsi_dicom::{ExportPreset, JpegDirectHtj2kProfile, MetadataInput, TransferSyntax};

#[derive(serde::Serialize)]
struct SampleCliOutput {
    value: u8,
}

#[test]
fn cli_version_flag_matches_the_package_version() {
    let error = Cli::try_parse_from(["wsi-dicom", "--version"]).unwrap_err();

    assert_eq!(error.kind(), ErrorKind::DisplayVersion);
    assert_eq!(
        error.to_string(),
        format!("wsi-dicom {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn cli_output_line_formats_summary_or_json() {
    let sample = SampleCliOutput { value: 7 };

    let summary = cli_output_line(false, &sample, |_| "summary".to_string()).unwrap();
    assert_eq!(summary, "summary");

    let json = cli_output_line(true, &sample, |_| "summary".to_string()).unwrap();
    assert_eq!(json, r#"{"value":7}"#);
}

#[test]
fn cli_requires_metadata_or_explicit_research_placeholder() {
    let err = MetadataInput::from_parts(None, false).unwrap_err();
    assert!(err.to_string().contains("metadata"));

    let metadata = MetadataInput::from_parts(None, true).unwrap();

    assert!(matches!(metadata, MetadataInput::ResearchPlaceholder));
}

#[test]
fn cli_coverage_full_frame_coverage_overrides_bounded_frame_count() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "coverage",
        "source.svs",
        "--max-frames-per-level",
        "1",
        "--full-frame-coverage",
    ])
    .unwrap();

    let Command::Coverage {
        max_frames_per_level,
        full_frame_coverage,
        ..
    } = cli.command
    else {
        panic!("expected coverage command");
    };

    assert_eq!(
        effective_max_frames_per_level(max_frames_per_level, full_frame_coverage),
        u64::MAX
    );
}

#[test]
fn cli_coverage_accepts_max_level_elapsed_limit_ms() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "coverage",
        "source.svs",
        "--max-level-ms",
        "250",
    ])
    .unwrap();

    let Command::Coverage { max_level_ms, .. } = cli.command else {
        panic!("expected coverage command");
    };

    assert_eq!(max_level_ms, Some(250));
}

#[test]
fn cli_coverage_accepts_source_device_decode_opt_in() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "coverage",
        "source.svs",
        "--source-device-decode",
    ])
    .unwrap();

    let Command::Coverage { encode, .. } = cli.command else {
        panic!("expected coverage command");
    };

    assert!(encode.source_device_decode);
}

#[test]
fn cli_convert_accepts_source_device_decode_opt_in() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "convert",
        "source.svs",
        "--out",
        "out",
        "--source-device-decode",
    ])
    .unwrap();

    let Command::Convert { export, .. } = cli.command else {
        panic!("expected convert command");
    };

    assert!(export.encode.source_device_decode);
}

#[test]
fn cli_convert_defers_transfer_syntax_when_omitted() {
    let cli = Cli::try_parse_from(["wsi-dicom", "convert", "source.svs", "--out", "out"]).unwrap();

    let Command::Convert { export, .. } = cli.command else {
        panic!("expected convert command");
    };

    assert_eq!(export.preset, ExportPreset::LosslessReview);
    assert_eq!(export.encode.transfer_syntax, None);
}

#[test]
fn cli_convert_accepts_fast_jpeg_preset() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "convert",
        "source.svs",
        "--out",
        "out",
        "--preset",
        "fast-jpeg",
    ])
    .unwrap();

    let Command::Convert { export, .. } = cli.command else {
        panic!("expected convert command");
    };

    assert_eq!(export.preset, ExportPreset::FastJpeg);
}

#[test]
fn cli_convert_rejects_metadata_and_research_placeholder_conflict() {
    let err = Cli::try_parse_from([
        "wsi-dicom",
        "convert",
        "source.svs",
        "--out",
        "out",
        "--metadata",
        "metadata.json",
        "--research-placeholder",
    ])
    .unwrap_err();

    assert!(err.to_string().contains("cannot be used with"));
}

#[test]
fn cli_sustain_convert_accepts_fast_jpeg_preset() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "sustain-convert",
        "source.svs",
        "--out",
        "out",
        "--preset",
        "fast-jpeg",
    ])
    .unwrap();

    let Command::SustainConvert { export, .. } = cli.command else {
        panic!("expected sustain-convert command");
    };

    assert_eq!(export.preset, ExportPreset::FastJpeg);
}

#[test]
fn fast_jpeg_preset_rejects_explicit_transfer_syntax() {
    let err = resolve_export_transfer_syntax(
        ExportPreset::FastJpeg,
        Some(TransferSyntax::Htj2kLosslessRpcl),
    )
    .unwrap_err();

    assert!(err
        .to_string()
        .contains("--preset fast-jpeg cannot be combined with --transfer-syntax"));
}

#[test]
fn export_preset_resolves_default_lossless_and_fast_jpeg() {
    assert_eq!(
        resolve_export_transfer_syntax(ExportPreset::LosslessReview, None).unwrap(),
        TransferSyntax::Htj2kLosslessRpcl
    );
    assert_eq!(
        resolve_export_transfer_syntax(ExportPreset::FastJpeg, None).unwrap(),
        TransferSyntax::JpegBaseline8Bit
    );
}

#[test]
fn cli_convert_defaults_tile_size_to_512() {
    let cli = Cli::try_parse_from(["wsi-dicom", "convert", "source.svs", "--out", "out"]).unwrap();

    let Command::Convert { export, .. } = cli.command else {
        panic!("expected convert command");
    };

    assert_eq!(export.encode.tile_size, 512);
    assert_eq!(export.max_instance_metadata_mib, 256);
    assert_eq!(export.max_total_metadata_mib, 1024);
}

#[test]
fn cli_convert_metadata_budgets_are_checked_and_converted_to_bytes() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "convert",
        "source.svs",
        "--out",
        "out",
        "--max-instance-metadata-mib",
        "64",
        "--max-total-metadata-mib",
        "512",
    ])
    .unwrap();

    let Command::Convert { export, .. } = cli.command else {
        panic!("expected convert command");
    };
    let options = export.options().unwrap();
    assert_eq!(options.max_instance_metadata_bytes, 64 * 1024 * 1024);
    assert_eq!(options.max_total_metadata_bytes, 512 * 1024 * 1024);

    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "convert",
        "source.svs",
        "--out",
        "out",
        "--max-instance-metadata-mib",
        &u64::MAX.to_string(),
    ])
    .unwrap();
    let Command::Convert { export, .. } = cli.command else {
        panic!("expected convert command");
    };
    assert!(export.options().is_err());
}

#[test]
fn cli_convert_preserves_explicit_htj2k_lossless_rpcl() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "convert",
        "source.svs",
        "--out",
        "out",
        "--transfer-syntax",
        "htj2k-lossless-rpcl",
    ])
    .unwrap();

    let Command::Convert { export, .. } = cli.command else {
        panic!("expected convert command");
    };

    assert_eq!(
        export.encode.transfer_syntax,
        Some(TransferSyntax::Htj2kLosslessRpcl)
    );
}

#[test]
fn cli_convert_accepts_explicit_htj2k_97_profile() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "convert",
        "source.svs",
        "--out",
        "out",
        "--transfer-syntax",
        "htj2k",
        "--jpeg-direct-htj2k-profile",
        "97",
    ])
    .unwrap();

    let Command::Convert { export, .. } = cli.command else {
        panic!("expected convert command");
    };

    assert_eq!(export.encode.transfer_syntax, Some(TransferSyntax::Htj2k));
    assert_eq!(
        export.encode.jpeg_direct_htj2k_profile,
        Some(JpegDirectHtj2kProfile::Lossy97)
    );
}

#[test]
fn cli_validate_accepts_validation_options() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "validate",
        "dicom-out",
        "--strict",
        "--json",
        "--dcmvalidate-iod",
        "vl-wsi.xml",
        "--htj2k-decoder",
        "ojph_expand -i {input} -o {output}",
        "--max-pixel-frames",
        "3",
        "--command-timeout-secs",
        "12",
    ])
    .unwrap();

    let Command::Validate {
        path,
        strict,
        json,
        dcmvalidate_iod,
        htj2k_decoder,
        max_pixel_frames,
        command_timeout_secs,
    } = cli.command
    else {
        panic!("expected validate command");
    };

    assert_eq!(path, PathBuf::from("dicom-out"));
    assert!(strict);
    assert!(json);
    assert_eq!(dcmvalidate_iod, Some(PathBuf::from("vl-wsi.xml")));
    assert_eq!(
        htj2k_decoder.as_deref(),
        Some("ojph_expand -i {input} -o {output}")
    );
    assert_eq!(max_pixel_frames, 3);
    assert_eq!(command_timeout_secs, 12);
}

#[test]
fn cli_validate_defaults_to_one_pixel_frame_smoke() {
    let cli = Cli::try_parse_from(["wsi-dicom", "validate", "dicom-out"]).unwrap();

    let Command::Validate {
        strict,
        json,
        dcmvalidate_iod,
        htj2k_decoder,
        max_pixel_frames,
        command_timeout_secs,
        ..
    } = cli.command
    else {
        panic!("expected validate command");
    };

    assert!(!strict);
    assert!(!json);
    assert_eq!(dcmvalidate_iod, None);
    assert_eq!(htj2k_decoder, None);
    assert_eq!(max_pixel_frames, 1);
    assert_eq!(command_timeout_secs, 60);
}

#[test]
fn cli_doctor_accepts_reviewer_tooling_options() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "doctor",
        "--strict",
        "--json",
        "--dcmvalidate-iod",
        "vl-wsi.xml",
        "--htj2k-decoder",
        "ojph_expand -i {input} -o {output}",
    ])
    .unwrap();

    let Command::Doctor {
        strict,
        json,
        dcmvalidate_iod,
        htj2k_decoder,
    } = cli.command
    else {
        panic!("expected doctor command");
    };

    assert!(strict);
    assert!(json);
    assert_eq!(dcmvalidate_iod, Some(PathBuf::from("vl-wsi.xml")));
    assert_eq!(
        htj2k_decoder.as_deref(),
        Some("ojph_expand -i {input} -o {output}")
    );
}

#[test]
fn cli_self_test_accepts_reviewer_evidence_options() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "self-test",
        "--strict",
        "--json",
        "--out",
        "evidence",
        "--keep-output",
        "--dcmvalidate-iod",
        "vl-wsi.xml",
        "--htj2k-decoder",
        "ojph_expand -i {input} -o {output}",
        "--command-timeout-secs",
        "12",
    ])
    .unwrap();

    let Command::SelfTest(SelfTestArgs {
        strict,
        json,
        out,
        keep_output,
        dcmvalidate_iod,
        htj2k_decoder,
        command_timeout_secs,
    }) = cli.command
    else {
        panic!("expected self-test command");
    };

    assert!(strict);
    assert!(json);
    assert_eq!(out, Some(PathBuf::from("evidence")));
    assert!(keep_output);
    assert_eq!(dcmvalidate_iod, Some(PathBuf::from("vl-wsi.xml")));
    assert_eq!(
        htj2k_decoder.as_deref(),
        Some("ojph_expand -i {input} -o {output}")
    );
    assert_eq!(command_timeout_secs, 12);
}
