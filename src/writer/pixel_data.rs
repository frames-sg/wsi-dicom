use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[cfg(test)]
use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::tags;
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};

use super::encoding::{write_item_header, write_tag};
use super::frame_index::FrameIndexSpool;
use super::functional_groups::PerFrameFunctionalGroupsPlan;
use super::persistence::{flush_and_sync_dicom_writer, PendingDicomOutput};
use crate::Error;

pub(super) const DICOM_FILE_WRITE_BUFFER_BYTES: usize = 4 * 1024 * 1024;
static SPOOL_COUNTER: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SpooledPixelDataFragment {
    pub(crate) spool_offset: u64,
    pub(crate) padded_len: u32,
}

pub(crate) struct PixelDataSpool {
    path: PathBuf,
    file: File,
    pub(super) index: FrameIndexSpool,
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
            file,
            index,
            next_extended_offset: 0,
            total_raw_bytes: 0,
        })
    }

    pub(crate) fn push_frame(&mut self, codestream: &[u8]) -> Result<(), Error> {
        let raw_len = u64::try_from(codestream.len()).map_err(|_| Error::Unsupported {
            reason: "encoded frame length exceeds u64".into(),
        })?;
        let padded_len_u32 = padded_fragment_len(raw_len)?;
        let padded_len = u64::from(padded_len_u32);
        let spool_offset = self.file.stream_position().map_err(|source| Error::Io {
            path: self.path.clone(),
            source,
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
                .seek(SeekFrom::Start(0))
                .map_err(|source| Error::Io {
                    path: self.path.clone(),
                    source,
                })?;
        let file = &mut self.file;
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
                let skipped = io::copy(&mut Read::by_ref(file).take(gap), &mut io::sink())
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
            writer.push_frame_from_reader(record.raw_len, file)?;
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

pub(crate) trait PixelDataSink {
    fn push_frame(&mut self, codestream: &[u8]) -> Result<(), Error>;

    fn push_owned_frame(&mut self, codestream: Vec<u8>) -> Result<(), Error> {
        self.push_frame(&codestream)
    }

    fn stream_frames_to(
        &mut self,
        writer: &mut StreamingPixelDataFrameWriter<'_>,
    ) -> Result<(), Error>;

    fn total_raw_bytes(&self) -> u64;
}

pub(crate) enum BufferedPixelDataSink {
    InMemory(InMemoryPixelDataSink),
    Spool(PixelDataSpool),
}

impl BufferedPixelDataSink {
    pub(crate) fn create(
        spool_path: PathBuf,
        frame_count: usize,
        use_in_memory_buffer: bool,
    ) -> Result<Self, Error> {
        if use_in_memory_buffer {
            Ok(Self::InMemory(InMemoryPixelDataSink::with_capacity(
                frame_count,
            )?))
        } else {
            Ok(Self::Spool(PixelDataSpool::create(
                spool_path,
                frame_count,
            )?))
        }
    }
}

impl PixelDataSink for BufferedPixelDataSink {
    fn push_frame(&mut self, codestream: &[u8]) -> Result<(), Error> {
        match self {
            Self::InMemory(buffer) => buffer.push_frame(codestream),
            Self::Spool(spool) => spool.push_frame(codestream),
        }
    }

    fn push_owned_frame(&mut self, codestream: Vec<u8>) -> Result<(), Error> {
        match self {
            Self::InMemory(buffer) => buffer.push_owned_frame(codestream),
            Self::Spool(spool) => spool.push_frame(&codestream),
        }
    }

    fn stream_frames_to(
        &mut self,
        writer: &mut StreamingPixelDataFrameWriter<'_>,
    ) -> Result<(), Error> {
        match self {
            Self::InMemory(buffer) => buffer.stream_frames_to(writer),
            Self::Spool(spool) => spool.stream_frames_to(writer),
        }
    }

    fn total_raw_bytes(&self) -> u64 {
        match self {
            Self::InMemory(buffer) => buffer.total_raw_bytes(),
            Self::Spool(spool) => spool.total_raw_bytes,
        }
    }
}

pub(crate) struct InMemoryPixelDataSink {
    frames: Vec<Vec<u8>>,
}

impl InMemoryPixelDataSink {
    fn with_capacity(frame_count: usize) -> Result<Self, Error> {
        let mut frames = Vec::new();
        frames
            .try_reserve_exact(frame_count)
            .map_err(|_| Error::Unsupported {
                reason: "in-memory PixelData frame index allocation exceeds available memory"
                    .into(),
            })?;
        Ok(Self { frames })
    }
}

impl PixelDataSink for InMemoryPixelDataSink {
    fn push_frame(&mut self, codestream: &[u8]) -> Result<(), Error> {
        checked_frame_len(codestream.len())?;
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(codestream.len())
            .map_err(|_| Error::Unsupported {
                reason: "encoded frame allocation exceeds available memory".into(),
            })?;
        owned.extend_from_slice(codestream);
        self.push_owned_frame(owned)?;
        Ok(())
    }

    fn push_owned_frame(&mut self, codestream: Vec<u8>) -> Result<(), Error> {
        checked_frame_len(codestream.len())?;
        self.frames.try_reserve(1).map_err(|_| Error::Unsupported {
            reason: "in-memory PixelData frame index allocation exceeds available memory".into(),
        })?;
        self.frames.push(codestream);
        Ok(())
    }

    fn stream_frames_to(
        &mut self,
        writer: &mut StreamingPixelDataFrameWriter<'_>,
    ) -> Result<(), Error> {
        for frame in &self.frames {
            writer.push_frame(frame)?;
        }
        Ok(())
    }

    fn total_raw_bytes(&self) -> u64 {
        self.frames
            .iter()
            .map(|frame| u64::try_from(frame.len()).unwrap_or(u64::MAX))
            .fold(0, u64::saturating_add)
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

#[cfg(any(test, feature = "bench-internals"))]
pub(crate) fn pixel_data_offsets_from_lengths(lengths: &[u64]) -> Result<Vec<u64>, Error> {
    let mut offsets = Vec::new();
    offsets
        .try_reserve_exact(lengths.len())
        .map_err(|_| Error::Unsupported {
            reason: "extended offset table exceeds available memory".into(),
        })?;
    let mut next_extended_offset = 0u64;
    for &raw_len in lengths {
        let padded_len = u64::from(padded_fragment_len(raw_len)?);
        offsets.push(next_extended_offset);
        next_extended_offset = next_extended_offset
            .checked_add(8)
            .and_then(|offset| offset.checked_add(padded_len))
            .ok_or_else(|| Error::Unsupported {
                reason: "extended offset table overflow".into(),
            })?;
    }
    Ok(offsets)
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
        spool.file.seek(SeekFrom::Start(0))?;
        write_encapsulated_pixel_data_from_spool(file, &mut spool.file, &fragments)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamedPixelDataWriteReport {
    pub(crate) frame_count: usize,
    pub(crate) metadata_bytes: u64,
    pub(crate) streaming_write_duration: Duration,
    pub(crate) pixel_data_patch_duration: Duration,
}

pub(crate) struct StreamingPixelDataFrameWriter<'a> {
    path: PathBuf,
    output: &'a mut BufWriter<File>,
    frame_count: usize,
    frames_written: usize,
    frame_index: FrameIndexSpool,
    next_extended_offset: u64,
    streaming_write_duration: Duration,
}

impl StreamingPixelDataFrameWriter<'_> {
    pub(crate) fn push_frame(&mut self, codestream: &[u8]) -> Result<(), Error> {
        let raw_len = u64::try_from(codestream.len()).map_err(|_| Error::Unsupported {
            reason: "encoded frame length exceeds u64".into(),
        })?;
        self.push_frame_impl(raw_len, |output| output.write_all(codestream))
    }

    pub(crate) fn push_frame_from_reader(
        &mut self,
        raw_len: u64,
        reader: &mut impl Read,
    ) -> Result<(), Error> {
        self.push_frame_impl(raw_len, |output| {
            let copied = io::copy(&mut reader.take(raw_len), output)?;
            if copied != raw_len {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "streamed PixelData frame reader ended before declared length",
                ));
            }
            Ok(())
        })
    }

    pub(crate) fn push_frame_with(
        &mut self,
        raw_len: u64,
        write_frame: impl FnOnce(&mut dyn Write) -> io::Result<()>,
    ) -> Result<(), Error> {
        self.push_frame_impl(raw_len, |output| write_frame(output))
    }

    fn push_frame_impl(
        &mut self,
        raw_len: u64,
        write_frame: impl FnOnce(&mut BufWriter<File>) -> io::Result<()>,
    ) -> Result<(), Error> {
        if self.frames_written >= self.frame_count {
            return Err(Error::DicomWrite {
                path: self.path.clone(),
                message: format!(
                    "streamed PixelData received more than {} frame(s)",
                    self.frame_count
                ),
            });
        }
        let padded_len_u32 = padded_fragment_len(raw_len)?;
        let padded_len = u64::from(padded_len_u32);
        let started = Instant::now();
        write_item_header(self.output, padded_len_u32).map_err(|source| Error::Io {
            path: self.path.clone(),
            source,
        })?;
        write_frame(self.output).map_err(|source| Error::Io {
            path: self.path.clone(),
            source,
        })?;
        if raw_len != padded_len {
            self.output.write_all(&[0]).map_err(|source| Error::Io {
                path: self.path.clone(),
                source,
            })?;
        }
        self.streaming_write_duration = self
            .streaming_write_duration
            .saturating_add(started.elapsed());
        self.frame_index
            .push(0, self.next_extended_offset, raw_len)?;
        self.next_extended_offset = self
            .next_extended_offset
            .checked_add(8)
            .and_then(|offset| offset.checked_add(padded_len))
            .ok_or_else(|| Error::Unsupported {
                reason: "extended offset table overflow".into(),
            })?;
        self.frames_written += 1;
        Ok(())
    }

    fn finish(self) -> Result<(StreamedPixelDataWriteReport, FrameIndexSpool), Error> {
        if self.frames_written != self.frame_count {
            return Err(Error::DicomWrite {
                path: self.path,
                message: format!(
                    "streamed PixelData wrote {} frame(s), expected {}",
                    self.frames_written, self.frame_count
                ),
            });
        }
        if self.frame_index.len() != self.frames_written as u64 {
            return Err(Error::DicomWrite {
                path: self.path,
                message: "streamed PixelData frame index count mismatch".into(),
            });
        }
        Ok((
            StreamedPixelDataWriteReport {
                frame_count: self.frames_written,
                metadata_bytes: 0,
                streaming_write_duration: self.streaming_write_duration,
                pixel_data_patch_duration: Duration::ZERO,
            },
            self.frame_index,
        ))
    }
}

pub(crate) struct StreamedDicomWritePlan {
    pub(crate) object: InMemDicomObject,
    pub(crate) meta: FileMetaTableBuilder,
    pub(crate) overwrite: bool,
    pub(crate) per_frame_plan: PerFrameFunctionalGroupsPlan,
    pub(crate) max_instance_metadata_bytes: u64,
    pub(crate) frame_count: usize,
}

pub(crate) fn write_dicom_object_with_streamed_pixel_data(
    path: &Path,
    plan: StreamedDicomWritePlan,
    write_frames: impl FnOnce(&mut StreamingPixelDataFrameWriter<'_>) -> Result<(), Error>,
) -> Result<StreamedPixelDataWriteReport, Error> {
    let StreamedDicomWritePlan {
        mut object,
        meta,
        overwrite,
        per_frame_plan,
        max_instance_metadata_bytes,
        frame_count,
    } = plan;
    if usize::try_from(per_frame_plan.frame_count()).ok() != Some(frame_count) {
        return Err(Error::DicomWrite {
            path: path.to_path_buf(),
            message: "per-frame metadata plan does not match PixelData frame count".into(),
        });
    }
    let offset_table_bytes = extended_offset_table_encoded_bytes(frame_count)?;
    let per_frame_budget = max_instance_metadata_bytes
        .checked_sub(offset_table_bytes)
        .ok_or_else(|| Error::InvalidOptions {
            reason: "max_instance_metadata_bytes is smaller than the extended offset tables".into(),
        })?;
    let per_frame_estimate = match per_frame_plan.encoded_len_with_limit(per_frame_budget) {
        Ok(bytes) => bytes,
        Err(Error::InvalidOptions { reason }) => {
            return Err(Error::InvalidOptions {
                reason: format!(
                    "instance metadata exceeds max_instance_metadata_bytes={max_instance_metadata_bytes}: {reason}"
                ),
            });
        }
        Err(error) => return Err(error),
    };
    let minimum_metadata_bytes = per_frame_estimate
        .checked_add(offset_table_bytes)
        .ok_or_else(|| Error::InvalidOptions {
            reason: "DICOM metadata estimate overflow".into(),
        })?;
    if minimum_metadata_bytes > max_instance_metadata_bytes {
        return Err(Error::InvalidOptions {
            reason: format!(
                "instance metadata estimate of at least {minimum_metadata_bytes} bytes exceeds max_instance_metadata_bytes={max_instance_metadata_bytes}"
            ),
        });
    }
    let frame_index = FrameIndexSpool::create(frame_index_spool_path(path))?;
    let output = PendingDicomOutput::create(path, overwrite)?;
    let file = output.reopen()?;
    let mut file = dicom_file_writer(file);
    object.remove_element(tags::EXTENDED_OFFSET_TABLE);
    object.remove_element(tags::EXTENDED_OFFSET_TABLE_LENGTHS);
    object.remove_element(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE);
    object
        .with_meta(meta)
        .map_err(|err| Error::DicomWrite {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?
        .write_all(&mut file)
        .map_err(|err| Error::DicomWrite {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
    let per_frame_bytes = per_frame_plan.write_to(&mut file, per_frame_budget)?;
    let extended_offset_table_locations =
        write_empty_extended_offset_tables(&mut file, frame_count).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let metadata_bytes = per_frame_bytes
        .checked_add(offset_table_bytes)
        .ok_or_else(|| Error::InvalidOptions {
            reason: "DICOM metadata byte count overflow".into(),
        })?;
    write_encapsulated_pixel_data_header(&mut file).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let mut writer = StreamingPixelDataFrameWriter {
        path: path.to_path_buf(),
        output: &mut file,
        frame_count,
        frames_written: 0,
        frame_index,
        next_extended_offset: 0,
        streaming_write_duration: Duration::ZERO,
    };
    write_frames(&mut writer)?;
    let (mut report, mut frame_index) = writer.finish()?;
    report.metadata_bytes = metadata_bytes;
    if report.metadata_bytes > max_instance_metadata_bytes {
        return Err(Error::InvalidOptions {
            reason: "incremental DICOM metadata accounting exceeded max_instance_metadata_bytes"
                .into(),
        });
    }
    write_encapsulated_pixel_data_trailer(&mut file).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    flush_and_sync_dicom_writer(&mut file, output.path())?;
    drop(file);

    let patch_started = Instant::now();
    patch_extended_offset_tables_from_spool(
        output.path(),
        extended_offset_table_locations,
        &mut frame_index,
    )?;
    report.pixel_data_patch_duration = patch_started.elapsed();
    output.persist()?;
    Ok(report)
}

#[cfg(test)]
pub(super) fn write_dicom_object_with_pixel_data(
    path: &Path,
    object: InMemDicomObject,
    meta: FileMetaTableBuilder,
    overwrite: bool,
    write_pixel_data: impl FnOnce(&mut BufWriter<File>) -> io::Result<()>,
) -> Result<(), Error> {
    let output = PendingDicomOutput::create(path, overwrite)?;
    let file = output.reopen()?;
    let mut file = dicom_file_writer(file);
    object
        .with_meta(meta)
        .map_err(|err| Error::DicomWrite {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?
        .write_all(&mut file)
        .map_err(|err| Error::DicomWrite {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
    write_pixel_data(&mut file).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    flush_and_sync_dicom_writer(&mut file, output.path())?;
    drop(file);
    output.persist()
}

pub(super) fn dicom_file_writer(file: File) -> BufWriter<File> {
    BufWriter::with_capacity(DICOM_FILE_WRITE_BUFFER_BYTES, file)
}

pub(crate) fn unique_spool_path(output_path: &Path) -> PathBuf {
    let counter = SPOOL_COUNTER.fetch_add(1, Ordering::Relaxed);
    let extension = output_path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("dcm");
    output_path.with_extension(format!(
        "{extension}.pixeldata.{}.{}.tmp",
        std::process::id(),
        counter
    ))
}

fn frame_index_spool_path(data_or_output_path: &Path) -> PathBuf {
    let counter = SPOOL_COUNTER.fetch_add(1, Ordering::Relaxed);
    let extension = data_or_output_path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("dcm");
    data_or_output_path.with_extension(format!(
        "{extension}.frameindex.{}.{}.tmp",
        std::process::id(),
        counter
    ))
}

#[cfg(test)]
pub(crate) fn write_encapsulated_pixel_data_from_frames(
    output: &mut impl Write,
    lengths: &[u64],
    mut write_frame: impl FnMut(usize, &mut dyn Write) -> io::Result<()>,
) -> io::Result<()> {
    write_encapsulated_pixel_data_header(output)?;
    for (idx, &raw_len) in lengths.iter().enumerate() {
        let padded_len = padded_fragment_len_io(raw_len)?;
        write_item_header(output, padded_len)?;
        {
            let mut limited = LimitedFragmentWriter {
                inner: output,
                remaining: raw_len,
            };
            write_frame(idx, &mut limited)?;
            if limited.remaining != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "direct PixelData frame ended before declared length",
                ));
            }
        }
        if raw_len % 2 != 0 {
            output.write_all(&[0])?;
        }
    }
    write_encapsulated_pixel_data_trailer(output)
}

#[cfg(test)]
pub(crate) fn write_encapsulated_pixel_data_from_spool(
    output: &mut impl Write,
    spool: &mut (impl Read + Seek),
    fragments: &[SpooledPixelDataFragment],
) -> std::io::Result<()> {
    write_encapsulated_pixel_data_header(output)?;
    let mut current_offset = 0u64;
    for fragment in fragments {
        if fragment.spool_offset < current_offset {
            spool.seek(SeekFrom::Start(fragment.spool_offset))?;
            current_offset = fragment.spool_offset;
        } else if fragment.spool_offset > current_offset {
            let gap = fragment.spool_offset - current_offset;
            let skipped = std::io::copy(&mut spool.by_ref().take(gap), &mut std::io::sink())?;
            if skipped != gap {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "spooled PixelData gap ended before next fragment",
                ));
            }
            current_offset = fragment.spool_offset;
        }
        write_item_header(output, fragment.padded_len)?;
        let mut limited = spool.by_ref().take(u64::from(fragment.padded_len));
        let copied = std::io::copy(&mut limited, output)?;
        if copied != u64::from(fragment.padded_len) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "spooled PixelData fragment ended before padded length",
            ));
        }
        current_offset = current_offset
            .checked_add(u64::from(fragment.padded_len))
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "spooled PixelData fragment offset overflow",
                )
            })?;
    }
    write_encapsulated_pixel_data_trailer(output)
}

