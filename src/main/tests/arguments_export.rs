use crate::cli_args::{Cli, Command};
use crate::cli_calibration::CalibrationCommand;
use clap::Parser;
use wsi_dicom::{
    AnnotationCoordinateSpace, AnnotationTarget, ColorManagement, JpegDirectHtj2kProfile, UidPolicy,
};

#[test]
fn cli_convert_defaults_icc_to_source_or_srgb() {
    let cli = Cli::try_parse_from(["wsi-dicom", "convert", "source.svs", "--out", "out"]).unwrap();

    let Command::Convert { export, .. } = cli.command else {
        panic!("expected convert command");
    };

    assert_eq!(
        export.color_management.resolve().unwrap(),
        ColorManagement::SourceOrSrgb
    );
}

#[test]
fn cli_convert_accepts_icc_policy() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "convert",
        "source.svs",
        "--out",
        "out",
        "--icc",
        "source-or-display-p3",
    ])
    .unwrap();

    let Command::Convert { export, .. } = cli.command else {
        panic!("expected convert command");
    };

    assert_eq!(
        export.color_management.resolve().unwrap(),
        ColorManagement::SourceOrDisplayP3
    );
}

#[test]
fn cli_convert_defaults_to_fresh_uids_and_accepts_deterministic_policy() {
    let default =
        Cli::try_parse_from(["wsi-dicom", "convert", "source.svs", "--out", "out"]).unwrap();
    let Command::Convert { export, .. } = default.command else {
        panic!("expected convert command");
    };
    assert_eq!(export.uid_policy, UidPolicy::Fresh);

    let deterministic = Cli::try_parse_from([
        "wsi-dicom",
        "convert",
        "source.svs",
        "--out",
        "out",
        "--uid-policy",
        "deterministic",
    ])
    .unwrap();
    let Command::Convert { export, .. } = deterministic.command else {
        panic!("expected convert command");
    };
    assert_eq!(export.uid_policy, UidPolicy::Deterministic);
}

#[test]
fn cli_sustain_convert_accepts_icc_policy() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "sustain-convert",
        "source.svs",
        "--out",
        "out",
        "--icc",
        "require-source",
    ])
    .unwrap();

    let Command::SustainConvert { export, .. } = cli.command else {
        panic!("expected sustain-convert command");
    };

    assert_eq!(
        export.color_management.resolve().unwrap(),
        ColorManagement::RequireSource
    );
}

#[test]
fn cli_rejects_incompatible_or_incomplete_configured_icc_arguments() {
    assert!(Cli::try_parse_from([
        "wsi-dicom",
        "convert",
        "source.svs",
        "--out",
        "out",
        "--icc-calibration-registry",
        "registry.json",
        "--icc-profile",
        "profile.icc",
        "--icc-profile-id",
        "profile-1",
    ])
    .is_err());
    assert!(Cli::try_parse_from([
        "wsi-dicom",
        "convert",
        "source.svs",
        "--out",
        "out",
        "--icc-profile",
        "profile.icc",
    ])
    .is_err());

    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "convert",
        "source.svs",
        "--out",
        "out",
        "--icc-conflict",
        "prefer-source",
    ])
    .unwrap();
    let Command::Convert { export, .. } = cli.command else {
        panic!("expected convert command");
    };
    assert!(export
        .color_management
        .resolve()
        .unwrap_err()
        .to_string()
        .contains("applies only"));
}

#[test]
fn cli_parses_calibration_inspect_and_create_interfaces() {
    let inspect = Cli::try_parse_from([
        "wsi-dicom",
        "calibration",
        "inspect",
        "--icc",
        "profile.icc",
    ])
    .unwrap();
    assert!(matches!(
        inspect.command,
        Command::Calibration {
            command: CalibrationCommand::Inspect { .. }
        }
    ));

    let create = Cli::try_parse_from([
        "wsi-dicom",
        "calibration",
        "create",
        "--icc",
        "profile.icc",
        "--id",
        "cal-1",
        "--manufacturer",
        "Vendor",
        "--model",
        "Model",
        "--serial",
        "Serial",
        "--out",
        "bundle",
    ])
    .unwrap();
    assert!(matches!(
        create.command,
        Command::Calibration {
            command: CalibrationCommand::Create { .. }
        }
    ));
}

