use std::io::{self, Write};

use rayon::prelude::*;

use super::{
    pixel_profile_from_raw_jpeg_tile, raw_jpeg_matches_frame_geometry,
    raw_jpeg_profile_can_passthrough, raw_rgb_passthrough_has_no_geometry_fallback,
    uncompressed_frame_bytes, Error, JpegBaselineFrameGeometry, JpegBaselineFrameLocation,
    PixelProfile, RawCompressedTile, Slide,
};

#[derive(Clone, Copy)]
pub(super) struct DirectJpegPassthroughFrame {
    pub(super) profile: PixelProfile,
    pub(super) compressed_bytes: u64,
    pub(super) uncompressed_bytes: u64,
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
        output.write_all(frame)
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
) -> Result<Option<Vec<DirectJpegPassthroughFrame>>, Error> {
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
    let planned = (0..frame_count)
        .into_par_iter()
        .map(|frame_idx| {
            let raw = match read_raw_jpeg_passthrough_tile(slide, location, geometry, frame_idx)? {
                Some(raw) => raw,
                None => return Ok(None),
            };
            let profile = pixel_profile_from_raw_jpeg_tile(&raw)?;
            if !raw_jpeg_profile_can_passthrough(profile, allow_raw_rgb_passthrough) {
                return Ok(None);
            }
            let compressed_bytes =
                u64::try_from(raw.data.len()).map_err(|_| Error::Unsupported {
                    reason: "JPEG passthrough frame length exceeds u64".into(),
                })?;
            Ok(Some(DirectJpegPassthroughFrame {
                profile,
                compressed_bytes,
                uncompressed_bytes: uncompressed_frame_bytes(&raw)?,
            }))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    if planned.iter().any(Option::is_none) {
        return Ok(None);
    }
    Ok(Some(planned.into_iter().flatten().collect()))
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
    Ok(raw.data)
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