fn write_encapsulated_pixel_data_header(output: &mut impl Write) -> std::io::Result<()> {
    write_tag(output, 0x7FE0, 0x0010)?;
    output.write_all(b"OB")?;
    output.write_all(&[0, 0])?;
    output.write_all(&u32::MAX.to_le_bytes())?;
    write_item_header(output, 0)
}

fn write_encapsulated_pixel_data_trailer(output: &mut impl Write) -> std::io::Result<()> {
    write_tag(output, 0xFFFE, 0xE0DD)?;
    output.write_all(&0u32.to_le_bytes())
}

#[cfg(test)]
struct LimitedFragmentWriter<'a, W: Write + ?Sized> {
    inner: &'a mut W,
    remaining: u64,
}

#[cfg(test)]
impl<W: Write + ?Sized> Write for LimitedFragmentWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if u64::try_from(buf.len()).unwrap_or(u64::MAX) > self.remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "direct PixelData frame exceeded declared length",
            ));
        }
        let written = self.inner.write(buf)?;
        self.remaining = self
            .remaining
            .checked_sub(u64::try_from(written).unwrap_or(u64::MAX))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "direct PixelData frame length accounting underflowed",
                )
            })?;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn padded_fragment_len(raw_len: u64) -> Result<u32, Error> {
    let padded_len = raw_len
        .checked_add(raw_len % 2)
        .ok_or_else(|| Error::Unsupported {
            reason: "encoded frame padded length overflow".into(),
        })?;
    u32::try_from(padded_len).map_err(|_| Error::Unsupported {
        reason: "encoded frame exceeds DICOM fragment item length limit".into(),
    })
}

