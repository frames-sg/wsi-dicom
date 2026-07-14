use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::report::InstanceReport;

const LOCK_FILE_NAME: &str = ".wsi-dicom-export.lock";
const TRANSACTION_PREFIX: &str = ".wsi-dicom-transaction-";
const MANIFEST_FILE_NAME: &str = "manifest.json";
const MANIFEST_LIMIT_BYTES: u64 = 1024 * 1024;

pub(super) struct OutputDirectoryLock {
    _file: File,
}

impl OutputDirectoryLock {
    pub(super) fn acquire(output_dir: &Path) -> Result<Self, Error> {
        let path = output_dir.join(LOCK_FILE_NAME);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(Error::Io {
                    path,
                    source: io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "export lock path must not be a symbolic link",
                    ),
                });
            }
            Ok(metadata) if !metadata.file_type().is_file() => {
                return Err(Error::Io {
                    path,
                    source: io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "export lock path must be a regular file",
                    ),
                });
            }
            Ok(_) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(Error::Io { path, source });
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
        file.try_lock().map_err(|source| Error::Io {
            path: path.clone(),
            source: io::Error::new(
                io::ErrorKind::WouldBlock,
                format!("another export is already using this output directory: {source}"),
            ),
        })?;
        Ok(Self { _file: file })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TransactionPhase {
    Prepared,
    Committing,
    Committed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EntryState {
    Staged,
    BackupMoved,
    Installed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TransactionEntry {
    name: String,
    had_original: bool,
    state: EntryState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TransactionManifest {
    version: u8,
    phase: TransactionPhase,
    entries: Vec<TransactionEntry>,
}

pub(super) struct ExportTransaction {
    output_dir: PathBuf,
    directory: Option<tempfile::TempDir>,
}

impl ExportTransaction {
    pub(super) fn begin(output_dir: &Path) -> Result<Self, Error> {
        recover_abandoned_transactions(output_dir)?;
        let directory = tempfile::Builder::new()
            .prefix(TRANSACTION_PREFIX)
            .tempdir_in(output_dir)
            .map_err(|source| Error::Io {
                path: output_dir.to_path_buf(),
                source,
            })?;
        Ok(Self {
            output_dir: output_dir.to_path_buf(),
            directory: Some(directory),
        })
    }

    pub(super) fn staging_dir(&self) -> &Path {
        self.directory
            .as_ref()
            .expect("transaction directory is retained until commit returns")
            .path()
    }

    pub(super) fn commit(
        mut self,
        reports: &mut [InstanceReport],
        overwrite: bool,
    ) -> Result<(), Error> {
        self.commit_inner(reports, overwrite, None)
    }

    fn commit_inner(
        &mut self,
        reports: &mut [InstanceReport],
        overwrite: bool,
        fail_after_installs: Option<usize>,
    ) -> Result<(), Error> {
        let transaction_dir = self.staging_dir().to_path_buf();
        let backup_dir = transaction_dir.join("backups");
        fs::create_dir(&backup_dir).map_err(|source| Error::Io {
            path: backup_dir.clone(),
            source,
        })?;

        let mut manifest = TransactionManifest {
            version: 1,
            phase: TransactionPhase::Prepared,
            entries: prepare_entries(reports, &transaction_dir, &self.output_dir, overwrite)?,
        };
        write_manifest(&transaction_dir, &manifest)?;
        manifest.phase = TransactionPhase::Committing;
        write_manifest(&transaction_dir, &manifest)?;

        let commit_result = (|| {
            commit_entries(
                &self.output_dir,
                &transaction_dir,
                &mut manifest,
                fail_after_installs,
            )?;
            sync_directory(&self.output_dir)?;
            manifest.phase = TransactionPhase::Committed;
            write_manifest(&transaction_dir, &manifest)
        })();
        if let Err(commit_error) = commit_result {
            return match rollback_entries(&self.output_dir, &transaction_dir, &manifest) {
                Ok(()) => Err(commit_error),
                Err(rollback_error) => {
                    let recovery_path = self.keep_directory();
                    Err(Error::ExportTransaction {
                        recovery_path,
                        reason: format!(
                            "commit failed ({commit_error}); rollback also failed ({rollback_error})"
                        ),
                    })
                }
            };
        }

        for report in reports {
            let name = staged_file_name(&report.path, &transaction_dir)?;
            report.path = self.output_dir.join(name);
        }
        Ok(())
    }

    fn keep_directory(&mut self) -> PathBuf {
        self.directory
            .take()
            .expect("transaction directory is present during rollback")
            .keep()
    }
}

fn prepare_entries(
    reports: &[InstanceReport],
    transaction_dir: &Path,
    output_dir: &Path,
    overwrite: bool,
) -> Result<Vec<TransactionEntry>, Error> {
    let mut entries = Vec::with_capacity(reports.len());
    for report in reports {
        let name = staged_file_name(&report.path, transaction_dir)?;
        let staged = transaction_dir.join(&name);
        let metadata = fs::symlink_metadata(&staged).map_err(|source| Error::Io {
            path: staged.clone(),
            source,
        })?;
        if !metadata.file_type().is_file() || metadata.len() == 0 {
            return Err(Error::DicomWrite {
                path: staged,
                message: "staged DICOM output must be a non-empty regular file".into(),
            });
        }

        let final_path = output_dir.join(&name);
        let had_original = match fs::symlink_metadata(&final_path) {
            Ok(metadata) => {
                if !overwrite {
                    return Err(Error::Io {
                        path: final_path,
                        source: io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            "output file appeared during export; enable overwrite to replace it",
                        ),
                    });
                }
                if !metadata.file_type().is_file() {
                    return Err(Error::Io {
                        path: final_path,
                        source: io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "overwrite destination must be a regular file and not a symlink",
                        ),
                    });
                }
                true
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => false,
            Err(source) => {
                return Err(Error::Io {
                    path: final_path,
                    source,
                });
            }
        };
        entries.push(TransactionEntry {
            name,
            had_original,
            state: EntryState::Staged,
        });
    }
    Ok(entries)
}

fn staged_file_name(path: &Path, transaction_dir: &Path) -> Result<String, Error> {
    if path.parent() != Some(transaction_dir) {
        return Err(Error::DicomWrite {
            path: path.to_path_buf(),
            message: "staged report path escaped the export transaction directory".into(),
        });
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && !name.contains(['/', '\\']))
        .map(str::to_owned)
        .ok_or_else(|| Error::DicomWrite {
            path: path.to_path_buf(),
            message: "staged DICOM output has an invalid file name".into(),
        })
}

fn commit_entries(
    output_dir: &Path,
    transaction_dir: &Path,
    manifest: &mut TransactionManifest,
    fail_after_installs: Option<usize>,
) -> Result<(), Error> {
    for index in 0..manifest.entries.len() {
        if fail_after_installs == Some(index) {
            return Err(Error::Io {
                path: output_dir.to_path_buf(),
                source: io::Error::other("injected export commit failure"),
            });
        }
        let entry = &manifest.entries[index];
        let final_path = output_dir.join(&entry.name);
        let staged_path = transaction_dir.join(&entry.name);
        let backup_path = transaction_dir.join("backups").join(&entry.name);
        if entry.had_original {
            fs::rename(&final_path, &backup_path).map_err(|source| Error::Io {
                path: final_path.clone(),
                source,
            })?;
            manifest.entries[index].state = EntryState::BackupMoved;
            write_manifest(transaction_dir, manifest)?;
        }
        fs::rename(&staged_path, &final_path).map_err(|source| Error::Io {
            path: final_path,
            source,
        })?;
        manifest.entries[index].state = EntryState::Installed;
        write_manifest(transaction_dir, manifest)?;
    }
    Ok(())
}

fn rollback_entries(
    output_dir: &Path,
    transaction_dir: &Path,
    manifest: &TransactionManifest,
) -> Result<(), Error> {
    for entry in manifest.entries.iter().rev() {
        let staged_path = transaction_dir.join(&entry.name);
        let final_path = output_dir.join(&entry.name);
        let backup_path = transaction_dir.join("backups").join(&entry.name);
        let backup_exists = backup_path.try_exists().map_err(|source| Error::Io {
            path: backup_path.clone(),
            source,
        })?;
        let staged_exists = staged_path.try_exists().map_err(|source| Error::Io {
            path: staged_path.clone(),
            source,
        })?;

        if backup_exists {
            remove_installed_file_if_present(&final_path)?;
            fs::rename(&backup_path, &final_path).map_err(|source| Error::Io {
                path: final_path,
                source,
            })?;
        } else if !staged_exists {
            if entry.had_original {
                return Err(Error::ExportTransaction {
                    recovery_path: transaction_dir.to_path_buf(),
                    reason: format!(
                        "backup for overwritten output {} is missing",
                        final_path.display()
                    ),
                });
            }
            remove_installed_file_if_present(&final_path)?;
        }
    }
    sync_directory(output_dir)
}

fn remove_installed_file_if_present(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => {
            fs::remove_file(path).map_err(|source| Error::Io {
                path: path.to_path_buf(),
                source,
            })
        }
        Ok(_) => Err(Error::Io {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "transaction destination changed to a non-regular file",
            ),
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn write_manifest(transaction_dir: &Path, manifest: &TransactionManifest) -> Result<(), Error> {
    let path = transaction_dir.join(MANIFEST_FILE_NAME);
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|err| Error::JsonSerialize {
        message: format!("failed to serialize export transaction manifest: {err}"),
    })?;
    if bytes.len() as u64 > MANIFEST_LIMIT_BYTES {
        return Err(Error::ExportTransaction {
            recovery_path: transaction_dir.to_path_buf(),
            reason: "export transaction manifest exceeds the 1 MiB safety limit".into(),
        });
    }
    let mut temporary = tempfile::Builder::new()
        .prefix(".manifest-")
        .tempfile_in(transaction_dir)
        .map_err(|source| Error::Io {
            path: transaction_dir.to_path_buf(),
            source,
        })?;
    temporary.write_all(&bytes).map_err(|source| Error::Io {
        path: temporary.path().to_path_buf(),
        source,
    })?;
    temporary.as_file().sync_all().map_err(|source| Error::Io {
        path: temporary.path().to_path_buf(),
        source,
    })?;
    temporary.persist(&path).map_err(|error| Error::Io {
        path,
        source: error.error,
    })?;
    sync_directory(transaction_dir)
}

fn read_manifest(transaction_dir: &Path) -> Result<TransactionManifest, Error> {
    let path = transaction_dir.join(MANIFEST_FILE_NAME);
    let metadata = fs::metadata(&path).map_err(|source| Error::Io {
        path: path.clone(),
        source,
    })?;
    if metadata.len() > MANIFEST_LIMIT_BYTES {
        return Err(Error::ExportTransaction {
            recovery_path: transaction_dir.to_path_buf(),
            reason: "abandoned transaction manifest exceeds the 1 MiB safety limit".into(),
        });
    }
    let bytes = fs::read(&path).map_err(|source| Error::Io {
        path: path.clone(),
        source,
    })?;
    let manifest: TransactionManifest =
        serde_json::from_slice(&bytes).map_err(|source| Error::Json {
            path: path.clone(),
            source,
        })?;
    if manifest.version != 1 {
        return Err(Error::ExportTransaction {
            recovery_path: transaction_dir.to_path_buf(),
            reason: format!(
                "unsupported transaction manifest version {}",
                manifest.version
            ),
        });
    }
    validate_manifest_names(transaction_dir, &manifest)?;
    Ok(manifest)
}

fn validate_manifest_names(
    transaction_dir: &Path,
    manifest: &TransactionManifest,
) -> Result<(), Error> {
    for entry in &manifest.entries {
        let path = Path::new(&entry.name);
        if entry.name.is_empty()
            || path.file_name().and_then(|name| name.to_str()) != Some(entry.name.as_str())
        {
            return Err(Error::ExportTransaction {
                recovery_path: transaction_dir.to_path_buf(),
                reason: "transaction manifest contains an unsafe output file name".into(),
            });
        }
    }
    Ok(())
}

fn recover_abandoned_transactions(output_dir: &Path) -> Result<(), Error> {
    let entries = fs::read_dir(output_dir).map_err(|source| Error::Io {
        path: output_dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| Error::Io {
            path: output_dir.to_path_buf(),
            source,
        })?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(TRANSACTION_PREFIX)
            || !entry
                .file_type()
                .map_err(|source| Error::Io {
                    path: entry.path(),
                    source,
                })?
                .is_dir()
        {
            continue;
        }
        let transaction_dir = entry.path();
        let manifest_path = transaction_dir.join(MANIFEST_FILE_NAME);
        if !manifest_path.try_exists().map_err(|source| Error::Io {
            path: manifest_path.clone(),
            source,
        })? {
            fs::remove_dir_all(&transaction_dir).map_err(|source| Error::ExportTransaction {
                recovery_path: transaction_dir.clone(),
                reason: format!(
                    "failed to remove abandoned pre-commit staging directory: {source}"
                ),
            })?;
            continue;
        }
        let manifest = read_manifest(&transaction_dir)?;
        if manifest.phase != TransactionPhase::Committed {
            rollback_entries(output_dir, &transaction_dir, &manifest)?;
        }
        fs::remove_dir_all(&transaction_dir).map_err(|source| Error::ExportTransaction {
            recovery_path: transaction_dir.clone(),
            reason: format!("failed to clean recovered export transaction: {source}"),
        })?;
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), Error> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| Error::Io {
                path: path.to_path_buf(),
                source,
            })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(path: PathBuf) -> InstanceReport {
        InstanceReport {
            path,
            ..InstanceReport::default()
        }
    }

    #[test]
    fn commit_installs_all_staged_files_and_updates_reports() {
        let output = tempfile::tempdir().unwrap();
        let transaction = ExportTransaction::begin(output.path()).unwrap();
        let staged = transaction.staging_dir().join("one.dcm");
        fs::write(&staged, b"new").unwrap();
        let mut reports = vec![report(staged)];

        transaction.commit(&mut reports, false).unwrap();

        assert_eq!(fs::read(output.path().join("one.dcm")).unwrap(), b"new");
        assert_eq!(reports[0].path, output.path().join("one.dcm"));
    }

    #[test]
    fn commit_failure_restores_every_overwritten_file() {
        let output = tempfile::tempdir().unwrap();
        fs::write(output.path().join("one.dcm"), b"old-one").unwrap();
        fs::write(output.path().join("two.dcm"), b"old-two").unwrap();
        let mut transaction = ExportTransaction::begin(output.path()).unwrap();
        let first = transaction.staging_dir().join("one.dcm");
        let second = transaction.staging_dir().join("two.dcm");
        fs::write(&first, b"new-one").unwrap();
        fs::write(&second, b"new-two").unwrap();
        let mut reports = vec![report(first), report(second)];

        transaction
            .commit_inner(&mut reports, true, Some(1))
            .expect_err("injected failure should roll back");

        assert_eq!(fs::read(output.path().join("one.dcm")).unwrap(), b"old-one");
        assert_eq!(fs::read(output.path().join("two.dcm")).unwrap(), b"old-two");
    }

    #[test]
    fn commit_failure_removes_newly_installed_files() {
        let output = tempfile::tempdir().unwrap();
        let mut transaction = ExportTransaction::begin(output.path()).unwrap();
        let first = transaction.staging_dir().join("one.dcm");
        let second = transaction.staging_dir().join("two.dcm");
        fs::write(&first, b"new-one").unwrap();
        fs::write(&second, b"new-two").unwrap();
        let mut reports = vec![report(first), report(second)];

        transaction
            .commit_inner(&mut reports, false, Some(1))
            .expect_err("injected failure should roll back");

        assert!(!output.path().join("one.dcm").exists());
        assert!(!output.path().join("two.dcm").exists());
    }

    #[test]
    fn output_lock_rejects_concurrent_export() {
        let output = tempfile::tempdir().unwrap();
        let _first = OutputDirectoryLock::acquire(output.path()).unwrap();
        let error = OutputDirectoryLock::acquire(output.path())
            .err()
            .expect("second lock should fail");
        assert!(error.to_string().contains("another export"));
    }

    #[cfg(unix)]
    #[test]
    fn output_lock_rejects_symlink_without_modifying_target() {
        let output = tempfile::tempdir().unwrap();
        let target = output.path().join("target");
        fs::write(&target, b"do not modify").unwrap();
        std::os::unix::fs::symlink(&target, output.path().join(LOCK_FILE_NAME)).unwrap();

        let error = OutputDirectoryLock::acquire(output.path())
            .err()
            .expect("symlink lock should fail");

        assert!(error.to_string().contains("symbolic link"));
        assert_eq!(fs::read(target).unwrap(), b"do not modify");
    }

    #[test]
    fn next_transaction_recovers_an_interrupted_overwrite_commit() {
        let output = tempfile::tempdir().unwrap();
        let final_path = output.path().join("one.dcm");
        fs::write(&final_path, b"old").unwrap();
        let mut interrupted = ExportTransaction::begin(output.path()).unwrap();
        let transaction_dir = interrupted.staging_dir().to_path_buf();
        let staged_path = transaction_dir.join("one.dcm");
        let backup_dir = transaction_dir.join("backups");
        let backup_path = backup_dir.join("one.dcm");
        fs::write(&staged_path, b"new").unwrap();
        fs::create_dir(&backup_dir).unwrap();
        let manifest = TransactionManifest {
            version: 1,
            phase: TransactionPhase::Committing,
            entries: vec![TransactionEntry {
                name: "one.dcm".into(),
                had_original: true,
                state: EntryState::Installed,
            }],
        };
        write_manifest(&transaction_dir, &manifest).unwrap();
        fs::rename(&final_path, &backup_path).unwrap();
        fs::rename(&staged_path, &final_path).unwrap();
        let retained_path = interrupted.keep_directory();

        let recovered = ExportTransaction::begin(output.path()).unwrap();

        assert_eq!(fs::read(&final_path).unwrap(), b"old");
        assert!(!retained_path.exists());
        drop(recovered);
    }
}