#[test]
fn cli_convert_accepts_named_htj2k_97_quality_profiles() {
    for (profile_arg, expected) in [
        ("lossy97", JpegDirectHtj2kProfile::Lossy97),
        ("lossy97-near", JpegDirectHtj2kProfile::Lossy97Near),
        ("lossy97-balanced", JpegDirectHtj2kProfile::Lossy97Balanced),
        (
            "lossy97-aggressive",
            JpegDirectHtj2kProfile::Lossy97Aggressive,
        ),
        ("lossy97-preview", JpegDirectHtj2kProfile::Lossy97Preview),
        (
            "lossy97-thumbnail",
            JpegDirectHtj2kProfile::Lossy97Thumbnail,
        ),
    ] {
        let cli = Cli::try_parse_from([
            "wsi-dicom",
            "convert",
            "source.svs",
            "--out",
            "out",
            "--transfer-syntax",
            "htj2k",
            "--jpeg-direct-htj2k-profile",
            profile_arg,
        ])
        .unwrap();

        let Command::Convert { export, .. } = cli.command else {
            panic!("expected convert command");
        };

        assert_eq!(export.encode.jpeg_direct_htj2k_profile, Some(expected));
    }
}

#[test]
fn cli_convert_accepts_gpu_encode_tuning_flags() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "convert",
        "source.svs",
        "--out",
        "out",
        "--gpu-encode-inflight-tiles",
        "8",
        "--gpu-encode-memory-mib",
        "4096",
        "--gpu-pipeline-depth",
        "3",
        "--gpu-row-batch-rows",
        "6",
        "--gpu-row-batch-target-tiles",
        "96",
    ])
    .unwrap();

    let Command::Convert { export, .. } = cli.command else {
        panic!("expected convert command");
    };

    let gpu_encode = export.encode.gpu_encode;
    assert_eq!(gpu_encode.gpu_encode_inflight_tiles, Some(8));
    assert_eq!(gpu_encode.gpu_encode_memory_mib, Some(4096));
    assert_eq!(gpu_encode.gpu_pipeline_depth, Some(3));
    assert_eq!(gpu_encode.gpu_row_batch_rows, Some(6));
    assert_eq!(gpu_encode.gpu_row_batch_target_tiles, Some(96));
}

#[test]
fn cli_convert_accepts_explicit_qupath_annotation_conversion() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "convert",
        "case.ndpi",
        "--out",
        "dicom-out",
        "--qupath-annotations",
        "case.geojson",
        "--annotation-mapping",
        "mapping.json",
        "--annotation-target",
        "ann",
        "--annotation-target",
        "sr",
        "--annotation-coordinate-space",
        "level0-pixels",
    ]);

    let cli = cli.unwrap_or_else(|error| panic!("combined conversion should parse: {error}"));
    let Command::Convert { annotations, .. } = cli.command else {
        panic!("expected convert command");
    };
    let options = annotations.options().unwrap().unwrap();
    assert_eq!(options.geojson_path, std::path::Path::new("case.geojson"));
    assert_eq!(options.mapping_path, std::path::Path::new("mapping.json"));
    assert_eq!(
        options.targets,
        vec![AnnotationTarget::Ann, AnnotationTarget::Sr]
    );
    assert_eq!(
        options.coordinate_space,
        AnnotationCoordinateSpace::Level0Pixels
    );
}

#[test]
fn cli_convert_rejects_incomplete_qupath_annotation_selection() {
    assert!(Cli::try_parse_from([
        "wsi-dicom",
        "convert",
        "case.ndpi",
        "--out",
        "dicom-out",
        "--qupath-annotations",
        "case.geojson",
    ])
    .is_err());

    assert!(Cli::try_parse_from([
        "wsi-dicom",
        "convert",
        "case.ndpi",
        "--out",
        "dicom-out",
        "--annotation-target",
        "ann",
    ])
    .is_err());
}

#[test]
fn cli_convert_accepts_jpeg_quality_and_j2k_decomposition_levels() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "convert",
        "source.svs",
        "--out",
        "out",
        "--jpeg-quality",
        "80",
        "--j2k-decomposition-levels",
        "0",
    ])
    .unwrap();

    let Command::Convert { export, .. } = cli.command else {
        panic!("expected convert command");
    };

    assert_eq!(export.encode.jpeg_quality, 80);
    assert_eq!(export.encode.j2k_decomposition_levels, Some(0));
}
