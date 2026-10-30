//! The Basic Directory IOD (PS3.3 Annex F) of a DICOMDIR: a File-set's
//! directory, built here like the one `dcmmkdir` writes for a VL WSI File,
//! passes every IOD check, each Directory Record is held to the Record
//! Selection Keys its Directory Record Type selects, and breaking one
//! Attribute fails the check of its family. The same DICOMDIR, written by
//! `write_dicomdir_fixture`, has no dciodvfy errors.

use super::*;

const EXPLICIT_VR_LITTLE_ENDIAN: &str = "1.2.840.10008.1.2.1";
const MEDIA_STORAGE_DIRECTORY_STORAGE: &str = "1.2.840.10008.1.3.10";
const VL_WSI_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.77.1.6";
const JPEG_2000_LOSSLESS: &str = "1.2.840.10008.1.2.4.90";
const FILE_SET_UID: &str = "1.2.826.0.1.3680043.10.999.40.1";
/// Directory `write_dicomdir_fixture` writes the DICOMDIR to.
const FIXTURE_DIR_VARIABLE: &str = "WSI_DICOM_IOD_FIXTURES";

fn us(tag: Tag, value: u16) -> InMemElement {
    DataElement::new(tag, VR::US, PrimitiveValue::from(value))
}

fn ul(tag: Tag, value: u32) -> InMemElement {
    DataElement::new(tag, VR::UL, PrimitiveValue::from(value))
}

/// The Directory Elements of one record (Table F.3-3) and its keys. Offsets
/// are left zero: the IOD tables bound their presence and VR, and the
/// File-set rule their Values.
fn record(record_type: &str, keys: Vec<InMemElement>) -> InMemDicomObject {
    let mut elements = vec![
        ul(tags::OFFSET_OF_THE_NEXT_DIRECTORY_RECORD, 0),
        us(tags::RECORD_IN_USE_FLAG, 0xFFFF),
        ul(tags::OFFSET_OF_REFERENCED_LOWER_LEVEL_DIRECTORY_ENTITY, 0),
        text(tags::DIRECTORY_RECORD_TYPE, VR::CS, record_type),
    ];
    elements.extend(keys);
    item(elements)
}

fn patient() -> InMemDicomObject {
    record(
        "PATIENT",
        vec![
            text(tags::PATIENT_NAME, VR::PN, "RESEARCH^PLACEHOLDER"),
            text(tags::PATIENT_ID, VR::LO, "RESEARCH"),
        ],
    )
}

fn study() -> InMemDicomObject {
    record(
        "STUDY",
        vec![
            text(tags::STUDY_DATE, VR::DA, "19700101"),
            text(tags::STUDY_TIME, VR::TM, "000000"),
            text(tags::ACCESSION_NUMBER, VR::SH, "RESEARCH"),
            text(
                tags::STUDY_DESCRIPTION,
                VR::LO,
                "Research placeholder WSI export",
            ),
            text(
                tags::STUDY_INSTANCE_UID,
                VR::UI,
                "1.2.826.0.1.3680043.10.999.40.2",
            ),
            text(tags::STUDY_ID, VR::SH, "1"),
        ],
    )
}

fn series() -> InMemDicomObject {
    record(
        "SERIES",
        vec![
            text(tags::MODALITY, VR::CS, "SM"),
            text(
                tags::SERIES_INSTANCE_UID,
                VR::UI,
                "1.2.826.0.1.3680043.10.999.40.3",
            ),
            text(tags::SERIES_NUMBER, VR::IS, "1"),
        ],
    )
}

fn image() -> InMemDicomObject {
    record(
        "IMAGE",
        vec![
            DataElement::new(
                tags::REFERENCED_FILE_ID,
                VR::CS,
                PrimitiveValue::Strs(["IMAGES".to_string(), "IM0001".to_string()].into()),
            ),
            text(
                tags::REFERENCED_SOP_CLASS_UID_IN_FILE,
                VR::UI,
                VL_WSI_STORAGE,
            ),
            text(
                tags::REFERENCED_SOP_INSTANCE_UID_IN_FILE,
                VR::UI,
                "1.2.826.0.1.3680043.10.999.40.4",
            ),
            text(
                tags::REFERENCED_TRANSFER_SYNTAX_UID_IN_FILE,
                VR::UI,
                JPEG_2000_LOSSLESS,
            ),
            text(tags::INSTANCE_NUMBER, VR::IS, "1"),
        ],
    )
}

/// The DICOMDIR of one VL WSI File: PATIENT, STUDY, SERIES and IMAGE records.
fn dicomdir(records: Vec<InMemDicomObject>) -> InMemDicomObject {
    item(vec![
        text(tags::FILE_SET_ID, VR::CS, "WSI_DICOM"),
        ul(
            tags::OFFSET_OF_THE_FIRST_DIRECTORY_RECORD_OF_THE_ROOT_DIRECTORY_ENTITY,
            0,
        ),
        ul(
            tags::OFFSET_OF_THE_LAST_DIRECTORY_RECORD_OF_THE_ROOT_DIRECTORY_ENTITY,
            0,
        ),
        us(tags::FILE_SET_CONSISTENCY_FLAG, 0),
        sq(tags::DIRECTORY_RECORD_SEQUENCE, records),
    ])
}

