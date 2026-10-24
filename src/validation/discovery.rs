use std::io::Read;
use std::path::{Path, PathBuf};

use crate::Error;

use super::ValidationOptions;

/// PS3.10 8.6: the reserved File ID of the DICOMDIR File.
const DICOMDIR: &str = "DICOMDIR";

/// What one walk of the input finds.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Discovered {
    /// The DICOM files, named .dcm or holding the PS3.10 DICM prefix, sorted.
    pub(super) files: Vec<PathBuf>,
    /// The File-set roots: each directory the walk visits that holds an entry
    /// named DICOMDIR (PS3.10 8.6), whatever that entry is, so the File-set
    /// check reports one that is not a regular file. Sorted; empty for a
    /// single-file input.
    pub(super) file_set_roots: Vec<PathBuf>,
}

pub(super) fn discover_dicom_files(
    input: &Path,
    options: &ValidationOptions,
) -> Result<Discovered, Error> {
    let metadata = std::fs::symlink_metadata(input).map_err(|source| Error::Io {
        path: input.to_path_buf(),
        source,
    })?;
    let mut found = Discovered::default();
    if metadata.file_type().is_symlink() {
        return Err(Error::Validation {
            reason: format!("refusing to validate symlink path {}", input.display()),
        });
    } else if metadata.is_file() {
        found.files.push(input.to_path_buf());
    } else if metadata.is_dir() {
        collect_dicom_files(input, options, &mut found)?;
        found.files.sort();
        found.file_set_roots.sort();
        found.file_set_roots.dedup();
    } else {
        return Err(Error::Validation {
            reason: format!("{} is not a regular file or directory", input.display()),
        });
    }
    if found.files.len() > options.max_files {
        return Err(Error::Validation {
            reason: format!(
                "DICOM validation found more than max_files={} files",
                options.max_files
            ),
        });
    }
    if found.files.is_empty() {
        return Err(Error::Validation {
            reason: format!(
                "no DICOM files (named .dcm, or holding the PS3.10 DICM prefix) found under {}",
                input.display()
            ),
        });
    }
    Ok(found)
}

/// Walks the directories below `root`, at most `max_depth` deep, refusing
/// symbolic links. Only DICOM files count against `max_files`.
fn collect_dicom_files(
    root: &Path,
    options: &ValidationOptions,
    found: &mut Discovered,
) -> Result<(), Error> {
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = pending.pop() {
        if depth > options.max_depth {
            return Err(Error::Validation {
                reason: format!(
                    "DICOM validation directory depth exceeds max_depth={} at {}",
                    options.max_depth,
                    dir.display()
                ),
            });
        }
        let entries = std::fs::read_dir(&dir).map_err(|source| Error::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| Error::Io {
                path: dir.clone(),
                source,
            })?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            if file_type.is_symlink() {
                return Err(Error::Validation {
                    reason: format!("refusing to traverse symlink {}", path.display()),
                });
            }
            if entry.file_name() == DICOMDIR {
                found.file_set_roots.push(dir.clone());
            }
            if file_type.is_dir() {
                pending.push((path, depth + 1));
            } else if file_type.is_file() && (has_dcm_extension(&path) || has_part10_prefix(&path)?)
            {
                found.files.push(path);
                if found.files.len() > options.max_files {
                    return Err(Error::Validation {
                        reason: format!(
                            "DICOM validation found more than max_files={} files",
                            options.max_files
                        ),
                    });
                }
            }
        }
    }
    Ok(())
}

fn has_dcm_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("dcm"))
}

/// The bytes of the PS3.10 File Preamble and DICM prefix (PS3.10 7.1).
const PREFIX_END: usize = 132;

/// Whether the file holds "DICM" at byte offset 128, after the 128-byte
/// File Preamble (PS3.10 7.1): a File-set's files have no extension, since a
/// File ID component holds no "." (PS3.10 8.5). At most 132 bytes are read.
fn has_part10_prefix(path: &Path) -> Result<bool, Error> {
    let file = std::fs::File::open(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut prefix = Vec::with_capacity(PREFIX_END);
    file.take(PREFIX_END as u64)
        .read_to_end(&mut prefix)
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(prefix.len() == PREFIX_END && &prefix[128..] == b"DICM")
}
