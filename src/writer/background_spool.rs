//! Pixel-data spool written on a background thread.
//!
//! Encoding produces frames faster than a slow or congested disk accepts them.
//! Writing the spool on its own thread keeps encoders busy; the spool layout,
//! frame order and later replay are those of [`PixelDataSpool`].

use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::thread::JoinHandle;

use super::pixel_data::{checked_frame_len, padded_fragment_len, PixelDataSpool};
use super::StreamingPixelDataFrameWriter;
use crate::Error;

/// Frames queued for the writer thread. Encoded frames are at most a few
/// hundred KiB, so the queue holds a bounded, small amount of memory.
const QUEUED_SPOOL_FRAMES: usize = 32;

pub(crate) struct BackgroundPixelDataSpool {
    sender: Option<SyncSender<Vec<u8>>>,
    writer: Option<JoinHandle<Result<PixelDataSpool, Error>>>,
    written: Option<PixelDataSpool>,
    total_raw_bytes: u64,
}

impl BackgroundPixelDataSpool {
    pub(crate) fn create(path: PathBuf, frame_count: usize) -> Result<Self, Error> {
        let mut spool = PixelDataSpool::create(path.clone(), frame_count)?;
        let (sender, frames) = sync_channel::<Vec<u8>>(QUEUED_SPOOL_FRAMES);
        let writer = std::thread::Builder::new()
            .name("wsi-dicom-spool".into())
            .spawn(move || {
                for frame in frames {
                    spool.push_frame(&frame)?;
                }
                Ok(spool)
            })
            .map_err(|source| Error::Io { path, source })?;
        Ok(Self {
            sender: Some(sender),
            writer: Some(writer),
            written: None,
            total_raw_bytes: 0,
        })
    }

    pub(crate) fn push_owned_frame(&mut self, codestream: Vec<u8>) -> Result<(), Error> {
        // Reject frames the spool cannot represent before queueing them, so
        // the byte total below matches what the writer accepts.
        let raw_len = checked_frame_len(codestream.len())?;
        padded_fragment_len(raw_len)?;
        let total_raw_bytes =
            self.total_raw_bytes
                .checked_add(raw_len)
                .ok_or_else(|| Error::Unsupported {
                    reason: "total encoded PixelData length overflow".into(),
                })?;
        let sent = self
            .sender
            .as_ref()
            .is_some_and(|sender| sender.send(codestream).is_ok());
        if !sent {
            // The writer stopped early; report why.
            return Err(self
                .finish_writing()
                .err()
                .unwrap_or_else(|| Error::Encode {
                    message: "pixel-data spool writer stopped before all frames were queued".into(),
                }));
        }
        self.total_raw_bytes = total_raw_bytes;
        Ok(())
    }

    pub(crate) fn stream_frames_to(
        &mut self,
        writer: &mut StreamingPixelDataFrameWriter<'_>,
    ) -> Result<(), Error> {
        let spool = self.finish_writing()?;
        spool.stream_frames_to(writer)
    }

    pub(crate) const fn total_raw_bytes(&self) -> u64 {
        self.total_raw_bytes
    }

    /// Close the queue and wait for every queued frame to reach the spool.
    fn finish_writing(&mut self) -> Result<&mut PixelDataSpool, Error> {
        self.sender = None;
        if let Some(writer) = self.writer.take() {
            let spool = writer.join().map_err(|_| Error::Encode {
                message: "pixel-data spool writer thread panicked".into(),
            })??;
            self.written = Some(spool);
        }
        self.written.as_mut().ok_or_else(|| Error::Encode {
            message: "pixel-data spool writer failed earlier".into(),
        })
    }
}

impl Drop for BackgroundPixelDataSpool {
    fn drop(&mut self) {
        // Join so the spool files are removed before the caller's staging
        // directory is cleaned up, including after an error.
        self.sender = None;
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}
