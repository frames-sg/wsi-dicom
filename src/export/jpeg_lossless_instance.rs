//! JPEG Lossless (Process 14, first-order prediction) instance export.
//!
//! Every frame is decoded to samples and re-encoded on the CPU.

use std::time::{Duration, Instant};

use j2k_jpeg::{JpegBackend, JpegLosslessEncodeOptions, JpegLosslessSamples};
use wsi_rs::Slide;

use super::cpu_batch::{frame_batch_len, map_cpu_frames};
use super::frame_region::{FrameRectGrid, FrameRectOverflowReasons, OutputFrameRect};
use super::icc_profile::resolve_icc_profile;
use super::j2k_policy::{lossless_j2k_cpu_row_batch_count, lossless_j2k_use_direct_pixel_data};
use super::lossless_j2k_cpu::prepare_cpu_input_batch;
use super::lossless_j2k_instance::PendingLosslessInstance;
use super::lossless_j2k_plan::source_tile_codec;
use super::tile_grid::TileGrid;
use super::{ensure_consistent_pixel_profile, InstanceExportContext};
use crate::coordinate::InstanceCoordinate;
use crate::error::Error;
use crate::lossy::{LossyCompressionAccumulator, LossyCompressionByteCounts};
use crate::options::{CodecValidation, NormalizedExportOptions, TransferSyntax};
use crate::report::ExportMetrics;
use crate::request::ExportRequest;
use crate::routing::j2k_route_tile_size;
use crate::tile::PixelProfile;
use crate::writer::{unique_spool_path, BufferedPixelDataSink, PixelDataSink};

pub(super) fn prepare_jpeg_lossless_instance(
    slide: &Slide,
    request: &ExportRequest,
    export: InstanceExportContext<'_>,
) -> Result<PendingLosslessInstance, Error> {
    let InstanceExportContext {
        options,
        metadata,
        identity,
        instance_number,
        coordinate,
        level,
        per_frame_plan,
    } = export;
    // The metadata budget preflight sizes every non-Baseline instance with
    // this grid, so JPEG Lossless must use the same one.
    let tile_size = j2k_route_tile_size(
        options.semantics.tile_size,
        options.semantics.transfer_syntax,
        level,
    )?;
    let (matrix_columns, matrix_rows) = level.dimensions;
    let grid = TileGrid::square(matrix_columns, matrix_rows, tile_size)?;
    let frame_count = export.frame_count(grid.frame_count_u32()?);
    let context = export.dicom_context(slide, request)?;
    let declared_lossy_compression =
        super::lossy_provenance::source_lossy_history(slide, coordinate)?;
    let mut pixel_data = BufferedPixelDataSink::create(
        unique_spool_path(&context.path),
        frame_count as usize,
        lossless_j2k_use_direct_pixel_data(frame_count, tile_size, rayon::current_num_threads()),
    )?;
    let frame_grid = FrameRectGrid {
        matrix_columns,
        matrix_rows,
        frame_columns: tile_size,
        frame_rows: tile_size,
    };
    let mut metrics = ExportMetrics::default();
    let mut pixel_profile = None;
    let mut source_lossy_compression = LossyCompressionAccumulator::default();

    let mut row = 0;
    while row < grid.tiles_down {
        let row_count = lossless_j2k_cpu_row_batch_count(grid.tiles_across, grid.tiles_down - row);
        let mut frames = grid_frames(frame_grid, grid.tiles_across, row, row_count)?;
        if let Some(plan) = per_frame_plan {
            frames.retain(|frame| plan.includes_grid_position(frame.row, frame.col));
        }
        for batch in frames.chunks(frame_batch_len(tile_size, tile_size)) {
            let encoded = encode_frame_batch(slide, level, coordinate, batch, tile_size, options)?;
            for frame in encoded {
                metrics.record_cpu_input();
                metrics.record_input_decode_duration(frame.input_decode_duration);
                metrics.record_compose_duration(frame.compose_duration);
                metrics.record_pixel_profile(frame.profile);
                metrics.record_encode_duration(frame.encode_duration);
                metrics.record_validation_duration(frame.validation_duration);
                metrics.record_transcode_route(false, false);
                ensure_consistent_pixel_profile(
                    &mut pixel_profile,
                    frame.profile,
                    "pixel profile changed across frames",
                )?;
                if let Some(source) = &frame.source_lossy_compression {
                    source_lossy_compression.observe(source)?;
                }
                if frame.compressed_source {
                    metrics.record_compressed_source_reencode();
                }
                let write_started = Instant::now();
                pixel_data.push_owned_frame(frame.data)?;
                metrics.record_write_duration(write_started.elapsed());
            }
        }
        row = row
            .checked_add(row_count)
            .ok_or_else(|| Error::Unsupported {
                reason: "JPEG Lossless row batch overflow".into(),
            })?;
    }

    let profile = pixel_profile.ok_or_else(|| Error::Unsupported {
        reason: "slide level produced no frames".into(),
    })?;
    let icc_profile = resolve_icc_profile(slide, request, metadata, coordinate, level, profile)?;
    // JPEG Lossless adds no lossy stage, so the history is the source's alone.
    let lossy_compression = if declared_lossy_compression.is_empty() {
        source_lossy_compression.into_history()?
    } else {
        declared_lossy_compression
    };

    Ok(PendingLosslessInstance {
        context,
        metadata: metadata.clone(),
        study_uid: identity.study_uid().to_string(),
        specimen_uid: identity.specimen_uid().to_string(),
        instance_number,
        tile_size,
        matrix_columns,
        matrix_rows,
        frame_count,
        profile,
        pixel_data,
        icc_profile: icc_profile.bytes,
        icc_profile_report: icc_profile.report,
        lossy_compression,
        metrics,
        transfer_syntax: options.semantics.transfer_syntax,
        overwrite: options.semantics.overwrite,
        max_instance_metadata_bytes: options.resources.max_instance_metadata_bytes,
        per_frame_plan: per_frame_plan.cloned(),
    })
}

