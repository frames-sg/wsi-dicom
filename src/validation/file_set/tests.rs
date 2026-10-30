//! File-set check tests. Each File-set is built in a temporary directory
//! from Secondary Capture objects written here, its DICOMDIR made by dcmtk
//! `dcmmkdir` (General Purpose profile), and then one requirement broken.

use std::path::{Path, PathBuf};
use std::process::Command;

use dicom_core::value::DataSetSequence;
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::mem::InMemElement;
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};

use super::consistency::check_record_class;
use super::file_id::{defects, resolve, Unresolved};
use super::icons::check_icons;
use super::records::{self, parse};
use super::run_file_set_check;
use super::structure::check_structure;
use crate::test_support::find_command_for_test;
use crate::validation::discovery::discover_dicom_files;
use crate::validation::iod::Family;
use crate::validation::{ValidationCheck, ValidationOptions, ValidationStatus};

const SECONDARY_CAPTURE: &str = "1.2.840.10008.5.1.4.1.1.7";
const STUDY_UID: &str = "1.2.826.0.1.3680043.10.999.70.1";
const SERIES_UID: &str = "1.2.826.0.1.3680043.10.999.70.2";
const FIRST_ROOT_RECORD: Tag = Tag(0x0004, 0x1200);
const LAST_ROOT_RECORD: Tag = Tag(0x0004, 0x1202);
const NEXT_RECORD: Tag = Tag(0x0004, 0x1400);
const LOWER_ENTITY: Tag = Tag(0x0004, 0x1420);

/// One Secondary Capture object of a fixture File-set.
#[derive(Clone)]
struct Instance {
    name: &'static str,
    sop_class: &'static str,
    sop_instance: &'static str,
    study: &'static str,
    series: &'static str,
    patient_name: &'static str,
    patient_id: &'static str,
    modality: &'static str,
    instance_number: &'static str,
    /// Specific Character Set (0008,0005), or "" for none.
    character_set: &'static str,
}

const FIRST: Instance = Instance {
    name: "SC1",
    sop_class: SECONDARY_CAPTURE,
    sop_instance: "1.2.826.0.1.3680043.10.999.70.3",
    study: STUDY_UID,
    series: SERIES_UID,
    patient_name: "FILE^SET",
    patient_id: "PAT1",
    modality: "OT",
    instance_number: "1",
    character_set: "",
};
const SECOND: Instance = Instance {
    name: "SC2",
    sop_class: SECONDARY_CAPTURE,
    sop_instance: "1.2.826.0.1.3680043.10.999.70.4",
    study: STUDY_UID,
    series: SERIES_UID,
    patient_name: "FILE^SET",
    patient_id: "PAT1",
    modality: "OT",
    instance_number: "2",
    character_set: "",
};

/// A File-set root `ROOT` inside a temporary directory, which also holds
/// files outside the File-set.
struct FileSet {
    dir: tempfile::TempDir,
    root: PathBuf,
}

impl FileSet {
    /// SC1 and SC2 in the directory AB, with a DICOMDIR made by dcmmkdir.
    fn new() -> Self {
        Self::with(&[FIRST, SECOND])
    }

    /// `instances` in the directory AB, with a DICOMDIR made by dcmmkdir.
    fn with(instances: &[Instance]) -> Self {
        Self::build(instances, &[])
    }

    /// `instances` in the directory AB, with a DICOMDIR made by dcmmkdir
    /// given `options` as well.
    fn build(instances: &[Instance], options: &[&str]) -> Self {
        let names: Vec<&str> = instances.iter().map(|instance| instance.name).collect();
        Self::create(&names, options, |file_set| {
            for instance in instances {
                write_instance(
                    &file_set.member(instance.name),
                    instance,
                    uids::EXPLICIT_VR_LITTLE_ENDIAN,
                );
            }
        })
    }

    /// The files `write` writes to AB/`names`, with a DICOMDIR made by
    /// dcmmkdir given `options` as well.
    fn create(names: &[&str], options: &[&str], write: impl FnOnce(&Self)) -> Self {
        let dir = tempfile::tempdir().expect("temporary directory");
        let root = dir.path().join("ROOT");
        std::fs::create_dir_all(root.join("AB")).expect("File-set directory");
        let file_set = Self { dir, root };
        write(&file_set);
        let output = Command::new("dcmmkdir")
            .arg("-Pgp")
            .args(options)
            .arg("+D")
            .arg(file_set.dicomdir())
            .arg("+id")
            .arg(&file_set.root)
            .args(names.iter().map(|name| format!("AB/{name}")))
            .output()
            .expect("these tests need dcmtk dcmmkdir on PATH");
        assert!(
            output.status.success(),
            "dcmmkdir failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        file_set
    }

    fn member(&self, name: &str) -> PathBuf {
        self.root.join("AB").join(name)
    }

    fn dicomdir(&self) -> PathBuf {
        self.root.join("DICOMDIR")
    }

    /// The File-set check of the File-set as discovery finds it.
    fn check(&self) -> ValidationCheck {
        self.check_with(&ValidationOptions::default())
    }

    fn check_with(&self, options: &ValidationOptions) -> ValidationCheck {
        let mut checks = discovered_checks(&self.root, options);
        assert_eq!(checks.len(), 1, "one File-set: {checks:?}");
        checks.remove(0)
    }

    /// The File-set check given the root and `files` without discovery,
    /// which refuses every symbolic link before the check would see one.
    fn check_undiscovered(&self, files: &[PathBuf]) -> ValidationCheck {
        let mut checks = run_file_set_check(
            std::slice::from_ref(&self.root),
            files,
            &ValidationOptions::default(),
        );
        assert_eq!(checks.len(), 1, "one File-set: {checks:?}");
        checks.remove(0)
    }

    fn directory(&self) -> records::Directory {
        parse(&std::fs::read(self.dicomdir()).expect("DICOMDIR")).expect("parsed DICOMDIR")
    }

    /// Replaces the one occurrence of `from` at or after byte `start` of
    /// the DICOMDIR with `to`, of the same length.
    fn patch(&self, start: usize, from: &[u8], to: &[u8]) {
        assert_eq!(from.len(), to.len(), "a patch keeps every offset");
        let path = self.dicomdir();
        let mut bytes = std::fs::read(&path).expect("DICOMDIR");
        let at = bytes[start..]
            .windows(from.len())
            .position(|window| window == from)
            .map(|position| start + position)
            .expect("bytes to patch");
        bytes[at..at + to.len()].copy_from_slice(to);
        std::fs::write(&path, bytes).expect("patched DICOMDIR");
    }

    /// Sets the UL offset `tag` holds at or after byte `start` to `value`.
    fn set_offset(&self, start: usize, tag: Tag, old: u32, value: u32) {
        self.patch(start, &ul(tag, old), &ul(tag, value));
    }

    /// The index of the first record of `record_type`.
    fn record(&self, record_type: &str) -> records::Record {
        self.directory()
            .records
            .into_iter()
            .find(|record| record.record_type() == record_type)
            .expect("record of that type")
    }
}

/// The File-set checks of the File-sets discovery finds under `input`.
fn discovered_checks(input: &Path, options: &ValidationOptions) -> Vec<ValidationCheck> {
    let discovered = discover_dicom_files(input, options).expect("discovered DICOM files");
    run_file_set_check(&discovered.file_set_roots, &discovered.files, options)
}

fn ul(tag: Tag, value: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&tag.group().to_le_bytes());
    bytes.extend_from_slice(&tag.element().to_le_bytes());
    bytes.extend_from_slice(b"UL\x04\x00");
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

fn offset(record: &records::Record, tag: Tag) -> u32 {
    record
        .get(tag)
        .and_then(records::Value::unsigned)
        .expect("offset")
}

fn put(object: &mut InMemDicomObject, tag: Tag, vr: VR, value: &str) {
    object.put(DataElement::new(tag, vr, PrimitiveValue::from(value)));
}

fn write_instance(path: &Path, instance: &Instance, transfer_syntax: &str) {
    let mut object = InMemDicomObject::new_empty();
    if !instance.character_set.is_empty() {
        put(
            &mut object,
            tags::SPECIFIC_CHARACTER_SET,
            VR::CS,
            instance.character_set,
        );
    }
    for (tag, vr, value) in [
        (tags::SOP_CLASS_UID, VR::UI, instance.sop_class),
        (tags::SOP_INSTANCE_UID, VR::UI, instance.sop_instance),
        (tags::PATIENT_NAME, VR::PN, instance.patient_name),
        (tags::PATIENT_ID, VR::LO, instance.patient_id),
        (tags::STUDY_DATE, VR::DA, "20260101"),
        (tags::STUDY_TIME, VR::TM, "120000"),
        (tags::STUDY_ID, VR::SH, "1"),
        (tags::ACCESSION_NUMBER, VR::SH, "A1"),
        (tags::STUDY_INSTANCE_UID, VR::UI, instance.study),
        (tags::SERIES_INSTANCE_UID, VR::UI, instance.series),
        (tags::MODALITY, VR::CS, instance.modality),
        (tags::SERIES_NUMBER, VR::IS, "1"),
        (tags::INSTANCE_NUMBER, VR::IS, instance.instance_number),
        (tags::CONVERSION_TYPE, VR::CS, "WSD"),
        (tags::PHOTOMETRIC_INTERPRETATION, VR::CS, "MONOCHROME2"),
    ] {
        put(&mut object, tag, vr, value);
    }
    for (tag, value) in [
        (tags::SAMPLES_PER_PIXEL, 1u16),
        (tags::ROWS, 2),
        (tags::COLUMNS, 2),
        (tags::BITS_ALLOCATED, 8),
        (tags::BITS_STORED, 8),
        (tags::HIGH_BIT, 7),
        (tags::PIXEL_REPRESENTATION, 0),
    ] {
        object.put(DataElement::new(tag, VR::US, PrimitiveValue::from(value)));
    }
    object.put(DataElement::new(
        tags::PIXEL_DATA,
        VR::OB,
        PrimitiveValue::U8(vec![0, 64, 128, 255].into()),
    ));
    object
        .with_meta(
            FileMetaTableBuilder::new()
                .media_storage_sop_class_uid(instance.sop_class)
                .media_storage_sop_instance_uid(instance.sop_instance)
                .transfer_syntax(transfer_syntax),
        )
        .expect("file meta")
        .write_to_file(path)
        .expect("write Secondary Capture object");
}

#[track_caller]
fn assert_passes(check: &ValidationCheck) {
    assert_eq!(check.name, "intrinsic-wsi-dicom-file-set");
    assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);
}

