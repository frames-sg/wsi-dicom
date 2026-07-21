#![forbid(unsafe_code)]

use std::{path::Path, process::Command};

use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::tags;
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};
use serde_json::Value;

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
