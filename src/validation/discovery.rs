use std::path::{Path, PathBuf};

use crate::Error;

use super::ValidationOptions;

pub(super) fn discover_dicom_files(
    input: &Path,
    options: &ValidationOptions,
) -> Result<Vec<PathBuf>, Error> {
    let metadata = std::fs::symlink_metadata(input).map_err(|source| Error::Io {
        path: input.to_path_buf(),
        source,
    })?;
    let mut files = Vec::new();
    if metadata.file_type().is_symlink() {
        return Err(Error::Validation {
            reason: format!("refusing to validate symlink path {}", input.display()),
        });
    } else if metadata.is_file() {
        files.push(input.to_path_buf());
    } else if metadata.is_dir() {
        collect_dicom_files(input, options, &mut files)?;
        files.sort();
    } else {
        return Err(Error::Validation {
            reason: format!("{} is not a regular file or directory", input.display()),
        });
    }
    if files.len() > options.max_files {
        return Err(Error::Validation {
            reason: format!(
                "DICOM validation found more than max_files={} files",
                options.max_files
            ),
        });
    }
    if files.is_empty() {
        return Err(Error::Validation {
            reason: format!("no .dcm files found under {}", input.display()),
        });
    }
    Ok(files)
}

fn collect_dicom_files(
    root: &Path,
    options: &ValidationOptions,
    files: &mut Vec<PathBuf>,
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
            } else if file_type.is_dir() {
                pending.push((path, depth + 1));
            } else if file_type.is_file() && has_dcm_extension(&path) {
                files.push(path);
                if files.len() > options.max_files {
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