fn records() -> Vec<InMemDicomObject> {
    vec![patient(), study(), series(), image()]
}

/// Writes `object` as `name`, its File Meta Information that of a DICOMDIR
/// (PS3.10 8.6): Media Storage Directory Storage and the File-set UID.
fn write(dir: &Path, name: &str, object: InMemDicomObject) -> PathBuf {
    let file = object
        .with_meta(
            FileMetaTableBuilder::new()
                .transfer_syntax(EXPLICIT_VR_LITTLE_ENDIAN)
                .media_storage_sop_class_uid(MEDIA_STORAGE_DIRECTORY_STORAGE)
                .media_storage_sop_instance_uid(FILE_SET_UID),
        )
        .expect("file meta");
    let path = dir.join(name);
    file.write_to_file(&path).expect("write DICOMDIR");
    path
}

/// Replaces the record at `index` of the Directory Record Sequence.
fn with_record(index: usize, edit: impl FnOnce(&mut InMemDicomObject)) -> InMemDicomObject {
    let mut records = records();
    edit(&mut records[index]);
    dicomdir(records)
}

#[test]
fn a_dicomdir_passes_every_iod_check() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(tmp.path(), "DICOMDIR", dicomdir(records()));
    let report = iod_report(&path, ValidationProfile::General);
    for name in IOD_CHECKS {
        let check = named(&report, name);
        assert_eq!(
            check.status,
            ValidationStatus::Passed,
            "{name}: {}",
            check.message
        );
    }
    assert!(named(&report, "intrinsic-wsi-dicom-iod-modules")
        .message
        .contains("F.3 Table F.3-1"));
}

/// Writes the DICOMDIR for dciodvfy:
/// `WSI_DICOM_IOD_FIXTURES=<dir> cargo test write_dicomdir_fixture -- --ignored`.
#[test]
#[ignore = "writes the DICOMDIR to $WSI_DICOM_IOD_FIXTURES for dciodvfy"]
fn write_dicomdir_fixture() {
    let dir = std::env::var_os(FIXTURE_DIR_VARIABLE)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("set {FIXTURE_DIR_VARIABLE} to the output directory"));
    std::fs::create_dir_all(&dir).unwrap();
    write(&dir, "DICOMDIR", dicomdir(records()));
}

/// One broken DICOMDIR and the failure it causes.
struct Breakage {
    case: &'static str,
    check: &'static str,
    expected: &'static str,
    object: fn() -> InMemDicomObject,
}