#[track_caller]
fn assert_fails(check: &ValidationCheck, needle: &str) {
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
    assert!(
        check.message.contains(needle),
        "expected {needle:?} in: {}",
        check.message
    );
}

#[test]
fn a_dcmmkdir_file_set_passes_and_dciodvfy_agrees() {
    let file_set = FileSet::new();
    let check = file_set.check();
    assert_passes(&check);
    assert!(
        check.message.contains("2 referenced Files, 2 DICOM Files"),
        "{}",
        check.message
    );
    assert_dciodvfy_accepts(&file_set.dicomdir(), "baseline");
}

#[test]
fn a_directory_without_a_dicomdir_is_not_a_file_set() {
    let dir = tempfile::tempdir().unwrap();
    write_instance(
        &dir.path().join("SC1.dcm"),
        &FIRST,
        uids::EXPLICIT_VR_LITTLE_ENDIAN,
    );
    assert!(discovered_checks(dir.path(), &ValidationOptions::default()).is_empty());
    assert!(
        discovered_checks(&dir.path().join("SC1.dcm"), &ValidationOptions::default()).is_empty()
    );
}

#[test]
fn a_root_without_a_file_set_fails_explicitly() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("X.dcm");
    std::fs::write(&file, b"not a directory").unwrap();
    let options = ValidationOptions::default();
    for (root, needle) in [
        (file, "is not a directory"),
        (dir.path().join("GONE"), "cannot be read"),
        (dir.path().to_path_buf(), "the DICOMDIR cannot be read"),
    ] {
        let checks = run_file_set_check(std::slice::from_ref(&root), &[], &options);
        assert_eq!(checks.len(), 1, "{checks:?}");
        assert_fails(&checks[0], needle);
    }
}

#[cfg(unix)]
#[test]
fn a_root_that_is_a_symbolic_link_is_refused() {
    let file_set = FileSet::new();
    let link = file_set.dir.path().join("LINK");
    std::os::unix::fs::symlink(&file_set.root, &link).unwrap();
    let checks = run_file_set_check(&[link], &[], &ValidationOptions::default());
    assert_eq!(checks.len(), 1, "{checks:?}");
    assert_fails(&checks[0], "security: refused: the File-set root");
}

#[test]
fn a_dicomdir_that_is_not_a_regular_file_fails() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ROOT");
    std::fs::create_dir_all(root.join("DICOMDIR")).unwrap();
    write_instance(&root.join("SC1"), &FIRST, uids::EXPLICIT_VR_LITTLE_ENDIAN);
    let checks = discovered_checks(dir.path(), &ValidationOptions::default());
    assert_eq!(checks.len(), 1, "{checks:?}");
    assert_fails(
        &checks[0],
        "the DICOMDIR is not a regular file (PS3.10 8.6)",
    );
}

#[test]
fn files_other_than_dicom_files_do_not_count_against_max_files() {
    let file_set = FileSet::new();
    for index in 0..5 {
        std::fs::write(file_set.root.join(format!("NOTE{index}")), b"notes").unwrap();
    }
    std::fs::create_dir(file_set.root.join("EMPTY")).unwrap();
    // DICOMDIR, SC1 and SC2 are the only DICOM files.
    let options = ValidationOptions {
        max_files: 3,
        ..ValidationOptions::default()
    };
    let check = file_set.check_with(&options);
    assert_passes(&check);
    assert!(check.message.contains("2 DICOM Files"), "{}", check.message);
}

#[test]
fn a_file_set_below_the_input_is_found_and_checked() {
    let file_set = FileSet::new();
    file_set.patch(0, b"AB\\SC1", b"AB\\SC9");
    let checks = discovered_checks(file_set.dir.path(), &ValidationOptions::default());
    assert_eq!(checks.len(), 1, "{checks:?}");
    assert_eq!(
        checks[0].path.as_deref(),
        Some(file_set.dicomdir().as_path())
    );
    assert_fails(&checks[0], "every referenced File shall exist");
}

