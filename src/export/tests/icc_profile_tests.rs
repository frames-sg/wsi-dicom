use super::*;
use crate::{
    create_icc_calibration_bundle, ColorManagement, IccCalibrationRegistry, IccConflictDecision,
    IccConflictPolicy, IccProfile, ScannerIdentity,
};

fn export_jpeg_baseline_icc_tiff_for_test(
    work_dir: &std::path::Path,
    jpeg: Vec<u8>,
    color_management: ColorManagement,
) -> Result<ExportReport, Error> {
    let source = work_dir.join("source.svs");
    write_tiled_jpeg_tiff(&source, 8, 8, 8, 8, std::slice::from_ref(&jpeg));
    export_dicom(ExportRequest {
        source_path: source,
        output_dir: work_dir.join("out"),
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            ..ExportOptions::default()
        },
        color_management,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
}

#[test]
fn source_or_srgb_embeds_srgb_when_source_icc_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg = encode_test_jpeg(8, 8, [160, 20, 40]);
    let report =
        export_jpeg_baseline_icc_tiff_for_test(tmp.path(), jpeg, ColorManagement::SourceOrSrgb)
            .unwrap();

    assert_eq!(
        report.instances[0].icc_profile_source,
        IccProfileSource::SynthesizedSrgb
    );
    assert_dicom_icc_header(&dicom_instance_icc_profile(&report.instances[0].path));
    assert_generated_icc_profile_eq(
        dicom_instance_icc_profile(&report.instances[0].path),
        synthetic_srgb_icc_profile_for_test(),
    );
}

#[test]
fn export_dicom_can_assume_display_p3_when_icc_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg = encode_test_jpeg(8, 8, [160, 20, 40]);
    let report = export_jpeg_baseline_icc_tiff_for_test(
        tmp.path(),
        jpeg,
        ColorManagement::SourceOrDisplayP3,
    )
    .unwrap();

    assert_eq!(
        report.instances[0].icc_profile_source,
        IccProfileSource::SynthesizedDisplayP3
    );
    assert_dicom_icc_header(&dicom_instance_icc_profile(&report.instances[0].path));
    assert_generated_icc_profile_eq(
        dicom_instance_icc_profile(&report.instances[0].path),
        synthetic_display_p3_icc_profile_for_test(),
    );
}

#[test]
fn export_dicom_require_source_fails_when_source_has_no_icc() {
    let tmp = tempfile::tempdir().unwrap();
    let jpeg = encode_test_jpeg(8, 8, [160, 20, 40]);
    let err =
        export_jpeg_baseline_icc_tiff_for_test(tmp.path(), jpeg, ColorManagement::RequireSource)
            .unwrap_err();

    assert!(err.to_string().contains("ICC"), "unexpected error: {err}");
}

#[test]
fn explicit_profile_rejects_a_conflict_before_output_state_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let source_profile = synthetic_srgb_icc_profile_for_test();
    let jpeg = jpeg_with_icc_profile(encode_test_jpeg(8, 8, [160, 20, 40]), &source_profile);
    let configured =
        IccProfile::from_bytes("configured-p3", synthetic_display_p3_icc_profile_for_test())
            .unwrap();
    let err = export_jpeg_baseline_icc_tiff_for_test(
        tmp.path(),
        jpeg,
        ColorManagement::ExplicitProfile {
            profile: configured,
            conflict: IccConflictPolicy::Fail,
        },
    )
    .unwrap_err();

    assert!(
        err.to_string().contains("conflict"),
        "unexpected error: {err}"
    );
    assert!(!tmp.path().join("out").exists());
}

