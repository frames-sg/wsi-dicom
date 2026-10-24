//! The Digital Signatures rule on exported files, through the validation
//! entry point: files signed in-process, and files signed by dcmsign with a
//! throwaway key, which pin the MAC byte stream to DCMTK's.

use std::process::Command;

use super::*;
use crate::test_support::find_command_for_test;
use crate::validation::digital_signatures::tests::{first_signature_stream, sign};

const DIGITAL_SIGNATURES_RULE: &str = "intrinsic-wsi-dicom-digital-signatures";
const RELATIONS_RULE: &str = "intrinsic-wsi-dicom-attribute-relations";
const DOES_NOT_VERIFY: &str =
    "Digital Signatures Sequence (FFFA,FFFA)[1]: Signature (0400,0120) does not verify";

fn report(path: &Path, profile: ValidationProfile) -> super::super::ValidationReport {
    validate_dicom_path_with_runner(
        path,
        &ValidationOptions {
            profile,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .expect("validation report")
}

/// The Tags of the Data Set's elements a MAC may include.
fn signable(object: &InMemDicomObject) -> Vec<Tag> {
    object
        .iter()
        .map(|element| element.header().tag)
        .filter(|tag| tag.group() >= 0x0008)
        .collect()
}

/// An exported instance signed in-process over every Data Element.
fn signed_export(root: &Path, name: &str) -> PathBuf {
    edited(
        export_valid_color_wsi_for_validation(root, name),
        |object| {
            let syntax = object
                .meta()
                .transfer_syntax()
                .trim_end_matches('\0')
                .to_string();
            let tags = signable(object);
            sign(object, &syntax, &tags);
        },
    )
}

#[test]
fn an_unsigned_export_passes_with_nothing_to_verify() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_for_validation(tmp.path(), "unsigned");
    let (status, message) = rule_result(&path, DIGITAL_SIGNATURES_RULE);
    assert_eq!(status, ValidationStatus::Passed, "{message}");
    assert!(message.ends_with("nothing to verify"), "{message}");
}

#[test]
fn a_signed_export_passes_every_rule_in_both_rule_sets() {
    let tmp = tempfile::tempdir().unwrap();
    let path = signed_export(tmp.path(), "signed");
    for profile in [ValidationProfile::General, ValidationProfile::Core] {
        let report = report(&path, profile);
        let check = report
            .checks
            .iter()
            .find(|check| check.name == DIGITAL_SIGNATURES_RULE)
            .expect("a digital signatures check");
        assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);
        assert!(
            check.message.ends_with("1 Digital Signature verified"),
            "{}",
            check.message
        );
        let failed: Vec<_> = report
            .checks
            .iter()
            .filter(|check| check.status == ValidationStatus::Failed)
            .collect();
        assert!(failed.is_empty(), "{profile:?}: {failed:#?}");
    }
}

#[test]
fn a_changed_signed_attribute_fails_the_rewritten_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited(signed_export(tmp.path(), "changed"), |object| {
        object.put(text(tags::PATIENT_ID, VR::LO, "CHANGED"));
    });
    let (status, message) = rule_result(&path, DIGITAL_SIGNATURES_RULE);
    assert_eq!(status, ValidationStatus::Failed, "{message}");
    assert!(message.contains(DOES_NOT_VERIFY), "{message}");
}

/// A change to a Data Elements Signed list.
type TagsEdit = fn(&mut Vec<Tag>);

/// Data Elements Signed order and exclusions are the attribute-relations
/// rule's (C.12.1.1.3.1.1); a MAC leaves excluded Tags out and follows Data
/// Set order, so the signature still verifies.
#[test]
fn unsorted_or_excluded_data_elements_signed_fail_attribute_relations_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let cases: [(&str, TagsEdit, &str); 2] = [
        (
            "unsorted",
            |signed| signed.reverse(),
            "does not list its Tags in the order they appear in the Data Set (PS3.3 2026d C.12.1.1.3.1.1)",
        ),
        (
            "excluded",
            |signed| signed.insert(0, tags::TRANSFER_SYNTAX_UID),
            "lists TransferSyntaxUID (0002,0010), but a Tag with a group below 0008 shall not be signed (PS3.3 2026d C.12.1.1.3.1.1)",
        ),
    ];
    for (name, edit, expected) in cases {
        let path = edited(signed_export(tmp.path(), name), |object| {
            first_item(object, tags::MAC_PARAMETERS_SEQUENCE, |parameters| {
                let mut signed = match parameters.get(tags::DATA_ELEMENTS_SIGNED).unwrap().value() {
                    dicom_core::value::Value::Primitive(PrimitiveValue::Tags(values)) => {
                        values.to_vec()
                    }
                    other => panic!("Data Elements Signed {other:?}"),
                };
                edit(&mut signed);
                parameters.put(DataElement::new(
                    tags::DATA_ELEMENTS_SIGNED,
                    VR::AT,
                    PrimitiveValue::Tags(signed.into_iter().collect()),
                ));
            });
        });
        let (status, message) = rule_result(&path, RELATIONS_RULE);
        assert_eq!(status, ValidationStatus::Failed, "{name}: {message}");
        assert!(message.contains(expected), "{name}: {message}");
        let (status, message) = rule_result(&path, DIGITAL_SIGNATURES_RULE);
        assert_eq!(status, ValidationStatus::Passed, "{name}: {message}");
    }
}

