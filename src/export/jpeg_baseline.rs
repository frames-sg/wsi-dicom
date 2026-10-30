use std::time::{Duration, Instant};

use j2k_jpeg::{EncodedJpeg, JpegBackend, JpegSamples, JpegSubsampling};
use wsi_rs::{
    Compression, EncodedTilePhotometricInterpretation, RawCompressedTile, Slide, TileLayout,
    TileRequest, WsiError,
};

use crate::error::Error;
use crate::tile::PixelProfile;

use super::frame_region::{FrameExtent, FrameLocation, OutputFrameRect};
use super::raw_tile::raw_tile_copies_as_frame;
use super::route_plan::{
    FrameRouteDecision, FrameRouteSource, RouteExecutionContext, RoutePlanner,
};

#[cfg(all(feature = "metal", target_os = "macos"))]
mod metal;
#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) use metal::encode_jpeg_baseline_metal_device_tile_batch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct JpegBaselineFrameGeometry {
    pub(crate) frame_columns: u32,
    pub(crate) frame_rows: u32,
    pub(crate) tiles_across: u64,
    pub(crate) tiles_down: u64,
    /// Total Pixel Matrix the frames cover.
    pub(crate) matrix_columns: u64,
    pub(crate) matrix_rows: u64,
}

impl JpegBaselineFrameGeometry {
    /// The frame at grid position (`col`, `row`).
    pub(crate) fn frame_extent(&self, col: u64, row: u64) -> FrameExtent {
        FrameExtent::at(
            col,
            row,
            self.frame_columns,
            self.frame_rows,
            self.matrix_columns,
            self.matrix_rows,
        )
    }
}

pub(crate) type JpegBaselineFrameLocation = FrameLocation;

pub(super) type JpegBaselineFallbackFrame = OutputFrameRect;

pub(super) enum JpegBaselinePlannedFrame {
    Passthrough {
        data: Vec<u8>,
        profile: PixelProfile,
        uncompressed_bytes: u64,
    },
    Retile {
        data: Vec<u8>,
        profile: PixelProfile,
        uncompressed_bytes: u64,
        retile_duration: Duration,
    },
    Blank {
        profile: PixelProfile,
        uncompressed_bytes: u64,
    },
    Fallback {
        frame: JpegBaselineFallbackFrame,
        source_lossy_compression: Option<crate::lossy::LossyCompressionByteCounts>,
        /// JPEG source pixels no compressed-domain route could preserve.
        compressed_source_reencode: bool,
    },
}

impl JpegBaselinePlannedFrame {
    pub(super) fn route_decision(&self, context: RouteExecutionContext) -> FrameRouteDecision {
        let source = match self {
            Self::Passthrough { .. } => FrameRouteSource::Jpeg {
                passthrough: true,
                retile: false,
                blank: false,
            },
            Self::Retile { .. } => FrameRouteSource::Jpeg {
                passthrough: false,
                retile: true,
                blank: false,
            },
            Self::Blank { .. } => FrameRouteSource::Jpeg {
                passthrough: false,
                retile: false,
                blank: true,
            },
            Self::Fallback { .. } => FrameRouteSource::Jpeg {
                passthrough: false,
                retile: false,
                blank: false,
            },
        };
        RoutePlanner::new(context).decide(source)
    }
}

pub(super) struct JpegBaselineMetalEncodedRun {
    pub(super) frames: Vec<Option<(EncodedJpeg, PixelProfile)>>,
    pub(super) input_decode_duration: Duration,
    pub(super) encode_duration: Duration,
    pub(super) input_decode_batches: u64,
    pub(super) encode_batches: u64,
}