#[test]
fn a_nested_file_set_is_not_an_unreferenced_member_of_the_outer_one() {
    let outer = FileSet::new();
    let inner = outer.root.join("INNER");
    std::fs::create_dir(&inner).unwrap();
    for entry in ["AB", "DICOMDIR"] {
        let from = outer.root.join(entry);
        let to = inner.join(entry);
        if from.is_dir() {
            std::fs::create_dir(&to).unwrap();
            for name in ["SC1", "SC2"] {
                std::fs::copy(from.join(name), to.join(name)).unwrap();
            }
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
    let checks = discovered_checks(&outer.root, &ValidationOptions::default());
    assert_eq!(checks.len(), 2, "{checks:?}");
    for check in &checks {
        assert_passes(check);
    }
}

#[test]
fn file_id_components_follow_ps3_10_8_2_and_8_5() {
    let valid = ["SUBDIR1", "SUBDIR2", "ABCDEFGH"].map(String::from);
    assert!(defects(&valid).is_empty());
    for bad in ["abc", "A.DCM", "ABCDEFGHI", "", "A B"] {
        assert!(
            !defects(&[bad.to_string()]).is_empty(),
            "{bad:?} must be refused"
        );
    }
    let nine: Vec<String> = (0..9).map(|index| format!("D{index}")).collect();
    assert!(defects(&nine)
        .iter()
        .any(|defect| defect.contains("9 Components")));
}

#[test]
fn a_referenced_file_id_outside_the_repertoire_fails() {
    let file_set = FileSet::new();
    file_set.patch(0, b"AB\\SC1", b"AB\\sc1");
    assert_fails(&file_set.check(), "outside the File ID repertoire");
}

#[test]
fn file_ids_leaving_the_root_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ROOT");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(dir.path().join("SECRET"), b"outside").unwrap();
    let canonical = root.canonicalize().unwrap();
    for traversal in [vec!["..", "SECRET"], vec!["/etc"], vec!["A/../../SECRET"]] {
        let components: Vec<String> = traversal.iter().map(|c| c.to_string()).collect();
        assert!(
            matches!(
                resolve(&root, &canonical, &components),
                Err(Unresolved::Escape(_))
            ),
            "{traversal:?} must be refused as path traversal"
        );
    }
    assert!(matches!(
        resolve(&root, &canonical, &["MISSING".to_string()]),
        Err(Unresolved::Missing(_))
    ));
}

#[test]
fn a_referenced_file_id_traversing_out_of_the_root_is_a_security_failure() {
    let file_set = FileSet::new();
    write_instance(
        &file_set.dir.path().join("SC1"),
        &FIRST,
        uids::EXPLICIT_VR_LITTLE_ENDIAN,
    );
    file_set.patch(0, b"AB\\SC1", b"..\\SC1");
    let check = file_set.check();
    assert_fails(&check, "security: Referenced File ID");
    assert_fails(&check, "path traversal");
}

#[cfg(unix)]
#[test]
fn a_referenced_symbolic_link_out_of_the_root_is_a_security_failure() {
    let file_set = FileSet::new();
    let outside = file_set.dir.path().join("OUTSIDE");
    std::fs::rename(file_set.member("SC1"), &outside).unwrap();
    std::os::unix::fs::symlink(&outside, file_set.member("SC1")).unwrap();
    let options = ValidationOptions::default();
    let refused = crate::validation::validate_dicom_path(&file_set.root, &options)
        .expect_err("discovery refuses the link");
    assert!(refused.to_string().contains("symlink"), "{refused}");
    // The check refuses it too, should it appear after discovery.
    let check = file_set.check_undiscovered(&[
        file_set.dicomdir(),
        file_set.member("SC1"),
        file_set.member("SC2"),
    ]);
    assert_fails(&check, "is a symbolic link");
    assert_fails(&check, "security:");
}

#[test]
fn a_referenced_file_that_does_not_exist_fails() {
    let file_set = FileSet::new();
    file_set.patch(0, b"AB\\SC1", b"AB\\SC9");
    assert_fails(&file_set.check(), "every referenced File shall exist");
}

#[test]
fn a_file_referenced_by_two_records_fails_and_leaves_one_unreferenced() {
    let file_set = FileSet::new();
    file_set.patch(0, b"AB\\SC2", b"AB\\SC1");
    let check = file_set.check();
    assert_fails(&check, "referenced by at most one Directory Record");
    assert_fails(&check, "no Directory Record references it");
}

#[test]
fn an_unreferenced_dicom_file_fails_and_a_non_dicom_file_does_not() {
    let file_set = FileSet::new();
    std::fs::write(file_set.root.join("README"), b"File-set notes").unwrap();
    assert_passes(&file_set.check());
    let third = Instance {
        name: "SC3",
        sop_instance: "1.2.826.0.1.3680043.10.999.70.5",
        ..FIRST
    };
    write_instance(
        &file_set.member("SC3"),
        &third,
        uids::EXPLICIT_VR_LITTLE_ENDIAN,
    );
    assert_fails(&file_set.check(), "SC3 is a DICOM File of the File-set");
}

#[test]
fn a_root_offset_not_at_an_item_fails() {
    let file_set = FileSet::new();
    let first = offset_value(&file_set, FIRST_ROOT_RECORD);
    file_set.set_offset(0, FIRST_ROOT_RECORD, first, first + 2);
    let check = file_set.check();
    assert_fails(&check, "not the first byte of an Item");
    assert_fails(&check, "is not reached from the Root Directory Entity");
}

#[test]
fn a_last_root_offset_that_is_not_the_end_of_the_root_chain_fails() {
    let file_set = FileSet::new();
    let last = offset_value(&file_set, LAST_ROOT_RECORD);
    let image = file_set.record("IMAGE").offset as u32;
    file_set.set_offset(0, LAST_ROOT_RECORD, last, image);
    assert_fails(&file_set.check(), "chain ends at the record at byte");
}

#[test]
fn a_cycle_of_next_offsets_fails() {
    let file_set = FileSet::new();
    let directory = file_set.directory();
    let images: Vec<_> = directory
        .records
        .iter()
        .filter(|record| record.record_type() == "IMAGE")
        .collect();
    let last = images.last().expect("image records");
    let first = images[0].offset as u32;
    file_set.set_offset(last.offset as usize, NEXT_RECORD, 0, first);
    assert_fails(&file_set.check(), "reached a second time");
}

#[test]
fn records_below_a_dropped_lower_level_offset_are_unreachable() {
    let file_set = FileSet::new();
    let series = file_set.record("SERIES");
    let lower = offset(&series, LOWER_ENTITY);
    file_set.set_offset(series.offset as usize, LOWER_ENTITY, lower, 0);
    assert_fails(
        &file_set.check(),
        "(Directory Record Type \"IMAGE\") is not reached",
    );
}

#[test]
fn a_record_below_a_parent_table_f_4_1_disallows_fails() {
    let file_set = FileSet::new();
    let image = file_set.record("IMAGE").offset as usize;
    file_set.patch(image, b"IMAGE ", b"STUDY ");
    assert_fails(&file_set.check(), "which Table F.4-1 does not allow");
}

#[test]
fn a_file_set_consistency_flag_other_than_zero_fails() {
    let file_set = FileSet::new();
    file_set.patch(
        0,
        b"\x04\x00\x12\x12US\x02\x00\x00\x00",
        b"\x04\x00\x12\x12US\x02\x00\xFF\xFF",
    );
    assert_fails(
        &file_set.check(),
        "File-set Consistency Flag (0004,1212) is FFFFH",
    );
}

#[test]
fn referenced_uids_in_file_must_match_the_file() {
    let file_set = FileSet::new();
    let renamed = Instance {
        sop_instance: "1.2.826.0.1.3680043.10.999.70.9",
        ..FIRST
    };
    write_instance(
        &file_set.member("SC1"),
        &renamed,
        uids::IMPLICIT_VR_LITTLE_ENDIAN,
    );
    let check = file_set.check();
    assert_fails(&check, "Referenced SOP Instance UID in File (0004,1511)");
    assert_fails(&check, "Media Storage SOP Instance UID (0002,0003)");
    assert_fails(&check, "Referenced Transfer Syntax UID in File (0004,1512)");
}

#[test]
fn record_keys_must_match_the_file() {
    let file_set = FileSet::new();
    let changed = Instance {
        patient_name: "OTHER^NAME",
        patient_id: "PAT2",
        modality: "XC",
        instance_number: "7",
        ..FIRST
    };
    write_instance(
        &file_set.member("SC1"),
        &changed,
        uids::EXPLICIT_VR_LITTLE_ENDIAN,
    );
    let check = file_set.check();
    assert_fails(&check, "Patient's Name (0010,0010) of the PATIENT record");
    assert_fails(&check, "Patient ID (0010,0020) of the PATIENT record");
    assert_fails(&check, "Modality (0008,0060) of the SERIES record");
    assert_fails(&check, "Instance Number (0020,0013) of the IMAGE record");
}

/// The record Keys a File contradicts are named with their Values withheld
/// (PS3.15 Table E.1-1): the message gives their lengths and where they
/// differ, never the planted name or ID.
#[test]
fn record_key_messages_withhold_patient_names_and_ids() {
    let planted = Instance {
        patient_name: "DOE^JANE",
        patient_id: "MRN12345",
        ..FIRST
    };
    let file_set = FileSet::with(std::slice::from_ref(&planted));
    assert_passes(&file_set.check());
    let changed = Instance {
        patient_name: "DOE^JOHN",
        patient_id: "MRN12399",
        ..planted
    };
    write_instance(
        &file_set.member("SC1"),
        &changed,
        uids::EXPLICIT_VR_LITTLE_ENDIAN,
    );
    let check = file_set.check();
    let offset = file_set.record("PATIENT").offset;
    let withheld = "(value withheld: identifying; 8 characters)";
    for (name, at) in [
        ("Patient's Name (0010,0010)", 6),
        ("Patient ID (0010,0020)", 7),
    ] {
        assert_fails(
            &check,
            &format!("{name} of the PATIENT record at byte {offset} is {withheld}, but "),
        );
        assert_fails(
            &check,
            &format!("SC1 has {withheld}, first differing at character {at} (PS3.3 F.5)"),
        );
    }
    for planted in ["DOE^J", "MRN123"] {
        assert!(
            !check.message.contains(planted),
            "{planted}: {}",
            check.message
        );
    }
}

/// Writes `instance` as SC1 alone, then checks that the File-set dcmmkdir
/// makes of it passes with `patient_id` decoded from its records, and that
/// a Patient ID differing from the record's only in a non-ASCII character
/// fails, the message giving where the withheld Values differ.
#[track_caller]
fn assert_keys_compare_in(instance: Instance, encoded_id: &[u8], differing_id: &'static str) {
    let file_set = FileSet::with(std::slice::from_ref(&instance));
    let bytes = std::fs::read(file_set.member("SC1")).unwrap();
    assert!(
        bytes
            .windows(encoded_id.len())
            .any(|window| window == encoded_id),
        "Patient ID encoded under {}",
        instance.character_set
    );
    let patient = file_set.record("PATIENT");
    assert_eq!(
        patient
            .get(tags::PATIENT_ID)
            .map(records::Value::text)
            .as_deref(),
        Some(instance.patient_id)
    );
    assert_eq!(
        patient
            .get(tags::PATIENT_NAME)
            .map(records::Value::text)
            .as_deref(),
        Some(instance.patient_name)
    );
    assert_passes(&file_set.check());
    let differing = Instance {
        patient_id: differing_id,
        ..instance
    };
    write_instance(
        &file_set.member("SC1"),
        &differing,
        uids::EXPLICIT_VR_LITTLE_ENDIAN,
    );
    let check = file_set.check();
    assert_fails(
        &check,
        &format!(
            "Patient ID (0010,0020) of the PATIENT record at byte {} is (value withheld: identifying; {} characters), but ",
            patient.offset,
            instance.patient_id.chars().count()
        ),
    );
    assert_fails(&check, "first differing at character 4 (PS3.3 F.5)");
    assert!(
        !check.message.contains(instance.patient_id),
        "{}",
        check.message
    );
}

#[test]
fn record_keys_in_iso_ir_100_compare_with_the_file() {
    let latin = Instance {
        patient_name: "M\u{fc}ller^Hans",
        patient_id: "PAT\u{fc}",
        character_set: "ISO_IR 100",
        ..FIRST
    };
    assert_keys_compare_in(latin, b"PAT\xFC", "PAT\u{fd}");
}

#[test]
fn record_keys_in_iso_2022_compare_with_the_file() {
    // ISO 2022 IR 100 designates ISO 8859-1 as G1 from the start, so its
    // Latin characters need no escape sequence.
    let latin = Instance {
        patient_name: "M\u{fc}ller^Hans",
        patient_id: "PAT\u{fc}",
        character_set: "ISO 2022 IR 100",
        ..FIRST
    };
    assert_keys_compare_in(latin, b"PAT\xFC", "PAT\u{fd}");
}

/// A DICOMDIR of three PATIENT records whose Patient IDs are `PAT` and
/// U+00FC, the DICOMDIR's Specific Character Set ISO_IR 192: the first
/// record declares none, the second ISO_IR 100, and the third, of undefined
/// length, none.
fn dicomdir_in_two_character_sets() -> Vec<u8> {
    let mut bytes = vec![0u8; 128];
    bytes.extend_from_slice(b"DICM");
    bytes.extend(short_element(
        tags::TRANSFER_SYNTAX_UID,
        b"UI",
        b"1.2.840.10008.1.2.1\0",
    ));
    let record = |charset: Option<&[u8]>, id: &[u8]| {
        let mut content = short_element(tags::DIRECTORY_RECORD_TYPE, b"CS", b"PATIENT ");
        if let Some(charset) = charset {
            content.extend(short_element(tags::SPECIFIC_CHARACTER_SET, b"CS", charset));
        }
        content.extend(short_element(tags::PATIENT_ID, b"LO", id));
        content
    };
    bytes.extend_from_slice(b"\x04\x00\x20\x12SQ\x00\x00\xFF\xFF\xFF\xFF");
    for content in [
        record(None, b"PAT\xC3\xBC "),
        record(Some(b"ISO_IR 100"), b"PAT\xFC"),
    ] {
        bytes.extend_from_slice(b"\xFE\xFF\x00\xE0");
        bytes.extend_from_slice(&(content.len() as u32).to_le_bytes());
        bytes.extend(content);
    }
    bytes.extend_from_slice(b"\xFE\xFF\x00\xE0\xFF\xFF\xFF\xFF");
    bytes.extend(record(None, b"PAT\xC3\xBC "));
    bytes.extend_from_slice(b"\xFE\xFF\x0D\xE0\x00\x00\x00\x00");
    bytes.extend_from_slice(b"\xFE\xFF\xDD\xE0\x00\x00\x00\x00");
    bytes.extend(short_element(
        tags::SPECIFIC_CHARACTER_SET,
        b"CS",
        b"ISO_IR 192",
    ));
    bytes
}

#[test]
fn record_text_decodes_under_its_own_character_set_or_the_dicomdirs() {
    let bytes = dicomdir_in_two_character_sets();
    let directory = parse(&bytes).expect("parsed DICOMDIR");
    assert_eq!(directory.records.len(), 3);
    let ids: Vec<String> = directory
        .records
        .iter()
        .map(|record| {
            record
                .get(tags::PATIENT_ID)
                .map(records::Value::text)
                .unwrap()
        })
        .collect();
    assert_eq!(ids, ["PAT\u{fc}"; 3]);
    // dicom-object reads each record under the same character set, the
    // second record's not lasting past it.
    let parsed = super::content::parse_records(&bytes, &directory.records).expect("records");
    let ids: Vec<String> = parsed
        .iter()
        .map(|record| crate::validation::iod::string_value(record, tags::PATIENT_ID, 1).unwrap())
        .collect();
    assert_eq!(ids, ["PAT\u{fc}"; 3]);
}

#[test]
fn a_dicomdir_not_in_explicit_vr_little_endian_fails() {
    let file_set = FileSet::new();
    file_set.patch(0, b"1.2.840.10008.1.2.1\0", b"1.2.840.10008.1.2.2\0");
    assert_fails(&file_set.check(), "shall use Explicit VR Little Endian");
}

#[test]
fn a_dicomdir_of_another_sop_class_fails() {
    let file_set = FileSet::new();
    file_set.patch(0, b"1.2.840.10008.1.3.10", b"1.2.840.10008.1.3.19");
    assert_fails(
        &file_set.check(),
        "Media Storage Directory Storage SOP Class",
    );
}

#[cfg(unix)]
#[test]
fn a_dicomdir_that_is_a_symbolic_link_is_refused() {
    let file_set = FileSet::new();
    let outside = file_set.dir.path().join("REAL");
    std::fs::rename(file_set.dicomdir(), &outside).unwrap();
    std::os::unix::fs::symlink(&outside, file_set.dicomdir()).unwrap();
    let options = ValidationOptions::default();
    assert!(crate::validation::validate_dicom_path(&file_set.root, &options).is_err());
    assert_fails(
        &file_set.check_undiscovered(&[]),
        "security: refused: the DICOMDIR is a symbolic link",
    );
}

#[test]
fn a_truncated_dicomdir_fails_to_parse() {
    let file_set = FileSet::new();
    let bytes = std::fs::read(file_set.dicomdir()).unwrap();
    std::fs::write(file_set.dicomdir(), &bytes[..bytes.len() - 7]).unwrap();
    assert_fails(&file_set.check(), "the DICOMDIR cannot be parsed");
}

#[test]
fn hostile_nesting_is_bounded() {
    let mut bytes = vec![0u8; 128];
    bytes.extend_from_slice(b"DICM");
    let syntax = b"1.2.840.10008.1.2.1\0";
    bytes.extend_from_slice(b"\x02\x00\x10\x00UI");
    bytes.extend_from_slice(&(syntax.len() as u16).to_le_bytes());
    bytes.extend_from_slice(syntax);
    for _ in 0..40 {
        bytes.extend_from_slice(b"\x08\x00\x15\x11SQ\0\0\xFF\xFF\xFF\xFF");
        bytes.extend_from_slice(b"\xFE\xFF\x00\xE0\xFF\xFF\xFF\xFF");
    }
    let error = parse(&bytes).expect_err("nesting beyond the bound");
    assert!(error.contains("deeper than"), "{error}");
}

fn offset_value(file_set: &FileSet, tag: Tag) -> u32 {
    file_set
        .directory()
        .data_set
        .get(&tag)
        .and_then(records::Value::unsigned)
        .expect("root offset")
}

#[test]
fn a_file_set_id_outside_the_repertoire_fails() {
    let file_set = FileSet::new();
    file_set.patch(0, b"DCMTK_MEDIA_DEMO", b"DCMTK MEDIA_DEMO");
    assert_fails(
        &file_set.check(),
        "File-set ID (0004,1130) \"DCMTK MEDIA_DEMO\"",
    );
}

#[test]
fn patient_study_and_series_keys_are_unique_in_the_file_set() {
    let other = Instance {
        patient_id: "PAT2",
        study: "1.2.826.0.1.3680043.10.999.71.1",
        series: "1.2.826.0.1.3680043.10.999.71.2",
        ..SECOND
    };
    let file_set = FileSet::with(&[FIRST, other]);
    assert_passes(&file_set.check());
    file_set.patch(0, b"PAT2", b"PAT1");
    file_set.patch(0, b"999.71.1", b"999.70.1");
    file_set.patch(0, b"999.71.2", b"999.70.2");
    let check = file_set.check();
    assert_fails(
        &check,
        "Patient ID (0010,0020) (value withheld: identifying; 4 characters) of the PATIENT record",
    );
    assert!(!check.message.contains("PAT1"), "{}", check.message);
    assert_fails(&check, "unique Value in the File-set (PS3.3 F.5.1)");
    assert_fails(&check, "(PS3.3 F.5.2)");
    assert_fails(&check, "(PS3.3 F.5.3)");
}

#[test]
fn an_image_record_referencing_a_non_image_object_fails() {
    let file_set = FileSet::new();
    let key_object = Instance {
        sop_class: "1.2.840.10008.5.1.4.1.1.88.59",
        ..FIRST
    };
    write_instance(
        &file_set.member("SC1"),
        &key_object,
        uids::EXPLICIT_VR_LITTLE_ENDIAN,
    );
    let check = file_set.check();
    assert_fails(
        &check,
        "shall be used to reference an Image SOP Instance (PS3.3 F.5.4)",
    );
    assert_fails(&check, "Referenced SOP Class UID in File (0004,1510)");
}

#[test]
fn two_records_referencing_one_lower_level_entity_fail() {
    // The SERIES record and the second IMAGE record both reference the
    // Directory Entity of the IMAGE records.
    let file_set = FileSet::new();
    let directory = file_set.directory();
    let images: Vec<_> = directory
        .records
        .iter()
        .filter(|record| record.record_type() == "IMAGE")
        .collect();
    let first = images[0].offset as u32;
    let second = images[1].offset as usize;
    file_set.set_offset(second, LOWER_ENTITY, 0, first);
    assert_fails(&file_set.check(), "reached a second time");
}

#[test]
fn a_lower_level_offset_not_at_an_item_fails() {
    let file_set = FileSet::new();
    let image = file_set.record("IMAGE");
    file_set.set_offset(image.offset as usize, LOWER_ENTITY, 0, 12_345);
    assert_fails(
        &file_set.check(),
        "Offset of Referenced Lower-Level Directory Entity (0004,1420) of the record at byte",
    );
}

#[test]
fn a_blank_file_set_uid_fails() {
    let file_set = FileSet::new();
    let directory = file_set.directory();
    let uid = directory
        .meta
        .get(&Tag(0x0002, 0x0003))
        .expect("Media Storage SOP Instance UID")
        .bytes
        .clone();
    file_set.patch(0, &uid, &vec![b' '; uid.len()]);
    assert_fails(&file_set.check(), "the File-set UID, is absent or empty");
}

/// A Directory with the Directory Information Module and `records`.
fn synthetic(records: Vec<records::Record>, first: u32, last: u32) -> records::Directory {
    let ul = |value: u32| records::Value {
        vr: VR::UL,
        bytes: value.to_le_bytes().to_vec(),
        decoded: None,
        items: 0,
        item_sets: Vec::new(),
    };
    let mut directory = records::Directory {
        has_records: true,
        records,
        ..Default::default()
    };
    directory.data_set.insert(FIRST_ROOT_RECORD, ul(first));
    directory.data_set.insert(LAST_ROOT_RECORD, ul(last));
    directory.data_set.insert(
        Tag(0x0004, 0x1212),
        records::Value {
            vr: VR::US,
            bytes: vec![0, 0],
            decoded: None,
            items: 0,
            item_sets: Vec::new(),
        },
    );
    directory
}

fn record_of_type(offset: u64, record_type: &str) -> records::Record {
    let mut record = records::Record {
        offset,
        ..Default::default()
    };
    let mut text = record_type.as_bytes().to_vec();
    if text.len() % 2 == 1 {
        text.push(b' ');
    }
    record.elements.insert(
        Tag(0x0004, 0x1430),
        records::Value {
            vr: VR::CS,
            bytes: text,
            decoded: None,
            items: 0,
            item_sets: Vec::new(),
        },
    );
    for tag in [NEXT_RECORD, LOWER_ENTITY] {
        record.elements.insert(
            tag,
            records::Value {
                vr: VR::UL,
                bytes: vec![0; 4],
                decoded: None,
                items: 0,
                item_sets: Vec::new(),
            },
        );
    }
    record
}

fn structure_messages(directory: &records::Directory) -> String {
    let mut family = Family::default();
    check_structure(directory, &mut family);
    family
        .check(Path::new("DICOMDIR"), "rule", "citation")
        .message
}

#[test]
fn an_empty_root_directory_entity_has_zero_offsets() {
    assert!(structure_messages(&synthetic(Vec::new(), 0, 0)).ends_with("conformant"));
    let message = structure_messages(&synthetic(Vec::new(), 500, 0));
    assert!(
        message.contains("(0004,1200) is 500, which is not the first byte of an Item"),
        "{message}"
    );
    let message = structure_messages(&synthetic(Vec::new(), 0, 500));
    assert!(message.contains("(0004,1202) is 500"), "{message}");
}

#[test]
fn a_next_offset_past_the_last_record_fails() {
    let mut patient = record_of_type(400, "PATIENT");
    patient.elements.insert(
        NEXT_RECORD,
        records::Value {
            vr: VR::UL,
            bytes: 9_999u32.to_le_bytes().to_vec(),
            decoded: None,
            items: 0,
            item_sets: Vec::new(),
        },
    );
    let message = structure_messages(&synthetic(vec![patient], 400, 400));
    assert!(
        message.contains(
            "Offset of the Next Directory Record (0004,1400) of the record at byte 400 is 9999"
        ),
        "{message}"
    );
}

#[test]
fn record_types_reference_their_sop_classes() {
    const SR: &str = "1.2.840.10008.5.1.4.1.1.88.33";
    const KEY_OBJECT: &str = "1.2.840.10008.5.1.4.1.1.88.59";
    const ANNOTATIONS: &str = "1.2.840.10008.5.1.4.1.1.91.1";
    const WSI: &str = "1.2.840.10008.5.1.4.1.1.77.1.6";
    const SEGMENTATION: &str = "1.2.840.10008.5.1.4.1.1.66.4";
    for (record_type, class, allowed, section) in [
        ("IMAGE", WSI, true, "F.5.4"),
        ("IMAGE", SEGMENTATION, true, "F.5.4"),
        ("IMAGE", SR, false, "F.5.4"),
        ("SR DOCUMENT", SR, true, "F.5.25"),
        ("SR DOCUMENT", KEY_OBJECT, false, "F.5.25"),
        ("SR DOCUMENT", WSI, false, "F.5.25"),
        ("KEY OBJECT DOC", KEY_OBJECT, true, "F.5.26"),
        ("KEY OBJECT DOC", SR, false, "F.5.26"),
        ("ANNOTATION", ANNOTATIONS, true, "F.5.47"),
        ("ANNOTATION", WSI, false, "F.5.47"),
    ] {
        let mut family = Family::default();
        let record = record_of_type(400, record_type);
        check_record_class(&record, Path::new("AB/X"), class, &mut family);
        let message = family
            .check(Path::new("DICOMDIR"), "rule", "citation")
            .message;
        assert_eq!(
            family.is_empty(),
            allowed,
            "{record_type} referencing {class}: {message}"
        );
        if !allowed {
            assert!(message.contains(section), "{message}");
        }
    }
}

/// One Explicit VR Little Endian Data Element with a 2-byte length.
fn short_element(tag: Tag, vr: &[u8; 2], value: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&tag.group().to_le_bytes());
    bytes.extend_from_slice(&tag.element().to_le_bytes());
    bytes.extend_from_slice(vr);
    bytes.extend_from_slice(&(value.len() as u16).to_le_bytes());
    bytes.extend_from_slice(value);
    bytes
}

