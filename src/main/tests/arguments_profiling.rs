use crate::cli_args::{Cli, Command};
use crate::cli_profile::max_level_elapsed_from_ms;
use clap::Parser;

#[test]
fn cli_profile_coverage_and_sustain_accept_equivalence_flags() {
    let profile = Cli::try_parse_from([
        "wsi-dicom",
        "profile",
        "source.svs",
        "--jpeg-quality",
        "80",
        "--j2k-decomposition-levels",
        "0",
    ])
    .unwrap();
    let Command::Profile { encode, .. } = profile.command else {
        panic!("expected profile command");
    };
    assert_eq!(encode.jpeg_quality, 80);
    assert_eq!(encode.j2k_decomposition_levels, Some(0));

    let coverage = Cli::try_parse_from([
        "wsi-dicom",
        "coverage",
        "source.svs",
        "--jpeg-quality",
        "80",
        "--j2k-decomposition-levels",
        "0",
    ])
    .unwrap();
    let Command::Coverage { encode, .. } = coverage.command else {
        panic!("expected coverage command");
    };
    assert_eq!(encode.jpeg_quality, 80);
    assert_eq!(encode.j2k_decomposition_levels, Some(0));

    let sustain_convert = Cli::try_parse_from([
        "wsi-dicom",
        "sustain-convert",
        "source.svs",
        "--out",
        "out",
        "--jpeg-quality",
        "80",
        "--j2k-decomposition-levels",
        "0",
    ])
    .unwrap();
    let Command::SustainConvert { export, .. } = sustain_convert.command else {
        panic!("expected sustain-convert command");
    };
    assert_eq!(export.encode.jpeg_quality, 80);
    assert_eq!(export.encode.j2k_decomposition_levels, Some(0));

    let sustain = Cli::try_parse_from([
        "wsi-dicom",
        "sustain",
        "source.svs",
        "--jpeg-quality",
        "80",
        "--j2k-decomposition-levels",
        "0",
    ])
    .unwrap();
    let Command::Sustain { encode, .. } = sustain.command else {
        panic!("expected sustain command");
    };
    assert_eq!(encode.jpeg_quality, 80);
    assert_eq!(encode.j2k_decomposition_levels, Some(0));
}

#[test]
fn cli_coverage_corpus_accepts_max_level_elapsed_limit_ms() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "coverage-corpus",
        "slides",
        "--max-level-ms",
        "250",
    ])
    .unwrap();

    let Command::CoverageCorpus { max_level_ms, .. } = cli.command else {
        panic!("expected coverage-corpus command");
    };

    assert_eq!(max_level_ms, Some(250));
}

#[test]
fn cli_sustain_accepts_max_level_elapsed_limit_ms() {
    let cli = Cli::try_parse_from([
        "wsi-dicom",
        "sustain",
        "source.svs",
        "--max-level-ms",
        "250",
    ])
    .unwrap();

    let Command::Sustain { max_level_ms, .. } = cli.command else {
        panic!("expected sustain command");
    };

    assert_eq!(max_level_ms, Some(250));
}

#[test]
fn cli_rejects_zero_max_level_elapsed_limit_ms() {
    let err = max_level_elapsed_from_ms(Some(0)).unwrap_err();

    assert!(
        err.to_string().contains("--max-level-ms"),
        "unexpected error: {err}"
    );
}
