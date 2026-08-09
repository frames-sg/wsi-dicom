#![forbid(unsafe_code)]

use std::{path::Path, process::Command};

use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::tags;
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[test]
fn shipped_binary_self_test_emits_json_and_preserves_validation_evidence() {
    let temporary_directory = tempfile::tempdir().expect("create temporary directory");
    let workspace = temporary_directory.path().join("self-test-evidence");

    let output = Command::new(env!("CARGO_BIN_EXE_wsi-dicom"))
        .arg("self-test")
        .arg("--json")
        .arg("--out")
        .arg(&workspace)
        .arg("--keep-output")
        .arg("--command-timeout-secs")
        .arg("15")
        .output()
        .expect("execute shipped wsi-dicom binary");

    assert!(
        output.status.success(),
        "wsi-dicom self-test failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "self-test stdout was not valid JSON: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });

    assert_eq!(report["kept_output"], true);
    assert_eq!(report["workspace"], workspace.to_string_lossy().as_ref());
    assert!(path_from_json(&report["source_path"]).is_file());
    assert!(path_from_json(&report["output_dir"]).is_dir());

    let instances = report["export_report"]["instances"]
        .as_array()
        .expect("export report instances array");
    assert!(
        !instances.is_empty(),
        "self-test must export a DICOM instance"
    );
    for instance in instances {
        assert!(path_from_json(&instance["path"]).is_file());
    }

    let validated_files = report["validation_report"]["files"]
        .as_array()
        .expect("validation report files array");
    assert_eq!(validated_files.len(), instances.len());
    assert!(validated_files
        .iter()
        .all(|path| path_from_json(path).is_file()));

    let checks = report["validation_report"]["checks"]
        .as_array()
        .expect("validation report checks array");
    assert!(
        !checks.is_empty(),
        "self-test must execute validation checks"
    );
    assert!(
        checks.iter().all(|check| check["status"] != "failed"),
        "self-test report contains a failed validation check: {checks:#?}"
    );
}

