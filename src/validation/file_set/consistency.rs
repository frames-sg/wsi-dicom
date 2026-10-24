//! Each Directory Record that references a File: its Referenced File ID
//! (PS3.10 8.2, 8.5, 8.6) resolves to a regular file inside the File-set
//! that no other record references (PS3.3 F.2.1), and the record's
//! Referenced SOP Class, SOP Instance and Transfer Syntax UIDs in File
//! (PS3.3 F.3.2.2) and the Keys of the record and its ancestors (PS3.3 F.5)
//! agree with that file, the Content Sequence of an SR DOCUMENT or KEY
//! OBJECT DOC record included (Tables F.5-25 and F.5-26).

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::content::{check_content_sequence, has_content_key};
use super::file_id::{self, Unresolved};
use super::notes::Notes;
use super::records::{Directory, Record, Value, REFERENCED_FILE_ID};
use super::structure::Tree;
use crate::validation::iod::{string_value, Family};
use crate::validation::is_image_storage_sop_class;
use crate::validation::ValidationOptions;

const REFERENCED_SOP_CLASS: Tag = Tag(0x0004, 0x1510);
const REFERENCED_SOP_INSTANCE: Tag = Tag(0x0004, 0x1511);
const REFERENCED_TRANSFER_SYNTAX: Tag = Tag(0x0004, 0x1512);
const DESCRIPTOR_FILE_ID: Tag = Tag(0x0004, 0x1141);
/// Segmentation, Label Map Segmentation and Parametric Map Storage: IODs
/// with an Image IE that PS3.6 does not list as Image Storage.
const IMAGE_IE_SOP_CLASSES: &[&str] = &[
    "1.2.840.10008.5.1.4.1.1.66.4",
    "1.2.840.10008.5.1.4.1.1.66.7",
    "1.2.840.10008.5.1.4.1.1.30",
];
/// The PS3.4 Structured Reporting Storage SOP Classes share this root.
const SR_SOP_CLASS_ROOT: &str = "1.2.840.10008.5.1.4.1.1.88.";
const KEY_OBJECT_SELECTION: &str = "1.2.840.10008.5.1.4.1.1.88.59";
const MICROSCOPY_BULK_SIMPLE_ANNOTATIONS: &str = "1.2.840.10008.5.1.4.1.1.91.1";

/// Record Keys compared with the referenced file's Data Set, wherever in
/// the record's lineage they appear.
const KEYS: &[(Tag, &str)] = &[
    (tags::PATIENT_NAME, "Patient's Name (0010,0010)"),
    (tags::PATIENT_ID, "Patient ID (0010,0020)"),
    (tags::STUDY_INSTANCE_UID, "Study Instance UID (0020,000D)"),
    (tags::SERIES_INSTANCE_UID, "Series Instance UID (0020,000E)"),
    (tags::MODALITY, "Modality (0008,0060)"),
    (tags::INSTANCE_NUMBER, "Instance Number (0020,0013)"),
];

/// The File-set root, as given and canonical.
pub(super) struct Root<'a> {
    pub(super) path: &'a Path,
    pub(super) canonical: &'a Path,
}

/// What the reference checks read besides the raw records.
pub(super) struct Context<'a> {
    pub(super) root: Root<'a>,
    pub(super) options: &'a ValidationOptions,
    /// The records as dicom-object reads them, aligned with the raw ones,
    /// or why they could not be read.
    pub(super) parsed: Result<&'a [InMemDicomObject], &'a str>,
}

/// Checks every record's Referenced File and the File-set Descriptor File;
/// returns the canonical paths of the files they reference.
pub(super) fn check_references(
    context: &Context<'_>,
    directory: &Directory,
    tree: &Tree,
    family: &mut Family,
    notes: &mut Notes,
) -> BTreeSet<PathBuf> {
    let root = &context.root;
    let mut referenced = BTreeSet::new();
    if let Some(value) = directory.data_set.get(&DESCRIPTOR_FILE_ID) {
        let what = "File-set Descriptor File ID (0004,1141)";
        if let Some(path) = resolve(root, value, what, family) {
            referenced.insert(path);
        }
    }
    for (index, record) in directory.records.iter().enumerate() {
        let Some(value) = record.get(REFERENCED_FILE_ID) else {
            uncompared(record, "it references no File", notes);
            continue;
        };
        let what = format!(
            "Referenced File ID (0004,1500) of the record at byte {}",
            record.offset
        );
        let Some(path) = resolve(root, value, &what, family) else {
            uncompared(record, "its Referenced File ID does not resolve", notes);
            continue;
        };
        if !referenced.insert(path.clone()) {
            uncompared(record, "its File is referenced by another record", notes);
            family.push(|| {
                format!(
                    "{what} names {}, which another Directory Record already references, but \
                     any File of the File-set shall be referenced by at most one Directory \
                     Record (PS3.3 F.2.1)",
                    path.display()
                )
            });
            continue;
        }
        let lineage: Vec<&Record> = tree
            .lineage(index)
            .into_iter()
            .map(|position| &directory.records[position])
            .collect();
        let referenced_file = ReferencedFile {
            path: &path,
            lineage: &lineage,
            parsed: context.parsed.map(|records| &records[index]),
        };
        referenced_file.check(context.options, family, notes);
    }
    referenced
}

