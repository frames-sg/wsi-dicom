//! The File-set's files (PS3.3 F.2.1): every DICOM File is referenced by a
//! Directory Record when the DICOMDIR has a Directory Information Module.
//! PS3.10 8.1 lets non-DICOM files go unreferenced. The files are those
//! discovery found, which refuses every symbolic link; each is checked again
//! here, as it may have changed since.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::consistency::Root;
use super::records::Directory;
use super::DICOMDIR;
use crate::validation::iod::Family;

/// Reports, when the DICOMDIR has a Directory Information Module, each DICOM
/// File of `files`, the File-set's own, that no record references (PS3.3
/// F.2.1), and each of them that is now a link out of the root. Returns the
/// number of DICOM Files besides the DICOMDIR.
pub(super) fn check_members(
    root: &Root<'_>,
    directory: &Directory,
    referenced: &BTreeSet<PathBuf>,
    files: &[&Path],
    family: &mut Family,
) -> usize {
    let dicomdir = root.canonical.join(DICOMDIR);
    let mut members = 0usize;
    for path in files {
        match Member::of(path, root.canonical) {
            Member::EscapingLink(reason) | Member::Unreadable(reason) => family.push(|| reason),
            Member::Other => {}
            Member::Dicom(canonical) if canonical == dicomdir => {}
            Member::Dicom(canonical) => {
                members += 1;
                if directory.has_records && !referenced.contains(&canonical) {
                    family.push(|| unreferenced(path));
                }
            }
        }
    }
    members
}

fn unreferenced(path: &Path) -> String {
    format!(
        "{} is a DICOM File of the File-set, but no Directory Record references it; with a \
         Directory Information Module, all DICOM Files of the File-set shall be referenced by a \
         Directory Record (PS3.3 F.2.1)",
        path.display()
    )
}

/// What one file of the File-set is.
enum Member {
    /// A DICOM File (a "DICM" prefix after the File Preamble), canonical.
    Dicom(PathBuf),
    /// A symbolic link whose target is outside the root or unresolvable.
    EscapingLink(String),
    Unreadable(String),
    /// A non-DICOM file, such as one named .dcm without the prefix, or a
    /// link inside the root, which discovery would have refused.
    Other,
}

impl Member {
    fn of(path: &Path, canonical_root: &Path) -> Self {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(err) => {
                return Self::Unreadable(format!("{} cannot be read: {err}", path.display()))
            }
        };
        if metadata.file_type().is_symlink() {
            return match path.canonicalize() {
                Ok(target) if target.starts_with(canonical_root) => Self::Other,
                Ok(target) => Self::EscapingLink(format!(
                    "security: {} is a symbolic link to {}, outside the File-set root; it is not \
                     followed (PS3.10 8.6)",
                    path.display(),
                    target.display()
                )),
                Err(err) => Self::EscapingLink(format!(
                    "security: {} is a symbolic link that does not resolve ({err}); it is not \
                     followed",
                    path.display()
                )),
            };
        }
        if !metadata.is_file() {
            return Self::Other;
        }
        match has_dicom_prefix(path) {
            Ok(false) => Self::Other,
            Ok(true) => match path.canonicalize() {
                Ok(canonical) => Self::Dicom(canonical),
                Err(err) => {
                    Self::Unreadable(format!("{} cannot be resolved: {err}", path.display()))
                }
            },
            Err(err) => Self::Unreadable(format!("{} cannot be read: {err}", path.display())),
        }
    }
}

/// PS3.10 7.1: a DICOM File has "DICM" after its 128-byte File Preamble.
fn has_dicom_prefix(path: &Path) -> std::io::Result<bool> {
    let mut head = Vec::with_capacity(132);
    std::fs::File::open(path)?
        .take(132)
        .read_to_end(&mut head)?;
    Ok(head.len() == 132 && &head[128..] == b"DICM")
}