#[test]
fn export_dicom_uses_embedded_jpeg_icc_when_available() {
    let tmp = tempfile::tempdir().unwrap();
    let icc_profile = synthetic_display_p3_icc_profile_for_test();
    let jpeg = jpeg_with_icc_profile(encode_test_jpeg(8, 8, [160, 20, 40]), &icc_profile);
    let report =
        export_jpeg_baseline_icc_tiff_for_test(tmp.path(), jpeg, ColorManagement::SourceOrSrgb)
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

#[test]
fn source_calibration_precedes_a_conflicting_embedded_jpeg_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let embedded_profile = synthetic_display_p3_icc_profile_for_test();
    let jpeg = jpeg_with_icc_profile(encode_test_jpeg(8, 8, [160, 20, 40]), &embedded_profile);
    let first =
        export_jpeg_baseline_icc_tiff_for_test(tmp.path(), jpeg, ColorManagement::SourceOrSrgb)
            .unwrap();
    let source = &first.instances[0].path;
    let source_profile = synthetic_srgb_icc_profile_for_test();
    let mut object = dicom_object::open_file(source).unwrap();
    let mut optical_path_sequence = object.take(tags::OPTICAL_PATH_SEQUENCE).unwrap();
    optical_path_sequence.items_mut().unwrap()[0].put(dicom_core::DataElement::new(
        tags::ICC_PROFILE,
        VR::OB,
        dicom_core::PrimitiveValue::from(source_profile.clone()),
    ));
    object.put(optical_path_sequence);
    object.write_to_file(source).unwrap();

    let second = export_dicom(ExportRequest {
        source_path: source.clone(),
        output_dir: tmp.path().join("second-out"),
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrDisplayP3,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();

    assert_eq!(
        second.instances[0].icc_profile_source,
        IccProfileSource::Source
    );
    assert_eq!(
        dicom_instance_icc_profile(&second.instances[0].path),
        source_profile
    );
}

#[test]
fn export_dicom_rejects_embedded_display_profile_class() {
    let tmp = tempfile::tempdir().unwrap();
    let display_profile = moxcms::ColorProfile::new_srgb().encode().unwrap();
    assert_eq!(&display_profile[12..16], b"mntr");
    let jpeg = jpeg_with_icc_profile(encode_test_jpeg(8, 8, [160, 20, 40]), &display_profile);

    let err =
        export_jpeg_baseline_icc_tiff_for_test(tmp.path(), jpeg, ColorManagement::SourceOrSrgb)
            .unwrap_err();

    assert!(err.to_string().contains("scnr"), "unexpected error: {err}");
}

#[test]
fn explicit_profile_conflict_policies_and_digest_match_are_reported() {
    for (name, conflict, expected_source, expected_decision) in [
        (
            "configured",
            IccConflictPolicy::PreferConfigured,
            IccProfileSource::ExplicitProfile,
            IccConflictDecision::PreferredConfigured,
        ),
        (
            "source",
            IccConflictPolicy::PreferSource,
            IccProfileSource::SourceJpeg,
            IccConflictDecision::PreferredSource,
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let source_profile = synthetic_srgb_icc_profile_for_test();
        let jpeg = jpeg_with_icc_profile(encode_test_jpeg(8, 8, [160, 20, 40]), &source_profile);
        let configured_profile = synthetic_display_p3_icc_profile_for_test();
        let expected_profile = match conflict {
            IccConflictPolicy::PreferConfigured => configured_profile.clone(),
            IccConflictPolicy::PreferSource => source_profile.clone(),
            IccConflictPolicy::Fail => unreachable!("the conflict cases select a profile"),
        };
        let configured = IccProfile::from_bytes("configured-p3", configured_profile).unwrap();
        let report = export_jpeg_baseline_icc_tiff_for_test(
            tmp.path(),
            jpeg,
            ColorManagement::ExplicitProfile {
                profile: configured,
                conflict,
            },
        )
        .unwrap();
        let instance = &report.instances[0];
        assert_eq!(instance.icc_profile_source, expected_source, "{name}");
        assert_eq!(instance.icc_conflict_decision, expected_decision, "{name}");
        assert_eq!(
            instance.icc_calibration_id.as_deref(),
            Some("configured-p3")
        );
        assert_eq!(dicom_instance_icc_profile(&instance.path), expected_profile);
    }

    let tmp = tempfile::tempdir().unwrap();
    let profile = synthetic_srgb_icc_profile_for_test();
    let jpeg = jpeg_with_icc_profile(encode_test_jpeg(8, 8, [160, 20, 40]), &profile);
    let configured = IccProfile::from_bytes("matching", profile.clone()).unwrap();
    let report = export_jpeg_baseline_icc_tiff_for_test(
        tmp.path(),
        jpeg,
        ColorManagement::ExplicitProfile {
            profile: configured,
            conflict: IccConflictPolicy::Fail,
        },
    )
    .unwrap();
    let instance = &report.instances[0];
    assert_eq!(
        instance.icc_conflict_decision,
        IccConflictDecision::DigestsMatch
    );
    assert_eq!(instance.icc_calibration_id.as_deref(), Some("matching"));
    assert_eq!(dicom_instance_icc_profile(&instance.path), profile);
}

#[test]
fn registry_calibration_matches_governed_scanner_metadata_and_embeds_only_verified_bytes() {
    use sha2::{Digest, Sha256};

    let tmp = tempfile::tempdir().unwrap();
    let profile_path = tmp.path().join("vendor-secret-input-name.icc");
    let profile = synthetic_display_p3_icc_profile_for_test();
    std::fs::write(&profile_path, &profile).unwrap();
    let bundle = tmp.path().join("bundle");
    let scanner = ScannerIdentity::new("Vendor", "Model X", "SN-42").unwrap();
    create_icc_calibration_bundle(&profile_path, "lab-modelx-sn42", &scanner, &bundle).unwrap();
    let registry = IccCalibrationRegistry::from_file(bundle.join("registry.json")).unwrap();

    let source = tmp.path().join("source.svs");
    let jpeg = encode_test_jpeg(8, 8, [160, 20, 40]);
    write_tiled_jpeg_tiff(&source, 8, 8, 8, 8, &[jpeg]);
    let mut metadata = DicomMetadata::research_placeholder();
    metadata.manufacturer = Some("  Vendor ".into());
    metadata.manufacturer_model_name = Some("Model X".into());
    metadata.device_serial_number = Some("SN-42  ".into());
    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: tmp.path().join("registry-out"),
        options: ExportOptions {
            tile_size: 8,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::Calibration {
            registry,
            conflict: IccConflictPolicy::Fail,
        },
        metadata: MetadataSource::Strict(Box::new(metadata)),
        level_filter: None,
    })
    .unwrap();

    let instance = &report.instances[0];
    let expected_digest = format!("{:x}", Sha256::digest(&profile));
    assert_eq!(
        instance.icc_profile_source,
        IccProfileSource::CalibrationRegistry
    );
    assert_eq!(
        instance.icc_calibration_id.as_deref(),
        Some("lab-modelx-sn42")
    );
    assert_eq!(
        instance.icc_profile_sha256.as_deref(),
        Some(expected_digest.as_str())
    );
    assert_eq!(
        instance.icc_conflict_decision,
        IccConflictDecision::NoConflict
    );
    assert_eq!(dicom_instance_icc_profile(&instance.path), profile);

    let dicom_bytes = std::fs::read(&instance.path).unwrap();
    let local_name = b"vendor-secret-input-name.icc";
    assert!(!dicom_bytes
        .windows(local_name.len())
        .any(|window| window == local_name));
    let report_json = serde_json::to_string(&report).unwrap();
    assert!(!report_json.contains("vendor-secret-input-name.icc"));
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
    let mut profile = moxcms::ColorProfile::new_srgb();
    profile.profile_class = moxcms::ProfileClass::InputDevice;
    profile.encode().unwrap()
}

fn synthetic_display_p3_icc_profile_for_test() -> Vec<u8> {
    let mut profile = moxcms::ColorProfile::new_display_p3();
    profile.profile_class = moxcms::ProfileClass::InputDevice;
    profile.encode().unwrap()
}

fn assert_dicom_icc_header(profile: &[u8]) {
    assert!(profile.len() >= 128);
    assert_eq!(&profile[12..16], b"scnr");
    assert_eq!(&profile[16..20], b"RGB ");
    assert!(matches!(&profile[20..24], b"XYZ " | b"Lab "));
    assert_eq!(&profile[36..40], b"acsp");
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
