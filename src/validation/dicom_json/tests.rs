//! DICOM JSON documents: each Annex F rule broken alone fails the JSON
//! Model check, and an exported slide's Metadata is held to the checks of
//! the file it came from.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dicom_dictionary_std::tags;

use super::{validate_dicom_json_path, validate_json_bytes, DicomJsonOptions, JsonRun, Source};
use crate::dicomweb::tests::fixture::{export_wsi, open, to_json};
use crate::validation::{ValidationCheck, ValidationStatus};
use crate::TransferSyntax;

const JSON_MODEL: &str = "intrinsic-wsi-dicom-json-model";

#[test]
fn bulk_data_references_do_not_disclose_patient_values() {
    for uri in [
        "https://example.org/PatientSentinel/bulk",
        "invalid PatientSentinel",
    ] {
        let text = document(&format!(
            r#"{SOP},"00282000":{{"vr":"OB","BulkDataURI":"{uri}"}}"#
        ));
        let checks = checks_of(
            &text,
            Source::Metadata,
            &DicomJsonOptions::default(),
            &BTreeMap::new(),
        );
        let report = serde_json::to_string(&checks).unwrap();
        assert!(!report.contains("PatientSentinel"), "{report}");
        assert!(report.contains("BulkDataURI"), "{report}");
    }
}

fn checks_of(
    document: &str,
    source: Source,
    options: &DicomJsonOptions,
    fetched: &BTreeMap<String, Vec<u8>>,
) -> Vec<ValidationCheck> {
    let run = JsonRun {
        label: PathBuf::from("doc.json"),
        source,
        options,
        fetched,
        transfer_syntaxes: &BTreeMap::new(),
        fetch_requested: false,
        fetch_notes: &[],
    };
    validate_json_bytes(document.as_bytes(), &run).checks
}

fn check<'c>(checks: &'c [ValidationCheck], name: &str) -> &'c ValidationCheck {
    checks
        .iter()
        .find(|check| check.name == name && check.path.as_deref() != Some(Path::new("doc.json")))
        .or_else(|| checks.iter().find(|check| check.name == name))
        .unwrap_or_else(|| panic!("no {name} check in {checks:#?}"))
}

/// A minimal Data Set wrapped as a Metadata Array.
fn document(members: &str) -> String {
    format!("[{{{members}}}]")
}

const SOP: &str = r#""00080016":{"vr":"UI","Value":["1.2.840.10008.5.1.4.1.1.7"]},"00080018":{"vr":"UI","Value":["1.2.3.4"]}"#;

#[test]
fn a_conforming_minimal_document_passes_the_json_model() {
    let text = document(&format!(
        r#"{SOP},"00100010":{{"vr":"PN","Value":[{{"Alphabetic":"Doe^Jane","Ideographic":"x"}},null]}},"00200013":{{"vr":"IS","Value":[7,"8"]}},"00280010":{{"vr":"US","Value":[512]}},"00281052":{{"vr":"DS","Value":[-0.5,"1e-3"]}},"00290010":{{"vr":"LO","Value":["ACME"]}},"00291000":{{"vr":"OB","InlineBinary":"AAEC"}},"00400275":{{"vr":"SQ","Value":[{{}}]}},"00660040":{{"vr":"OL"}},"7FE00010":{{"vr":"OW","BulkDataURI":"./bulkdata/7FE00010"}}"#
    ));
    let checks = checks_of(
        &text,
        Source::Metadata,
        &DicomJsonOptions::default(),
        &BTreeMap::new(),
    );
    let model = check(&checks, JSON_MODEL);
    assert_eq!(model.status, ValidationStatus::Passed, "{}", model.message);
}