/// Runs `command`, panicking with its output unless it succeeds.
fn succeed(command: &mut Command) {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("cannot run {command:?}: {error}"));
    assert!(
        output.status.success(),
        "{command:?} failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// dcmsign and openssl, or why the interoperability tests are skipped.
fn dcmtk_tools() -> Option<(String, String)> {
    let tools = find_command_for_test("dcmsign").zip(find_command_for_test("openssl"));
    if tools.is_none() {
        eprintln!("skipping dcmsign interoperability: dcmsign or openssl not found");
    }
    tools
}

/// A throwaway RSA key and self-signed certificate for one test run.
fn throwaway_key(openssl: &str, dir: &Path, name: &str) -> (PathBuf, PathBuf) {
    let key = dir.join(format!("{name}-key.pem"));
    let certificate = dir.join(format!("{name}-certificate.pem"));
    succeed(
        Command::new(openssl)
            .args([
                "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
            ])
            .arg("-subj")
            .arg(format!("/CN=wsi-dicom {name} (test only)"))
            .arg("-keyout")
            .arg(&key)
            .arg("-out")
            .arg(&certificate),
    );
    (key, certificate)
}

#[test]
fn dcmsign_signatures_verify_byte_for_byte_and_fail_once_changed() {
    let Some((dcmsign, openssl)) = dcmtk_tools() else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let fixture = export_valid_color_wsi_for_validation(tmp.path(), "interop");
    let (key, certificate) = throwaway_key(&openssl, tmp.path(), "dcmsign-signer");
    // RIPEMD160 (dcmsign's default) over every Data Element, then SHA512 over
    // a UID, a Sequence and the encapsulated Pixel Data.
    let runs: [(&str, &[&str]); 2] = [
        ("profile-none", &["--profile-none"]),
        (
            "pixel-and-sequence",
            &[
                "--profile-none",
                "--mac-sha512",
                "--tag",
                "0008,0018",
                "--tag",
                "0040,0560",
                "--tag",
                "7fe0,0010",
            ],
        ),
    ];
    for (name, options) in runs {
        let signed = tmp.path().join(format!("{name}.dcm"));
        let dump = tmp.path().join(format!("{name}.mac"));
        succeed(
            Command::new(&dcmsign)
                .arg("--sign")
                .args([&key, &certificate])
                .args(["--null-passwd", "--dump"])
                .arg(&dump)
                .args(options)
                .args([&fixture, &signed]),
        );

        let object = dicom_object::open_file(&signed).unwrap();
        assert!(
            first_signature_stream(&object) == std::fs::read(&dump).unwrap(),
            "{name}: the MAC byte stream differs from the one dcmsign hashed"
        );
        let (status, message) = rule_result(&signed, DIGITAL_SIGNATURES_RULE);
        assert_eq!(status, ValidationStatus::Passed, "{name}: {message}");
        assert!(
            message.ends_with("1 Digital Signature verified"),
            "{name}: {message}"
        );

        // One signed Attribute, inside a signed Sequence, changes.
        let changed = tmp.path().join(format!("{name}-changed.dcm"));
        std::fs::copy(&signed, &changed).unwrap();
        let changed = edited(changed, |object| {
            first_item(object, tags::SPECIMEN_DESCRIPTION_SEQUENCE, |specimen| {
                specimen.put(text(tags::SPECIMEN_IDENTIFIER, VR::LO, "CHANGED"));
            });
        });
        let (status, message) = rule_result(&changed, DIGITAL_SIGNATURES_RULE);
        assert_eq!(status, ValidationStatus::Failed, "{name}: {message}");
        assert!(message.contains(DOES_NOT_VERIFY), "{name}: {message}");
    }
}

/// An openssl TSA configuration for `openssl ts -reply`.
fn tsa_configuration(dir: &Path, key: &Path, certificate: &Path) -> PathBuf {
    let configuration = dir.join("tsa.cnf");
    std::fs::write(dir.join("tsa-serial"), "01\n").unwrap();
    std::fs::write(
        &configuration,
        format!(
            "[ tsa ]\ndefault_tsa = test_tsa\n[ test_tsa ]\nserial = {}\nsigner_cert = {}\nsigner_key = {}\nsigner_digest = sha256\ndefault_policy = 1.2.826.0.1.3680043.10.999.2\nother_policies = 1.2.826.0.1.3680043.10.999.3\ndigests = sha256, sha384, sha512\naccuracy = secs:1\ness_cert_id_alg = sha256\n",
            dir.join("tsa-serial").display(),
            certificate.display(),
            key.display()
        ),
    )
    .unwrap();
    configuration
}

#[test]
fn dcmsign_certified_timestamps_verify_and_fail_for_another_signature() {
    let Some((dcmsign, openssl)) = dcmtk_tools() else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let fixture = export_valid_color_wsi_for_validation(dir, "timestamped");
    let (key, certificate) = throwaway_key(&openssl, dir, "dcmsign-signer");
    let tsa_key = dir.join("tsa-key.pem");
    let tsa_certificate = dir.join("tsa-certificate.pem");
    let tsa = Command::new(&openssl)
        .args([
            "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
        ])
        .args(["-subj", "/CN=wsi-dicom test TSA (test only)"])
        .args(["-addext", "extendedKeyUsage=critical,timeStamping"])
        .arg("-keyout")
        .arg(&tsa_key)
        .arg("-out")
        .arg(&tsa_certificate)
        .output()
        .unwrap();
    if !tsa.status.success() {
        eprintln!(
            "skipping the Certified Timestamp interoperability: {openssl} cannot make a TSA certificate: {}",
            String::from_utf8_lossy(&tsa.stderr)
        );
        return;
    }
    let configuration = tsa_configuration(dir, &tsa_key, &tsa_certificate);
    let (query, uid, pending, reply, signed) = (
        dir.join("query.tsq"),
        dir.join("uid"),
        dir.join("pending.dcm"),
        dir.join("reply.tsr"),
        dir.join("timestamped.dcm"),
    );
    succeed(
        Command::new(&dcmsign)
            .arg("--sign")
            .args([&key, &certificate])
            .args(["--null-passwd", "--mac-sha256", "--timestamp-file"])
            .args([&query, &uid, &fixture, &pending]),
    );
    succeed(
        Command::new(&openssl)
            .args(["ts", "-reply", "-config"])
            .arg(&configuration)
            .arg("-queryfile")
            .arg(&query)
            .arg("-out")
            .arg(&reply),
    );
    succeed(
        Command::new(&dcmsign)
            .arg("--insert-timestamp")
            .args([&query, &reply, &uid, &pending, &signed]),
    );
    let (status, message) = rule_result(&signed, DIGITAL_SIGNATURES_RULE);
    assert_eq!(status, ValidationStatus::Passed, "{message}");
    assert!(
        message.ends_with("1 Digital Signature verified, 1 with a verified Certified Timestamp"),
        "{message}"
    );

    // The token of another Signature: one bit of its messageImprint differs.
    let other = edited(signed, |object| {
        first_item(object, tags::DIGITAL_SIGNATURES_SEQUENCE, |item| {
            let signature = item
                .get(tags::SIGNATURE)
                .unwrap()
                .to_bytes()
                .unwrap()
                .into_owned();
            let digest = <sha2::Sha256 as sha2::Digest>::digest(&signature);
            let mut token = item
                .get(tags::CERTIFIED_TIMESTAMP)
                .unwrap()
                .to_bytes()
                .unwrap()
                .into_owned();
            let at = token
                .windows(digest.len())
                .position(|window| window == digest.as_slice())
                .expect("the messageImprint in the token");
            token[at] ^= 1;
            item.put(DataElement::new(
                tags::CERTIFIED_TIMESTAMP,
                VR::OB,
                PrimitiveValue::from(token),
            ));
        });
    });
    let (status, message) = rule_result(&other, DIGITAL_SIGNATURES_RULE);
    assert_eq!(status, ValidationStatus::Failed, "{message}");
    assert!(
        message.contains(
            "Certified Timestamp (0400,0310) messageImprint is not the SHA256 hash of Signature (0400,0120)"
        ),
        "{message}"
    );
}