#[derive(Debug, Clone, Copy)]
struct GridFrame {
    row: u64,
    col: u64,
    rect: OutputFrameRect,
}

fn grid_frames(
    grid: FrameRectGrid,
    tiles_across: u64,
    start_row: u64,
    row_count: u64,
) -> Result<Vec<GridFrame>, Error> {
    (start_row..start_row.saturating_add(row_count))
        .flat_map(|row| (0..tiles_across).map(move |col| (row, col)))
        .map(|(row, col)| {
            let rect = OutputFrameRect::clamped(
                col,
                row,
                grid,
                FrameRectOverflowReasons {
                    x: "JPEG Lossless tile x offset overflow",
                    y: "JPEG Lossless tile y offset overflow",
                },
            )?;
            Ok(GridFrame { row, col, rect })
        })
        .collect()
}

struct EncodedJpegLosslessFrame {
    data: Vec<u8>,
    profile: crate::tile::PixelProfile,
    source_lossy_compression: Option<LossyCompressionByteCounts>,
    compressed_source: bool,
    input_decode_duration: Duration,
    compose_duration: Duration,
    encode_duration: Duration,
    validation_duration: Duration,
}

fn encode_frame_batch(
    slide: &Slide,
    level: &wsi_rs::Level,
    location: InstanceCoordinate,
    batch: &[GridFrame],
    tile_size: u32,
    options: &NormalizedExportOptions,
) -> Result<Vec<EncodedJpegLosslessFrame>, Error> {
    let pixel_reencode = options.semantics.pixel_reencode;
    let codec_validation = options.execution.codec_validation;
    let rects: Vec<_> = batch.iter().map(|frame| frame.rect).collect();
    let prepared = prepare_cpu_input_batch(
        slide,
        level,
        location,
        &rects,
        tile_size,
        options.resources.max_prepared_frame_bytes,
    )?;
    let work: Vec<_> = batch.iter().zip(&prepared).collect();
    map_cpu_frames(&work, rayon::current_num_threads(), |&(frame, tile)| {
        let frame_error = |error| match error {
            Error::Encode { message } => Error::FrameEncode {
                level: location.level_idx,
                row: frame.row,
                col: frame.col,
                message,
            },
            other => other,
        };
        let source = source_tile_codec(
            slide,
            level,
            location,
            frame.rect,
            grid_index_i64(frame.col, "column")?,
            grid_index_i64(frame.row, "row")?,
        )?;
        let compressed_source = source.as_ref().is_some_and(|source| source.compressed);
        // JPEG Lossless has no compressed-domain route: every compressed
        // source frame is decoded and re-encoded.
        if compressed_source && !pixel_reencode.permits(TransferSyntax::JpegLosslessSv1) {
            return Err(Error::Unsupported {
                reason: format!(
                    "JPEG Lossless frame row={} col={} has compressed source pixels; {}",
                    frame.row,
                    frame.col,
                    pixel_reencode.refusal(TransferSyntax::JpegLosslessSv1),
                ),
            });
        }
        let (data, encode_duration, validation_duration) = encode_jpeg_lossless_frame(
            &tile.bytes,
            tile.profile,
            tile_size,
            tile_size,
            codec_validation,
        )
        .map_err(frame_error)?;
        let source_lossy_compression = source.and_then(|source| source.lossy_compression);
        Ok(EncodedJpegLosslessFrame {
            data,
            profile: tile.profile,
            source_lossy_compression,
            compressed_source,
            input_decode_duration: tile.input_decode_duration,
            compose_duration: tile.compose_duration,
            encode_duration,
            validation_duration,
        })
    })
}

