use std::io::{self, Write};

use rayon::prelude::*;

use super::jpeg_baseline::uncompressed_frame_bytes;
use super::{
    ensure_consistent_pixel_profile, pixel_profile_from_raw_jpeg_tile,
    raw_jpeg_matches_frame_geometry, raw_jpeg_profile_can_passthrough,
    raw_rgb_passthrough_has_no_geometry_fallback, JpegBaselineFrameGeometry,
    JpegBaselineFrameLocation,
};
use crate::error::Error;
use crate::tile::PixelProfile;
use wsi_rs::{RawCompressedTile, Slide};

const DIRECT_JPEG_PASSTHROUGH_PLAN_CHUNK_FRAMES: usize = 2_048;

#[derive(Clone, Copy)]
pub(super) struct DirectJpegPassthroughFrame {
    pub(super) profile: PixelProfile,
    pub(super) compressed_bytes: u64,
    pub(super) uncompressed_bytes: u64,
}

pub(super) struct DirectJpegPassthroughPlan {
    pub(super) profile: PixelProfile,
    pub(super) compressed_bytes: u64,
    pub(super) uncompressed_bytes: u64,
    pub(super) frame_count: usize,
}

pub(super) struct DirectJpegPassthroughFrameWriter<'a> {
    slide: &'a Slide,
    location: JpegBaselineFrameLocation,
    geometry: JpegBaselineFrameGeometry,
    frame_count: usize,
    chunk_size: usize,
    chunk_start: usize,
    chunk_frames: Vec<Vec<u8>>,
}

impl<'a> DirectJpegPassthroughFrameWriter<'a> {
    pub(super) fn new(
        slide: &'a Slide,
        location: JpegBaselineFrameLocation,
        geometry: JpegBaselineFrameGeometry,
        frame_count: usize,
        chunk_size: usize,
    ) -> Self {
        Self {
            slide,
            location,
            geometry,
            frame_count,
            chunk_size: chunk_size.max(1),
            chunk_start: 0,
            chunk_frames: Vec::new(),
        }
    }

    pub(super) fn write_frame(&mut self, idx: usize, output: &mut dyn Write) -> io::Result<()> {
        output.write_all(self.frame(idx)?)
    }

    pub(super) fn frame_len(&mut self, idx: usize) -> io::Result<u64> {
        u64::try_from(self.frame(idx)?.len())
            .map_err(|_| io::Error::other("JPEG passthrough frame length exceeds u64"))
    }

    fn frame(&mut self, idx: usize) -> io::Result<&[u8]> {
        let chunk_end = self.chunk_start.saturating_add(self.chunk_frames.len());
        if idx < self.chunk_start || idx >= chunk_end {
            self.load_chunk(idx)?;
        }
        let frame = self
            .chunk_frames
            .get(idx - self.chunk_start)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "frame index out of range")
            })?;
        Ok(frame)
    }

    fn load_chunk(&mut self, idx: usize) -> io::Result<()> {
        if idx >= self.frame_count {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "frame index out of range",
            ));
        }
        let end = idx.saturating_add(self.chunk_size).min(self.frame_count);
        let frames = (idx..end)
            .into_par_iter()
            .map(|frame_idx| {
                read_direct_jpeg_passthrough_frame(
                    self.slide,
                    self.location,
                    self.geometry,
                    frame_idx,
                )
            })
            .collect::<io::Result<Vec<_>>>()?;
        self.chunk_start = idx;
        self.chunk_frames = frames;
        Ok(())
    }
}