/// Writes `root`/DICOMDIR: a Basic Directory whose Root Directory Entity is
/// empty, built byte by byte without any tool, so a validated set can hold
/// a File-set wherever dcmtk is absent.
pub(in crate::validation) fn write_empty_file_set(root: &Path) {
    let mut meta = Vec::new();
    meta.extend_from_slice(b"\x02\x00\x01\x00OB\x00\x00\x02\x00\x00\x00\x00\x01");
    for (element, value) in [
        (0x0002u16, &b"1.2.840.10008.1.3.10"[..]),
        (0x0003, b"1.2.826.0.1.3680043.10.999.72.1\0"),
        (0x0010, b"1.2.840.10008.1.2.1\0"),
        (0x0012, b"1.2.826.0.1.3680043.10.999"),
    ] {
        meta.extend(short_element(Tag(0x0002, element), b"UI", value));
    }
    let mut bytes = vec![0u8; 128];
    bytes.extend_from_slice(b"DICM");
    bytes.extend(short_element(
        Tag(0x0002, 0x0000),
        b"UL",
        &(meta.len() as u32).to_le_bytes(),
    ));
    bytes.extend(meta);
    bytes.extend(short_element(Tag(0x0004, 0x1130), b"CS", b""));
    bytes.extend(short_element(FIRST_ROOT_RECORD, b"UL", &[0; 4]));
    bytes.extend(short_element(LAST_ROOT_RECORD, b"UL", &[0; 4]));
    bytes.extend(short_element(Tag(0x0004, 0x1212), b"US", &[0; 2]));
    bytes.extend_from_slice(b"\x04\x00\x20\x12SQ\x00\x00\x00\x00\x00\x00");
    std::fs::create_dir_all(root).expect("File-set root");
    std::fs::write(root.join("DICOMDIR"), bytes).expect("write DICOMDIR");
}

