#![forbid(unsafe_code)]

use std::{path::Path, process::Command};

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

fn path_from_json(value: &Value) -> &Path {
    Path::new(value.as_str().expect("JSON path string"))
}
