use std::fs;
use std::path::{Path, PathBuf};

use crate::Error;

pub(super) fn collect_wsi_candidate_paths(
    root: &Path,
    max_sources: usize,
    max_depth: usize,
) -> Result<Vec<PathBuf>, Error> {
    let root_metadata = fs::symlink_metadata(root).map_err(|source| Error::Io {
        path: root.to_path_buf(),
        source,
    })?;
    if root_metadata.file_type().is_symlink() {
        return Err(Error::Unsupported {
            reason: format!("corpus coverage refuses symlink root {}", root.display()),
        });
    }
    if root_metadata.is_file() {
        if !is_wsi_candidate_path(root) {
            return Ok(Vec::new());
        }
        if max_sources == 0 {
            return Err(Error::Unsupported {
                reason: "corpus coverage found more than max_sources=0 candidate files".into(),
            });
        }
        return Ok(vec![root.to_path_buf()]);
    }
    if !root_metadata.is_dir() {
        return Err(Error::Unsupported {
            reason: format!(
                "corpus coverage root is not a file or directory: {}",
                root.display()
            ),
        });
    }

    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut candidates = Vec::new();
    while let Some((directory, depth)) = pending.pop() {
        if depth > max_depth {
            return Err(Error::Unsupported {
                reason: format!(
                    "corpus coverage directory depth exceeds max_depth={} at {}",
                    max_depth,
                    directory.display()
                ),
            });
        }
        let entries = fs::read_dir(&directory).map_err(|source| Error::Io {
            path: directory.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| Error::Io {
                path: directory.clone(),
                source,
            })?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            if file_type.is_symlink() {
                return Err(Error::Unsupported {
                    reason: format!(
                        "corpus coverage refuses symlink traversal at {}",
                        path.display()
                    ),
                });
            }
            if file_type.is_dir() {
                pending.try_reserve(1).map_err(|_| Error::Unsupported {
                    reason: "corpus coverage directory queue exceeds available memory".into(),
                })?;
                pending.push((path, depth + 1));
            } else if file_type.is_file() && is_wsi_candidate_path(&path) {
                candidates.try_reserve(1).map_err(|_| Error::Unsupported {
                    reason: "corpus coverage candidate list exceeds available memory".into(),
                })?;
                candidates.push(path);
                if candidates.len() > max_sources {
                    return Err(Error::Unsupported {
                        reason: format!(
                            "corpus coverage found more than max_sources={} candidate files",
                            max_sources
                        ),
                    });
                }
            }
        }
    }
    candidates.sort();
    Ok(candidates)
}

fn is_wsi_candidate_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("svs" | "tif" | "tiff" | "ndpi" | "scn" | "dcm" | "mrxs" | "vms" | "vmu")
    )
}
