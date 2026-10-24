//! Disk-backed storage for encoded pixel frames and their replay index.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

#[cfg(test)]
use dicom_core::{DataElement, PrimitiveValue, VR};
#[cfg(test)]
use dicom_dictionary_std::tags;
#[cfg(test)]
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};

use super::frame_index::FrameIndexSpool;
use super::pixel_data::{checked_frame_len, padded_fragment_len, StreamingPixelDataFrameWriter};
#[cfg(test)]
use super::pixel_data::{
    write_dicom_object_with_pixel_data, write_encapsulated_pixel_data_from_spool,
    SpooledPixelDataFragment,
};
use super::spool_path::frame_index_spool_path;
use crate::Error;

pub(crate) struct PixelDataSpool {
    path: PathBuf,
    file: BufWriter<File>,
    pub(super) index: FrameIndexSpool,
    next_spool_offset: u64,
    next_extended_offset: u64,
    total_raw_bytes: u64,
}

impl PixelDataSpool {
    pub(crate) fn create(path: PathBuf, _frame_count: usize) -> Result<Self, Error> {
        let index = FrameIndexSpool::create(frame_index_spool_path(&path))?;
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
            file: BufWriter::with_capacity(256 * 1024, file),
            index,
            next_spool_offset: 0,
            next_extended_offset: 0,
            total_raw_bytes: 0,
        })
    }

    pub(crate) fn push_frame(&mut self, codestream: &[u8]) -> Result<(), Error> {
        let raw_len = checked_frame_len(codestream.len())?;
        let padded_len_u32 = padded_fragment_len(raw_len)?;
        let padded_len = u64::from(padded_len_u32);
        let spool_offset = self.next_spool_offset;
        let next_spool_offset =
            spool_offset
                .checked_add(padded_len)
                .ok_or_else(|| Error::Unsupported {
                    reason: "pixel-data spool offset overflow".into(),
                })?;
        self.file
            .write_all(codestream)
            .map_err(|source| Error::Io {
                path: self.path.clone(),
                source,
            })?;
        if raw_len != padded_len {
            self.file.write_all(&[0]).map_err(|source| Error::Io {
                path: self.path.clone(),
                source,
            })?;
        }
        self.index
            .push(spool_offset, self.next_extended_offset, raw_len)?;
        self.next_spool_offset = next_spool_offset;
        self.total_raw_bytes =
            self.total_raw_bytes
                .checked_add(raw_len)
                .ok_or_else(|| Error::Unsupported {
                    reason: "total encoded PixelData length overflow".into(),
                })?;
        self.next_extended_offset = self
            .next_extended_offset
            .checked_add(8)
            .and_then(|offset| offset.checked_add(padded_len))
            .ok_or_else(|| Error::Unsupported {
                reason: "extended offset table overflow".into(),
            })?;
        Ok(())
    }

    pub(crate) fn stream_frames_to(
        &mut self,
        writer: &mut StreamingPixelDataFrameWriter<'_>,
    ) -> Result<(), Error> {
        self.file.flush().map_err(|source| Error::Io {
            path: self.path.clone(),
            source,
        })?;
        let mut current_offset =
            self.file
                .get_mut()
                .seek(SeekFrom::Start(0))
                .map_err(|source| Error::Io {
                    path: self.path.clone(),
                    source,
                })?;
        let mut file = BufReader::with_capacity(256 * 1024, self.file.get_mut());
        let path = self.path.clone();
        self.index.replay(|record| {
            if record.source_offset < current_offset {
                current_offset =
                    file.seek(SeekFrom::Start(record.source_offset))
                        .map_err(|source| Error::Io {
                            path: path.clone(),
                            source,
                        })?;
            } else if record.source_offset > current_offset {
                let gap = record.source_offset - current_offset;
                let skipped = io::copy(&mut Read::by_ref(&mut file).take(gap), &mut io::sink())
                    .map_err(|source| Error::Io {
                        path: path.clone(),
                        source,
                    })?;
                if skipped != gap {
                    return Err(Error::DicomWrite {
                        path: path.clone(),
                        message: "spooled PixelData gap ended before next frame".into(),
                    });
                }
                current_offset = record.source_offset;
            }
            writer.push_frame_from_reader(record.raw_len, &mut file)?;
            current_offset =
                current_offset
                    .checked_add(record.raw_len)
                    .ok_or_else(|| Error::Unsupported {
                        reason: "spooled PixelData frame offset overflow".into(),
                    })?;
            Ok(())
        })
    }
}

impl Drop for PixelDataSpool {
    fn drop(&mut self) {
        if let Err(err) = fs::remove_file(&self.path) {
            if err.kind() != io::ErrorKind::NotFound {
                eprintln!(
                    "wsi-dicom: failed to remove pixel-data spool {}: {err}",
                    self.path.display()
                );
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn write_dicom_object_with_spooled_pixel_data(
    path: &Path,
    mut object: InMemDicomObject,
    meta: FileMetaTableBuilder,
    overwrite: bool,
    spool: &mut PixelDataSpool,
) -> Result<(), Error> {
    spool.file.flush().map_err(|source| Error::Io {
        path: spool.path.clone(),
        source,
    })?;
    let mut fragments = Vec::new();
    let mut offsets = Vec::new();
    let mut lengths = Vec::new();
    spool.index.replay(|record| {
        fragments.push(SpooledPixelDataFragment {
            spool_offset: record.source_offset,
            padded_len: padded_fragment_len(record.raw_len)?,
        });
        offsets.push(record.extended_offset);
        lengths.push(record.raw_len);
        Ok(())
    })?;
    object.put(DataElement::new(
        tags::EXTENDED_OFFSET_TABLE,
        VR::OV,
        PrimitiveValue::U64(offsets.into()),
    ));
    object.put(DataElement::new(
        tags::EXTENDED_OFFSET_TABLE_LENGTHS,
        VR::OV,
        PrimitiveValue::U64(lengths.into()),
    ));

    write_dicom_object_with_pixel_data(path, object, meta, overwrite, |file| {
        spool.file.get_mut().seek(SeekFrom::Start(0))?;
        write_encapsulated_pixel_data_from_spool(file, spool.file.get_mut(), &fragments)
    })
}
