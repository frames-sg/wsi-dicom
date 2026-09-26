//! Disk-backed JPEG ordering, retaining only encoded/retiled exceptions.

use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use crate::writer::StreamingPixelDataFrameWriter;
use crate::Error;

pub(super) struct JpegFrameSpool {
    path: PathBuf,
    payload: BufWriter<File>,
    // Each 16-byte record is (source-reference flag, raw length). Its ordinal
    // identifies a source tile; encoded exceptions consume payload sequentially.
    plan: BufWriter<tempfile::NamedTempFile>,
    frames: usize,
}

impl JpegFrameSpool {
    pub(super) fn create(path: PathBuf) -> Result<Self, Error> {
        let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        let plan = tempfile::Builder::new()
            .prefix(".jpeg-frame-plan-")
            .tempfile_in(parent)
            .map_err(|source| Error::Io {
                path: parent.into(),
                source,
            })?;
        let payload = OpenOptions::new()
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
            payload: BufWriter::with_capacity(256 * 1024, payload),
            plan: BufWriter::with_capacity(64 * 1024, plan),
            frames: 0,
        })
    }

    pub(super) fn push_frame(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.payload
            .write_all(bytes)
            .map_err(|source| self.io_error(source))?;
        self.push_record(false, bytes.len())
    }

    pub(super) fn push_source_frame(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.push_record(true, bytes.len())
    }

    fn push_record(&mut self, source: bool, len: usize) -> Result<(), Error> {
        let len = u64::try_from(len).map_err(|_| Error::Unsupported {
            reason: "JPEG frame length exceeds u64".into(),
        })?;
        let next = self
            .frames
            .checked_add(1)
            .ok_or_else(|| Error::Unsupported {
                reason: "JPEG frame count exceeds platform limit".into(),
            })?;
        let mut record = [0u8; 16];
        record[..8].copy_from_slice(&u64::from(source).to_le_bytes());
        record[8..].copy_from_slice(&len.to_le_bytes());
        self.plan
            .write_all(&record)
            .map_err(|source| self.io_error(source))?;
        self.frames = next;
        Ok(())
    }

    pub(super) fn finish_preparation(&mut self) -> Result<(), Error> {
        self.payload
            .flush()
            .map_err(|source| self.io_error(source))?;
        self.plan.flush().map_err(|source| self.io_error(source))
    }

    pub(super) fn stream_frames_to(
        &mut self,
        writer: &mut StreamingPixelDataFrameWriter<'_>,
        mut read_source: impl FnMut(usize) -> Result<Vec<u8>, Error>,
    ) -> Result<(), Error> {
        self.finish_preparation()?;
        self.payload
            .get_mut()
            .seek(SeekFrom::Start(0))
            .map_err(|source| self.io_error(source))?;
        self.plan
            .get_mut()
            .seek(SeekFrom::Start(0))
            .map_err(|source| self.io_error(source))?;
        let path = self.path.clone();
        let mut payload = BufReader::with_capacity(256 * 1024, self.payload.get_mut());
        let mut plan = BufReader::with_capacity(64 * 1024, self.plan.get_mut());
        for index in 0..self.frames {
            let mut record = [0u8; 16];
            plan.read_exact(&mut record).map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            let source = u64::from_le_bytes(record[..8].try_into().expect("fixed record flag"));
            let len = u64::from_le_bytes(record[8..].try_into().expect("fixed record length"));
            match source {
                0 => writer.push_frame_from_reader(len, &mut payload)?,
                1 => {
                    let bytes = read_source(index)?;
                    if bytes.len() as u64 != len {
                        return Err(Error::SlideRead { message: format!(
                            "JPEG source frame {index} length changed after planning: expected {len}, got {}", bytes.len()) });
                    }
                    writer.push_frame(&bytes)?;
                }
                _ => {
                    return Err(Error::DicomWrite {
                        path: path.clone(),
                        message: "invalid JPEG frame plan record".into(),
                    })
                }
            }
        }
        Ok(())
    }

    fn io_error(&self, source: std::io::Error) -> Error {
        Error::Io {
            path: self.path.clone(),
            source,
        }
    }
}

impl Drop for JpegFrameSpool {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!(
                    "wsi-dicom: failed to remove JPEG spool {}: {error}",
                    self.path.display()
                );
            }
        }
    }
}