fn grid_index_i64(index: u64, axis: &str) -> Result<i64, Error> {
    i64::try_from(index).map_err(|_| Error::Unsupported {
        reason: format!("JPEG Lossless tile {axis} exceeds i64"),
    })
}

/// Encode one prepared frame as SV1 JPEG Lossless at the profile's bit depth,
/// returning the bytes with encode and validation durations.
pub(super) fn encode_jpeg_lossless_frame(
    bytes: &[u8],
    profile: PixelProfile,
    width: u32,
    height: u32,
    codec_validation: CodecValidation,
) -> Result<(Vec<u8>, Duration, Duration), Error> {
    let wide;
    let samples = match (
        profile.components,
        profile.bits_allocated,
        profile.photometric_interpretation,
    ) {
        (1, 8, "MONOCHROME2") => JpegLosslessSamples::Gray8 {
            data: bytes,
            width,
            height,
        },
        (3, 8, "RGB") => JpegLosslessSamples::Rgb8 {
            data: bytes,
            width,
            height,
        },
        (1, 16, "MONOCHROME2") => {
            wide = little_endian_u16_samples(bytes)?;
            JpegLosslessSamples::Gray16 {
                data: &wide,
                width,
                height,
            }
        }
        (3, 16, "RGB") => {
            wide = little_endian_u16_samples(bytes)?;
            JpegLosslessSamples::Rgb16 {
                data: &wide,
                width,
                height,
            }
        }
        (components, bits, photometric) => {
            return Err(Error::UnsupportedPixelData {
                reason: format!(
                    "JPEG Lossless supports 8- or 16-bit MONOCHROME2 or RGB, got {components} component(s) at {bits} bits as {photometric}"
                ),
            });
        }
    };
    // The writer declares Bits Stored equal to Bits Allocated.
    let mut options = JpegLosslessEncodeOptions::new(profile.bits_allocated as u8);
    options.backend = JpegBackend::Cpu;
    let encode_started = Instant::now();
    let encoded =
        j2k_jpeg::encode_jpeg_lossless(samples, options).map_err(|source| Error::Encode {
            message: format!("JPEG Lossless encode failed: {source}"),
        })?;
    let encode_duration = encode_started.elapsed();
    let validation_started = Instant::now();
    if codec_validation == CodecValidation::RoundTrip {
        validate_jpeg_lossless_round_trip(&encoded.data, samples)?;
    }
    Ok((encoded.data, encode_duration, validation_started.elapsed()))
}

fn little_endian_u16_samples(bytes: &[u8]) -> Result<Vec<u16>, Error> {
    let (pairs, remainder) = bytes.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(Error::UnsupportedPixelData {
            reason: "16-bit frame byte length is odd".into(),
        });
    }
    Ok(pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect())
}