const BREAKAGES: [Breakage; 10] = [
    Breakage {
        case: "no-file-set-identification",
        check: "intrinsic-wsi-dicom-iod-modules",
        expected: "File-set Identification Module (PS3.3 F.3.2.1, Usage M): required by Table F.3-1",
        object: || {
            let mut object = dicomdir(records());
            object.remove_element(tags::FILE_SET_ID);
            object
        },
    },
    Breakage {
        case: "image-without-file-id",
        check: "intrinsic-wsi-dicom-iod-attribute-presence",
        expected: "Directory Record Sequence[4] > Referenced File ID (0004,1500): Type 1C missing",
        object: || {
            with_record(3, |record| {
                record.remove_element(tags::REFERENCED_FILE_ID);
            })
        },
    },
    // The PATIENT record's Record Selection Keys (Table F.5-1).
    Breakage {
        case: "patient-without-id",
        check: "intrinsic-wsi-dicom-iod-attribute-presence",
        expected: "Directory Record Sequence[1] > Patient ID (0010,0020): Type 1 missing (PS3.3 F.5.1 Table F.5-1)",
        object: || {
            with_record(0, |record| {
                record.remove_element(tags::PATIENT_ID);
            })
        },
    },
    // "Type shall be defined by Private Record UID (0004,1432)."
    Breakage {
        case: "private-record-without-uid",
        check: "intrinsic-wsi-dicom-iod-attribute-presence",
        expected: "Private Record UID (0004,1432): Type 1C missing",
        object: || dicomdir(vec![record("PRIVATE", Vec::new())]),
    },
    // "The Value 0000H shall never be present."
    Breakage {
        case: "record-not-in-use",
        check: "intrinsic-wsi-dicom-iod-enumerated-values",
        expected: "Record In-use Flag (0004,1410) \"0\" is not an Enumerated Value [65535]",
        object: || {
            with_record(1, |record| {
                record.put(us(tags::RECORD_IN_USE_FLAG, 0));
            })
        },
    },
    // "Other Values are reserved and shall not be set by File-set Creators".
    Breakage {
        case: "record-reserved-flag",
        check: "intrinsic-wsi-dicom-iod-enumerated-values",
        expected: "Record In-use Flag (0004,1410) \"1\" is not an Enumerated Value [65535]",
        object: || {
            with_record(1, |record| {
                record.put(us(tags::RECORD_IN_USE_FLAG, 1));
            })
        },
    },
    // "The Value FFFFH shall never be present."
    Breakage {
        case: "inconsistent-file-set",
        check: "intrinsic-wsi-dicom-iod-enumerated-values",
        expected: "File-set Consistency Flag (0004,1212) \"65535\" is not an Enumerated Value [0]",
        object: || {
            let mut object = dicomdir(records());
            object.put(us(tags::FILE_SET_CONSISTENCY_FLAG, 0xFFFF));
            object
        },
    },
    Breakage {
        case: "file-id-of-nine-components",
        check: "intrinsic-wsi-dicom-iod-vr-vm",
        expected: "(0004,1500)",
        object: || {
            with_record(3, |record| {
                let components = (1..=9).map(|index| format!("D{index}")).collect::<Vec<_>>();
                record.put(DataElement::new(
                    tags::REFERENCED_FILE_ID,
                    VR::CS,
                    PrimitiveValue::Strs(components.into()),
                ));
            })
        },
    },
    Breakage {
        case: "study-date-format",
        check: "intrinsic-wsi-dicom-iod-value-format",
        expected: "(0008,0020)",
        object: || {
            with_record(1, |record| {
                record.put(text(tags::STUDY_DATE, VR::DA, "1970-01-01"));
            })
        },
    },
    // An SR DOCUMENT record's Concept Name Code Sequence holds one Item (Table F.5-25).
    Breakage {
        case: "sr-record-two-titles",
        check: "intrinsic-wsi-dicom-iod-sequence-items",
        expected: "Concept Name Code Sequence (0040,A043): 2 Items where exactly 1 is required (PS3.3 F.5.25 Table F.5-25)",
        object: || {
            let mut records = records();
            let title = || code("DCM", "126000", "Imaging Measurement Report");
            records.push(record(
                "SR DOCUMENT",
                vec![
                    text(tags::INSTANCE_NUMBER, VR::IS, "2"),
                    text(tags::COMPLETION_FLAG, VR::CS, "COMPLETE"),
                    text(tags::VERIFICATION_FLAG, VR::CS, "UNVERIFIED"),
                    text(tags::CONTENT_DATE, VR::DA, "19700101"),
                    text(tags::CONTENT_TIME, VR::TM, "000000"),
                    sq(tags::CONCEPT_NAME_CODE_SEQUENCE, vec![title(), title()]),
                ],
            ));
            dicomdir(records)
        },
    },
];

#[test]
fn breaking_a_dicomdir_attribute_of_each_family_fails_its_check() {
    let tmp = tempfile::tempdir().unwrap();
    for breakage in BREAKAGES {
        let path = write(tmp.path(), breakage.case, (breakage.object)());
        let check = iod_check(&path, ValidationProfile::General, breakage.check);
        assert_eq!(
            check.status,
            ValidationStatus::Failed,
            "{}: {}",
            breakage.case,
            check.message
        );
        assert!(
            check.message.contains(breakage.expected),
            "{} message lacks {:?}: {}",
            breakage.case,
            breakage.expected,
            check.message
        );
    }
}

#[test]
fn a_record_is_held_only_to_the_keys_its_type_selects() {
    let tmp = tempfile::tempdir().unwrap();
    // A SERIES record keyed like a PATIENT record misses the Series keys only.
    let path = write(
        tmp.path(),
        "series-keyed-as-patient",
        with_record(2, |record| {
            record.remove_element(tags::SERIES_INSTANCE_UID);
            record.put(text(tags::PATIENT_ID, VR::LO, "RESEARCH"));
        }),
    );
    let check = iod_check(
        &path,
        ValidationProfile::General,
        "intrinsic-wsi-dicom-iod-attribute-presence",
    );
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
    assert!(
        check.message.contains(
            "Directory Record Sequence[3] > Series Instance UID (0020,000E): Type 1 missing"
        ),
        "{}",
        check.message
    );
    assert!(!check.message.contains("Patient ID"), "{}", check.message);
    // A PATIENT record does not reference a File, so it shall not name one.
    let path = write(
        tmp.path(),
        "patient-names-a-file",
        with_record(0, |record| {
            record.put(text(tags::REFERENCED_FILE_ID, VR::CS, "IM0001"));
        }),
    );
    assert_iod_failure(
        &path,
        "intrinsic-wsi-dicom-iod-attribute-presence",
        "Directory Record Sequence[1] > Referenced File ID (0004,1500): Type 1C present although its condition is not met",
    );
}