#[test]
fn each_annex_f_violation_fails_the_json_model_check() {
    for (members, reason) in [
        (
            r#""00080016":{"vr":"UI","Value":["1"]},"00080016":{"vr":"UI","Value":["1"]}"#,
            "F.3.1",
        ),
        (
            r#""00080018":{"vr":"UI","Value":["1"]},"00080016":{"vr":"UI","Value":["1"]}"#,
            "ascending order",
        ),
        (r#""0008001a":{"vr":"UI","Value":["1"]}"#, "lowercase"),
        (r#""0008001":{"vr":"UI","Value":["1"]}"#, "eight character"),
        (r#""00080000":{"vr":"UL","Value":[4]}"#, "Group Length"),
        (r#""00080016":{"Value":["1"]}"#, "no \"vr\""),
        (r#""00080016":{"vr":"XX","Value":["1"]}"#, "Table F.2.3-1"),
        (
            r#""00080016":{"vr":"UI","Value":["1"],"keyword":"SOPClassUID"}"#,
            "not part of an Attribute object",
        ),
        (
            r#""00080016":{"vr":"UI","Value":["1"],"InlineBinary":"AA=="}"#,
            "at most one",
        ),
        (r#""00080016":{"vr":"UI","Value":"1"}"#, "not an Array"),
        (r#""00080016":{"vr":"UI","Value":[]}"#, "F.2.5"),
        (r#""00080016":{"vr":"UI","Value":[null]}"#, "F.2.5"),
        (
            r#""00080008":{"vr":"CS","Value":["A",""]}"#,
            "null, not as an empty String",
        ),
        (
            r#""00280010":{"vr":"US","Value":["512"]}"#,
            "requires a Number",
        ),
        (
            r#""00280010":{"vr":"US","Value":[70000]}"#,
            "outside 0 to 65535",
        ),
        (
            r#""00280010":{"vr":"US","Value":[1,null]}"#,
            "binary VR has no empty Value",
        ),
        (r#""00200013":{"vr":"IS","Value":[1.5]}"#, "not an integer"),
        (
            r#""00209165":{"vr":"AT","Value":["0020000d"]}"#,
            "uppercase hexadecimal",
        ),
        (
            r#""00100010":{"vr":"PN","Value":["Doe^Jane"]}"#,
            "Person Name component groups",
        ),
        (
            r#""00100010":{"vr":"PN","Value":[{"Alphabetic":""}]}"#,
            "only non-empty component groups",
        ),
        (
            r#""00100010":{"vr":"PN","Value":[{"Given":"Jane"}]}"#,
            "is not \"Alphabetic\"",
        ),
        (
            r#""00400275":{"vr":"SQ","Value":[null]}"#,
            "an empty Item an empty object",
        ),
        (
            r#""00291000":{"vr":"OB","Value":[1]}"#,
            "InlineBinary or BulkDataURI",
        ),
        (
            r#""00080060":{"vr":"CS","InlineBinary":"AA=="}"#,
            "encodes only OB",
        ),
        (
            r#""00291000":{"vr":"OB","InlineBinary":"not base64!"}"#,
            "not base64",
        ),
        (
            r#""00291000":{"vr":"OW","InlineBinary":"AA=="}"#,
            "multiple of 2",
        ),
        (r#""00080060":{"vr":"CS","BulkDataURI":"x"}"#, "may have"),
        (
            r#""7FE00010":{"vr":"OB","BulkDataURI":"file:///etc/passwd"}"#,
            "scheme other than",
        ),
        (r#""7FE00010":{"vr":"OB","BulkDataURI":"a b"}"#, "RFC 3986"),
        (
            r#""00291000":{"vr":"OB","InlineBinary":"AA=="}"#,
            "no Private Creator",
        ),
        (r#""00290010":{"vr":"SH","Value":["ACME"]}"#, "is LO"),
        (
            r#""00020010":{"vr":"UI","Value":["1.2.840.10008.1.2.1"]}"#,
            "10.4.1.1.2",
        ),
        (r#""00181050":{"vr":"FL","Value":[1e39]}"#, "binary32"),
        // F.1 and F.2.3: the encoding rules of each VR (PS3.5 6.2).
        (
            r#""00080020":{"vr":"DA","Value":["2026-01-01"]}"#,
            "not of the form YYYYMMDD",
        ),
        (
            r#""00080060":{"vr":"CS","Value":["sm"]}"#,
            "outside the permitted repertoire",
        ),
        (r#""00080018":{"vr":"UI","Value":["1.2.x"]}"#, "not a UID"),
        (
            r#""00081030":{"vr":"LO","Value":["AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"]}"#,
            "maximum; a Value follows the Definition",
        ),
        (
            r#""00200013":{"vr":"IS","Value":["1.5"]}"#,
            "Length of Value of its VR",
        ),
    ] {
        let checks = checks_of(
            &document(members),
            Source::Metadata,
            &DicomJsonOptions::default(),
            &BTreeMap::new(),
        );
        let model = check(&checks, JSON_MODEL);
        assert_eq!(
            model.status,
            ValidationStatus::Failed,
            "{members}: {}",
            model.message
        );
        assert!(
            model.message.contains(reason),
            "{members}: {}",
            model.message
        );
    }
}

#[test]
fn documents_that_are_not_dicom_json_fail_whole() {
    for (text, source, reason) in [
        ("not json", Source::File, "not JSON text"),
        (
            "{\"00080016\":{\"vr\":\"UI\"}}",
            Source::Metadata,
            "single top-level Array",
        ),
        ("[1]", Source::File, "a result is a JSON Number"),
        ("\u{feff}[]", Source::File, "byte order mark"),
    ] {
        let checks = checks_of(text, source, &DicomJsonOptions::default(), &BTreeMap::new());
        let model = checks
            .iter()
            .find(|check| check.name == JSON_MODEL && check.status == ValidationStatus::Failed)
            .unwrap_or_else(|| panic!("{text:?}: {checks:#?}"));
        assert!(
            model.message.contains(reason),
            "{text:?}: {}",
            model.message
        );
    }
    // F.2: the DICOM JSON Model's character repertoire is UTF-8.
    // "Müller" in ISO 8859-1: 0xFC is no UTF-8 sequence.
    let mut latin1 =
        format!("[{{{SOP},\"00100010\":{{\"vr\":\"PN\",\"Value\":[{{\"Alphabetic\":\"M")
            .into_bytes();
    latin1.push(0xFC);
    latin1.extend_from_slice(b"ller\"}]}}]");
    let run = JsonRun {
        label: PathBuf::from("doc.json"),
        source: Source::File,
        options: &DicomJsonOptions::default(),
        fetched: &BTreeMap::new(),
        transfer_syntaxes: &BTreeMap::new(),
        fetch_requested: false,
        fetch_notes: &[],
    };
    let checks = validate_json_bytes(&latin1, &run).checks;
    let model = check(&checks, JSON_MODEL);
    assert_eq!(model.status, ValidationStatus::Failed, "{}", model.message);
    assert!(model.message.contains("not UTF-8"), "{}", model.message);
    // A saved document may hold one Data Set without the Array.
    let lone = format!("{{{SOP}}}");
    let checks = checks_of(
        &lone,
        Source::File,
        &DicomJsonOptions::default(),
        &BTreeMap::new(),
    );
    assert!(checks
        .iter()
        .filter(|check| check.name == JSON_MODEL)
        .all(|check| check.status == ValidationStatus::Passed));
}

/// An exported slide's Metadata: Pixel Data and ICC Profile by BulkDataURI.
fn exported(root: &Path, transfer_syntax: TransferSyntax) -> (String, Vec<u8>) {
    let path = export_wsi(root, "json", (32, 32, 16), transfer_syntax);
    let object = open(&path);
    let icc = icc(&object).expect("the export embeds an ICC Profile");
    let json = to_json(&object, &|tag, top| match tag {
        tags::ICC_PROFILE => Some("bulk/icc".to_string()),
        tags::PIXEL_DATA if top => Some("bulk/pixeldata".to_string()),
        _ => None,
    });
    (json, icc)
}

fn icc(object: &dicom_object::InMemDicomObject) -> Option<Vec<u8>> {
    if let Some(element) = object.get(tags::ICC_PROFILE) {
        return element.to_bytes().ok().map(|bytes| bytes.to_vec());
    }
    object
        .get(tags::OPTICAL_PATH_SEQUENCE)?
        .items()?
        .iter()
        .find_map(icc)
}

fn with_syntax(uid: &str) -> DicomJsonOptions {
    DicomJsonOptions {
        transfer_syntax: Some(uid.to_string()),
        ..DicomJsonOptions::default()
    }
}

#[test]
fn an_exported_slide_metadata_passes_and_reports_what_needs_bytes() {
    let root = tempfile::tempdir().unwrap();
    let (json, icc) = exported(root.path(), TransferSyntax::Jpeg2000Lossless);
    let fetched = BTreeMap::from([("bulk/icc".to_string(), icc)]);
    let checks = checks_of(
        &json,
        Source::Metadata,
        &with_syntax("1.2.840.10008.1.2.4.90"),
        &fetched,
    );
    let failed: Vec<&ValidationCheck> = checks
        .iter()
        .filter(|check| check.status == ValidationStatus::Failed)
        .collect();
    assert!(failed.is_empty(), "{failed:#?}");
    for name in [
        JSON_MODEL,
        "intrinsic-wsi-dicom-json-bulkdata",
        "intrinsic-wsi-dicom-iod-modules",
        "intrinsic-wsi-dicom-iod-attribute-presence",
        "intrinsic-wsi-dicom-iod-vr-vm",
        "intrinsic-wsi-dicom-iod-value-format",
        "intrinsic-wsi-dicom-icc-profile",
        "intrinsic-wsi-dicom-image-metadata",
        "intrinsic-wsi-dicom-tile-geometry",
    ] {
        assert_eq!(
            check(&checks, name).status,
            ValidationStatus::Passed,
            "{name}"
        );
    }
    for (name, needs) in [
        ("intrinsic-pixel-structure", "Pixel Data Value"),
        ("intrinsic-wsi-dicom-codestream", "encoded Frames"),
        ("intrinsic-wsi-dicom-encoding", "PS3.5 encoded Data Set"),
        ("intrinsic-wsi-dicom-file-meta", "File Meta Information"),
        ("intrinsic-wsi-dicom-identity-set", "File Meta Information"),
    ] {
        let found = check(&checks, name);
        assert_eq!(found.status, ValidationStatus::Skipped, "{name}");
        assert!(found.message.contains(needs), "{name}: {}", found.message);
    }
}

#[test]
fn metadata_is_held_to_the_iod_requirements_of_a_file() {
    let root = tempfile::tempdir().unwrap();
    let (json, icc) = exported(root.path(), TransferSyntax::Jpeg2000Lossless);
    let fetched = BTreeMap::from([("bulk/icc".to_string(), icc)]);
    // Image Type (0008,0008) is Type 1 in the VL Whole Slide Microscopy
    // Image Module.
    let start = json.find("\"00080008\":").expect("Image Type");
    let end = json[start..].find("},").expect("its object ends") + start + 2;
    let broken = format!("{}{}", &json[..start], &json[end..]);
    let checks = checks_of(
        &broken,
        Source::Metadata,
        &with_syntax("1.2.840.10008.1.2.4.90"),
        &fetched,
    );
    let presence = check(&checks, "intrinsic-wsi-dicom-iod-attribute-presence");
    assert_eq!(
        presence.status,
        ValidationStatus::Failed,
        "{}",
        presence.message
    );
    assert!(
        presence.message.contains("(0008,0008)"),
        "{}",
        presence.message
    );
}

#[test]
fn without_a_transfer_syntax_syntax_dependent_checks_are_undecidable() {
    let root = tempfile::tempdir().unwrap();
    let (json, icc) = exported(root.path(), TransferSyntax::JpegBaseline8Bit);
    let fetched = BTreeMap::from([("bulk/icc".to_string(), icc)]);
    let image_metadata = |options: &DicomJsonOptions| {
        let checks = checks_of(&json, Source::Metadata, options, &fetched);
        check(&checks, "intrinsic-wsi-dicom-image-metadata").clone()
    };
    let unknown = image_metadata(&DicomJsonOptions::default());
    assert_eq!(
        unknown.status,
        ValidationStatus::Skipped,
        "{}",
        unknown.message
    );
    assert!(
        unknown.message.contains("undecidable"),
        "{}",
        unknown.message
    );
    assert!(
        unknown.message.contains("1.2.840.10008.1.2.4.50"),
        "{}",
        unknown.message
    );
    let baseline = image_metadata(&with_syntax("1.2.840.10008.1.2.4.50"));
    assert_eq!(
        baseline.status,
        ValidationStatus::Passed,
        "{}",
        baseline.message
    );
    let native = image_metadata(&with_syntax("1.2.840.10008.1.2.1"));
    assert_eq!(
        native.status,
        ValidationStatus::Failed,
        "{}",
        native.message
    );
}

#[test]
fn checks_reading_an_unfetched_bulkdata_value_are_skipped_until_it_is_fetched() {
    let root = tempfile::tempdir().unwrap();
    let (json, icc) = exported(root.path(), TransferSyntax::Jpeg2000Lossless);
    let options = with_syntax("1.2.840.10008.1.2.4.90");
    let unfetched = checks_of(&json, Source::Metadata, &options, &BTreeMap::new());
    let bulk = check(&unfetched, "intrinsic-wsi-dicom-json-bulkdata");
    assert_eq!(bulk.status, ValidationStatus::Skipped);
    assert!(
        bulk.message.contains("--fetch-bulkdata"),
        "{}",
        bulk.message
    );
    let icc_check = check(&unfetched, "intrinsic-wsi-dicom-icc-profile");
    assert_eq!(
        icc_check.status,
        ValidationStatus::Skipped,
        "{}",
        icc_check.message
    );
    assert!(
        icc_check.message.contains("BulkDataURI"),
        "{}",
        icc_check.message
    );

    let fetched = BTreeMap::from([("bulk/icc".to_string(), icc)]);
    let checked = checks_of(&json, Source::Metadata, &options, &fetched);
    assert_eq!(
        check(&checked, "intrinsic-wsi-dicom-icc-profile").status,
        ValidationStatus::Passed
    );
    // A fetched ICC Profile that is not one fails as in a file.
    let garbage = BTreeMap::from([("bulk/icc".to_string(), vec![7; 200])]);
    let broken = checks_of(&json, Source::Metadata, &options, &garbage);
    assert_eq!(
        check(&broken, "intrinsic-wsi-dicom-icc-profile").status,
        ValidationStatus::Failed
    );
}

#[test]
fn a_saved_document_validates_through_the_path_entry_point() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("metadata.json");
    std::fs::write(&path, document(SOP)).unwrap();
    let report = validate_dicom_json_path(&path, &DicomJsonOptions::default()).unwrap();
    assert_eq!(report.files, vec![path.clone()]);
    assert!(report
        .checks
        .iter()
        .any(|check| check.name == JSON_MODEL && check.status == ValidationStatus::Passed));
    let mut small = DicomJsonOptions::default();
    small.validation.max_input_bytes = 8;
    assert!(validate_dicom_json_path(&path, &small)
        .unwrap_err()
        .to_string()
        .contains("max_input_bytes=8"));
}

#[test]
fn search_results_read_with_their_model_violations() {
    let (objects, violations) = super::read_json_data_sets(
        document(r#""00080016":{"vr":"UI","Value":["1"]},"00080016":{"vr":"UI","Value":["1"]}"#)
            .as_bytes(),
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(objects.len(), 1);
    assert!(violations.unwrap().contains("F.3.1"));
    assert!(super::read_json_data_sets(b"7", &BTreeMap::new()).is_err());
}

/// A minimal Data Set of SOP Class `class`, SOP Instance
/// 1.2.826.0.1.3680043.10.999.81 and Patient ID `patient`.
fn instance(class: &str, patient: &str) -> String {
    format!(
        r#"{{"00080016":{{"vr":"UI","Value":["{class}"]}},"00080018":{{"vr":"UI","Value":["1.2.826.0.1.3680043.10.999.81"]}},"00100020":{{"vr":"LO","Value":["{patient}"]}}}}"#
    )
}

fn set_checks_of(document: &str) -> Vec<ValidationCheck> {
    checks_of(
        document,
        Source::Metadata,
        &DicomJsonOptions::default(),
        &BTreeMap::new(),
    )
}

fn identity_set(checks: &[ValidationCheck]) -> &ValidationCheck {
    checks
        .iter()
        .find(|check| check.name == "intrinsic-wsi-dicom-identity-set" && check.path.is_none())
        .unwrap_or_else(|| panic!("no identity set check in {checks:#?}"))
}

#[test]
fn the_shared_set_checks_compare_the_data_sets_of_a_document() {
    // Parametric Map Storage, an IOD the set checks compare.
    let class = "1.2.840.10008.5.1.4.1.1.30";
    let identical = format!("[{},{}]", instance(class, "P1"), instance(class, "P1"));
    let check = identity_set(&set_checks_of(&identical)).clone();
    assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);
    assert!(
        check.message.contains("identical copies"),
        "{}",
        check.message
    );

    let coerced = format!("[{},{}]", instance(class, "P1"), instance(class, "P2"));
    let check = identity_set(&set_checks_of(&coerced)).clone();
    assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);
    assert!(
        check.message.contains("advisory") && check.message.contains("PatientID (0010,0020)"),
        "{}",
        check.message
    );

    let other_class = format!(
        "[{},{}]",
        instance(class, "P1"),
        instance("1.2.840.10008.5.1.4.1.1.66.4", "P1")
    );
    let check = identity_set(&set_checks_of(&other_class)).clone();
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
    assert!(
        check.message.contains("they are of SOP Classes"),
        "{}",
        check.message
    );
}

#[test]
fn only_data_sets_sharing_a_sop_instance_uid_are_digested() {
    use super::Origin;
    let uid = |value: &str| format!(r#"{{"00080018":{{"vr":"UI","Value":["{value}"]}}}}"#);
    let document = format!(
        "[{},{},{},{},{}]",
        uid("1.2.3"),
        uid("1.2.4"),
        uid("1.2.3"),
        // Two SOP Instance UID members: not plainly given.
        r#"{"00080018":{"vr":"UI","Value":["1.2.5"]},"00080018":{"vr":"UI","Value":["1.2.6"]}}"#,
        r#"{"00080016":{"vr":"UI","Value":["1.2.840.10008.5.1.4.1.1.30"]}}"#,
    );
    let parsed = super::json::parse(&document, super::json_depth()).unwrap();
    let data_sets = super::data_sets(&parsed);
    let repeated = super::repeated_instance_uids(&data_sets);
    assert_eq!(repeated.into_iter().collect::<Vec<_>>(), ["1.2.3"]);
    let repeated = super::repeated_instance_uids(&data_sets);
    let origins: Vec<Origin> = data_sets
        .iter()
        .map(|(_, members)| super::copy_origin(members, &repeated))
        .collect();
    assert_eq!(
        origins,
        [
            Origin::Label,
            Origin::UniqueLabel,
            Origin::Label,
            Origin::Label,
            Origin::Label
        ]
    );
}

#[test]
fn no_check_reads_the_document_label_as_a_file() {
    let root = tempfile::tempdir().unwrap();
    // A label naming a file on disk that is not DICOM: a check that read it
    // would fail on its bytes.
    let label = root.path().join("label.json");
    std::fs::write(&label, b"not a DICOM file").unwrap();
    let options = DicomJsonOptions::default();
    let empty = BTreeMap::new();
    let run = JsonRun {
        label: label.clone(),
        source: Source::Metadata,
        options: &options,
        fetched: &empty,
        transfer_syntaxes: &BTreeMap::new(),
        fetch_requested: false,
        fetch_notes: &[],
    };
    let document = format!("[{}]", instance("1.2.840.10008.5.1.4.1.1.30", "P1"));
    let checks = validate_json_bytes(document.as_bytes(), &run).checks;
    for (name, needs) in [
        ("intrinsic-wsi-dicom-file-meta", "Metadata does not include"),
        ("intrinsic-wsi-dicom-identity-set", "File Meta Information"),
        ("intrinsic-wsi-dicom-codestream", "encoded Frames"),
    ] {
        let found = checks
            .iter()
            .find(|check| check.name == name && check.path.is_some())
            .unwrap_or_else(|| panic!("no {name} check in {checks:#?}"));
        assert_eq!(
            found.status,
            ValidationStatus::Skipped,
            "{name}: {}",
            found.message
        );
        assert!(found.message.contains(needs), "{name}: {}", found.message);
    }
    for check in &checks {
        assert!(
            !check.message.contains("not a DICOM file") && !check.message.contains("cannot open"),
            "{} read the label: {}",
            check.name,
            check.message
        );
    }
}

/// The JSON Model check and the checks of the Data Set read from a
/// document name each defect of an identifying Value without the Value
/// (PS3.15 Table E.1-1): a Person Name and a Patient ID with a control
/// character, and the out-of-range Floating Point Value of a Subject Age
/// Content Item, whose Concept Table E.3.4-1 removes.
#[test]
fn json_messages_withhold_identifying_values() {
    let content = r#""0040A730":{"vr":"SQ","Value":[{"0040A040":{"vr":"CS","Value":["NUM"]},"0040A043":{"vr":"SQ","Value":[{"00080100":{"vr":"SH","Value":["121033"]},"00080102":{"vr":"SH","Value":["DCM"]},"00080104":{"vr":"LO","Value":["Subject Age"]}}]},"0040A300":{"vr":"SQ","Value":[{"0040A161":{"vr":"FD","Value":[8675309e999]}}]}}]}"#;
    let text = document(&format!(
        r#"{SOP},"00100010":{{"vr":"PN","Value":[{{"Alphabetic":"DOE^JANE\u0007"}}]}},"00100020":{{"vr":"LO","Value":["MRN12345\u0007"]}},{content}"#
    ));
    let checks = checks_of(
        &text,
        Source::Metadata,
        &DicomJsonOptions::default(),
        &BTreeMap::new(),
    );
    let model = check(&checks, JSON_MODEL);
    for expected in [
        "/0/00100010/Value/0: PN Value (value withheld: identifying; 9 characters): character 9 (control character U+0007) outside the permitted repertoire",
        "/0/00100020/Value/0: LO Value (value withheld: identifying; 9 characters): character 9 (control character U+0007)",
        "/0040A161/Value/0: FD Value (value withheld: identifying; 11 characters) is outside the range of an IEEE 754 binary64",
    ] {
        assert!(model.message.contains(expected), "lacks {expected:?}: {}", model.message);
    }
    for check in &checks {
        for planted in ["DOE^JANE", "MRN12345", "8675309"] {
            assert!(
                !check.message.contains(planted),
                "{planted} in {}: {}",
                check.name,
                check.message
            );
        }
    }
}