pub(super) fn jpeg_baseline_frame_geometry(
    level: &wsi_rs::Level,
    fallback_tile_size: u32,
) -> Result<JpegBaselineFrameGeometry, Error> {
    if fallback_tile_size == 0 {
        return Err(Error::InvalidOptions {
            reason: "tile_size must be greater than zero".into(),
        });
    }
    let (matrix_columns, matrix_rows) = level.dimensions;
    let (frame_columns, frame_rows, tiles_across, tiles_down) = match level.tile_layout {
        TileLayout::WholeLevel {
            virtual_tile_width,
            virtual_tile_height,
            ..
        } => {
            if virtual_tile_width == 0 || virtual_tile_height == 0 {
                return Err(Error::Unsupported {
                    reason:
                        "JPEG Baseline WholeLevel export requires nonzero virtual tile geometry"
                            .into(),
                });
            }
            if native_jpeg_frame_geometry_is_viewer_friendly(
                virtual_tile_width,
                virtual_tile_height,
                fallback_tile_size,
            ) {
                (
                    virtual_tile_width,
                    virtual_tile_height,
                    matrix_columns.div_ceil(u64::from(virtual_tile_width)),
                    matrix_rows.div_ceil(u64::from(virtual_tile_height)),
                )
            } else {
                (
                    fallback_tile_size,
                    fallback_tile_size,
                    matrix_columns.div_ceil(u64::from(fallback_tile_size)),
                    matrix_rows.div_ceil(u64::from(fallback_tile_size)),
                )
            }
        }
        TileLayout::Regular {
            tile_width,
            tile_height,
            tiles_across,
            tiles_down,
        } if tile_width == fallback_tile_size && tile_height == fallback_tile_size => (
            fallback_tile_size,
            fallback_tile_size,
            tiles_across,
            tiles_down,
        ),
        TileLayout::Regular { .. } | TileLayout::Irregular { .. } | _ => (
            fallback_tile_size,
            fallback_tile_size,
            matrix_columns.div_ceil(u64::from(fallback_tile_size)),
            matrix_rows.div_ceil(u64::from(fallback_tile_size)),
        ),
    };
    if frame_columns == 0 || frame_rows == 0 {
        return Err(Error::Unsupported {
            reason: "JPEG Baseline frame geometry must be nonzero".into(),
        });
    }
    if frame_columns > u16::MAX as u32 || frame_rows > u16::MAX as u32 {
        return Err(Error::Unsupported {
            reason: format!(
                "DICOM Rows/Columns require u16 frame geometry, got {frame_columns}x{frame_rows}"
            ),
        });
    }
    Ok(JpegBaselineFrameGeometry {
        frame_columns,
        frame_rows,
        tiles_across,
        tiles_down,
        matrix_columns,
        matrix_rows,
    })
}

pub(crate) fn jpeg_baseline_route_frame_geometry(
    slide: &Slide,
    level: &wsi_rs::Level,
    location: JpegBaselineFrameLocation,
    fallback_tile_size: u32,
) -> Result<JpegBaselineFrameGeometry, Error> {
    if let Some(geometry) = jpeg_baseline_native_regular_passthrough_geometry(
        slide,
        level,
        location,
        fallback_tile_size,
    )? {
        return Ok(geometry);
    }
    jpeg_baseline_frame_geometry(level, fallback_tile_size)
}