#[test]
fn an_empty_file_set_passes_and_dciodvfy_agrees() {
    let dir = tempfile::tempdir().unwrap();
    write_empty_file_set(dir.path());
    let checks = discovered_checks(dir.path(), &ValidationOptions::default());
    assert_eq!(checks.len(), 1, "{checks:?}");
    assert_passes(&checks[0]);
    assert_dciodvfy_accepts(&dir.path().join("DICOMDIR"), "empty");
}

/// dciodvfy, the independent oracle, reports no Error for `dicomdir`. Skipped
/// when dciodvfy is not installed, unless `WSI_DICOM_REQUIRED_EXTERNAL_TOOLS`
/// names it.
fn assert_dciodvfy_accepts(dicomdir: &Path, which: &str) {
    let Some(dciodvfy) = find_command_for_test("dciodvfy") else {
        eprintln!("skipping dciodvfy validation: dciodvfy not found");
        return;
    };
    let output = Command::new(dciodvfy)
        .arg(dicomdir)
        .output()
        .expect("run dciodvfy");
    let report = String::from_utf8_lossy(&output.stderr).to_string()
        + &String::from_utf8_lossy(&output.stdout);
    assert!(
        !report.lines().any(|line| line.starts_with("Error")),
        "dciodvfy rejects the {which} DICOMDIR: {report}"
    );
}

