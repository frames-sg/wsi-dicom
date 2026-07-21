use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use crate::Error;

const FRAME_INDEX_RECORD_BYTES: u64 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameIndexRecord {
    pub(crate) source_offset: u64,
    pub(crate) extended_offset: u64,
    pub(crate) raw_len: u64,
}

pub(crate) struct FrameIndexSpool {
    path: PathBuf,
    file: File,
    records: u64,
}

impl FrameIndexSpool {
    pub(crate) fn create(path: PathBuf) -> Result<Self, Error> {
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
        Ok(Self {
            path,
            file,
            records: 0,
        })
    }

    pub(crate) fn push(
        &mut self,
        source_offset: u64,
        extended_offset: u64,
        raw_len: u64,
    ) -> Result<(), Error> {
        self.file
            .seek(SeekFrom::End(0))
            .and_then(|_| self.file.write_all(&source_offset.to_le_bytes()))
            .and_then(|_| self.file.write_all(&extended_offset.to_le_bytes()))
            .and_then(|_| self.file.write_all(&raw_len.to_le_bytes()))
            .map_err(|source| Error::Io {
                path: self.path.clone(),
                source,
            })?;
        self.records = self
            .records
            .checked_add(1)
            .ok_or_else(|| Error::Unsupported {
                reason: "frame index record count overflow".into(),
            })?;
        Ok(())
    }

    pub(crate) fn len(&self) -> u64 {
        self.records
    }

    pub(crate) fn replay(
        &mut self,
        mut visit: impl FnMut(FrameIndexRecord) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.file.flush().map_err(|source| Error::Io {
            path: self.path.clone(),
            source,
        })?;
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|source| Error::Io {
                path: self.path.clone(),
                source,
            })?;

        let mut bytes = [0u8; FRAME_INDEX_RECORD_BYTES as usize];
        for _ in 0..self.records {
            self.file
                .read_exact(&mut bytes)
                .map_err(|source| Error::Io {
                    path: self.path.clone(),
                    source,
                })?;
            visit(FrameIndexRecord {
                source_offset: u64::from_le_bytes(bytes[0..8].try_into().unwrap()),
                extended_offset: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
                raw_len: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            })?;
        }

        let expected_len = self
            .records
            .checked_mul(FRAME_INDEX_RECORD_BYTES)
            .ok_or_else(|| Error::Unsupported {
                reason: "frame index byte length overflow".into(),
            })?;
        let actual_len = self.file.stream_position().map_err(|source| Error::Io {
            path: self.path.clone(),
            source,
        })?;
        if actual_len != expected_len {
            return Err(Error::DicomWrite {
                path: self.path.clone(),
                message: format!(
                    "frame index ended at {actual_len} bytes, expected {expected_len} bytes"
                ),
            });
        }
        Ok(())
    }
}

impl Drop for FrameIndexSpool {
    fn drop(&mut self) {
        if let Err(err) = fs::remove_file(&self.path) {
            if err.kind() != io::ErrorKind::NotFound {
                eprintln!(
                    "wsi-dicom: failed to remove frame-index spool {}: {err}",
                    self.path.display()
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncated_spool_fails_replay_and_is_removed_on_drop() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("frames.index");
        let mut spool = FrameIndexSpool::create(path.clone()).unwrap();
        spool.push(1, 2, 3).unwrap();
        spool.file.set_len(8).unwrap();

        let error = spool
            .replay(|_| Ok(()))
            .expect_err("truncated fixed record must fail closed");
        assert!(error.to_string().contains("failed to fill whole buffer"));
        drop(spool);
        assert!(!path.exists());
    }
}
