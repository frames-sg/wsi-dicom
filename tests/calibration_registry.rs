use std::fs;

use sha2::{Digest, Sha256};
use wsi_dicom::{
    create_icc_calibration_bundle, IccCalibrationRegistry, IccProfile, ScannerIdentity,
    ICC_CALIBRATION_REGISTRY_MAX_BYTES, ICC_PROFILE_MAX_BYTES,
};

fn input_profile() -> Vec<u8> {
    let mut profile = moxcms::ColorProfile::new_srgb();
    profile.profile_class = moxcms::ProfileClass::InputDevice;
    profile.encode().unwrap()
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn registry_loads_verified_bytes_and_matches_trimmed_governed_metadata_exactly() {
    let directory = tempfile::tempdir().unwrap();
    let profiles = directory.path().join("profiles");
    fs::create_dir(&profiles).unwrap();
    let bytes = input_profile();
    fs::write(profiles.join("profile.icc"), &bytes).unwrap();
    fs::write(
        directory.path().join("registry.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "calibrations": [{
                "id": "lab-at2-sn123-2026q3",
                "scanner": {
                    "manufacturer": "  Leica Biosystems ",
                    "model_name": "Aperio AT2",
                    "device_serial_number": "SN123  "
                },
                "icc_profile": "profiles/profile.icc",
                "sha256": digest(&bytes)
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let registry =
        IccCalibrationRegistry::from_file(directory.path().join("registry.json")).unwrap();
    let scanner = ScannerIdentity::new("Leica Biosystems", "Aperio AT2", "SN123").unwrap();
    let calibration = registry.match_scanner(&scanner).unwrap();

    assert_eq!(calibration.id(), "lab-at2-sn123-2026q3");
    assert_eq!(calibration.bytes(), bytes);
    assert_eq!(calibration.sha256(), digest(&bytes));
    assert!(registry
        .match_scanner(&ScannerIdentity::new("leica biosystems", "Aperio AT2", "SN123").unwrap())
        .is_none());
}

#[test]
fn registry_rejects_unknown_fields_and_parent_traversal() {
    let directory = tempfile::tempdir().unwrap();
    let bytes = input_profile();
    fs::write(directory.path().join("profile.icc"), &bytes).unwrap();

    for (name, profile_path, extra) in [
        ("unknown", "profile.icc", Some(("unexpected", true))),
        ("traversal", "../profile.icc", None),
    ] {
        let mut calibration = serde_json::json!({
            "id": name,
            "scanner": {
                "manufacturer": "Vendor",
                "model_name": "Model",
                "device_serial_number": "Serial"
            },
            "icc_profile": profile_path,
            "sha256": digest(&bytes)
        });
        if let Some((key, value)) = extra {
            calibration
                .as_object_mut()
                .unwrap()
                .insert(key.into(), value.into());
        }
        let registry_path = directory.path().join(format!("{name}.json"));
        fs::write(
            &registry_path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "calibrations": [calibration]
            }))
            .unwrap(),
        )
        .unwrap();

        let error = IccCalibrationRegistry::from_file(&registry_path).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("unknown field") || message.contains("relative"),
            "unexpected error: {message}"
        );
    }
}

#[test]
fn registry_rejects_duplicate_ids_scanners_and_checksum_mismatches() {
    let directory = tempfile::tempdir().unwrap();
    let bytes = input_profile();
    fs::write(directory.path().join("profile.icc"), &bytes).unwrap();
    let scanner = serde_json::json!({
        "manufacturer": "Vendor",
        "model_name": "Model",
        "device_serial_number": "Serial"
    });

    for (name, calibrations, expected) in [
        (
            "duplicate-id",
            vec![
                serde_json::json!({"id":"same","scanner":scanner,"icc_profile":"profile.icc","sha256":digest(&bytes)}),
                serde_json::json!({"id":"same","scanner":{"manufacturer":"V2","model_name":"M2","device_serial_number":"S2"},"icc_profile":"profile.icc","sha256":digest(&bytes)}),
            ],
            "duplicate calibration id",
        ),
        (
            "duplicate-scanner",
            vec![
                serde_json::json!({"id":"one","scanner":scanner,"icc_profile":"profile.icc","sha256":digest(&bytes)}),
                serde_json::json!({"id":"two","scanner":scanner,"icc_profile":"profile.icc","sha256":digest(&bytes)}),
            ],
            "duplicate scanner",
        ),
        (
            "checksum",
            vec![
                serde_json::json!({"id":"one","scanner":scanner,"icc_profile":"profile.icc","sha256":"0000000000000000000000000000000000000000000000000000000000000000"}),
            ],
            "checksum",
        ),
    ] {
        let registry_path = directory.path().join(format!("{name}.json"));
        fs::write(
            &registry_path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "calibrations": calibrations
            }))
            .unwrap(),
        )
        .unwrap();
        let error = IccCalibrationRegistry::from_file(&registry_path).unwrap_err();
        assert!(
            error.to_string().contains(expected),
            "unexpected error: {error}"
        );
    }
}

