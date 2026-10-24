#![forbid(unsafe_code)]

//! `validate --conventions` and `requirements --json` through the shipped
//! binary: convention results are reported apart from the DICOM checks and
//! do not change them or the exit status.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn wsi_dicom() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wsi-dicom"))
}

/// The DICOM files the self-test writes to `workspace`.
fn self_test_output(workspace: &Path) -> std::path::PathBuf {
    let output = wsi_dicom()
        .args(["self-test", "--json", "--keep-output", "--out"])
        .arg(workspace)
        .output()
        .expect("execute self-test");
    assert!(
        output.status.success(),
        "self-test failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("self-test JSON");
    Path::new(report["output_dir"].as_str().expect("output_dir")).to_path_buf()
}

fn validate(path: &Path, extra: &[&str]) -> (Output, Value) {
    let output = wsi_dicom()
        .args(["validate", "--json"])
        .args(extra)
        .arg(path)
        .output()
        .expect("execute validate");
    let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "validate JSON: {error}\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output, report)
}

/// Each DICOM check's name, path, status and message; external tools'
/// elapsed times differ between runs.
fn verdicts(report: &Value) -> Vec<[Value; 4]> {
    report["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .map(|check| ["name", "path", "status", "message"].map(|field| check[field].clone()))
        .collect()
}

#[test]
fn shipped_binary_reports_conventions_apart_from_dicom_checks() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let dicom = self_test_output(&temporary.path().join("self-test"));

    let (plain, plain_report) = validate(&dicom, &[]);
    let (with, report) = validate(
        &dicom,
        &["--conventions", "wg26,ihe-dpia", "--conventions", "interop"],
    );
    assert!(plain_report.get("conventions").is_none());
    assert_eq!(plain.status.code(), with.status.code());
    assert_eq!(verdicts(&plain_report), verdicts(&report));

    let conventions = &report["conventions"];
    assert_eq!(
        conventions["profiles"],
        serde_json::json!(["wg26", "ihe-dpia", "interop"])
    );
    let checks = conventions["checks"].as_array().expect("convention checks");
    assert!(!checks.is_empty());
    for check in checks {
        let name = check["name"].as_str().expect("name");
        let profile = check["profile"].as_str().expect("profile");
        assert!(
            name.starts_with(&format!("convention-{profile}-")),
            "{name}"
        );
        assert!(!check["source"].as_str().expect("source").is_empty());
        assert!(matches!(
            check["status"].as_str(),
            Some("passed" | "failed" | "skipped")
        ));
    }
    // The research-placeholder slide records no specimen preparation, which
    // the connectathon requires but DICOM does not.
    assert!(checks.iter().any(|check| {
        check["name"] == "convention-wg26-specimen-preparation" && check["status"] == "failed"
    }));
    assert!(conventions["advisories"].is_array());

    let text = wsi_dicom()
        .args(["validate", "--conventions", "wg26"])
        .arg(&dicom)
        .output()
        .expect("execute validate");
    let summary = String::from_utf8_lossy(&text.stdout);
    assert!(
        summary.contains("conventions wg26 (not DICOM failures)"),
        "{summary}"
    );
    assert!(summary.contains("failed convention-wg26-specimen-preparation"));
}

#[test]
fn shipped_binary_gates_on_conventions_only_when_asked() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let dicom = self_test_output(&temporary.path().join("self-test"));
    let (ungated, _) = validate(&dicom, &["--conventions", "wg26"]);
    assert!(ungated.status.success());
    let (gated, report) = validate(&dicom, &["--conventions", "wg26", "--fail-on-conventions"]);
    assert!(!gated.status.success());
    assert_eq!(report["conventions"]["fail_on_conventions"], true);
    let stderr = String::from_utf8_lossy(&gated.stderr);
    assert!(
        stderr.contains("0 validation check(s) failed")
            && stderr.contains("convention check(s) failed, and --fail-on-conventions is set"),
        "{stderr}"
    );
    // Advisories alone never fail: interop has no checks to fail.
    let (advised, report) = validate(
        &dicom,
        &["--conventions", "interop", "--fail-on-conventions"],
    );
    assert!(
        advised.status.success(),
        "{}",
        String::from_utf8_lossy(&advised.stderr)
    );
    assert!(report["conventions"]["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .all(|check| check["status"] != "failed"));
    // The flag needs a profile to gate on.
    let lone = wsi_dicom()
        .args(["validate", "--fail-on-conventions"])
        .arg(&dicom)
        .output()
        .expect("execute validate");
    assert!(!lone.status.success());
    assert!(String::from_utf8_lossy(&lone.stderr).contains("--conventions"));
}

#[test]
fn shipped_binary_runs_conventions_on_dicom_json() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let document = temporary.path().join("metadata.json");
    // A VL WSI Data Set without a Specimen Description Sequence.
    std::fs::write(
        &document,
        r#"[{"00080016":{"vr":"UI","Value":["1.2.840.10008.5.1.4.1.1.77.1.6"]},"00080018":{"vr":"UI","Value":["1.2.826.0.1.3680043.10.999.8.200.1"]},"0020000E":{"vr":"UI","Value":["1.2.826.0.1.3680043.10.999.8.200.2"]}}]"#,
    )
    .expect("write document");
    let run = |extra: &[&str]| {
        wsi_dicom()
            .args(["validate-json", "--json", "--conventions", "wg26"])
            .args(extra)
            .arg(&document)
            .output()
            .expect("execute validate-json")
    };
    let output = run(&[]);
    let report: Value = serde_json::from_slice(&output.stdout).expect("validate-json JSON");
    let checks = report["conventions"]["checks"].as_array().expect("checks");
    assert!(checks.iter().any(|check| {
        check["name"] == "convention-wg26-sp2025-specimen-type" && check["status"] == "failed"
    }));
    assert!(checks.iter().any(|check| {
        check["name"] == "convention-wg26-transfer-syntax"
            && check["status"] == "skipped"
            && check["message"]
                .as_str()
                .is_some_and(|message| message.contains("needs the Transfer Syntax"))
    }));
    let gated = run(&["--fail-on-conventions"]);
    assert!(!gated.status.success());
    assert!(String::from_utf8_lossy(&gated.stderr).contains("--fail-on-conventions is set"));
}

#[test]
fn shipped_binary_rejects_an_unknown_convention_profile() {
    let output = wsi_dicom()
        .args(["validate", "--conventions", "wg25", "."])
        .output()
        .expect("execute validate");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ihe-dpia-draft-jpeg"), "{stderr}");
}

#[test]
fn shipped_binary_lists_conventions_apart_from_requirements() {
    let output = wsi_dicom()
        .args(["requirements", "--json"])
        .output()
        .expect("execute requirements");
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).expect("requirements JSON");
    let conventions = report["conventions"].as_array().expect("conventions");
    assert!(conventions.iter().any(|entry| {
        entry["name"] == "convention-wg26-sp2025-focal-planes"
            && entry["kind"] == "check"
            && entry["source"]
                .as_str()
                .is_some_and(|source| source.contains("vl.tpl:702"))
    }));
    assert!(report["requirements"]
        .as_array()
        .expect("requirements")
        .iter()
        .all(|entry| !entry["text"]
            .as_str()
            .unwrap_or_default()
            .contains("convention-")));
}
