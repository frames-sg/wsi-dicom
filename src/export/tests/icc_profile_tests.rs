use super::*;

#[test]
fn export_dicom_defaults_missing_icc_to_assumed_srgb() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg = encode_test_jpeg(8, 8, [160, 20, 40]);
    let source = tmp.path().join("source.svs");
    write_tiled_jpeg_tiff(&source, 8, 8, 8, 8, std::slice::from_ref(&jpeg));
    let out = tmp.path().join("out");

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out,
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();

    assert_eq!(
        report.instances[0].icc_profile_source,
        IccProfileSource::SynthesizedSrgb
    );
    assert_generated_icc_profile_eq(
        dicom_instance_icc_profile(&report.instances[0].path),
        synthetic_srgb_icc_profile_for_test(),
    );
}

#[test]
fn export_dicom_can_assume_display_p3_when_icc_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg = encode_test_jpeg(8, 8, [160, 20, 40]);
    let source = tmp.path().join("source.svs");
    write_tiled_jpeg_tiff(&source, 8, 8, 8, 8, std::slice::from_ref(&jpeg));
    let out = tmp.path().join("out");

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out,
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            icc_profile_policy: IccProfilePolicy::FallbackDisplayP3,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();

    assert_eq!(
        report.instances[0].icc_profile_source,
        IccProfileSource::SynthesizedDisplayP3
    );
    assert_generated_icc_profile_eq(
        dicom_instance_icc_profile(&report.instances[0].path),
        synthetic_display_p3_icc_profile_for_test(),
    );
}

#[test]
fn export_dicom_strict_icc_fails_when_source_has_no_icc() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg = encode_test_jpeg(8, 8, [160, 20, 40]);
    let source = tmp.path().join("source.svs");
    write_tiled_jpeg_tiff(&source, 8, 8, 8, 8, std::slice::from_ref(&jpeg));
    let out = tmp.path().join("out");

    let err = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out,
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            icc_profile_policy: IccProfilePolicy::Strict,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap_err();

    assert!(err.to_string().contains("ICC"), "unexpected error: {err}");
}

#[test]
fn export_dicom_can_omit_missing_icc_when_requested() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg = encode_test_jpeg(8, 8, [160, 20, 40]);
    let source = tmp.path().join("source.svs");
    write_tiled_jpeg_tiff(&source, 8, 8, 8, 8, std::slice::from_ref(&jpeg));
    let out = tmp.path().join("out");

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out,
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            icc_profile_policy: IccProfilePolicy::OmitIfMissing,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();

    assert_eq!(
        report.instances[0].icc_profile_source,
        IccProfileSource::OmittedMissing
    );
    assert!(!dicom_instance_has_icc_profile(&report.instances[0].path));
}

#[test]
fn export_dicom_uses_embedded_jpeg_icc_when_available() {
    let tmp = tempfile::tempdir().unwrap();
    let icc_profile = synthetic_display_p3_icc_profile_for_test();
    let jpeg = jpeg_with_icc_profile(encode_test_jpeg(8, 8, [160, 20, 40]), &icc_profile);
    let source = tmp.path().join("source.svs");
    write_tiled_jpeg_tiff(&source, 8, 8, 8, 8, std::slice::from_ref(&jpeg));
    let out = tmp.path().join("out");

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out,
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();

    assert_eq!(
        report.instances[0].icc_profile_source,
        IccProfileSource::SourceJpeg
    );
    assert_eq!(
        dicom_instance_icc_profile(&report.instances[0].path),
        icc_profile
    );
}

fn jpeg_with_icc_profile(jpeg: Vec<u8>, icc_profile: &[u8]) -> Vec<u8> {
    assert!(jpeg.starts_with(&[0xFF, 0xD8]));
    let payload_len = 14 + icc_profile.len();
    let segment_len = u16::try_from(payload_len + 2).expect("ICC APP2 segment fits in JPEG");
    let mut out = Vec::with_capacity(jpeg.len() + payload_len + 4);
    out.extend_from_slice(&jpeg[..2]);
    out.extend_from_slice(&[0xFF, 0xE2]);
    out.extend_from_slice(&segment_len.to_be_bytes());
    out.extend_from_slice(b"ICC_PROFILE\0");
    out.extend_from_slice(&[1, 1]);
    out.extend_from_slice(icc_profile);
    out.extend_from_slice(&jpeg[2..]);
    out
}

fn dicom_instance_has_icc_profile(path: &std::path::Path) -> bool {
    let object = dicom_object::open_file(path).unwrap();
    let optical_path = object
        .element(tags::OPTICAL_PATH_SEQUENCE)
        .unwrap()
        .items()
        .unwrap();
    optical_path[0].element(tags::ICC_PROFILE).is_ok()
}

fn dicom_instance_icc_profile(path: &std::path::Path) -> Vec<u8> {
    let object = dicom_object::open_file(path).unwrap();
    let optical_path = object
        .element(tags::OPTICAL_PATH_SEQUENCE)
        .unwrap()
        .items()
        .unwrap();
    optical_path[0]
        .element(tags::ICC_PROFILE)
        .unwrap()
        .to_bytes()
        .unwrap()
        .into_owned()
}

fn synthetic_srgb_icc_profile_for_test() -> Vec<u8> {
    moxcms::ColorProfile::new_srgb().encode().unwrap()
}

fn synthetic_display_p3_icc_profile_for_test() -> Vec<u8> {
    moxcms::ColorProfile::new_display_p3().encode().unwrap()
}

fn assert_generated_icc_profile_eq(mut actual: Vec<u8>, mut expected: Vec<u8>) {
    normalize_icc_profile_creation_datetime(&mut actual);
    normalize_icc_profile_creation_datetime(&mut expected);
    assert_eq!(actual, expected);
}

fn normalize_icc_profile_creation_datetime(profile: &mut [u8]) {
    const ICC_CREATION_DATETIME: std::ops::Range<usize> = 24..36;
    if let Some(created_at) = profile.get_mut(ICC_CREATION_DATETIME) {
        created_at.fill(0);
    }
}