/// Notes that the Content Sequence of an SR DOCUMENT or KEY OBJECT DOC
/// `record` is not compared with a document, and why.
fn uncompared(record: &Record, why: &str, notes: &mut Notes) {
    let record_type = record.record_type();
    if has_content_key(&record_type) {
        notes.note(format!(
            "the Content Sequence of the {record_type} record at byte {} is not compared with \
             a document (PS3.3 Tables F.5-25, F.5-26): {why}",
            record.offset
        ));
    }
}

/// The canonical path of the file `value`'s File ID names, after its
/// PS3.10 8.2 and 8.5 defects are recorded; `None` when it does not resolve.
fn resolve(root: &Root<'_>, value: &Value, what: &str, family: &mut Family) -> Option<PathBuf> {
    let components = value.texts();
    let shown = components.join("\\");
    for defect in file_id::defects(&components) {
        family.push(|| format!("{what} \"{}\" {defect}", shown.escape_default()));
    }
    match file_id::resolve(root.path, root.canonical, &components) {
        Ok(path) => Some(path),
        Err(unresolved) => {
            let reason = match &unresolved {
                Unresolved::Escape(_) => "security: ",
                Unresolved::Missing(_) => "",
            };
            family.push(|| {
                format!(
                    "{reason}{what} \"{}\" {}; every referenced File shall exist in the File-set \
                     (PS3.10 8.6, PS3.3 F.2.1)",
                    shown.escape_default(),
                    unresolved.message()
                )
            });
            None
        }
    }
}

/// A record's referenced file, with the record (`lineage[0]`), its
/// ancestors and the record as dicom-object reads it.
struct ReferencedFile<'a> {
    path: &'a Path,
    lineage: &'a [&'a Record],
    parsed: Result<&'a InMemDicomObject, &'a str>,
}

impl ReferencedFile<'_> {
    /// Compares the record and its ancestors with the file.
    fn check(&self, options: &ValidationOptions, family: &mut Family, notes: &mut Notes) {
        let Some(object) = self.open(options, family) else {
            uncompared(self.lineage[0], "its File cannot be read", notes);
            return;
        };
        let record = self.lineage[0];
        check_reference_uids(record, self.path, &object, family);
        check_record_class(
            record,
            self.path,
            trim_uid(object.meta().media_storage_sop_class_uid()),
            family,
        );
        check_keys(self.path, self.lineage, &object, family);
        if has_content_key(&record.record_type()) {
            match self.parsed {
                Ok(parsed) => check_content_sequence(record, parsed, self.path, &object, family),
                Err(reason) => uncompared(record, reason, notes),
            }
        }
    }

    fn open(&self, options: &ValidationOptions, family: &mut Family) -> Option<DefaultDicomObject> {
        match open(self.path, options.max_input_bytes) {
            Ok(object) => Some(object),
            Err(reason) => {
                family.push(|| {
                    format!(
                        "the record at byte {} references {}, which {reason} (PS3.3 F.3.2.2)",
                        self.lineage[0].offset,
                        self.path.display()
                    )
                });
                None
            }
        }
    }
}

/// PS3.3 F.3.2.2: the record's Referenced SOP Class, SOP Instance and
/// Transfer Syntax UIDs in File agree with the File Meta Information and
/// the Data Set of the file at `path`.
fn check_reference_uids(
    record: &Record,
    path: &Path,
    object: &DefaultDicomObject,
    family: &mut Family,
) {
    let meta = object.meta();
    let class = string_value(object, tags::SOP_CLASS_UID, 1);
    let instance = string_value(object, tags::SOP_INSTANCE_UID, 1);
    let comparisons = [
        (
            REFERENCED_SOP_CLASS,
            "Referenced SOP Class UID in File (0004,1510)",
            vec![
                (
                    "Media Storage SOP Class UID (0002,0002)",
                    Some(meta.media_storage_sop_class_uid()),
                ),
                ("SOP Class UID (0008,0016)", class.as_deref()),
            ],
        ),
        (
            REFERENCED_SOP_INSTANCE,
            "Referenced SOP Instance UID in File (0004,1511)",
            vec![
                (
                    "Media Storage SOP Instance UID (0002,0003)",
                    Some(meta.media_storage_sop_instance_uid()),
                ),
                ("SOP Instance UID (0008,0018)", instance.as_deref()),
            ],
        ),
        (
            REFERENCED_TRANSFER_SYNTAX,
            "Referenced Transfer Syntax UID in File (0004,1512)",
            vec![(
                "Transfer Syntax UID (0002,0010)",
                Some(meta.transfer_syntax()),
            )],
        ),
    ];
    for (tag, name, file_values) in comparisons {
        let Some(value) = record.get(tag) else {
            family.push(|| {
                format!(
                    "{name}, required as the record at byte {} references a SOP Instance, is \
                     absent (PS3.3 F.3.2.2)",
                    record.offset
                )
            });
            continue;
        };
        let expected = value.text();
        for (file_name, found) in file_values {
            let found = found.map(trim_uid);
            if found != Some(expected.as_str()) {
                family.push(|| mismatch(record, path, name, &expected, file_name, found));
            }
        }
    }
}