#[test]
fn shipped_binary_rejects_malformed_compressed_pixel_data_without_external_tools() {
    let temporary_directory = tempfile::tempdir().expect("create temporary directory");
    let path = temporary_directory.path().join("malformed.dcm");
    write_compressed_transfer_syntax_with_primitive_pixel_data(&path);

    let output = Command::new(env!("CARGO_BIN_EXE_wsi-dicom"))
        .arg("validate")
        .arg(&path)
        .arg("--max-pixel-frames")
        .arg("0")
        .arg("--json")
        .output()
        .expect("execute shipped wsi-dicom binary");

    assert!(
        !output.status.success(),
        "malformed compressed Pixel Data unexpectedly passed validation"
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "validation stdout was not JSON: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let checks = report["checks"].as_array().expect("validation checks");
    assert!(checks.iter().any(|check| {
        check["name"] == "intrinsic-pixel-structure" && check["status"] == "failed"
    }));
}

#[test]
fn calibration_bundle_cli_flow_embeds_verified_profile_without_leaking_local_paths() {
    let temporary_directory = tempfile::tempdir().expect("create temporary directory");
    let self_test_workspace = temporary_directory.path().join("synthetic-source");
    let self_test = Command::new(env!("CARGO_BIN_EXE_wsi-dicom"))
        .arg("self-test")
        .arg("--json")
        .arg("--out")
        .arg(&self_test_workspace)
        .arg("--keep-output")
        .arg("--command-timeout-secs")
        .arg("15")
        .output()
        .expect("create synthetic color source through shipped CLI");
    assert!(
        self_test.status.success(),
        "self-test source creation failed: {}",
        String::from_utf8_lossy(&self_test.stderr)
    );
    let self_test_report: Value = serde_json::from_slice(&self_test.stdout).unwrap();
    let source = path_from_json(&self_test_report["source_path"]);

    let mut generated = moxcms::ColorProfile::new_display_p3();
    generated.profile_class = moxcms::ProfileClass::InputDevice;
    let profile = generated.encode().unwrap();
    let profile_path = temporary_directory
        .path()
        .join("local-vendor-target-profile.icc");
    std::fs::write(&profile_path, &profile).unwrap();
    let expected_digest = format!("{:x}", Sha256::digest(&profile));
    let bundle = temporary_directory.path().join("calibration-bundle");

    let created = Command::new(env!("CARGO_BIN_EXE_wsi-dicom"))
        .arg("calibration")
        .arg("create")
        .arg("--icc")
        .arg(&profile_path)
        .arg("--id")
        .arg("research-scanner")
        .arg("--manufacturer")
        .arg("wsi-dicom")
        .arg("--model")
        .arg("wsi-dicom")
        .arg("--serial")
        .arg("RESEARCH")
        .arg("--out")
        .arg(&bundle)
        .output()
        .expect("create calibration bundle");
    assert!(
        created.status.success(),
        "calibration create failed: {}",
        String::from_utf8_lossy(&created.stderr)
    );

    let inspected = Command::new(env!("CARGO_BIN_EXE_wsi-dicom"))
        .arg("calibration")
        .arg("inspect")
        .arg("--icc")
        .arg(bundle.join("profiles/profile.icc"))
        .output()
        .expect("inspect calibration profile");
    assert!(inspected.status.success());
    assert!(String::from_utf8_lossy(&inspected.stdout).contains(&expected_digest));

    let output_dir = temporary_directory.path().join("calibrated-output");
    let converted = Command::new(env!("CARGO_BIN_EXE_wsi-dicom"))
        .arg("convert")
        .arg(source)
        .arg("--out")
        .arg(&output_dir)
        .arg("--research-placeholder")
        .arg("--icc-calibration-registry")
        .arg(bundle.join("registry.json"))
        .arg("--backend")
        .arg("cpu")
        .arg("--transfer-syntax")
        .arg("jpeg2000-lossless")
        .arg("--tile-size")
        .arg("4")
        .arg("--json")
        .output()
        .expect("convert synthetic source with calibration registry");
    assert!(
        converted.status.success(),
        "calibrated conversion failed with {}\nstdout:\n{}\nstderr:\n{}",
        converted.status,
        String::from_utf8_lossy(&converted.stdout),
        String::from_utf8_lossy(&converted.stderr)
    );
    let report: Value = serde_json::from_slice(&converted.stdout).unwrap();
    let instance = &report["instances"][0];
    assert_eq!(instance["icc_profile_source"], "calibration_registry");
    assert_eq!(instance["icc_profile_sha256"], expected_digest);
    assert_eq!(instance["icc_calibration_id"], "research-scanner");
    assert_eq!(instance["icc_conflict_decision"], "no_conflict");

    let dicom_path = path_from_json(&instance["path"]);
    let object = dicom_object::open_file(dicom_path).unwrap();
    let optical_paths = object
        .element(tags::OPTICAL_PATH_SEQUENCE)
        .unwrap()
        .items()
        .unwrap();
    let embedded = optical_paths[0]
        .element(tags::ICC_PROFILE)
        .unwrap()
        .to_bytes()
        .unwrap();
    assert_eq!(embedded.as_ref(), profile);

    let dicom_bytes = std::fs::read(dicom_path).unwrap();
    let local_path = profile_path.to_string_lossy();
    assert!(!dicom_bytes
        .windows(local_path.len())
        .any(|window| window == local_path.as_bytes()));
    assert!(!String::from_utf8_lossy(&converted.stdout).contains(local_path.as_ref()));
}

fn write_compressed_transfer_syntax_with_primitive_pixel_data(path: &Path) {
    let sop_class = "1.2.840.10008.5.1.4.1.1.77.1.6";
    let sop_instance = "1.2.826.0.1.3680043.10.999.991";
    let mut object = InMemDicomObject::new_empty();
    object.put(DataElement::<InMemDicomObject>::new(
        tags::SOP_CLASS_UID,
        VR::UI,
        PrimitiveValue::from(sop_class),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::SOP_INSTANCE_UID,
        VR::UI,
        PrimitiveValue::from(sop_instance),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::NUMBER_OF_FRAMES,
        VR::IS,
        PrimitiveValue::from("1"),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::PIXEL_DATA,
        VR::OB,
        PrimitiveValue::U8(vec![1, 2, 3, 4].into()),
    ));
    object
        .with_meta(
            FileMetaTableBuilder::new()
                .media_storage_sop_class_uid(sop_class)
                .media_storage_sop_instance_uid(sop_instance)
                .transfer_syntax("1.2.840.10008.1.2.4.50"),
        )
        .expect("file meta")
        .write_to_file(path)
        .expect("write malformed DICOM");
}

fn path_from_json(value: &Value) -> &Path {
    Path::new(value.as_str().expect("JSON path string"))
}
