//! The Directory Records of a File-set for the rules that hold a record to
//! what they hold the file it references to, such as the de-identification
//! rule (PS3.15 E.1.1). It reads the DICOMDIR within the File-set check's
//! bounds and resolves Referenced File IDs as that check does; the check
//! reports the File-set's defects, this listing only reads it.

use std::path::{Path, PathBuf};

use dicom_object::InMemDicomObject;

use super::records::REFERENCED_FILE_ID;
use super::{content, file_id, read_dicomdir, structure, DICOMDIR};
use crate::validation::iod::Family;
use crate::validation::ValidationOptions;

/// One Directory Record of a File-set's DICOMDIR.
#[derive(Debug, Clone)]
pub(in crate::validation) struct DirectoryRecord {
    /// The byte offset of its Item in the DICOMDIR.
    pub(in crate::validation) offset: u64,
    /// Its Directory Record Type (0004,1430), e.g. "IMAGE"; "" when absent.
    pub(in crate::validation) record_type: String,
    /// The indices, in the listing, of the records above it, the record of
    /// the Root Directory Entity first (e.g. PATIENT, STUDY, SERIES). Empty
    /// for a record of the Root Directory Entity and for one no Directory
    /// Entity reaches.
    pub(in crate::validation) ancestry: Vec<usize>,
    /// Its own Attributes, as dicom-object reads its Item.
    pub(in crate::validation) attributes: InMemDicomObject,
    /// The canonical path of the file its Referenced File ID (0004,1500)
    /// names, when that resolves to a regular file inside the File-set root
    /// without leaving it.
    pub(in crate::validation) referenced_file: Option<PathBuf>,
}

/// The Directory Records of the DICOMDIR of the File-set rooted at `root`,
/// in the order of the Directory Record Sequence, read within the bounds of
/// `options`. The error says why the DICOMDIR cannot be listed.
pub(in crate::validation) fn directory_records(
    root: &Path,
    options: &ValidationOptions,
) -> Result<Vec<DirectoryRecord>, String> {
    let path = root.join(DICOMDIR);
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|err| format!("{} cannot be read: {err}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "{} is not a regular file (a symbolic link is refused)",
            path.display()
        ));
    }
    let canonical_root = root.canonicalize().map_err(|err| {
        format!(
            "the File-set root {} cannot be resolved: {err}",
            root.display()
        )
    })?;
    let (bytes, directory) = read_dicomdir(&path, options)?;
    let parsed = content::parse_records(&bytes, &directory.records)?;
    let tree = structure::check_structure(&directory, &mut Family::default());
    let listing = directory
        .records
        .iter()
        .zip(parsed)
        .enumerate()
        .map(|(index, (record, attributes))| {
            let mut ancestry = tree.lineage(index);
            ancestry.remove(0);
            ancestry.reverse();
            DirectoryRecord {
                offset: record.offset,
                record_type: record.record_type(),
                ancestry,
                attributes,
                referenced_file: record
                    .get(REFERENCED_FILE_ID)
                    .and_then(|value| file_id::resolve(root, &canonical_root, &value.texts()).ok()),
            }
        })
        .collect();
    Ok(listing)
}