fn validate_jpeg_lossless_round_trip(
    encoded: &[u8],
    samples: JpegLosslessSamples<'_>,
) -> Result<(), Error> {
    let validation_error = |reason: String| Error::Encode {
        message: format!("JPEG Lossless round-trip validation failed: {reason}"),
    };
    let (decoded, _) = j2k_jpeg::Decoder::new(encoded)
        .map_err(|err| validation_error(err.to_string()))?
        .decode_request(j2k_jpeg::DecodeRequest::full(samples.pixel_format()))
        .map_err(|err| validation_error(err.to_string()))?;
    let decoded = decoded.as_slice();
    let identical = match samples {
        JpegLosslessSamples::Gray8 { data, .. } | JpegLosslessSamples::Rgb8 { data, .. } => {
            decoded == data
        }
        JpegLosslessSamples::Gray16 { data, .. } | JpegLosslessSamples::Rgb16 { data, .. } => {
            let (pairs, remainder) = decoded.as_chunks::<2>();
            remainder.is_empty()
                && pairs.len() == data.len()
                && pairs
                    .iter()
                    .zip(data)
                    .all(|(pair, &sample)| u16::from_ne_bytes(*pair) == sample)
        }
    };
    if !identical {
        return Err(validation_error(
            "decoded samples differ from the encoded frame".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::frame_region::PreparedCpuRegion;
    use super::*;

    fn prepared(bytes: Vec<u8>, components: u8, bits_allocated: u16) -> PreparedCpuRegion {
        PreparedCpuRegion {
            bytes: Arc::new(bytes),
            profile: PixelProfile {
                components,
                bits_allocated,
                photometric_interpretation: if components == 1 {
                    "MONOCHROME2"
                } else {
                    "RGB"
                },
            },
            input_decode_duration: Duration::ZERO,
            compose_duration: Duration::ZERO,
        }
    }

    #[test]
    fn frames_round_trip_exactly_at_8_and_16_bits() {
        let size = 8u32;
        let pixels = (size * size) as usize;
        let rgb8 = (0..pixels * 3).map(|i| (i * 37 % 256) as u8).collect();
        let gray16: Vec<u8> = (0..pixels)
            .flat_map(|i| ((i as u16).wrapping_mul(2_731)).to_le_bytes())
            .collect();
        let rgb16: Vec<u8> = (0..pixels * 3)
            .flat_map(|i| ((i as u16).wrapping_mul(40_503)).to_le_bytes())
            .collect();
        for tile in [
            prepared(rgb8, 3, 8),
            prepared(vec![7; pixels], 1, 8),
            prepared(gray16, 1, 16),
            prepared(rgb16, 3, 16),
        ] {
            let (data, _, _) = encode_jpeg_lossless_frame(
                &tile.bytes,
                tile.profile,
                size,
                size,
                CodecValidation::RoundTrip,
            )
            .unwrap();
            assert_eq!(&data[..2], [0xFF, 0xD8]);
            assert!(
                data.windows(2).any(|marker| marker == [0xFF, 0xC3]),
                "frame must be a lossless SOF3 JPEG"
            );
        }
    }

    #[test]
    fn unsupported_profiles_are_rejected_before_encode() {
        let mut tile = prepared(vec![0; 8 * 8 * 3], 3, 8);
        tile.profile.photometric_interpretation = "YBR_FULL_422";
        let error =
            encode_jpeg_lossless_frame(&tile.bytes, tile.profile, 8, 8, CodecValidation::Disabled)
                .unwrap_err();
        assert!(
            matches!(error, Error::UnsupportedPixelData { .. }),
            "{error}"
        );
    }

    #[test]
    fn odd_length_16_bit_frames_are_rejected() {
        assert!(little_endian_u16_samples(&[1, 2, 3]).is_err());
        assert_eq!(little_endian_u16_samples(&[1, 2]).unwrap(), [0x0201]);
    }

    #[test]
    fn round_trip_validation_reports_mismatched_samples() {
        let data = vec![9u8; 64];
        let encoded = j2k_jpeg::encode_jpeg_lossless(
            JpegLosslessSamples::Gray8 {
                data: &data,
                width: 8,
                height: 8,
            },
            JpegLosslessEncodeOptions::new(8),
        )
        .unwrap();
        let other = vec![10u8; 64];
        let error = validate_jpeg_lossless_round_trip(
            &encoded.data,
            JpegLosslessSamples::Gray8 {
                data: &other,
                width: 8,
                height: 8,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("round-trip"), "{error}");
    }
}
