use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::Error;

pub(super) struct PendingDicomOutput {
    final_path: PathBuf,
    temp_file: tempfile::NamedTempFile,
    overwrite: bool,
}

impl PendingDicomOutput {
    pub(super) fn create(final_path: &Path, overwrite: bool) -> Result<Self, Error> {
        validate_final_output_path(final_path, overwrite)?;
        let parent = output_parent_dir(final_path);
        let temp_file = tempfile::Builder::new()
            .prefix(".wsi-dicom-output-")
            .suffix(".tmp")
            .tempfile_in(parent)
            .map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        Ok(Self {
            final_path: final_path.to_path_buf(),
            temp_file,
            overwrite,
        })
    }

    pub(super) fn path(&self) -> &Path {
        self.temp_file.path()
    }

    pub(super) fn reopen(&self) -> Result<File, Error> {
        self.temp_file.reopen().map_err(|source| Error::Io {
            path: self.temp_file.path().to_path_buf(),
            source,
        })
    }

    pub(super) fn persist(self) -> Result<(), Error> {
        if self.overwrite {
            reject_symlink_output_path(&self.final_path)?;
        }
        let final_path = self.final_path.clone();
        let result = if self.overwrite {
            self.temp_file.persist(&final_path)
        } else {
            self.temp_file.persist_noclobber(&final_path)
        };
        result.map(|_| ()).map_err(|error| Error::Io {
            path: final_path,
            source: error.error,
        })
    }
}

fn output_parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn validate_final_output_path(path: &Path, overwrite: bool) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(_) if !overwrite => Err(Error::Io {
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::AlreadyExists, "output path already exists"),
        }),
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::Io {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "refusing to overwrite symlink output path",
            ),
        }),
        Ok(metadata) if !metadata.file_type().is_file() => Err(Error::Io {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "refusing to overwrite non-file output path",
            ),
        }),
        Ok(_) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn reject_symlink_output_path(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::Io {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "refusing to overwrite symlink output path",
            ),
        }),
        Ok(_) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(super) fn flush_and_sync_dicom_writer(
    file: &mut BufWriter<File>,
    path: &Path,
) -> Result<(), Error> {
    file.flush().map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.get_ref().sync_all().map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}
