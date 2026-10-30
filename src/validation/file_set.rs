//! File-sets (PS3.10 chapter 8) and their DICOMDIR (PS3.3 Annex F Basic
//! Directory IOD): what spans the DICOMDIR's records and the File-set's
//! files. The Basic Directory IOD's Module, Attribute and Record Selection
//! Key tables are the IOD checks'; this rule holds the File-set to the rest:
//!
//! - PS3.10 8.6: the DICOMDIR is a Media Storage Directory in Explicit VR
//!   Little Endian;
//! - PS3.3 F.3.2.2 and F.2.1: every record offset points at an Item of the
//!   Directory Record Sequence, the Directory Entities form a tree reached
//!   from the Root Directory Entity, and Table F.4-1 allows each record below
//!   its parent;
//! - PS3.10 8.2, 8.5, 8.6 and PS3.3 F.2.1: every Referenced File ID is a
//!   valid File ID naming one file inside the File-set, referenced once, and
//!   the record and its ancestors agree with that file;
//! - PS3.3 F.5: Patient, Study and Series keys are unique in the File-set,
//!   the IMAGE, SR DOCUMENT, KEY OBJECT DOC and ANNOTATION records of the
//!   pathology objects reference SOP Instances of their classes, and the
//!   Content Sequence of an SR DOCUMENT or KEY OBJECT DOC record holds all,
//!   and only, the HAS CONCEPT MOD Content Items of its document's root,
//!   each held to the PS3.3 C.17 and C.18 Content Item rules;
//! - PS3.3 F.7: Icon Images are monochrome or palette color, unsigned, of 1
//!   or 8 bits, with square pixels;
//! - PS3.3 F.2.1: with a Directory Information Module, every DICOM File of
//!   the File-set is referenced. PS3.10 8.1 lets non-DICOM files go
//!   unreferenced.
//!
//! What it does not validate it names in its message as an advisory: records
//! of a Directory Record Type outside the pathology-slide scope, and record
//! Content Sequences not compared with a document.
//!
//! The DICOMDIR is untrusted: its size, record count and nesting are
//! bounded, and a File ID or link leading out of the File-set root is
//! refused and reported before anything it names is opened.

mod consistency;
mod content;
mod file_id;
mod icons;
pub(in crate::validation) mod listing;
mod members;
mod notes;
mod records;
mod structure;
#[cfg(test)]
pub(super) mod tests;

use std::io::Read;
use std::path::{Path, PathBuf};

use dicom_core::Tag;

use super::disclosure::shown;
use super::iod::file_set::basic_directory_sop_class;
use super::iod::{Family, EDITION};
use super::{intrinsic_check, ValidationCheck, ValidationOptions, ValidationStatus};
use consistency::{Context, Root};
use notes::Notes;
use records::Directory;

const FILE_SET_RULE: &str = "intrinsic-wsi-dicom-file-set";
/// PS3.10 8.6: the reserved File ID of the DICOMDIR File.
const DICOMDIR: &str = "DICOMDIR";
/// Largest DICOMDIR File read, whatever `max_input_bytes` admits.
const MAX_DICOMDIR_BYTES: u64 = 256 * 1024 * 1024;
const MEDIA_STORAGE_SOP_CLASS: Tag = Tag(0x0002, 0x0002);
const MEDIA_STORAGE_SOP_INSTANCE: Tag = Tag(0x0002, 0x0003);
const FILE_SET_ID: Tag = Tag(0x0004, 0x1130);

fn citation() -> String {
    format!("DICOM PS3.10 {EDITION} chapter 8 and PS3.3 {EDITION} Annex F")
}

/// The File-set check of each of `roots`, the directories discovery found
/// holding an entry named DICOMDIR (PS3.10 8.6). `files` are the DICOM
/// files discovery found; those below a root and outside any File-set nested
/// in it are that File-set's own.
pub(super) fn run_file_set_check(
    roots: &[PathBuf],
    files: &[PathBuf],
    options: &ValidationOptions,
) -> Vec<ValidationCheck> {
    roots
        .iter()
        .map(|root| check_root(root, &own_files(root, roots, files), options))
        .collect()
}

/// The files of `files` in the File-set rooted at `root`: below it, and
/// not below another of `roots` nested in it (PS3.10 8.1).
fn own_files<'f>(root: &Path, roots: &[PathBuf], files: &'f [PathBuf]) -> Vec<&'f Path> {
    let nested: Vec<&PathBuf> = roots
        .iter()
        .filter(|other| other.as_path() != root && other.starts_with(root))
        .collect();
    files
        .iter()
        .filter(|file| {
            file.starts_with(root) && !nested.iter().any(|other| file.starts_with(other))
        })
        .map(PathBuf::as_path)
        .collect()
}

/// The File-set check of `root`. Discovery found the root and its DICOMDIR;
/// both are checked again here, as either may have changed since.
fn check_root(root: &Path, files: &[&Path], options: &ValidationOptions) -> ValidationCheck {
    match std::fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return failure(
                root,
                format!(
                    "security: refused: the File-set root {} is a symbolic link",
                    root.display()
                ),
            );
        }
        Ok(metadata) if !metadata.is_dir() => {
            return failure(
                root,
                format!("the File-set root {} is not a directory", root.display()),
            );
        }
        Ok(_) => {}
        Err(err) => {
            return failure(
                root,
                format!("the File-set root {} cannot be read: {err}", root.display()),
            );
        }
    }
    let path = root.join(DICOMDIR);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(err) => return failure(&path, format!("the DICOMDIR cannot be read: {err}")),
    };
    if metadata.file_type().is_symlink() {
        return failure(
            &path,
            "security: refused: the DICOMDIR is a symbolic link, which could lead outside the \
             File-set root (PS3.10 8.6)"
                .to_string(),
        );
    }
    if !metadata.is_file() {
        return failure(
            &path,
            "the DICOMDIR is not a regular file (PS3.10 8.6)".to_string(),
        );
    }
    check_file_set(root, &path, files, options)
}

