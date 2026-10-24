//! PS3.3 C.7.4.2: the Synchronization Module check of the Parametric Map and
//! the Enhanced, Comprehensive and Comprehensive 3D SR documents, per object
//! and over the set.

use super::*;
use crate::validation::image_semantics::{run_family_set_checks, FamilyCorpus};
use crate::validation::tests::iod::structured_report::write_documents;

/// The SR fixture documents whose IODs include the Synchronization Module.
const SYNCHRONIZED_DOCUMENTS: [&str; 3] =
    ["enhanced-sr", "comprehensive-sr", "comprehensive-3d-sr"];
/// The KOS and Basic Text SR, whose IODs do not.
const UNSYNCHRONIZED_DOCUMENTS: [&str; 2] = ["kos", "basic-text-sr"];
const SERIES: &str = "1.2.826.0.1.3680043.10.999.7.94";
const SYNCHRONIZATION_FRAME: &str = "1.2.840.10008.15.1.1";

/// The SR fixture document `name`, written in `dir`, after `edit`.
fn document(dir: &Path, name: &str, edit: impl FnOnce(&mut DefaultDicomObject)) -> PathBuf {
    write_documents(dir);
    let path = dir.join(format!("{name}.dcm"));
    let mut file = dicom_object::open_file(&path).expect("SR fixture");
    edit(&mut file);
    file.write_to_file(&path).expect("write SR fixture");
    path
}

/// The per-object Synchronization check of the file at `path`, if it ran.
fn synchronization_check(path: &Path) -> Option<ValidationCheck> {
    iod_report(path, ValidationProfile::General)
        .checks
        .into_iter()
        .find(|check| check.name == SYNCHRONIZATION && check.path.is_some())
}

#[test]
fn the_ntp_source_address_of_a_parametric_map_is_an_ip_address() {
    for address in ["192.168.1.1", "12:34:56:78:9a:bc:de:f0", "fe80::1"] {
        assert_family_passes("parametric-map", SYNCHRONIZATION, move |object| {
            object.put(text(tags::NTP_SOURCE_ADDRESS, VR::LO, address));
        });
    }
    for address in ["ntp.example.org", "192.168.1", "12:34:zz::1"] {
        assert_family_fails(
            "parametric-map",
            SYNCHRONIZATION,
            "is neither an IPv4 address in dotted decimal",
            move |object| {
                object.put(text(tags::NTP_SOURCE_ADDRESS, VR::LO, address));
            },
        );
    }
}

#[test]
fn an_sr_document_with_the_synchronization_module_holds_its_ntp_source_address() {
    let tmp = tempfile::tempdir().unwrap();
    for name in SYNCHRONIZED_DOCUMENTS {
        let valid = document(tmp.path(), name, |file| {
            file.put(text(tags::NTP_SOURCE_ADDRESS, VR::LO, "192.168.1.1"));
        });
        let check = synchronization_check(&valid).expect("the check runs on the document");
        assert_eq!(
            check.status,
            ValidationStatus::Passed,
            "{name}: {}",
            check.message
        );
        let malformed = document(tmp.path(), name, |file| {
            file.put(text(tags::NTP_SOURCE_ADDRESS, VR::LO, "ntp.example.org"));
        });
        let check = synchronization_check(&malformed).expect("the check runs on the document");
        assert_eq!(
            check.status,
            ValidationStatus::Failed,
            "{name}: {}",
            check.message
        );
        assert!(
            check.message.contains(
                "NTP Source Address (0018,1803) \"ntp.example.org\" is neither an IPv4 address in dotted decimal"
            ),
            "{name}: {}",
            check.message
        );
    }
    for name in UNSYNCHRONIZED_DOCUMENTS {
        let path = document(tmp.path(), name, |_| {});
        assert!(
            synchronization_check(&path).is_none(),
            "the Synchronization check ran on {name}"
        );
    }
}

/// The set checks of the files at `paths`.
fn set_checks(paths: &[PathBuf]) -> Vec<ValidationCheck> {
    let mut corpus = FamilyCorpus::default();
    for path in paths {
        corpus.observe(path, &dicom_object::open_file(path).expect("object"));
    }
    run_family_set_checks(&corpus)
}

/// Puts the object in the synchronized Series `SERIES`.
fn synchronize(object: &mut InMemDicomObject) {
    object.put(text(
        tags::SYNCHRONIZATION_FRAME_OF_REFERENCE_UID,
        VR::UI,
        SYNCHRONIZATION_FRAME,
    ));
    object.put(text(tags::SERIES_INSTANCE_UID, VR::UI, SERIES));
}

#[test]
fn a_synchronized_series_of_a_parametric_map_and_an_sr_document_shares_one_frame_of_reference() {
    let tmp = tempfile::tempdir().unwrap();
    let map = write(tmp.path(), "map", fixture("parametric-map"), synchronize);
    for (name, frame) in [
        ("comprehensive-sr", None),
        ("enhanced-sr", Some("1.2.826.0.1.3680043.10.999.7.61")),
    ] {
        let report = document(tmp.path(), name, |file| {
            synchronize(file);
            if let Some(uid) = frame {
                file.put(text(tags::FRAME_OF_REFERENCE_UID, VR::UI, uid));
            }
        });
        let checks = set_checks(&[map.clone(), report.clone()]);
        let check = checks
            .iter()
            .find(|check| check.name == SYNCHRONIZATION)
            .unwrap_or_else(|| panic!("no Synchronization set check: {checks:?}"));
        assert_eq!(check.path, None, "a set check names no file");
        match frame {
            // The parametric map fixture is in Frame of Reference 7.61.
            Some(_) => assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message),
            None => {
                assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
                assert!(
                    check.message.contains(&format!(
                        "{} has a Synchronization Frame of Reference UID and Frame of Reference UID 1.2.826.0.1.3680043.10.999.7.61, but 1 other Instance(s) of its Series have another, {} with absent",
                        map.display(),
                        report.display()
                    )),
                    "{}",
                    check.message
                );
            }
        }
    }
}