#[test]
fn explicit_profile_rejects_display_class_and_bundle_round_trips() {
    let display_profile = moxcms::ColorProfile::new_srgb().encode().unwrap();
    let error = IccProfile::from_bytes("display", display_profile).unwrap_err();
    assert!(error.to_string().contains("scnr"));

    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("vendor.icc");
    fs::write(&source, input_profile()).unwrap();
    let output = directory.path().join("bundle");
    let scanner = ScannerIdentity::new("Vendor", "Model", "Serial").unwrap();
    create_icc_calibration_bundle(&source, "cal-1", &scanner, &output).unwrap();

    let registry = IccCalibrationRegistry::from_file(output.join("registry.json")).unwrap();
    let profile = registry.match_scanner(&scanner).unwrap();
    assert_eq!(profile.id(), "cal-1");
    assert_eq!(profile.bytes(), input_profile());
}

#[test]
fn registry_and_profile_resource_limits_are_enforced_before_parsing() {
    let directory = tempfile::tempdir().unwrap();
    let oversized_registry = directory.path().join("oversized-registry.json");
    fs::File::create(&oversized_registry)
        .unwrap()
        .set_len(ICC_CALIBRATION_REGISTRY_MAX_BYTES + 1)
        .unwrap();
    assert!(IccCalibrationRegistry::from_file(&oversized_registry)
        .unwrap_err()
        .to_string()
        .contains("limit"));

    let oversized_profile = directory.path().join("oversized.icc");
    fs::File::create(&oversized_profile)
        .unwrap()
        .set_len(ICC_PROFILE_MAX_BYTES + 1)
        .unwrap();
    assert!(IccProfile::from_file("oversized", &oversized_profile)
        .unwrap_err()
        .to_string()
        .contains("limit"));

    let entries = (0..257)
        .map(|index| {
            serde_json::json!({
                "id": format!("cal-{index}"),
                "scanner": {
                    "manufacturer": "Vendor",
                    "model_name": "Model",
                    "device_serial_number": format!("S-{index}")
                },
                "icc_profile": "unused.icc",
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            })
        })
        .collect::<Vec<_>>();
    let too_many = directory.path().join("too-many.json");
    fs::write(
        &too_many,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "calibrations": entries
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(IccCalibrationRegistry::from_file(&too_many)
        .unwrap_err()
        .to_string()
        .contains("limit is 256"));
}

#[test]
fn registry_rejects_malformed_hash_absolute_path_missing_file_and_empty_scanner_fields() {
    let directory = tempfile::tempdir().unwrap();
    let absolute = directory.path().join("profile.icc");
    fs::write(&absolute, input_profile()).unwrap();
    let cases = [
        (
            "malformed-hash",
            "profile.icc".to_string(),
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string(),
            "Vendor",
            "lowercase hexadecimal",
        ),
        (
            "absolute",
            absolute.display().to_string(),
            digest(&input_profile()),
            "Vendor",
            "relative path",
        ),
        (
            "windows-absolute",
            r"C:\profiles\profile.icc".to_string(),
            digest(&input_profile()),
            "Vendor",
            "portable relative path",
        ),
        (
            "windows-traversal",
            r"..\profile.icc".to_string(),
            digest(&input_profile()),
            "Vendor",
            "portable relative path",
        ),
        (
            "missing",
            "missing.icc".to_string(),
            digest(&input_profile()),
            "Vendor",
            "No such file",
        ),
        (
            "empty-scanner",
            "profile.icc".to_string(),
            digest(&input_profile()),
            "   ",
            "must not be empty",
        ),
    ];

    for (name, profile_path, hash, manufacturer, expected) in cases {
        let path = directory.path().join(format!("{name}.json"));
        fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "calibrations": [{
                    "id": name,
                    "scanner": {
                        "manufacturer": manufacturer,
                        "model_name": "Model",
                        "device_serial_number": "Serial"
                    },
                    "icc_profile": profile_path,
                    "sha256": hash
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let error = IccCalibrationRegistry::from_file(&path).unwrap_err();
        assert!(
            error.to_string().contains(expected),
            "{name}: unexpected error: {error}"
        );
    }
}
