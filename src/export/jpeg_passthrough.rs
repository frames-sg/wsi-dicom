use std::io::{self, Write};

use super::{
    pixel_profile_from_raw_jpeg_tile, raw_jpeg_matches_frame_geometry,
    raw_jpeg_profile_can_passthrough, raw_rgb_passthrough_has_no_geometry_fallback,
    JpegBaselineFrameGeometry, JpegBaselineFrameLocation,
};
use crate::error::Error;
use crate::tile::PixelProfile;
use wsi_rs::{RawCompressedTile, Slide};

pub(super) struct DirectJpegPassthroughStart {
    pub(super) profile: PixelProfile,
    pub(super) first_frame: Vec<u8>,
}

pub(super) struct DirectJpegPassthroughFrameWriter<'a> {
    slide: &'a Slide,
    location: JpegBaselineFrameLocation,
    geometry: JpegBaselineFrameGeometry,
    frame_count: usize,
    expected_profile: PixelProfile,
    chunk_frame_limit: usize,
    chunk_byte_budget: usize,
    chunk_start: usize,
    chunk_frames: Vec<Vec<u8>>,
    pending_frame: Option<(usize, Vec<u8>)>,
    became_ineligible: bool,
}

impl<'a> DirectJpegPassthroughFrameWriter<'a> {
    pub(super) fn new(
        slide: &'a Slide,
        location: JpegBaselineFrameLocation,
        geometry: JpegBaselineFrameGeometry,
        frame_count: usize,
        start: DirectJpegPassthroughStart,
        chunk_frame_limit: usize,
        chunk_byte_budget: usize,
    ) -> Self {
        let DirectJpegPassthroughStart {
            profile: expected_profile,
            first_frame,
        } = start;
        Self {
            slide,
            location,
            geometry,
            frame_count,
            expected_profile,
            chunk_frame_limit: chunk_frame_limit.max(1),
            chunk_byte_budget: chunk_byte_budget.max(first_frame.len()).max(1),
            chunk_start: 0,
            chunk_frames: vec![first_frame],
            pending_frame: None,
            became_ineligible: false,
        }
    }

    pub(super) fn write_frame(&mut self, idx: usize, output: &mut dyn Write) -> io::Result<()> {
        output.write_all(self.frame(idx)?)
    }

    pub(super) fn frame_len(&mut self, idx: usize) -> io::Result<u64> {
        u64::try_from(self.frame(idx)?.len())
            .map_err(|_| io::Error::other("JPEG passthrough frame length exceeds u64"))
    }

    pub(super) fn became_ineligible(&self) -> bool {
        self.became_ineligible
    }

    #[cfg(test)]
    pub(super) fn buffered_chunk_bytes(&self) -> usize {
        self.chunk_frames.iter().map(Vec::len).sum()
    }

    #[cfg(test)]
    pub(super) fn pending_frame_bytes(&self) -> Option<usize> {
        self.pending_frame.as_ref().map(|(_, frame)| frame.len())
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
        self.chunk_frames.clear();
        self.chunk_start = idx;
        let first = match self.pending_frame.take() {
            Some((pending_index, frame)) if pending_index == idx => Ok(frame),
            Some((pending_index, _)) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "buffered direct JPEG frame index {pending_index} does not match requested {idx}"
                ),
            )),
            None => read_direct_jpeg_passthrough_frame(
                self.slide,
                self.location,
                self.geometry,
                self.expected_profile,
                idx,
            ),
        };
        let first = match first {
            Ok(frame) => frame,
            Err(error) => {
                self.became_ineligible = true;
                return Err(error);
            }
        };
        let mut buffered_bytes = first.len();
        let remaining_frames = self.frame_count - idx;
        let mut frames = Vec::with_capacity(remaining_frames.min(self.chunk_frame_limit));
        frames.push(first);
        let mut next_index = idx + 1;
        while next_index < self.frame_count
            && frames.len() < self.chunk_frame_limit
            && buffered_bytes < self.chunk_byte_budget
        {
            let frame = match read_direct_jpeg_passthrough_frame(
                self.slide,
                self.location,
                self.geometry,
                self.expected_profile,
                next_index,
            ) {
                Ok(frame) => frame,
                Err(error) => {
                    self.became_ineligible = true;
                    return Err(error);
                }
            };
            let next_bytes = buffered_bytes.saturating_add(frame.len());
            if next_bytes > self.chunk_byte_budget {
                self.pending_frame = Some((next_index, frame));
                break;
            }
            buffered_bytes = next_bytes;
            frames.push(frame);
            next_index += 1;
        }
        self.chunk_frames = frames;
        Ok(())
    }
}

pub(super) fn try_prepare_direct_jpeg_passthrough(
    slide: &Slide,
    location: JpegBaselineFrameLocation,
    level: &wsi_rs::Level,
    geometry: JpegBaselineFrameGeometry,
) -> Result<Option<DirectJpegPassthroughStart>, Error> {
    let allow_raw_rgb_passthrough = raw_rgb_passthrough_has_no_geometry_fallback(level, geometry);
    let Some(raw) = read_raw_jpeg_passthrough_tile(slide, location, geometry, 0)? else {
        return Ok(None);
    };
    let Ok(profile) = pixel_profile_from_raw_jpeg_tile(&raw) else {
        return Ok(None);
    };
    if !raw_jpeg_profile_can_passthrough(profile, allow_raw_rgb_passthrough) {
        return Ok(None);
    }
    Ok(Some(DirectJpegPassthroughStart {
        profile,
        first_frame: raw.into_data(),
    }))
}

fn read_direct_jpeg_passthrough_frame(
    slide: &Slide,
    location: JpegBaselineFrameLocation,
    geometry: JpegBaselineFrameGeometry,
    expected_profile: PixelProfile,
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
    let profile = pixel_profile_from_raw_jpeg_tile(&raw)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    if profile != expected_profile {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "direct JPEG passthrough frame pixel profile changed",
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
