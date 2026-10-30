//! Buffer selection for encoded frames awaiting streamed DICOM output.

use std::path::PathBuf;

use super::background_spool::BackgroundPixelDataSpool;
use super::pixel_data::{checked_frame_len, StreamingPixelDataFrameWriter};
use crate::Error;

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
    Spool(BackgroundPixelDataSpool),
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
            Ok(Self::Spool(BackgroundPixelDataSpool::create(
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
            Self::Spool(spool) => spool.push_owned_frame(codestream.to_vec()),
        }
    }

    fn push_owned_frame(&mut self, codestream: Vec<u8>) -> Result<(), Error> {
        match self {
            Self::InMemory(buffer) => buffer.push_owned_frame(codestream),
            Self::Spool(spool) => spool.push_owned_frame(codestream),
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
            Self::Spool(spool) => spool.total_raw_bytes(),
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