fn check_file_set(
    root: &Path,
    path: &Path,
    files: &[&Path],
    options: &ValidationOptions,
) -> ValidationCheck {
    let canonical_root = match root.canonicalize() {
        Ok(canonical) => canonical,
        Err(err) => {
            return failure(path, format!("the File-set root cannot be resolved: {err}"));
        }
    };
    let (bytes, directory) = match read_dicomdir(path, options) {
        Ok(read) => read,
        Err(reason) => return failure(path, reason),
    };
    let mut family = Family::default();
    let mut notes = Notes::default();
    check_media_storage(&directory, &mut family);
    let tree = structure::check_structure(&directory, &mut family);
    icons::check_icons(&directory.records, &mut family);
    notes.record_types(&directory.records);
    let parsed = content::parse_records(&bytes, &directory.records);
    if let Ok(parsed) = &parsed {
        content::check_content_items(&directory.records, parsed, &mut family);
    }
    let context = Context {
        root: Root {
            path: root,
            canonical: &canonical_root,
        },
        options,
        parsed: parsed.as_deref().map_err(String::as_str),
    };
    let referenced =
        consistency::check_references(&context, &directory, &tree, &mut family, &mut notes);
    let members =
        members::check_members(&context.root, &directory, &referenced, files, &mut family);
    let citation = citation();
    let mut check = if family.is_empty() {
        intrinsic_check(
            FILE_SET_RULE,
            Some(path.to_path_buf()),
            ValidationStatus::Passed,
            format!(
                "{citation}: {} Directory Record{}, {} referenced File{}, {members} DICOM File{} \
                 in the File-set",
                directory.records.len(),
                plural(directory.records.len()),
                referenced.len(),
                plural(referenced.len()),
                plural(members)
            ),
        )
    } else {
        family.check(path, FILE_SET_RULE, &citation)
    };
    check.message.push_str(&notes.suffix());
    check
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

fn failure(path: &Path, reason: String) -> ValidationCheck {
    let mut family = Family::default();
    family.push(|| reason);
    family.check(path, FILE_SET_RULE, &citation())
}

/// Reads the DICOMDIR at `path`, bounded by `max_input_bytes` and
/// [`MAX_DICOMDIR_BYTES`], and parses it; returns its bytes as well.
fn read_dicomdir(path: &Path, options: &ValidationOptions) -> Result<(Vec<u8>, Directory), String> {
    let limit = options.max_input_bytes.min(MAX_DICOMDIR_BYTES);
    let file =
        std::fs::File::open(path).map_err(|err| format!("the DICOMDIR cannot be opened: {err}"))?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|err| format!("the DICOMDIR cannot be read: {err}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!(
            "the DICOMDIR exceeds {limit} bytes, the most this check reads"
        ));
    }
    let directory = records::parse(&bytes)
        .map_err(|reason| format!("the DICOMDIR cannot be parsed: {reason}"))?;
    Ok((bytes, directory))
}

/// PS3.10 8.6: the DICOMDIR's File Meta Information names the Media Storage
/// Directory Storage SOP Class and, as SOP Instance, the File-set UID.
fn check_media_storage(directory: &Directory, family: &mut Family) {
    let expected = basic_directory_sop_class();
    let class = directory
        .meta
        .get(&MEDIA_STORAGE_SOP_CLASS)
        .map(records::Value::text);
    if class.as_deref() != Some(expected) {
        family.push(|| {
            format!(
                "Media Storage SOP Class UID (0002,0002) of the DICOMDIR is {}, but shall be the \
                 Media Storage Directory Storage SOP Class {expected} (PS3.10 8.6)",
                class.map_or_else(
                    || "absent".to_string(),
                    |class| shown(MEDIA_STORAGE_SOP_CLASS, &class).to_string()
                )
            )
        });
    }
    let instance = directory
        .meta
        .get(&MEDIA_STORAGE_SOP_INSTANCE)
        .map(records::Value::text)
        .unwrap_or_default();
    if instance.is_empty() {
        family.push(|| {
            "Media Storage SOP Instance UID (0002,0003) of the DICOMDIR, the File-set UID, is \
             absent or empty (PS3.10 8.6)"
                .to_string()
        });
    }
    check_file_set_id(directory, family);
}

/// PS3.10 8.1 and 8.5: a File-set ID is zero to sixteen characters of
/// uppercase A-Z, 0-9 and underscore; unlike CS, it admits no SPACE.
fn check_file_set_id(directory: &Directory, family: &mut Family) {
    let Some(value) = directory.data_set.get(&FILE_SET_ID) else {
        return;
    };
    let id = value.text();
    let length = id.chars().count();
    let outside = id
        .chars()
        .enumerate()
        .find(|(_, character)| !file_id::in_repertoire(*character));
    if length > 16 || outside.is_some() {
        let shown = shown(FILE_SET_ID, &id);
        family.push(|| {
            format!(
                "File-set ID (0004,1130) {shown} has {length} characters{}, but a File-set ID is \
                 zero to 16 characters of uppercase A-Z, 0-9 and _ (PS3.10 8.1, 8.5)",
                outside.map_or_else(String::new, |(index, character)| format!(
                    " including {}",
                    shown.character(character, index + 1)
                ))
            )
        });
    }
}