#[test]
fn dcmmkdir_icon_images_pass_and_broken_ones_fail() {
    let file_set = FileSet::build(&[FIRST, SECOND], &["+X", "-Xs", "8"]);
    let directory = file_set.directory();
    assert!(
        directory
            .records
            .iter()
            .any(|record| record.get(super::records::ICON_IMAGE_SEQUENCE).is_some()),
        "dcmmkdir +X writes Icon Images"
    );
    assert_passes(&file_set.check());
    file_set.patch(
        0,
        b"\x28\x00\x03\x01US\x02\x00\x00\x00",
        b"\x28\x00\x03\x01US\x02\x00\x01\x00",
    );
    assert_fails(
        &file_set.check(),
        "Pixel Representation (0028,0103) 1, but shall be unsigned",
    );
}

fn us(value: u16) -> records::Value {
    records::Value {
        vr: VR::US,
        bytes: value.to_le_bytes().to_vec(),
        decoded: None,
        items: 0,
        item_sets: Vec::new(),
    }
}

fn text(vr: VR, value: &str) -> records::Value {
    records::Value {
        vr,
        bytes: value.as_bytes().to_vec(),
        decoded: None,
        items: 0,
        item_sets: Vec::new(),
    }
}

/// The F.7 findings of an IMAGE record whose Icon Image is `icon` edited by
/// `edit`.
fn icon_messages(
    edit: impl FnOnce(&mut std::collections::BTreeMap<Tag, records::Value>),
) -> String {
    let mut icon = std::collections::BTreeMap::new();
    icon.insert(tags::SAMPLES_PER_PIXEL, us(1));
    icon.insert(
        tags::PHOTOMETRIC_INTERPRETATION,
        text(VR::CS, "MONOCHROME2 "),
    );
    icon.insert(tags::ROWS, us(8));
    icon.insert(tags::COLUMNS, us(8));
    icon.insert(tags::BITS_ALLOCATED, us(8));
    icon.insert(tags::BITS_STORED, us(8));
    icon.insert(tags::HIGH_BIT, us(7));
    icon.insert(tags::PIXEL_REPRESENTATION, us(0));
    edit(&mut icon);
    let mut record = record_of_type(400, "IMAGE");
    record.elements.insert(
        super::records::ICON_IMAGE_SEQUENCE,
        records::Value {
            vr: VR::SQ,
            bytes: Vec::new(),
            decoded: None,
            items: 1,
            item_sets: vec![icon],
        },
    );
    let mut family = Family::default();
    check_icons(&[record], &mut family);
    family
        .check(Path::new("DICOMDIR"), "rule", "citation")
        .message
}