fn checked_frame_len(len: usize) -> Result<u64, Error> {
    u64::try_from(len).map_err(|_| Error::Unsupported {
        reason: "encoded frame length exceeds u64".into(),
    })
}

#[cfg(test)]
fn padded_fragment_len_io(raw_len: u64) -> io::Result<u32> {
    padded_fragment_len(raw_len).map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))
}

fn extended_offset_table_encoded_bytes(frame_count: usize) -> Result<u64, Error> {
    let value_bytes = u64::try_from(frame_count)
        .ok()
        .and_then(|count| count.checked_mul(std::mem::size_of::<u64>() as u64))
        .ok_or_else(|| Error::InvalidOptions {
            reason: "extended offset table metadata estimate overflow".into(),
        })?;
    u32::try_from(value_bytes).map_err(|_| Error::InvalidOptions {
        reason: "extended offset table value exceeds the DICOM element length limit".into(),
    })?;
    value_bytes
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(24))
        .ok_or_else(|| Error::InvalidOptions {
            reason: "extended offset table metadata estimate overflow".into(),
        })
}

pub(crate) fn extended_offset_table_metadata_bytes(frame_count: u32) -> Result<u64, Error> {
    let frame_count = usize::try_from(frame_count).map_err(|_| Error::InvalidOptions {
        reason: "DICOM frame count exceeds platform addressable memory".into(),
    })?;
    extended_offset_table_encoded_bytes(frame_count)
}

