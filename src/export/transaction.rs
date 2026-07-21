use std::collections::HashSet;
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
        self.commit_inner(reports, overwrite, CommitFaults::default())
    }

    fn commit_inner(
        &mut self,
        reports: &mut [InstanceReport],
        overwrite: bool,
        faults: CommitFaults,
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
                faults.fail_after_installs,
                faults.fail_install_rename_at,
            )?;
            if faults.fail_output_sync {
                return Err(Error::Io {
                    path: self.output_dir.clone(),
                    source: io::Error::other("injected export output directory sync failure"),
                });
            }
            sync_directory(&self.output_dir)?;
            manifest.phase = TransactionPhase::Committed;
            write_manifest(&transaction_dir, &manifest)?;
            if faults.fail_after_committed_manifest {
                return Err(Error::Io {
                    path: transaction_dir.join(MANIFEST_FILE_NAME),
                    source: io::Error::other(
                        "injected failure after persisting the committed transaction marker",
                    ),
                });
            }
            Ok(())
        })();
        if let Err(commit_error) = commit_result {
            manifest.phase = TransactionPhase::Committing;
            if let Err(journal_error) = write_manifest(&transaction_dir, &manifest) {
                let recovery_path = self.keep_directory();
                return Err(Error::ExportTransaction {
                    recovery_path,
                    reason: format!(
                        "commit failed ({commit_error}); rollback could not start because its journal marker failed ({journal_error})"
                    ),
                });
            }
            let rollback_result = if faults.fail_rollback {
                Err(Error::Io {
                    path: transaction_dir.clone(),
                    source: io::Error::other("injected export rollback failure"),
                })
            } else {
                rollback_entries(&self.output_dir, &transaction_dir, &manifest)
            };
            return match rollback_result {
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

#[derive(Debug, Default, Clone, Copy)]
struct CommitFaults {
    fail_after_installs: Option<usize>,
    fail_install_rename_at: Option<usize>,
    fail_output_sync: bool,
    fail_after_committed_manifest: bool,
    fail_rollback: bool,
}

fn prepare_entries(
    reports: &[InstanceReport],
    transaction_dir: &Path,
    output_dir: &Path,
    overwrite: bool,
) -> Result<Vec<TransactionEntry>, Error> {
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(reports.len())
        .map_err(|_| Error::ExportTransaction {
            recovery_path: transaction_dir.to_path_buf(),
            reason: "export transaction manifest exceeds available memory".into(),
        })?;
    let mut names = HashSet::new();
    names
        .try_reserve(reports.len())
        .map_err(|_| Error::ExportTransaction {
            recovery_path: transaction_dir.to_path_buf(),
            reason: "export transaction manifest exceeds available memory".into(),
        })?;
    for report in reports {
        let name = staged_file_name(&report.path, transaction_dir)?;
        let staged = transaction_dir.join(&name);
        if !names.insert(name.clone()) {
            return Err(Error::DicomWrite {
                path: staged,
                message: format!("duplicate flat output file name {name}"),
            });
        }
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
    fail_install_rename_at: Option<usize>,
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
        if fail_install_rename_at == Some(index) {
            return Err(Error::Io {
                path: final_path,
                source: io::Error::other("injected staged output rename failure"),
            });
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
        let backup_metadata = match fs::symlink_metadata(&backup_path) {
            Ok(metadata) if metadata.file_type().is_file() => Some(metadata),
            Ok(_) => {
                return Err(Error::Io {
                    path: backup_path,
                    source: io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "transaction backup must be a regular file and not a symlink",
                    ),
                });
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => None,
            Err(source) => {
                return Err(Error::Io {
                    path: backup_path,
                    source,
                });
            }
        };
        let staged_exists = staged_path.try_exists().map_err(|source| Error::Io {
            path: staged_path.clone(),
            source,
        })?;

        if let Some(backup_metadata) = backup_metadata {
            restore_backup(output_dir, &backup_path, &backup_metadata, &final_path)?;
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

fn restore_backup(
    output_dir: &Path,
    backup_path: &Path,
    backup_metadata: &fs::Metadata,
    final_path: &Path,
) -> Result<(), Error> {
    let mut restored = tempfile::Builder::new()
        .prefix(".wsi-dicom-rollback-")
        .tempfile_in(output_dir)
        .map_err(|source| Error::Io {
            path: output_dir.to_path_buf(),
            source,
        })?;
    let mut backup = File::open(backup_path).map_err(|source| Error::Io {
        path: backup_path.to_path_buf(),
        source,
    })?;
    io::copy(&mut backup, &mut restored).map_err(|source| Error::Io {
        path: restored.path().to_path_buf(),
        source,
    })?;
    restored
        .as_file()
        .set_permissions(backup_metadata.permissions())
        .map_err(|source| Error::Io {
            path: restored.path().to_path_buf(),
            source,
        })?;
    restored.as_file().sync_all().map_err(|source| Error::Io {
        path: restored.path().to_path_buf(),
        source,
    })?;
    remove_installed_file_if_present(final_path)?;
    restored.persist(final_path).map_err(|error| Error::Io {
        path: final_path.to_path_buf(),
        source: error.error,
    })?;
    Ok(())
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
            if let Err(rollback_error) = rollback_entries(output_dir, &transaction_dir, &manifest) {
                return Err(Error::ExportTransaction {
                    recovery_path: transaction_dir.clone(),
                    reason: format!(
                        "recovery rollback failed ({rollback_error}); the journal and backups were retained"
                    ),
                });
            }
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
    fn duplicate_output_names_are_rejected_before_overwrite_promotion() {
        let output = tempfile::tempdir().unwrap();
        let final_path = output.path().join("one.dcm");
        fs::write(&final_path, b"old").unwrap();
        let transaction = ExportTransaction::begin(output.path()).unwrap();
        let staged = transaction.staging_dir().join("one.dcm");
        fs::write(&staged, b"new").unwrap();
        let mut reports = vec![report(staged.clone()), report(staged)];

        let error = transaction
            .commit(&mut reports, true)
            .expect_err("duplicate flat output names must be rejected");

        assert!(error.to_string().contains("duplicate"));
        assert_eq!(fs::read(final_path).unwrap(), b"old");
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
            .commit_inner(
                &mut reports,
                true,
                CommitFaults {
                    fail_after_installs: Some(1),
                    ..CommitFaults::default()
                },
            )
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
            .commit_inner(
                &mut reports,
                false,
                CommitFaults {
                    fail_after_installs: Some(1),
                    ..CommitFaults::default()
                },
            )
            .expect_err("injected failure should roll back");

        assert!(!output.path().join("one.dcm").exists());
        assert!(!output.path().join("two.dcm").exists());
    }

    #[test]
    fn install_rename_failure_restores_the_moved_original() {
        let output = tempfile::tempdir().unwrap();
        let final_path = output.path().join("one.dcm");
        fs::write(&final_path, b"old").unwrap();
        let mut transaction = ExportTransaction::begin(output.path()).unwrap();
        let staged = transaction.staging_dir().join("one.dcm");
        fs::write(&staged, b"new").unwrap();
        let mut reports = vec![report(staged)];

        transaction
            .commit_inner(
                &mut reports,
                true,
                CommitFaults {
                    fail_install_rename_at: Some(0),
                    ..CommitFaults::default()
                },
            )
            .expect_err("injected install rename failure should roll back");

        assert_eq!(fs::read(final_path).unwrap(), b"old");
    }

    #[test]
    fn output_sync_failure_rolls_back_every_promoted_file() {
        let output = tempfile::tempdir().unwrap();
        let overwritten_path = output.path().join("one.dcm");
        fs::write(&overwritten_path, b"old").unwrap();
        let mut transaction = ExportTransaction::begin(output.path()).unwrap();
        let overwritten_staged = transaction.staging_dir().join("one.dcm");
        let new_staged = transaction.staging_dir().join("two.dcm");
        fs::write(&overwritten_staged, b"new-one").unwrap();
        fs::write(&new_staged, b"new-two").unwrap();
        let mut reports = vec![report(overwritten_staged), report(new_staged)];

        transaction
            .commit_inner(
                &mut reports,
                true,
                CommitFaults {
                    fail_output_sync: true,
                    ..CommitFaults::default()
                },
            )
            .expect_err("injected output directory sync failure should roll back");

        assert_eq!(fs::read(overwritten_path).unwrap(), b"old");
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

    #[test]
    fn rollback_failure_retains_recoverable_state_and_reports_both_failures() {
        let output = tempfile::tempdir().unwrap();
        let final_path = output.path().join("one.dcm");
        let second_final_path = output.path().join("two.dcm");
        fs::write(&final_path, b"old").unwrap();
        fs::write(&second_final_path, b"old-two").unwrap();
        let mut transaction = ExportTransaction::begin(output.path()).unwrap();
        let staged = transaction.staging_dir().join("one.dcm");
        let second_staged = transaction.staging_dir().join("two.dcm");
        fs::write(&staged, b"new").unwrap();
        fs::write(&second_staged, b"new-two").unwrap();
        let mut reports = vec![report(staged), report(second_staged)];

        let error = transaction
            .commit_inner(
                &mut reports,
                true,
                CommitFaults {
                    fail_after_installs: Some(1),
                    fail_rollback: true,
                    ..CommitFaults::default()
                },
            )
            .expect_err("injected rollback failure must require recovery");
        let (recovery_path, reason) = match error {
            Error::ExportTransaction {
                recovery_path,
                reason,
            } => (recovery_path, reason),
            other => panic!("unexpected error: {other}"),
        };
        assert!(reason.contains("commit failed"));
        assert!(reason.contains("rollback also failed"));
        assert!(recovery_path.is_dir());
        assert_eq!(
            fs::read(recovery_path.join("backups/one.dcm")).unwrap(),
            b"old"
        );
        assert_eq!(fs::read(&final_path).unwrap(), b"new");

        let recovered = ExportTransaction::begin(output.path()).unwrap();
        assert_eq!(fs::read(&final_path).unwrap(), b"old");
        assert_eq!(fs::read(&second_final_path).unwrap(), b"old-two");
        assert!(!recovery_path.exists());
        drop(recovered);
    }

    #[test]
    fn committed_marker_error_rejournals_rollback_before_recovery_is_retained() {
        let output = tempfile::tempdir().unwrap();
        let final_path = output.path().join("one.dcm");
        fs::write(&final_path, b"old").unwrap();
        let mut transaction = ExportTransaction::begin(output.path()).unwrap();
        let staged = transaction.staging_dir().join("one.dcm");
        fs::write(&staged, b"new").unwrap();
        let mut reports = vec![report(staged)];

        let error = transaction
            .commit_inner(
                &mut reports,
                true,
                CommitFaults {
                    fail_after_committed_manifest: true,
                    fail_rollback: true,
                    ..CommitFaults::default()
                },
            )
            .expect_err("a failed rollback after the committed marker requires recovery");
        let recovery_path = match error {
            Error::ExportTransaction { recovery_path, .. } => recovery_path,
            other => panic!("unexpected error: {other}"),
        };
        assert_eq!(
            read_manifest(&recovery_path).unwrap().phase,
            TransactionPhase::Committing,
            "recovery must not mistake a failed rollback for a committed export"
        );

        let recovered = ExportTransaction::begin(output.path()).unwrap();
        assert_eq!(fs::read(final_path).unwrap(), b"old");
        assert!(!recovery_path.exists());
        drop(recovered);
    }

    #[test]
    fn rollback_keeps_backups_until_the_transaction_is_cleaned() {
        let output = tempfile::tempdir().unwrap();
        let transaction_dir = output.path().join("transaction");
        let backup_dir = transaction_dir.join("backups");
        fs::create_dir_all(&backup_dir).unwrap();
        let first_final = output.path().join("one.dcm");
        let second_final = output.path().join("two.dcm");
        fs::write(&first_final, b"new-one").unwrap();
        fs::write(&second_final, b"new-two").unwrap();
        fs::write(backup_dir.join("one.dcm"), b"old-one").unwrap();
        fs::write(backup_dir.join("two.dcm"), b"old-two").unwrap();
        let manifest = TransactionManifest {
            version: 1,
            phase: TransactionPhase::Committing,
            entries: vec![
                TransactionEntry {
                    name: "one.dcm".into(),
                    had_original: true,
                    state: EntryState::Installed,
                },
                TransactionEntry {
                    name: "two.dcm".into(),
                    had_original: true,
                    state: EntryState::Installed,
                },
            ],
        };

        rollback_entries(output.path(), &transaction_dir, &manifest).unwrap();

        assert_eq!(fs::read(&first_final).unwrap(), b"old-one");
        assert_eq!(fs::read(&second_final).unwrap(), b"old-two");
        assert_eq!(fs::read(backup_dir.join("one.dcm")).unwrap(), b"old-one");
        assert_eq!(fs::read(backup_dir.join("two.dcm")).unwrap(), b"old-two");
    }

    #[test]
    fn rollback_can_resume_after_a_partial_restore_failure() {
        let output = tempfile::tempdir().unwrap();
        let transaction_dir = output.path().join("transaction");
        let backup_dir = transaction_dir.join("backups");
        fs::create_dir_all(&backup_dir).unwrap();
        let first_final = output.path().join("one.dcm");
        let second_final = output.path().join("two.dcm");
        fs::create_dir(&first_final).unwrap();
        fs::write(&second_final, b"new-two").unwrap();
        fs::write(backup_dir.join("one.dcm"), b"old-one").unwrap();
        fs::write(backup_dir.join("two.dcm"), b"old-two").unwrap();
        let manifest = TransactionManifest {
            version: 1,
            phase: TransactionPhase::Committing,
            entries: vec![
                TransactionEntry {
                    name: "one.dcm".into(),
                    had_original: true,
                    state: EntryState::Installed,
                },
                TransactionEntry {
                    name: "two.dcm".into(),
                    had_original: true,
                    state: EntryState::Installed,
                },
            ],
        };

        rollback_entries(output.path(), &transaction_dir, &manifest)
            .expect_err("a non-file destination must interrupt rollback");
        assert_eq!(fs::read(&second_final).unwrap(), b"old-two");
        assert_eq!(
            fs::read(backup_dir.join("two.dcm")).unwrap(),
            b"old-two",
            "a completed partial restore must retain its backup for restart"
        );

        fs::rename(&first_final, output.path().join("obstruction")).unwrap();
        fs::write(&first_final, b"new-one").unwrap();
        rollback_entries(output.path(), &transaction_dir, &manifest)
            .expect("rollback should be idempotent after the obstruction is removed");

        assert_eq!(fs::read(&first_final).unwrap(), b"old-one");
        assert_eq!(fs::read(&second_final).unwrap(), b"old-two");
    }

    #[test]
    fn restart_recovery_failure_is_typed_and_retains_the_journal() {
        let output = tempfile::tempdir().unwrap();
        let final_path = output.path().join("one.dcm");
        fs::create_dir(&final_path).unwrap();
        let mut interrupted = ExportTransaction::begin(output.path()).unwrap();
        let transaction_dir = interrupted.staging_dir().to_path_buf();
        let backup_dir = transaction_dir.join("backups");
        fs::create_dir(&backup_dir).unwrap();
        fs::write(backup_dir.join("one.dcm"), b"old-one").unwrap();
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
        let retained_path = interrupted.keep_directory();

        let error = ExportTransaction::begin(output.path())
            .err()
            .expect("restart recovery should report the obstruction");

        match error {
            Error::ExportTransaction {
                recovery_path,
                reason,
            } => {
                assert_eq!(recovery_path, retained_path);
                assert!(reason.contains("recovery rollback failed"));
            }
            other => panic!("unexpected error: {other}"),
        }
        assert!(retained_path.join(MANIFEST_FILE_NAME).is_file());
        assert_eq!(
            fs::read(retained_path.join("backups/one.dcm")).unwrap(),
            b"old-one"
        );
    }
}