pub(super) fn try_plan_direct_jpeg_passthrough_frames(
    slide: &Slide,
    location: JpegBaselineFrameLocation,
    level: &wsi_rs::Level,
    geometry: JpegBaselineFrameGeometry,
) -> Result<Option<DirectJpegPassthroughPlan>, Error> {
    let frame_count = geometry
        .tiles_across
        .checked_mul(geometry.tiles_down)
        .ok_or_else(|| Error::Unsupported {
            reason: "JPEG passthrough frame count overflow".into(),
        })?;
    let frame_count = usize::try_from(frame_count).map_err(|_| Error::Unsupported {
        reason: "JPEG passthrough frame count exceeds platform addressable memory".into(),
    })?;
    let allow_raw_rgb_passthrough = raw_rgb_passthrough_has_no_geometry_fallback(level, geometry);
    let mut profile = None;
    let mut compressed_bytes = 0u64;
    let mut uncompressed_bytes = 0u64;
    let mut chunk_start = 0usize;
    while chunk_start < frame_count {
        let chunk_end = chunk_start
            .saturating_add(DIRECT_JPEG_PASSTHROUGH_PLAN_CHUNK_FRAMES)
            .min(frame_count);
        let planned = (chunk_start..chunk_end)
            .into_par_iter()
            .map(|frame_idx| {
                let raw =
                    match read_raw_jpeg_passthrough_tile(slide, location, geometry, frame_idx)? {
                        Some(raw) => raw,
                        None => return Ok(None),
                    };
                let Ok(profile) = pixel_profile_from_raw_jpeg_tile(&raw) else {
                    return Ok(None);
                };
                if !raw_jpeg_profile_can_passthrough(profile, allow_raw_rgb_passthrough) {
                    return Ok(None);
                }
                let compressed_bytes =
                    u64::try_from(raw.data().len()).map_err(|_| Error::Unsupported {
                        reason: "JPEG passthrough frame length exceeds u64".into(),
                    })?;
                Ok(Some(DirectJpegPassthroughFrame {
                    profile,
                    compressed_bytes,
                    uncompressed_bytes: uncompressed_frame_bytes(&raw)?,
                }))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        for frame in planned {
            let Some(frame) = frame else {
                return Ok(None);
            };
            ensure_consistent_pixel_profile(
                &mut profile,
                frame.profile,
                "JPEG passthrough pixel profile changed across frames",
            )?;
            compressed_bytes = compressed_bytes.saturating_add(frame.compressed_bytes);
            uncompressed_bytes = uncompressed_bytes.saturating_add(frame.uncompressed_bytes);
        }
        chunk_start = chunk_end;
    }
    let profile = profile.ok_or_else(|| Error::Unsupported {
        reason: "slide level produced no frames".into(),
    })?;
    Ok(Some(DirectJpegPassthroughPlan {
        profile,
        compressed_bytes,
        uncompressed_bytes,
        frame_count,
    }))
}

fn read_direct_jpeg_passthrough_frame(
    slide: &Slide,
    location: JpegBaselineFrameLocation,
    geometry: JpegBaselineFrameGeometry,
    frame_idx: usize,
) -> io::Result<Vec<u8>> {
    let (col, row) =
        jpeg_passthrough_tile_coordinates(geometry, frame_idx).map_err(io::Error::other)?;
    let raw = slide
        .read_raw_compressed_tile(&location.tile_request(col, row))
        .map_err(io::Error::other)?;
    if !raw_jpeg_matches_frame_geometry(&raw, geometry.frame_columns, geometry.frame_rows) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "direct JPEG passthrough frame is no longer passthrough-eligible",
        ));
    }
    Ok(raw.into_data())
}

pub(crate) fn read_raw_jpeg_passthrough_tile(
    slide: &Slide,
    location: JpegBaselineFrameLocation,
    geometry: JpegBaselineFrameGeometry,
    frame_idx: usize,
) -> Result<Option<RawCompressedTile>, Error> {
    let (col, row) = jpeg_passthrough_tile_coordinates(geometry, frame_idx)?;
    let raw = match slide.read_raw_compressed_tile(&location.tile_request(col, row)) {
        Ok(raw) => raw,
        Err(_) => return Ok(None),
    };
    Ok(
        raw_jpeg_matches_frame_geometry(&raw, geometry.frame_columns, geometry.frame_rows)
            .then_some(raw),
    )
}

fn jpeg_passthrough_tile_coordinates(
    geometry: JpegBaselineFrameGeometry,
    frame_idx: usize,
) -> Result<(i64, i64), Error> {
    let frame_idx = u64::try_from(frame_idx).map_err(|_| Error::Unsupported {
        reason: "JPEG passthrough frame index exceeds u64".into(),
    })?;
    let col = frame_idx % geometry.tiles_across;
    let row = frame_idx / geometry.tiles_across;
    let col = i64::try_from(col).map_err(|_| Error::Unsupported {
        reason: "JPEG passthrough tile column exceeds i64".into(),
    })?;
    let row = i64::try_from(row).map_err(|_| Error::Unsupported {
        reason: "JPEG passthrough tile row exceeds i64".into(),
    })?;
    Ok((col, row))
}