/// PS3.3 F.5.4, F.5.25, F.5.26 and F.5.47: the SOP Class each Directory
/// Record Type of the pathology objects shall be used to reference. An
/// Image SOP Instance is one of a PS3.6 Image Storage SOP Class or of an IOD
/// with an Image IE that PS3.6 lists apart (Segmentation, Parametric Map).
/// Other record types reference objects outside the pathology scope.
pub(super) fn check_record_class(record: &Record, path: &Path, class: &str, family: &mut Family) {
    let record_type = record.record_type();
    let (allowed, section, what) = match record_type.as_str() {
        "IMAGE" => (
            is_image_storage_sop_class(class) || IMAGE_IE_SOP_CLASSES.contains(&class),
            "F.5.4",
            "an Image SOP Instance",
        ),
        "SR DOCUMENT" => (
            class.starts_with(SR_SOP_CLASS_ROOT) && class != KEY_OBJECT_SELECTION,
            "F.5.25",
            "an SR Document",
        ),
        "KEY OBJECT DOC" => (
            class == KEY_OBJECT_SELECTION,
            "F.5.26",
            "a Key Object Selection Document",
        ),
        "ANNOTATION" => (
            class == MICROSCOPY_BULK_SIMPLE_ANNOTATIONS,
            "F.5.47",
            "a Microscopy Bulk Simple Annotations SOP Instance",
        ),
        _ => return,
    };
    if !allowed {
        family.push(|| {
            format!(
                "the {record_type} record at byte {} references {}, of SOP Class {class}, but \
                 this Directory Record shall be used to reference {what} (PS3.3 {section})",
                record.offset,
                path.display()
            )
        });
    }
}

/// PS3.3 F.5: the record Keys of `lineage` agree with the file's values.
fn check_keys(path: &Path, lineage: &[&Record], object: &DefaultDicomObject, family: &mut Family) {
    for ancestor in lineage {
        for &(tag, name) in KEYS {
            let Some(value) = ancestor.get(tag) else {
                continue;
            };
            let expected = value.text();
            let found = string_value(object, tag, 1);
            if !same_key(tag, &expected, found.as_deref()) {
                family.push(|| {
                    format!(
                        "{name} of the {} record at byte {} is \"{}\", but {} has {} (PS3.3 F.5)",
                        ancestor.record_type(),
                        ancestor.offset,
                        expected.escape_default(),
                        path.display(),
                        found.as_deref().map_or_else(
                            || "none".to_string(),
                            |found| format!("\"{}\"", found.escape_default())
                        )
                    )
                });
            }
        }
    }
}

/// Whether a record Key equals the file's Value: Instance Number (IS) as an
/// integer, the others as text without padding.
fn same_key(tag: Tag, expected: &str, found: Option<&str>) -> bool {
    let Some(found) = found else {
        return expected.is_empty();
    };
    let found = found.trim_matches([' ', '\0']);
    if tag == tags::INSTANCE_NUMBER {
        if let (Ok(left), Ok(right)) = (expected.parse::<i64>(), found.parse::<i64>()) {
            return left == right;
        }
    }
    expected == found
}

fn mismatch(
    record: &Record,
    path: &Path,
    name: &str,
    expected: &str,
    file_name: &str,
    found: Option<&str>,
) -> String {
    format!(
        "{name} of the record at byte {} is \"{}\", but {file_name} of {} is {} (PS3.3 F.3.2.2)",
        record.offset,
        expected.escape_default(),
        path.display(),
        found.map_or_else(
            || "absent".to_string(),
            |found| format!("\"{}\"", found.escape_default())
        )
    )
}

fn trim_uid(value: &str) -> &str {
    value.trim_end_matches(['\0', ' '])
}

/// Opens `path` as a DICOM File up to its Pixel Data, reading at most
/// `max_input_bytes`.
fn open(path: &Path, max_input_bytes: u64) -> Result<DefaultDicomObject, String> {
    let file = std::fs::File::open(path).map_err(|err| format!("cannot be opened: {err}"))?;
    let length = file
        .metadata()
        .map_err(|err| format!("cannot be inspected: {err}"))?
        .len();
    if length > max_input_bytes {
        return Err(format!(
            "has {length} bytes, exceeding max_input_bytes={max_input_bytes}"
        ));
    }
    dicom_object::OpenFileOptions::new()
        .read_preamble(dicom_object::file::ReadPreamble::Always)
        .read_until(tags::PIXEL_DATA)
        .from_reader(file.take(max_input_bytes))
        .map_err(|err| format!("cannot be read as a DICOM File: {err}"))
}