fn jpeg_baseline_native_regular_passthrough_geometry(
    slide: &Slide,
    level: &wsi_rs::Level,
    location: JpegBaselineFrameLocation,
    fallback_tile_size: u32,
) -> Result<Option<JpegBaselineFrameGeometry>, Error> {
    let TileLayout::Regular {
        tile_width,
        tile_height,
        tiles_across,
        tiles_down,
    } = level.tile_layout
    else {
        return Ok(None);
    };
    if tile_width == 0 || tile_height == 0 {
        return Err(Error::Unsupported {
            reason: "JPEG Baseline Regular export requires nonzero tile geometry".into(),
        });
    }
    if !native_jpeg_frame_geometry_is_viewer_friendly(tile_width, tile_height, fallback_tile_size) {
        return Ok(None);
    }

    let geometry = JpegBaselineFrameGeometry {
        frame_columns: tile_width,
        frame_rows: tile_height,
        tiles_across,
        tiles_down,
        matrix_columns: level.dimensions.0,
        matrix_rows: level.dimensions.1,
    };
    let allow_raw_rgb_passthrough =
        raw_rgb_passthrough_has_no_geometry_fallback(slide, level, location, geometry);
    for (col, row) in native_regular_probe_tile_coords(tiles_across, tiles_down) {
        let raw = match read_raw_source_tile(slide, &location.tile_request(col, row)) {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        let frame = geometry.frame_extent(col.unsigned_abs(), row.unsigned_abs());
        if !raw_jpeg_matches_frame_geometry(&raw, frame) {
            continue;
        }
        let Ok(profile) = pixel_profile_from_raw_jpeg_tile(&raw) else {
            continue;
        };
        if raw_jpeg_profile_can_passthrough(profile, allow_raw_rgb_passthrough) {
            return Ok(Some(geometry));
        }
    }

    Ok(None)
}

fn native_regular_probe_tile_coords(tiles_across: u64, tiles_down: u64) -> Vec<(i64, i64)> {
    fn push_unique(coords: &mut Vec<(i64, i64)>, col: u64, row: u64) {
        let (Ok(col), Ok(row)) = (i64::try_from(col), i64::try_from(row)) else {
            return;
        };
        if !coords.contains(&(col, row)) {
            coords.push((col, row));
        }
    }

    if tiles_across == 0 || tiles_down == 0 {
        return Vec::new();
    }

    let mut coords = Vec::new();
    let last_col = tiles_across - 1;
    let last_row = tiles_down - 1;
    let mid_col = tiles_across / 2;
    let mid_row = tiles_down / 2;
    for (col, row) in [
        (0, 0),
        (mid_col, mid_row),
        (last_col, last_row),
        (0, mid_row),
        (mid_col, 0),
        (last_col, mid_row),
        (mid_col, last_row),
        (last_col, 0),
        (0, last_row),
    ] {
        push_unique(&mut coords, col, row);
    }

    let grid_steps = 8_u64;
    for row_step in 0..grid_steps {
        let row = last_row.saturating_mul(row_step) / (grid_steps - 1);
        for col_step in 0..grid_steps {
            let col = last_col.saturating_mul(col_step) / (grid_steps - 1);
            push_unique(&mut coords, col, row);
        }
    }

    for idx in 0..tiles_across.saturating_mul(tiles_down).min(4096) {
        push_unique(&mut coords, idx % tiles_across, idx / tiles_across);
    }

    coords
}

fn native_jpeg_frame_geometry_is_viewer_friendly(
    frame_columns: u32,
    frame_rows: u32,
    fallback_tile_size: u32,
) -> bool {
    if frame_columns == 0 || frame_rows == 0 || fallback_tile_size == 0 {
        return false;
    }
    frame_columns.max(frame_rows) <= fallback_tile_size
        && frame_columns.min(frame_rows) >= fallback_tile_size.div_ceil(2)
}

pub(crate) fn pixel_profile_from_raw_jpeg_tile(
    raw: &RawCompressedTile,
) -> Result<PixelProfile, Error> {
    if raw.compression() != Compression::Jpeg {
        return Err(Error::Unsupported {
            reason: format!(
                "JPEG passthrough requires JPEG compression, got {:?}",
                raw.compression()
            ),
        });
    }
    if raw.bits_allocated() != 8 {
        return Err(Error::UnsupportedPixelData {
            reason: format!(
                "JPEG passthrough requires 8-bit samples, got {}",
                raw.bits_allocated()
            ),
        });
    }
    let encoded = j2k_jpeg::Decoder::inspect(raw.data()).map_err(|err| Error::Unsupported {
        reason: format!("JPEG passthrough could not inspect the encoded frame: {err}"),
    })?;
    if encoded.sof_kind != j2k_jpeg::SofKind::Baseline8 {
        return Err(Error::Unsupported {
            reason: format!(
                "JPEG Baseline passthrough requires SOF0, got {:?}",
                encoded.sof_kind
            ),
        });
    }
    // Edge tiles report a logical size inside their full encoded tile.
    if encoded.bit_depth != 8
        || encoded.sampling.len() != usize::from(raw.samples_per_pixel())
        || encoded.dimensions.0 < raw.width()
        || encoded.dimensions.1 < raw.height()
    {
        return Err(Error::UnsupportedPixelData {
            reason: format!(
                "JPEG passthrough metadata does not match the encoded frame (metadata {}x{} {}-bit/{} components, encoded {}x{} {}-bit/{} components)",
                raw.width(),
                raw.height(),
                raw.bits_allocated(),
                raw.samples_per_pixel(),
                encoded.dimensions.0,
                encoded.dimensions.1,
                encoded.bit_depth,
                encoded.sampling.len()
            ),
        });
    }
    validate_raw_jpeg_color_evidence(raw)?;
    let photometric_interpretation = match raw.photometric_interpretation() {
        EncodedTilePhotometricInterpretation::Monochrome2 => "MONOCHROME2",
        EncodedTilePhotometricInterpretation::Rgb => "RGB",
        EncodedTilePhotometricInterpretation::YbrFull422 => "YBR_FULL_422",
        // VL Whole Slide Microscopy admits no MONOCHROME1, and copying the
        // frame as MONOCHROME2 would invert it.
        EncodedTilePhotometricInterpretation::Monochrome1 => {
            return Err(Error::UnsupportedPixelData {
                reason: "JPEG passthrough tile is MONOCHROME1, which VL Whole Slide Microscopy does not admit; the frame is decoded instead".into(),
            });
        }
        _ => {
            return Err(Error::UnsupportedPixelData {
                reason: "JPEG passthrough tile has unsupported photometric interpretation".into(),
            });
        }
    };
    let components =
        u8::try_from(raw.samples_per_pixel()).map_err(|_| Error::UnsupportedPixelData {
            reason: format!(
                "JPEG passthrough component count exceeds u8: {}",
                raw.samples_per_pixel()
            ),
        })?;
    Ok(PixelProfile {
        components,
        bits_allocated: raw.bits_allocated(),
        photometric_interpretation,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum JpegColorEvidence {
    Rgb,
    /// No color marker and component identifiers 0, 1, 2: the Aperio RGB
    /// convention, which wsi-rs also decodes as RGB unless the container
    /// declares YCbCr. Decoders that infer color from the bitstream assume
    /// YCbCr here, so [`declare_untransformed_rgb`] adds an Adobe marker.
    RgbComponentIds012,
    Ybr,
    Ambiguous,
}

fn validate_raw_jpeg_color_evidence(raw: &RawCompressedTile) -> Result<(), Error> {
    if raw.samples_per_pixel() != 3 {
        return Ok(());
    }
    let evidence = jpeg_color_evidence(raw.data())?;
    let consistent = matches!(
        (raw.photometric_interpretation(), evidence),
        (
            EncodedTilePhotometricInterpretation::Rgb,
            JpegColorEvidence::Rgb
        ) | (
            EncodedTilePhotometricInterpretation::Rgb,
            JpegColorEvidence::RgbComponentIds012
        ) | (
            EncodedTilePhotometricInterpretation::YbrFull422,
            JpegColorEvidence::Ybr
        )
    );
    if consistent {
        Ok(())
    } else {
        Err(Error::UnsupportedPixelData {
            reason: "JPEG passthrough color interpretation is ambiguous or conflicts with the encoded frame; decode and re-encode is required"
                .into(),
        })
    }
}

pub(super) fn jpeg_color_evidence(data: &[u8]) -> Result<JpegColorEvidence, Error> {
    let mut jfif = false;
    let mut sof = None;
    for segment in j2k_jpeg::iter_segments(data) {
        let segment = segment.map_err(|error| Error::Unsupported {
            reason: format!("JPEG passthrough could not inspect color markers: {error}"),
        })?;
        match segment.marker {
            0xE0 if segment.payload.starts_with(b"JFIF\0") => jfif = true,
            0xEE if segment.payload.len() >= 12 && segment.payload.starts_with(b"Adobe") => {
                return Ok(match segment.payload[11] {
                    0 => JpegColorEvidence::Rgb,
                    1 => JpegColorEvidence::Ybr,
                    2 => JpegColorEvidence::Ambiguous,
                    _ => JpegColorEvidence::Ambiguous,
                });
            }
            marker if j2k_jpeg::is_sof_marker(marker) => {
                sof = Some(
                    j2k_jpeg::parse_sof_info(marker, segment.payload).map_err(|error| {
                        Error::Unsupported {
                            reason: format!(
                                "JPEG passthrough could not inspect color components: {error}"
                            ),
                        }
                    })?,
                );
            }
            0xDA => break,
            _ => {}
        }
    }
    let Some(sof) = sof else {
        return Ok(JpegColorEvidence::Ambiguous);
    };
    let factors = sof.sampling.components();
    if factors.len() == 3
        && (factors[0].0 > factors[1].0
            || factors[0].0 > factors[2].0
            || factors[0].1 > factors[1].1
            || factors[0].1 > factors[2].1)
    {
        return Ok(JpegColorEvidence::Ybr);
    }
    if sof.component_ids.as_slice() == b"RGB" {
        return Ok(JpegColorEvidence::Rgb);
    }
    if jfif {
        return Ok(JpegColorEvidence::Ybr);
    }
    if sof.component_ids.as_slice() == [0, 1, 2] {
        return Ok(JpegColorEvidence::RgbComponentIds012);
    }
    Ok(JpegColorEvidence::Ambiguous)
}

/// Adobe APP14 segment: version 100, no flags, transform 0 (untransformed).
const ADOBE_APP14_UNTRANSFORMED: [u8; 16] = [
    0xFF, 0xEE, 0x00, 0x0E, b'A', b'd', b'o', b'b', b'e', 0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Read a raw source tile through [`declare_untransformed_rgb`].
pub(crate) fn read_raw_source_tile(
    slide: &Slide,
    request: &TileRequest,
) -> Result<RawCompressedTile, WsiError> {
    declare_untransformed_rgb(slide.read_raw_compressed_tile(request)?)
}

/// Make container-declared RGB explicit in the JPEG bitstream.
///
/// PS3.5 8.2.1 takes the color space of JPEG components from Photometric
/// Interpretation, but decoders that infer it from the bitstream read a
/// marker-less frame with component identifiers 0, 1, 2 as YCbCr. Such a frame
/// whose container declares RGB gains an Adobe APP14 transform-0 segment after
/// SOI; the entropy-coded data and decoded samples are unchanged, as are the
/// tile's region and significant bits. Other tiles are returned as read.
pub(crate) fn declare_untransformed_rgb(
    raw: RawCompressedTile,
) -> Result<RawCompressedTile, WsiError> {
    if raw.compression() != Compression::Jpeg
        || raw.samples_per_pixel() != 3
        || raw.photometric_interpretation() != EncodedTilePhotometricInterpretation::Rgb
        || !raw.data().starts_with(&[0xFF, 0xD8])
        || !matches!(
            jpeg_color_evidence(raw.data()),
            Ok(JpegColorEvidence::RgbComponentIds012)
        )
    {
        return Ok(raw);
    }
    let (width, height, bits_allocated) = (raw.width(), raw.height(), raw.bits_allocated());
    let (tile_region, significant_bits) = (raw.tile_region(), raw.significant_bits());
    let data = raw.into_data();
    let mut declared = Vec::new();
    declared
        .try_reserve_exact(data.len() + ADOBE_APP14_UNTRANSFORMED.len())
        .map_err(|_| WsiError::Unsupported {
            reason: "JPEG RGB color declaration exceeds available memory".into(),
        })?;
    declared.extend_from_slice(&data[..2]);
    declared.extend_from_slice(&ADOBE_APP14_UNTRANSFORMED);
    declared.extend_from_slice(&data[2..]);
    let builder = RawCompressedTile::builder(Compression::Jpeg)
        .dimensions(width, height)
        .bits_allocated(bits_allocated)
        .samples_per_pixel(3)
        .photometric_interpretation(EncodedTilePhotometricInterpretation::Rgb)
        .tile_region(tile_region);
    let builder = match significant_bits {
        Some(bits) => builder.significant_bits(bits),
        None => builder,
    };
    builder
        .data(declared)
        .build()
        .map_err(|error| WsiError::Unsupported {
            reason: format!("JPEG RGB color declaration produced an invalid tile: {error}"),
        })
}

pub(crate) fn raw_jpeg_profile_can_passthrough(
    profile: PixelProfile,
    allow_raw_rgb_passthrough: bool,
) -> bool {
    profile.photometric_interpretation != "RGB" || allow_raw_rgb_passthrough
}

/// Whether a raw JPEG tile encodes exactly one output `frame`.
///
/// wsi-rs reports the logical (cropped) size of right and bottom TIFF edge
/// tiles, which encode a full physical tile, and DICOM edge frames as their
/// full frame with the logical size as the tile region. A TILED_FULL edge
/// frame is also full size, with pixels beyond the Total Pixel Matrix as
/// padding, so the encoded size must equal the frame and the logical size
/// may be smaller. The tile must also sit where the frame needs it; see
/// [`raw_tile_copies_as_frame`].
pub(crate) fn raw_jpeg_matches_frame_geometry(raw: &RawCompressedTile, frame: FrameExtent) -> bool {
    raw.compression() == Compression::Jpeg
        && raw.width() <= frame.columns
        && raw.height() <= frame.rows
        && raw_tile_copies_as_frame(raw, frame)
        && jpeg_encoded_dimensions(raw.data()) == Some((frame.columns, frame.rows))
}

fn jpeg_encoded_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    for segment in j2k_jpeg::iter_segments(data) {
        let segment = segment.ok()?;
        if j2k_jpeg::is_sof_marker(segment.marker) {
            let sof = j2k_jpeg::parse_sof_info(segment.marker, segment.payload).ok()?;
            let (width, height) = sof.dimensions;
            return Some((u32::from(width), u32::from(height)));
        }
        if segment.marker == 0xDA {
            return None;
        }
    }
    None
}

const WSI_RS_EMPTY_TIFF_TILE_REASON: &str = "empty TIFF tiles";

pub(super) fn raw_compressed_error_is_empty_tile(err: &wsi_rs::WsiError) -> bool {
    matches!(
        err,
        wsi_rs::WsiError::Unsupported { reason }
            if reason.contains(WSI_RS_EMPTY_TIFF_TILE_REASON)
    )
}

pub(super) fn blank_jpeg_baseline_frame(
    frame_columns: u32,
    frame_rows: u32,
) -> Result<JpegBaselinePlannedFrame, Error> {
    let profile = PixelProfile {
        components: 3,
        bits_allocated: 8,
        photometric_interpretation: "YBR_FULL_422",
    };
    let uncompressed_bytes =
        jpeg_baseline_fallback_uncompressed_bytes(frame_columns, frame_rows, profile)?;
    Ok(JpegBaselinePlannedFrame::Blank {
        profile,
        uncompressed_bytes,
    })
}

pub(super) fn encode_blank_jpeg_baseline_frame(
    frame_columns: u32,
    frame_rows: u32,
    jpeg_quality: u8,
    optimize_huffman: bool,
    uncompressed_bytes: u64,
    cache: &mut Option<(Vec<u8>, Duration)>,
) -> Result<(Vec<u8>, Duration), Error> {
    if let Some((data, _)) = cache {
        return Ok((data.clone(), Duration::ZERO));
    }
    let pixels_len = usize::try_from(uncompressed_bytes).map_err(|_| Error::Unsupported {
        reason: "blank JPEG Baseline frame byte count exceeds addressable memory".into(),
    })?;
    let pixels = vec![255u8; pixels_len];
    let encode_started = Instant::now();
    let encoded = encode_jpeg_baseline_cpu_fragment(
        JpegSamples::Rgb8 {
            data: &pixels,
            width: frame_columns,
            height: frame_rows,
        },
        jpeg_quality,
        optimize_huffman,
        JpegSubsampling::Ybr422,
        jpeg_baseline_cpu_restart_interval(frame_columns, frame_rows, JpegSubsampling::Ybr422),
    )?;
    let duration = encode_started.elapsed();
    let data = encoded.data;
    *cache = Some((data.clone(), duration));
    Ok((data, duration))
}

/// Whether every frame of an RGB JPEG level can be copied, so no frame needs
/// a YBR re-encode that would mix photometric interpretations in one instance.
pub(crate) fn raw_rgb_passthrough_has_no_geometry_fallback(
    slide: &Slide,
    level: &wsi_rs::Level,
    location: JpegBaselineFrameLocation,
    geometry: JpegBaselineFrameGeometry,
) -> bool {
    let full_frame_grid = level
        .dimensions
        .0
        .is_multiple_of(u64::from(geometry.frame_columns))
        && level
            .dimensions
            .1
            .is_multiple_of(u64::from(geometry.frame_rows));
    match level.tile_layout {
        TileLayout::Regular {
            tile_width,
            tile_height,
            ..
        } => {
            tile_width == geometry.frame_columns
                && tile_height == geometry.frame_rows
                && (full_frame_grid || edge_tiles_match_frame_geometry(slide, location, geometry))
        }
        TileLayout::WholeLevel {
            virtual_tile_width,
            virtual_tile_height,
            ..
        } => {
            virtual_tile_width == geometry.frame_columns
                && virtual_tile_height == geometry.frame_rows
        }
        TileLayout::Irregular { .. } | _ => false,
    }
}

/// TIFF pads edge tiles to full size, so a level that is not a multiple of the
/// tile size still copies whole when its last column and row tiles do.
fn edge_tiles_match_frame_geometry(
    slide: &Slide,
    location: JpegBaselineFrameLocation,
    geometry: JpegBaselineFrameGeometry,
) -> bool {
    let (Some(last_col), Some(last_row)) = (
        geometry.tiles_across.checked_sub(1),
        geometry.tiles_down.checked_sub(1),
    ) else {
        return false;
    };
    let (Ok(last_col), Ok(last_row)) = (i64::try_from(last_col), i64::try_from(last_row)) else {
        return false;
    };
    [(last_col, 0), (0, last_row), (last_col, last_row)]
        .into_iter()
        .all(|(col, row)| {
            slide
                .read_raw_compressed_tile(&location.tile_request(col, row))
                .is_ok_and(|raw| {
                    raw_jpeg_matches_frame_geometry(
                        &raw,
                        geometry.frame_extent(col.unsigned_abs(), row.unsigned_abs()),
                    )
                })
        })
}

pub(super) fn uncompressed_frame_bytes(raw: &RawCompressedTile) -> Result<u64, Error> {
    crate::lossy::uncompressed_pixel_bytes(
        u64::from(raw.width()),
        u64::from(raw.height()),
        u64::from(raw.samples_per_pixel()),
        raw.bits_allocated(),
    )
}

pub(super) fn jpeg_baseline_samples(
    data: &[u8],
    profile: PixelProfile,
    width: u32,
    height: u32,
) -> Result<JpegSamples<'_>, Error> {
    match profile.components {
        1 => Ok(JpegSamples::Gray8 {
            data,
            width,
            height,
        }),
        3 => Ok(JpegSamples::Rgb8 {
            data,
            width,
            height,
        }),
        components => Err(Error::UnsupportedPixelData {
            reason: format!("JPEG Baseline supports 1 or 3 components, got {components}"),
        }),
    }
}

pub(super) fn jpeg_baseline_output_profile(
    source: PixelProfile,
) -> Result<(PixelProfile, JpegSubsampling), Error> {
    if source.bits_allocated != 8 {
        return Err(Error::UnsupportedPixelData {
            reason: format!(
                "JPEG Baseline fallback requires 8-bit samples, got {}",
                source.bits_allocated
            ),
        });
    }
    match source.components {
        1 => Ok((source, JpegSubsampling::Gray)),
        3 => Ok((
            PixelProfile {
                components: 3,
                bits_allocated: 8,
                photometric_interpretation: "YBR_FULL_422",
            },
            JpegSubsampling::Ybr422,
        )),
        components => Err(Error::UnsupportedPixelData {
            reason: format!("JPEG Baseline supports 1 or 3 components, got {components}"),
        }),
    }
}

pub(super) fn jpeg_baseline_fallback_uncompressed_bytes(
    frame_columns: u32,
    frame_rows: u32,
    profile: PixelProfile,
) -> Result<u64, Error> {
    crate::lossy::uncompressed_pixel_bytes(
        u64::from(frame_columns),
        u64::from(frame_rows),
        u64::from(profile.components),
        profile.bits_allocated,
    )
}

pub(super) fn encode_jpeg_baseline_cpu_fragment(
    samples: JpegSamples<'_>,
    jpeg_quality: u8,
    optimize_huffman: bool,
    subsampling: JpegSubsampling,
    restart_interval: Option<u16>,
) -> Result<EncodedJpeg, Error> {
    let options = j2k_jpeg::JpegEncodeOptions {
        quality: jpeg_quality,
        subsampling,
        restart_interval,
        backend: JpegBackend::Cpu,
    };
    if optimize_huffman {
        j2k_jpeg::encode_jpeg_baseline_optimized(samples, options)
    } else {
        j2k_jpeg::encode_jpeg_baseline(samples, options)
    }
    .map_err(|source| Error::Encode {
        message: source.to_string(),
    })
}

pub(super) fn jpeg_baseline_cpu_restart_interval(
    frame_columns: u32,
    frame_rows: u32,
    subsampling: JpegSubsampling,
) -> Option<u16> {
    let (mcu_width, mcu_height) = match subsampling {
        JpegSubsampling::Gray | JpegSubsampling::Ybr444 => (8, 8),
        JpegSubsampling::Ybr422 => (16, 8),
        JpegSubsampling::Ybr420 => (16, 16),
    };
    let mcu_count = frame_columns
        .div_ceil(mcu_width)
        .saturating_mul(frame_rows.div_ceil(mcu_height));
    (mcu_count > 64).then_some(64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lossless_gray_1x1() -> Vec<u8> {
        vec![
            0xff, 0xd8, // SOI
            0xff, 0xc3, 0x00, 0x0b, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xff,
            0xc4, 0x00, 0x14, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // category zero
            0xff, 0xda, 0x00, 0x08, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x7f, 0xff, 0xd9,
        ]
    }

    fn color_sof(component_ids: [u8; 3], sampling: [(u8, u8); 3]) -> Vec<u8> {
        let mut jpeg = vec![
            0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x01, 0x00, 0x01, 0x03,
        ];
        for index in 0..3 {
            jpeg.push(component_ids[index]);
            jpeg.push((sampling[index].0 << 4) | sampling[index].1);
            jpeg.push(0);
        }
        jpeg.extend_from_slice(&[0xff, 0xd9]);
        jpeg
    }

    #[test]
    fn empty_tile_detection_is_limited_to_wsi_rs_unsupported_reason() {
        assert!(raw_compressed_error_is_empty_tile(
            &wsi_rs::WsiError::Unsupported {
                reason: "JPEG passthrough does not support empty TIFF tiles".into(),
            }
        ));
        assert!(!raw_compressed_error_is_empty_tile(
            &wsi_rs::WsiError::TileRead {
                col: 0,
                row: 0,
                level: 0,
                reason: "empty TIFF tiles".into(),
            }
        ));
    }

    #[test]
    fn jpeg_baseline_passthrough_rejects_lossless_sof3() {
        let raw = RawCompressedTile::builder(Compression::Jpeg)
            .dimensions(1, 1)
            .bits_allocated(8)
            .samples_per_pixel(1)
            .photometric_interpretation(EncodedTilePhotometricInterpretation::Monochrome2)
            .data(lossless_gray_1x1())
            .build()
            .unwrap();

        let error = pixel_profile_from_raw_jpeg_tile(&raw)
            .expect_err("SOF3 must not be emitted under the JPEG Baseline UID");
        assert!(error.to_string().contains("requires SOF0"));
    }

    #[test]
    fn jpeg_passthrough_requires_unambiguous_encoded_color_evidence() {
        assert_eq!(
            jpeg_color_evidence(&color_sof([1, 2, 3], [(1, 1); 3])).unwrap(),
            JpegColorEvidence::Ambiguous
        );
        assert_eq!(
            jpeg_color_evidence(&color_sof([1, 2, 3], [(2, 1), (1, 1), (1, 1)])).unwrap(),
            JpegColorEvidence::Ybr
        );
        assert_eq!(
            jpeg_color_evidence(&color_sof(*b"RGB", [(1, 1); 3])).unwrap(),
            JpegColorEvidence::Rgb
        );
    }
}