#[test]
fn icon_images_follow_f_7() {
    assert!(icon_messages(|_| {}).ends_with("conformant"));
    assert!(icon_messages(|icon| {
        icon.insert(tags::PIXEL_ASPECT_RATIO, text(VR::IS, "1\\1 "));
    })
    .ends_with("conformant"));
    for (edit, needle) in [
        (
            (|icon: &mut std::collections::BTreeMap<Tag, records::Value>| {
                icon.insert(tags::SAMPLES_PER_PIXEL, us(3));
            }) as fn(&mut std::collections::BTreeMap<Tag, records::Value>),
            "Samples per Pixel (0028,0002) 3",
        ),
        (
            |icon| {
                icon.insert(tags::PHOTOMETRIC_INTERPRETATION, text(VR::CS, "RGB "));
            },
            "only monochrome and palette color images",
        ),
        (
            |icon| {
                icon.insert(tags::PLANAR_CONFIGURATION, us(0));
            },
            "Planar Configuration (0028,0006), which shall not be present",
        ),
        (
            |icon| {
                icon.insert(tags::BITS_ALLOCATED, us(16));
            },
            "Bits Allocated (0028,0100) 16, but shall have 1 or 8",
        ),
        (
            |icon| {
                icon.insert(tags::BITS_STORED, us(4));
                icon.insert(tags::HIGH_BIT, us(3));
            },
            "Bits Stored (0028,0101) 4, but shall have 1 or 8",
        ),
        (
            |icon| {
                icon.insert(tags::HIGH_BIT, us(8));
            },
            "High Bit (0028,0102) 8, but shall have one less than Bits Stored 8",
        ),
        (
            |icon| {
                icon.insert(tags::PIXEL_REPRESENTATION, us(1));
            },
            "Pixel Representation (0028,0103) 1",
        ),
        (
            |icon| {
                icon.insert(tags::PIXEL_ASPECT_RATIO, text(VR::IS, "2\\1 "));
            },
            "Pixel Aspect Ratio (0028,0034) \"2\\\\1\", but shall have a Value of 1:1",
        ),
        (
            |icon| {
                icon.insert(
                    tags::PHOTOMETRIC_INTERPRETATION,
                    text(VR::CS, "PALETTE COLOR "),
                );
                icon.insert(tags::BITS_ALLOCATED, us(1));
                icon.insert(tags::BITS_STORED, us(1));
                icon.insert(tags::HIGH_BIT, us(0));
            },
            "uses a Palette Color Lookup Table with Bits Allocated (0028,0100) 1",
        ),
    ] {
        let message = icon_messages(edit);
        assert!(
            message.contains(needle),
            "expected {needle:?} in: {message}"
        );
        assert!(message.contains("PS3.3 F.7"), "{message}");
    }
}

#[test]
fn validating_a_file_set_runs_the_file_set_and_file_meta_checks_on_the_dicomdir() {
    let file_set = FileSet::new();
    let options = ValidationOptions::default();
    let report = crate::validation::validate_dicom_path(&file_set.root, &options).unwrap();
    for member in [
        file_set.dicomdir(),
        file_set.member("SC1"),
        file_set.member("SC2"),
    ] {
        assert!(
            report.files.contains(&member),
            "{member:?} in {:?}",
            report.files
        );
    }
    let named = |report: &crate::validation::ValidationReport, name: &str| {
        report
            .checks
            .iter()
            .find(|check| {
                check.name == name && check.path.as_deref() == Some(file_set.dicomdir().as_path())
            })
            .cloned()
            .unwrap_or_else(|| panic!("no {name} check of the DICOMDIR"))
    };
    assert_passes(&named(&report, "intrinsic-wsi-dicom-file-set"));
    assert_eq!(
        named(&report, "intrinsic-wsi-dicom-file-meta").status,
        ValidationStatus::Passed
    );
    // PS3.10 8.6: the DICOMDIR complies with the File Format of Section 7.
    file_set.patch(
        0,
        b"\x02\x00\x01\x00OB\x00\x00\x02\x00\x00\x00\x00\x01",
        b"\x02\x00\x01\x00OB\x00\x00\x02\x00\x00\x00\x00\x00",
    );
    let report = crate::validation::validate_dicom_path(&file_set.root, &options).unwrap();
    let file_meta = named(&report, "intrinsic-wsi-dicom-file-meta");
    assert_fails(&file_meta, "File Meta Information Version (0002,0001)");
}

#[test]
fn private_records_follow_f_2_and_f_3() {
    let mut patient = record_of_type(400, "PATIENT");
    patient.elements.insert(
        LOWER_ENTITY,
        records::Value {
            vr: VR::UL,
            bytes: 500u32.to_le_bytes().to_vec(),
            decoded: None,
            items: 0,
            item_sets: Vec::new(),
        },
    );
    let private = record_of_type(500, "PRIVATE");
    let stray = record_of_type(600, "PRIVATE");
    let message = structure_messages(&synthetic(vec![patient, private, stray], 400, 400));
    assert!(
        message
            .contains("the record at byte 600 (Directory Record Type \"PRIVATE\") is not reached"),
        "{message}"
    );
    assert!(!message.contains("byte 500"), "{message}");
}

#[test]
fn directory_records_list_each_record_with_its_ancestry_and_file() {
    let file_set = FileSet::new();
    let records = super::listing::directory_records(&file_set.root, &ValidationOptions::default())
        .expect("a listing");
    let types: Vec<&str> = records
        .iter()
        .map(|record| record.record_type.as_str())
        .collect();
    assert_eq!(types, ["PATIENT", "STUDY", "SERIES", "IMAGE", "IMAGE"]);
    let image = &records[3];
    assert!(records
        .windows(2)
        .all(|pair| pair[0].offset < pair[1].offset));
    let ancestry: Vec<&str> = image
        .ancestry
        .iter()
        .map(|&index| records[index].record_type.as_str())
        .collect();
    assert_eq!(ancestry, ["PATIENT", "STUDY", "SERIES"]);
    assert_eq!(
        image.referenced_file.as_deref(),
        Some(file_set.member("SC1").canonicalize().unwrap().as_path())
    );
    assert!(records[0].referenced_file.is_none());
    assert_eq!(
        crate::validation::iod::string_value(&records[0].attributes, tags::PATIENT_ID, 1)
            .as_deref(),
        Some("PAT1")
    );
    file_set.patch(0, b"AB\\SC1", b"..\\SC1");
    let records = super::listing::directory_records(&file_set.root, &ValidationOptions::default())
        .expect("a listing");
    assert!(
        records[3].referenced_file.is_none(),
        "a File ID leaving the root is not resolved"
    );
}

/// The HAS CONCEPT MOD Content Item naming the language `code` (TID 1204).
fn language(code: &str, meaning: &str) -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    put(
        &mut item,
        tags::RELATIONSHIP_TYPE,
        VR::CS,
        "HAS CONCEPT MOD",
    );
    put(&mut item, tags::VALUE_TYPE, VR::CS, "CODE");
    item.put(code_sequence(
        tags::CONCEPT_NAME_CODE_SEQUENCE,
        "121049",
        "DCM",
        "Language of Content Item and Descendants",
    ));
    item.put(code_sequence(
        tags::CONCEPT_CODE_SEQUENCE,
        code,
        "RFC5646",
        meaning,
    ));
    item
}

fn code_sequence(tag: Tag, value: &str, scheme: &str, meaning: &str) -> InMemElement {
    let mut code = InMemDicomObject::new_empty();
    put(&mut code, tags::CODE_VALUE, VR::SH, value);
    put(&mut code, tags::CODING_SCHEME_DESIGNATOR, VR::SH, scheme);
    put(&mut code, tags::CODE_MEANING, VR::LO, meaning);
    DataElement::new(tag, VR::SQ, DataSetSequence::from(vec![code]))
}

const SR_INSTANCE: &str = "1.2.826.0.1.3680043.10.999.73.3";