fn write_empty_extended_offset_tables(
    output: &mut (impl Write + Seek),
    frame_count: usize,
) -> io::Result<ExtendedOffsetTableLocations> {
    let value_bytes = frame_count
        .checked_mul(std::mem::size_of::<u64>())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "extended offset table byte length overflow",
            )
        })?;
    let value_bytes = u32::try_from(value_bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "extended offset table exceeds DICOM element length limit",
        )
    })?;
    let offset_table_value_offset = write_empty_ov_element(output, 0x7FE0, 0x0001, value_bytes)?;
    let length_table_value_offset = write_empty_ov_element(output, 0x7FE0, 0x0002, value_bytes)?;
    Ok(ExtendedOffsetTableLocations {
        offset_table_value_offset,
        length_table_value_offset,
    })
}

fn write_empty_ov_element(
    output: &mut (impl Write + Seek),
    group: u16,
    element: u16,
    value_len: u32,
) -> io::Result<u64> {
    write_tag(output, group, element)?;
    output.write_all(b"OV")?;
    output.write_all(&[0, 0])?;
    output.write_all(&value_len.to_le_bytes())?;
    let value_offset = output.stream_position()?;
    write_zero_bytes(output, u64::from(value_len))?;
    Ok(value_offset)
}

fn write_zero_bytes(output: &mut impl Write, mut count: u64) -> io::Result<()> {
    const ZERO_CHUNK: [u8; 8192] = [0; 8192];
    while count != 0 {
        let len = usize::try_from(count.min(ZERO_CHUNK.len() as u64)).unwrap();
        output.write_all(&ZERO_CHUNK[..len])?;
        count -= len as u64;
    }
    Ok(())
}

fn patch_extended_offset_tables_from_spool(
    path: &Path,
    locations: ExtendedOffsetTableLocations,
    frame_index: &mut FrameIndexSpool,
) -> Result<(), Error> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.seek(SeekFrom::Start(locations.offset_table_value_offset))
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
    frame_index.replay(|record| {
        file.write_all(&record.extended_offset.to_le_bytes())
            .map_err(|source| Error::Io {
                path: path.to_path_buf(),
                source,
            })
    })?;
    file.seek(SeekFrom::Start(locations.length_table_value_offset))
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
    frame_index.replay(|record| {
        file.write_all(&record.raw_len.to_le_bytes())
            .map_err(|source| Error::Io {
                path: path.to_path_buf(),
                source,
            })
    })?;
    file.sync_all().map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

struct ExtendedOffsetTableLocations {
    offset_table_value_offset: u64,
    length_table_value_offset: u64,
}