/// A Comprehensive SR document in the fixture's Study whose root holds
/// `modifiers` and one CONTAINS TEXT Content Item.
fn write_sr(path: &Path, modifiers: Vec<InMemDicomObject>) {
    const COMPREHENSIVE_SR: &str = "1.2.840.10008.5.1.4.1.1.88.33";
    let mut object = InMemDicomObject::new_empty();
    for (tag, vr, value) in [
        (tags::SOP_CLASS_UID, VR::UI, COMPREHENSIVE_SR),
        (tags::SOP_INSTANCE_UID, VR::UI, SR_INSTANCE),
        (tags::PATIENT_NAME, VR::PN, "FILE^SET"),
        (tags::PATIENT_ID, VR::LO, "PAT1"),
        (tags::STUDY_DATE, VR::DA, "20260101"),
        (tags::STUDY_TIME, VR::TM, "120000"),
        (tags::STUDY_ID, VR::SH, "1"),
        (tags::ACCESSION_NUMBER, VR::SH, "A1"),
        (tags::STUDY_INSTANCE_UID, VR::UI, STUDY_UID),
        (
            tags::SERIES_INSTANCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.73.2",
        ),
        (tags::MODALITY, VR::CS, "SR"),
        (tags::SERIES_NUMBER, VR::IS, "2"),
        (tags::INSTANCE_NUMBER, VR::IS, "1"),
        (tags::COMPLETION_FLAG, VR::CS, "COMPLETE"),
        (tags::VERIFICATION_FLAG, VR::CS, "UNVERIFIED"),
        (tags::CONTENT_DATE, VR::DA, "20260101"),
        (tags::CONTENT_TIME, VR::TM, "120000"),
        (tags::VALUE_TYPE, VR::CS, "CONTAINER"),
        (tags::CONTINUITY_OF_CONTENT, VR::CS, "SEPARATE"),
    ] {
        put(&mut object, tag, vr, value);
    }
    object.put(code_sequence(
        tags::CONCEPT_NAME_CODE_SEQUENCE,
        "126000",
        "DCM",
        "Imaging Measurement Report",
    ));
    let mut text = InMemDicomObject::new_empty();
    put(&mut text, tags::RELATIONSHIP_TYPE, VR::CS, "CONTAINS");
    put(&mut text, tags::VALUE_TYPE, VR::CS, "TEXT");
    text.put(code_sequence(
        tags::CONCEPT_NAME_CODE_SEQUENCE,
        "121071",
        "DCM",
        "Finding",
    ));
    put(&mut text, tags::TEXT_VALUE, VR::UT, "none");
    let mut items = modifiers;
    items.push(text);
    object.put(DataElement::new(
        tags::CONTENT_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(items),
    ));
    object
        .with_meta(
            FileMetaTableBuilder::new()
                .media_storage_sop_class_uid(COMPREHENSIVE_SR)
                .media_storage_sop_instance_uid(SR_INSTANCE)
                .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN),
        )
        .expect("file meta")
        .write_to_file(path)
        .expect("write SR document");
}

/// SC1 and the SR document SR1, whose root holds an English language
/// modifier, with a DICOMDIR made by dcmmkdir.
fn sr_file_set() -> FileSet {
    FileSet::create(&["SC1", "SR1"], &[], |file_set| {
        write_instance(
            &file_set.member("SC1"),
            &FIRST,
            uids::EXPLICIT_VR_LITTLE_ENDIAN,
        );
        write_sr(&file_set.member("SR1"), vec![language("eng", "English")]);
    })
}

#[test]
fn an_sr_document_record_holds_the_root_concept_modifiers() {
    let file_set = sr_file_set();
    assert!(file_set
        .record("SR DOCUMENT")
        .get(tags::CONTENT_SEQUENCE)
        .is_some());
    let check = file_set.check();
    assert_passes(&check);
    assert!(
        !check.message.contains("advisory"),
        "the record's Content Items are validated: {}",
        check.message
    );
    // A second root modifier the record lacks.
    write_sr(
        &file_set.member("SR1"),
        vec![language("eng", "English"), language("fra", "French")],
    );
    assert_fails(
        &file_set.check(),
        "lacks 1 of the HAS CONCEPT MOD Content Items of the root",
    );
    // A different modifier: the record's is not the root's.
    write_sr(&file_set.member("SR1"), vec![language("deu", "German")]);
    let check = file_set.check();
    assert_fails(&check, "lacks 1 of the HAS CONCEPT MOD Content Items");
    assert_fails(
        &check,
        "holds 1 Content Item that is not a HAS CONCEPT MOD Content Item",
    );
    assert_fails(&check, "(PS3.3 Table F.5-25)");
}

#[test]
fn a_key_object_doc_record_holds_only_the_root_concept_modifiers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("KO1");
    write_sr(&path, Vec::new());
    let document = dicom_object::OpenFileOptions::new()
        .open_file(&path)
        .unwrap();
    let record = record_of_type(756, "KEY OBJECT DOC");
    let mut parsed = InMemDicomObject::new_empty();
    parsed.put(DataElement::new(
        tags::CONTENT_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![language("eng", "English")]),
    ));
    let mut family = Family::default();
    super::content::check_content_sequence(&record, &parsed, &path, &document, &mut family);
    let message = family
        .check(Path::new("DICOMDIR"), "rule", "citation")
        .message;
    assert!(
        message.contains("holds 1 Content Item that is not a HAS CONCEPT MOD Content Item")
            && message.contains("(PS3.3 Table F.5-26)"),
        "{message}"
    );
}

/// A HAS CONCEPT MOD TEXT Content Item holding `text`.
fn text_modifier(text: &str) -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    put(
        &mut item,
        tags::RELATIONSHIP_TYPE,
        VR::CS,
        "HAS CONCEPT MOD",
    );
    put(&mut item, tags::VALUE_TYPE, VR::CS, "TEXT");
    item.put(code_sequence(
        tags::CONCEPT_NAME_CODE_SEQUENCE,
        "121106",
        "DCM",
        "Comment",
    ));
    put(&mut item, tags::TEXT_VALUE, VR::UT, text);
    item
}

#[test]
fn record_content_items_follow_the_structured_report_rules() {
    let content = |items: Vec<InMemDicomObject>| {
        let mut parsed = InMemDicomObject::new_empty();
        parsed.put(DataElement::new(
            tags::CONTENT_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(items),
        ));
        parsed
    };
    let messages = |parsed: InMemDicomObject| {
        let mut family = Family::default();
        super::content::check_content_items(
            &[record_of_type(756, "SR DOCUMENT")],
            &[parsed],
            &mut family,
        );
        family
            .check(Path::new("DICOMDIR"), "rule", "citation")
            .message
    };
    let valid = messages(content(vec![
        language("eng", "English"),
        text_modifier("plain"),
    ]));
    assert!(valid.ends_with("conformant"), "{valid}");
    let broken = messages(content(vec![
        language("eng", "English"),
        text_modifier("a\tb"),
    ]));
    assert!(
        broken.contains(
            "the record at byte 756: Content Sequence[2] > Text Value (0040,A160) holds HT"
        ) && broken.contains("Table C.17-5"),
        "{broken}"
    );
    // dcmmkdir copies the root's modifiers into the record, the broken one
    // with them.
    let file_set = FileSet::create(&["SC1", "SR1"], &[], |file_set| {
        write_instance(
            &file_set.member("SC1"),
            &FIRST,
            uids::EXPLICIT_VR_LITTLE_ENDIAN,
        );
        write_sr(&file_set.member("SR1"), vec![text_modifier("a\tb")]);
    });
    let record = file_set.record("SR DOCUMENT");
    assert_fails(
        &file_set.check(),
        &format!(
            "the record at byte {}: Content Sequence[1] > Text Value (0040,A160) holds HT",
            record.offset
        ),
    );
}

#[test]
fn out_of_scope_and_unknown_record_types_are_named_as_not_validated() {
    let file_set = FileSet::new();
    let directory = file_set.directory();
    let images: Vec<usize> = directory
        .records
        .iter()
        .filter(|record| record.record_type() == "IMAGE")
        .map(|record| record.offset as usize)
        .collect();
    file_set.patch(images[0], b"IMAGE ", b"PLAN  ");
    let check = file_set.check();
    assert_passes(&check);
    assert!(
        check.message.contains("1 record of Directory Record Type \"PLAN\" not validated: it references objects outside the pathology-slide scope"),
        "{}",
        check.message
    );
    file_set.patch(images[1], b"IMAGE ", b"VISIT ");
    let check = file_set.check();
    assert!(
        check.message.contains(
            "\"VISIT\" not validated: it is not a Directory Record Type of PS3.3 Table F.4-1"
        ),
        "{}",
        check.message
    );
}
