use std::collections::{BTreeMap, HashSet};
#[cfg(all(feature = "metal", target_os = "macos"))]
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(all(feature = "metal", target_os = "macos"))]
use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[cfg(test)]
use j2k::J2kLosslessSamples;
#[cfg(test)]
use j2k::{J2kView, ReversibleTransform};
#[cfg(test)]
use j2k_core::CompressedTransferSyntax;
#[cfg(all(feature = "metal", target_os = "macos"))]
use j2k_core::PixelFormat as J2kPixelFormat;
use j2k_jpeg::{EncodedJpeg, JpegBackend, JpegSamples, JpegSubsampling};
#[cfg(all(feature = "metal", target_os = "macos"))]
use j2k_jpeg_metal::{encode_jpeg_baseline_batch_from_metal_buffers, JpegBaselineMetalEncodeTile};
use rayon::prelude::*;
#[cfg(all(feature = "metal", target_os = "macos"))]
use wsi_rs::DeviceTile;
#[cfg(test)]
use wsi_rs::EncodedTilePhotometricInterpretation;
#[cfg(test)]
use wsi_rs::LevelSourceKind;
#[cfg(any(test, all(feature = "metal", target_os = "macos")))]
use wsi_rs::TileLayout;
#[cfg(all(feature = "metal", target_os = "macos"))]
use wsi_rs::TileRequest;
use wsi_rs::{
    Compression, LevelIdx, PlaneIdx, PlaneSelection, RawCompressedTile, RegionRequest, SceneId,
    SeriesId, Slide,
};
#[cfg(all(feature = "metal", target_os = "macos"))]
use wsi_rs::{TileOutputPreference, TilePixels};

#[cfg(test)]
use crate::api::Export;
use crate::defaults::default_transfer_syntax_for_source;
#[cfg(all(feature = "metal", target_os = "macos"))]
use crate::encode;
use crate::encode::{DicomJ2kEncoder, EncodedDicomJ2kFrame};
use crate::error::Error;
use crate::instance_context::{DicomInstanceContext, InstanceDicomObjectParams};
use crate::metadata::DicomMetadata;
#[cfg(test)]
use crate::metadata::MetadataSource;
use crate::options::{
    CodecValidation, EncodeBackendPreference, ExportOptions, JpegDirectHtj2kProfile, TransferSyntax,
};
use crate::report::{
    duration_as_reported_micros, EncodedFrame, ExportMetrics, ExportReport, IccProfileSource,
    InstanceReport, JpegRetileRejectionReason, RouteCorpusCoverageFailure,
    RouteCorpusCoverageReport, RouteCoverageReport, RouteProfileReport,
};
#[cfg(test)]
use crate::report::{GpuEncodeMetrics, RouteCounters, WriteTimings};
use crate::request::DefaultTransferSyntaxRequest;
#[cfg(test)]
use crate::request::FrameSamples;
use crate::request::{
    ExportRequest, J2kFrameEncodeRequest, RouteCoverageRequest, RouteCoverageTarget,
    RouteProfileRequest, RouteProgressSink,
};
#[cfg(all(feature = "metal", target_os = "macos"))]
use crate::routing::level_is_synthetic_downsample;
use crate::routing::{
    j2k_encode_backend, j2k_encode_transfer_syntax, j2k_family_passthrough_probe_allowed,
    j2k_route_tile_size, unsupported_j2k_route_error,
};
#[cfg(all(feature = "metal", target_os = "macos"))]
use crate::tile::pixel_profile_from_device_format;
#[cfg(test)]
use crate::tile::prepare_tile_samples;
use crate::tile::{optical_path_groups, prepare_tile_samples_with_limit, PixelProfile};
use crate::uid::{deterministic_instance_path, uid_from_seed};
use crate::writer::{
    pixel_data_offsets_from_lengths, unique_spool_path, write_dicom_object_with_direct_pixel_data,
    write_dicom_object_with_spooled_pixel_data, write_dicom_object_with_streamed_pixel_data,
    BufferedPixelDataSink, FrameGrid, LossyCompressionMetadata, PixelDataOffsetTables,
    PixelDataSink, PixelDataSpool,
};

mod frame_region;
mod hybrid_lane;
mod icc_profile;
mod j2k_direct_htj2k;
mod j2k_policy;
mod jpeg_baseline;
mod jpeg_baseline_instance;
mod jpeg_direct_htj2k;
mod jpeg_passthrough;
mod jpeg_retile;
mod lossless_j2k_cpu;
mod lossless_j2k_instance;
mod lossless_j2k_plan;
#[cfg(all(feature = "metal", target_os = "macos"))]
mod metal_compose;
#[cfg(all(feature = "metal", target_os = "macos"))]
mod metal_input;
#[cfg(all(feature = "metal", target_os = "macos"))]
mod metal_route;
#[cfg(all(feature = "metal", target_os = "macos"))]
mod metal_row_batch;
mod profiling;
#[cfg(all(feature = "metal", target_os = "macos"))]
mod route_cache;
mod tile_grid;
#[cfg(all(feature = "metal", target_os = "macos"))]
use self::metal_compose::{MetalComposeTileRequest, MetalStripComposer};
#[cfg(all(test, feature = "metal", target_os = "macos"))]
use self::metal_input::{
    cpu_input_device_encode_auto_allowed, cpu_input_device_encode_auto_probe_allowed,
    select_auto_lossless_j2k_probe_route, wsi_rs_device_decode_opted_in,
    AutoLosslessJ2kRouteCandidate, CpuEncodedTileRun,
};
#[cfg(all(feature = "metal", target_os = "macos"))]
use self::metal_input::{
    empty_metal_tile_run, metal_j2k_encode_batch_count, probe_auto_metal_input_tile_run,
    try_encode_metal_input_tile_run, MetalEncodedRowRunKey, MetalEncodedTileRun,
    MetalInputTileReader, MetalSourceTileKey, PendingMetalEncodedGridRun,
    PendingMetalEncodedTileRun, RoutedLosslessJ2kTile,
};
#[cfg(all(feature = "metal", target_os = "macos"))]
use metal_route::{
    output_frame_maps_to_wsi_rs_tile, output_tile_maps_to_wsi_rs_tile, regular_tiled_source_layout,
    whole_level_strip_layout,
};
#[cfg(all(feature = "metal", target_os = "macos"))]
use metal_row_batch::{
    try_encode_metal_aligned_tile_run, try_encode_metal_whole_level_strip_run,
    WholeLevelStripLayout,
};
#[cfg(all(feature = "metal", target_os = "macos"))]
use route_cache::{
    cached_auto_metal_input_decision, flush_persistent_auto_metal_input_route_cache_if_requested,
    load_persistent_auto_metal_input_route_cache_if_requested,
    store_cached_auto_metal_input_decision, AutoLosslessJ2kRouteDecision,
    AutoMetalInputRouteCacheKey,
};
#[cfg(all(test, feature = "metal", target_os = "macos"))]
use route_cache::{
    clear_auto_metal_input_route_cache_for_tests,
    clear_auto_metal_input_route_cache_state_for_tests, WSI_DICOM_AUTO_ROUTE_CACHE_ENV,
};

use self::frame_region::PreparedCpuRegion;
use self::frame_region::{FrameRectGrid, FrameRectOverflowReasons, OutputFrameRect};
use self::icc_profile::resolve_icc_profile;
#[cfg(test)]
use self::j2k_policy::j2k_passthrough_frame;
use self::j2k_policy::{
    j2k_fallback_profile, j2k_fallback_reversible_transform, j2k_non_passthrough_encode_allowed,
    lossless_j2k_cpu_fallback_indices, reject_lossy_j2k_lossless_fallback,
};
#[cfg(test)]
use self::jpeg_baseline::jpeg_baseline_frame_geometry;
use self::jpeg_baseline::{
    blank_jpeg_baseline_frame, encode_jpeg_baseline_cpu_fragment,
    jpeg_baseline_cpu_restart_interval, jpeg_baseline_fallback_uncompressed_bytes,
    raw_compressed_error_is_empty_tile, uncompressed_frame_bytes, JpegBaselineFallbackFrame,
    JpegBaselineMetalEncodedRun, JpegBaselinePlannedFrame,
};
pub(crate) use self::jpeg_baseline::{
    jpeg_baseline_route_frame_geometry, pixel_profile_from_raw_jpeg_tile,
    raw_jpeg_matches_frame_geometry, raw_jpeg_profile_can_passthrough,
    raw_rgb_passthrough_has_no_geometry_fallback, JpegBaselineFrameGeometry,
    JpegBaselineFrameLocation,
};
use self::jpeg_baseline_instance::export_jpeg_passthrough_instance;
pub(crate) use self::jpeg_passthrough::read_raw_jpeg_passthrough_tile;
use self::jpeg_passthrough::{
    try_plan_direct_jpeg_passthrough_frames, DirectJpegPassthroughFrameWriter,
};
use self::jpeg_retile::{read_raw_jpeg_retile_display_tile, RawJpegRetileProbe};
use self::lossless_j2k_cpu::{
    encode_cpu_input_lossless_j2k_planned_batch, lossless_j2k_samples_from_prepared_region,
    prepare_cpu_input_lossless_j2k_tile, LosslessJ2kCpuBatchOutcome, LosslessJ2kCpuBatchSettings,
};
#[cfg(test)]
use self::lossless_j2k_cpu::{encode_cpu_input_lossless_j2k_tile_batch, LosslessJ2kCpuBatchFrame};
use self::lossless_j2k_instance::export_instance;
#[cfg(all(feature = "metal", target_os = "macos"))]
use self::lossless_j2k_instance::{prepare_lossless_j2k_instance, PendingLosslessJ2kInstance};
pub(crate) use self::lossless_j2k_plan::{plan_lossless_j2k_row, LosslessJ2kPlannedFrame};
use self::lossless_j2k_plan::{plan_lossless_j2k_rows, J2kPassthroughFrame};
use self::profiling::{check_route_level_deadline, validate_max_level_elapsed, RouteLevelDeadline};
use self::tile_grid::{checked_frame_count_u32, TileGrid};

type EncodedJpegBaselineFrame = (EncodedJpeg, PixelProfile, Duration, Duration, Duration);

#[derive(Clone, Copy)]
struct DicomExportInstanceJob<'a> {
    ordinal: usize,
    instance_number: u32,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    level: &'a wsi_rs::Level,
}

#[derive(Clone, Copy)]
struct DicomRouteProfileJob<'a> {
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    level: &'a wsi_rs::Level,
}

impl DicomRouteProfileJob<'_> {
    fn location(&self) -> JpegBaselineFrameLocation {
        JpegBaselineFrameLocation {
            scene_idx: self.scene_idx,
            series_idx: self.series_idx,
            level_idx: self.level_idx,
            z: self.z,
            c: self.c,
            t: self.t,
        }
    }
}

fn j2k_lossy_compression_method(transfer_syntax: TransferSyntax) -> &'static str {
    match transfer_syntax {
        TransferSyntax::Htj2k
        | TransferSyntax::Htj2kLossless
        | TransferSyntax::Htj2kLosslessRpcl => "ISO_15444_15",
        TransferSyntax::Jpeg2000 | TransferSyntax::Jpeg2000Lossless => "ISO_15444_1",
        TransferSyntax::JpegBaseline8Bit | TransferSyntax::ExplicitVrLittleEndian => "ISO_10918_1",
    }
}

#[cfg(all(test, feature = "metal", target_os = "macos"))]
const STATUMEN_JPEG_DEVICE_DECODE_ENV: &str = "STATUMEN_JPEG_DEVICE_DECODE";

#[cfg(all(test, feature = "metal", target_os = "macos"))]
const STATUMEN_JP2K_DEVICE_DECODE_ENV: &str = "STATUMEN_JP2K_DEVICE_DECODE";

const DIRECT_JPEG_PASSTHROUGH_WRITE_CHUNK_FRAMES: usize = 2048;

const WSI_DICOM_EXPORT_INSTANCE_WORKERS_ENV: &str = "WSI_DICOM_EXPORT_INSTANCE_WORKERS";

#[cfg(all(feature = "metal", target_os = "macos"))]
const WSI_DICOM_METAL_ROW_BATCH_ROWS_ENV: &str = "WSI_DICOM_METAL_ROW_BATCH_ROWS";

#[cfg(all(feature = "metal", target_os = "macos"))]
const DEFAULT_METAL_ROW_BATCH_TARGET_TILES: usize = 384;
const PREFER_DEVICE_TINY_HTJ2K_RPCL_CPU_MAX_FRAMES: u64 = 128;

#[cfg(all(feature = "metal", target_os = "macos"))]
const DEFAULT_GPU_PIPELINE_DEPTH: usize = 2;

#[cfg(all(feature = "metal", target_os = "macos"))]
fn effective_gpu_pipeline_depth(options: &ExportOptions) -> usize {
    options
        .gpu_pipeline_depth
        .unwrap_or(DEFAULT_GPU_PIPELINE_DEPTH)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn effective_gpu_row_batch_target_tiles(options: &ExportOptions) -> Option<usize> {
    Some(
        options
            .gpu_row_batch_target_tiles
            .unwrap_or(DEFAULT_METAL_ROW_BATCH_TARGET_TILES),
    )
}

fn effective_lossless_j2k_encode_backend(
    options: &ExportOptions,
    frame_count: u64,
) -> EncodeBackendPreference {
    if options.encode_backend == EncodeBackendPreference::PreferDevice {
        if options.transfer_syntax == TransferSyntax::Jpeg2000Lossless {
            // Keep classic J2K lossless on CPU until Metal beats CPU in route-level benchmarks.
            return EncodeBackendPreference::CpuOnly;
        }
        if options.transfer_syntax == TransferSyntax::Htj2kLosslessRpcl
            && frame_count <= PREFER_DEVICE_TINY_HTJ2K_RPCL_CPU_MAX_FRAMES
        {
            return EncodeBackendPreference::CpuOnly;
        }
    }
    j2k_encode_backend(options.transfer_syntax, options.encode_backend)
}

fn jpeg_direct_htj2k_supported_for_backend(
    transfer_syntax: TransferSyntax,
    backend: EncodeBackendPreference,
) -> bool {
    if !jpeg_direct_htj2k::transfer_syntax(transfer_syntax) {
        return false;
    }
    backend != EncodeBackendPreference::RequireDevice
}

#[cfg(all(feature = "metal", target_os = "macos"))]
const LOSSLESS_J2K_AUTO_ROUTE_PROBE_MAX_FRAMES: usize = 16;

#[cfg(all(feature = "metal", target_os = "macos"))]
const LOSSLESS_J2K_AUTO_ROUTE_MIN_FRAMES: u64 = 16;

#[cfg(all(feature = "metal", target_os = "macos"))]
const LOSSLESS_J2K_AUTO_PARTIAL_GPU_MIN_FRAMES: usize = 32;

#[cfg(all(feature = "metal", target_os = "macos"))]
const LOSSLESS_J2K_AUTO_ROUTE_SPEEDUP_NUMERATOR: u128 = 92;

#[cfg(all(feature = "metal", target_os = "macos"))]
const LOSSLESS_J2K_AUTO_ROUTE_SPEEDUP_DENOMINATOR: u128 = 100;

#[cfg(any(test, not(all(feature = "metal", target_os = "macos"))))]
const LOSSLESS_J2K_CPU_ROW_BATCH_TARGET_TILES: u64 = 256;
const LOSSLESS_J2K_DIRECT_PIXELDATA_MAX_MEMORY_BYTES: u64 = 256 * 1024 * 1024;
const LOSSLESS_J2K_DIRECT_PIXELDATA_BYTES_PER_PIXEL: u64 = 6;

#[cfg(any(test, not(all(feature = "metal", target_os = "macos"))))]
fn lossless_j2k_cpu_row_batch_count(tiles_across: u64, remaining_rows: u64) -> u64 {
    if tiles_across == 0 {
        return 1;
    }
    let rows = LOSSLESS_J2K_CPU_ROW_BATCH_TARGET_TILES
        .div_ceil(tiles_across)
        .max(1);
    rows.min(remaining_rows.max(1))
}

fn lossless_j2k_direct_pixel_data_memory_bytes(rayon_threads: usize) -> u64 {
    let scaled = u64::try_from(rayon_threads)
        .unwrap_or(u64::MAX)
        .saturating_mul(32 * 1024 * 1024);
    scaled.clamp(
        64 * 1024 * 1024,
        LOSSLESS_J2K_DIRECT_PIXELDATA_MAX_MEMORY_BYTES,
    )
}

fn lossless_j2k_use_direct_pixel_data(
    frame_count: u32,
    tile_size: u32,
    rayon_threads: usize,
) -> bool {
    if frame_count == 0 || rayon_threads <= 1 {
        return false;
    }
    let estimated_bytes = u64::from(frame_count)
        .saturating_mul(u64::from(tile_size))
        .saturating_mul(u64::from(tile_size))
        .saturating_mul(LOSSLESS_J2K_DIRECT_PIXELDATA_BYTES_PER_PIXEL);
    estimated_bytes <= lossless_j2k_direct_pixel_data_memory_bytes(rayon_threads)
}

fn level_pixel_spacing_mm(slide: &Slide, level: &wsi_rs::Level) -> Option<(f64, f64)> {
    let (mpp_x, mpp_y) = slide.dataset().properties.mpp()?;
    let downsample = level.downsample;
    if !(mpp_x.is_finite() && mpp_y.is_finite() && downsample.is_finite()) {
        return None;
    }
    if mpp_x <= 0.0 || mpp_y <= 0.0 || downsample <= 0.0 {
        return None;
    }
    Some((mpp_y * downsample / 1000.0, mpp_x * downsample / 1000.0))
}

fn require_pixel_spacing_mm(pixel_spacing_mm: Option<(f64, f64)>) -> Result<(f64, f64), Error> {
    pixel_spacing_mm.ok_or_else(|| Error::Metadata {
        reason: "VL WSI VOLUME export requires pixel spacing metadata".into(),
    })
}

fn route_profile_available_frames(
    slide: &Slide,
    options: &ExportOptions,
    level: &wsi_rs::Level,
    location: JpegBaselineFrameLocation,
) -> Result<u64, Error> {
    if options.transfer_syntax == TransferSyntax::JpegBaseline8Bit {
        let geometry =
            jpeg_baseline_route_frame_geometry(slide, level, location, options.tile_size)?;
        return geometry
            .tiles_across
            .checked_mul(geometry.tiles_down)
            .ok_or_else(|| Error::Unsupported {
                reason: "route profile JPEG frame count overflow".into(),
            });
    }
    let (matrix_columns, matrix_rows) = level.dimensions;
    let tile_size = j2k_route_tile_size(options, level)?;
    matrix_columns
        .div_ceil(u64::from(tile_size))
        .checked_mul(matrix_rows.div_ceil(u64::from(tile_size)))
        .ok_or_else(|| Error::Unsupported {
            reason: "route profile frame count overflow".into(),
        })
}

/// Encode one composed tile into finished compressed DICOM frame bytes.
pub fn encode_dicom_j2k_frame(request: J2kFrameEncodeRequest<'_>) -> Result<EncodedFrame, Error> {
    if !request.transfer_syntax.is_lossless_j2k_family() {
        return Err(Error::Unsupported {
            reason: "single-frame DICOM J2K encode requires a JPEG 2000 or HTJ2K transfer syntax"
                .into(),
        });
    }

    let mut encoder = DicomJ2kEncoder::new(
        request.encode_backend,
        request.transfer_syntax,
        request.codec_validation,
    );
    let encoded = encoder.encode(request.samples.to_j2k()?)?;
    let bytes = encoded.codestream_bytes()?.to_vec();

    Ok(EncodedFrame {
        transfer_syntax_uid: request.transfer_syntax.uid(),
        bytes,
        used_device_encode: encoded.used_device_encode,
        used_device_validation: encoded.used_device_validation,
        encode_micros: encoded.encode_duration.as_micros(),
        validation_micros: encoded.validation_duration.as_micros(),
    })
}

/// Export a wsi-rs-readable WSI into DICOM VL Whole Slide Microscopy files.
pub fn export_dicom(request: ExportRequest) -> Result<ExportReport, Error> {
    request.validate()?;
    #[cfg(all(feature = "metal", target_os = "macos"))]
    load_persistent_auto_metal_input_route_cache_if_requested()?;
    if request.options.transfer_syntax != TransferSyntax::JpegBaseline8Bit
        && !request.options.transfer_syntax.is_j2k_family()
    {
        return Err(Error::Unsupported {
            reason: "only JPEG Baseline passthrough, JPEG 2000, JPEG 2000 Lossless, and HTJ2K transfer syntaxes are implemented"
                .into(),
        });
    }
    let metadata = request.metadata.resolve()?;
    fs::create_dir_all(&request.output_dir).map_err(|source| Error::Io {
        path: request.output_dir.clone(),
        source,
    })?;

    let slide = Slide::open(&request.source_path).map_err(|source| Error::SourceOpen {
        path: request.source_path.clone(),
        message: source.to_string(),
    })?;

    let study_uid = metadata
        .study_instance_uid
        .clone()
        .unwrap_or_else(|| uid_from_seed(&format!("study:{}", request.source_path.display())));
    let jobs = dicom_export_instance_jobs(&slide, &request)?;
    preflight_output_paths(&request, &jobs)?;
    let instances = export_dicom_instance_jobs(&slide, &request, &metadata, &study_uid, &jobs)?;

    if instances.is_empty() {
        return Err(Error::Unsupported {
            reason: match request.level_filter {
                Some(level) => {
                    format!("export level {level} is not available or produced no frames")
                }
                None => "slide produced no exportable DICOM instances".into(),
            },
        });
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    flush_persistent_auto_metal_input_route_cache_if_requested()?;

    let metrics = instances
        .iter()
        .fold(ExportMetrics::default(), |mut metrics, instance| {
            metrics.add_assign(instance.metrics);
            metrics
        });

    Ok(ExportReport {
        output_dir: request.output_dir,
        instances,
        metrics,
    })
}

fn dicom_export_instance_jobs<'a>(
    slide: &'a Slide,
    request: &ExportRequest,
) -> Result<Vec<DicomExportInstanceJob<'a>>, Error> {
    let mut jobs = Vec::new();
    for (scene_idx, scene) in slide.dataset().scenes.iter().enumerate() {
        for (series_idx, series) in scene.series.iter().enumerate() {
            for (level_idx, level) in series.levels.iter().enumerate() {
                let level_idx = u32::try_from(level_idx).map_err(|_| Error::Unsupported {
                    reason: "export level index exceeds u32".into(),
                })?;
                if request
                    .level_filter
                    .is_some_and(|requested_level| requested_level != level_idx)
                {
                    continue;
                }
                for z in 0..series.axes.z {
                    for t in 0..series.axes.t {
                        for c in optical_path_groups(series.axes.c) {
                            let instance_number =
                                u32::try_from(jobs.len() + 1).map_err(|_| Error::Unsupported {
                                    reason: "DICOM instance count exceeds u32".into(),
                                })?;
                            jobs.push(DicomExportInstanceJob {
                                ordinal: jobs.len(),
                                instance_number,
                                scene_idx,
                                series_idx,
                                level_idx,
                                z,
                                c,
                                t,
                                level,
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(jobs)
}

fn dicom_route_profile_jobs(
    slide: &Slide,
    level_filter: Option<u32>,
    max_levels: Option<u32>,
) -> Result<Vec<DicomRouteProfileJob<'_>>, Error> {
    let max_levels =
        max_levels
            .map(usize::try_from)
            .transpose()
            .map_err(|_| Error::Unsupported {
                reason: "route profiling max_levels exceeds platform addressable memory".into(),
            })?;
    let mut jobs = Vec::new();
    for (scene_idx, scene) in slide.dataset().scenes.iter().enumerate() {
        for (series_idx, series) in scene.series.iter().enumerate() {
            let level_limit = max_levels
                .unwrap_or(series.levels.len())
                .min(series.levels.len());
            for (level_idx, level) in series.levels.iter().take(level_limit).enumerate() {
                let level_idx = u32::try_from(level_idx).map_err(|_| Error::Unsupported {
                    reason: "route profiling level index exceeds u32".into(),
                })?;
                if level_filter.is_some_and(|requested_level| requested_level != level_idx) {
                    continue;
                }
                for z in 0..series.axes.z {
                    for t in 0..series.axes.t {
                        for c in optical_path_groups(series.axes.c) {
                            jobs.push(DicomRouteProfileJob {
                                scene_idx,
                                series_idx,
                                level_idx,
                                z,
                                c,
                                t,
                                level,
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(jobs)
}

fn preflight_output_paths(
    request: &ExportRequest,
    jobs: &[DicomExportInstanceJob<'_>],
) -> Result<(), Error> {
    let mut paths = HashSet::with_capacity(jobs.len());
    for job in jobs {
        let path =
            deterministic_instance_path(&request.output_dir, job.level_idx, job.z, job.c, job.t);
        if !paths.insert(path.clone()) {
            return Err(Error::InvalidOptions {
                reason: format!("multiple export instances would write {}", path.display()),
            });
        }
        if !request.options.overwrite && path.exists() {
            return Err(Error::Io {
                path,
                source: std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "output file exists; enable overwrite to replace it",
                ),
            });
        }
    }
    Ok(())
}

fn export_dicom_instance_jobs(
    slide: &Slide,
    request: &ExportRequest,
    metadata: &DicomMetadata,
    study_uid: &str,
    jobs: &[DicomExportInstanceJob<'_>],
) -> Result<Vec<InstanceReport>, Error> {
    if jobs.len() <= 1 {
        return export_dicom_instance_jobs_serial(slide, request, metadata, study_uid, jobs);
    }

    if let Some(configured) = configured_export_instance_worker_count()? {
        let workers = configured.max(1).min(jobs.len());
        if workers <= 1 {
            return export_dicom_instance_jobs_serial(slide, request, metadata, study_uid, jobs);
        }
        return export_dicom_instance_jobs_parallel(
            slide, request, metadata, study_uid, jobs, workers,
        );
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    if hybrid_lane::prefer_device_htj2k_rpcl_hybrid_export_lanes_enabled(request, jobs)? {
        return hybrid_lane::export_dicom_instance_jobs_prefer_device_htj2k_hybrid_lanes(
            slide, request, metadata, study_uid, jobs,
        );
    }

    let default_workers = default_export_instance_worker_count(
        &request.options,
        jobs.len(),
        rayon::current_num_threads(),
    );
    if default_workers > 1 {
        return export_dicom_instance_jobs_parallel(
            slide,
            request,
            metadata,
            study_uid,
            jobs,
            default_workers,
        );
    }

    export_dicom_instance_jobs_serial(slide, request, metadata, study_uid, jobs)
}

fn export_dicom_instance_jobs_serial(
    slide: &Slide,
    request: &ExportRequest,
    metadata: &DicomMetadata,
    study_uid: &str,
    jobs: &[DicomExportInstanceJob<'_>],
) -> Result<Vec<InstanceReport>, Error> {
    jobs.iter()
        .map(|job| export_dicom_instance_job(slide, request, metadata, study_uid, job))
        .collect()
}

fn export_dicom_instance_jobs_parallel(
    slide: &Slide,
    request: &ExportRequest,
    metadata: &DicomMetadata,
    study_uid: &str,
    jobs: &[DicomExportInstanceJob<'_>],
    workers: usize,
) -> Result<Vec<InstanceReport>, Error> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .thread_name(|idx| format!("wsi-dicom-export-{idx}"))
        .build()
        .map_err(|err| Error::InvalidOptions {
            reason: format!("failed to initialize DICOM export worker pool: {err}"),
        })?;
    let mut reports = pool.install(|| {
        jobs.par_iter()
            .map(|job| {
                export_dicom_instance_job(slide, request, metadata, study_uid, job)
                    .map(|report| (job.ordinal, report))
            })
            .collect::<Result<Vec<_>, _>>()
    })?;
    reports.sort_by_key(|(ordinal, _)| *ordinal);
    Ok(reports.into_iter().map(|(_, report)| report).collect())
}

#[cfg_attr(not(all(feature = "metal", target_os = "macos")), allow(dead_code))]
fn dicom_instance_job_frame_count(
    options: &ExportOptions,
    job: &DicomExportInstanceJob<'_>,
) -> Result<u64, Error> {
    let tile_size = j2k_route_tile_size(options, job.level)?;
    let (matrix_columns, matrix_rows) = job.level.dimensions;
    TileGrid::square(matrix_columns, matrix_rows, tile_size)?.frame_count_u64()
}

fn export_dicom_instance_job(
    slide: &Slide,
    request: &ExportRequest,
    metadata: &DicomMetadata,
    study_uid: &str,
    job: &DicomExportInstanceJob<'_>,
) -> Result<InstanceReport, Error> {
    if request.options.transfer_syntax == TransferSyntax::JpegBaseline8Bit {
        export_jpeg_passthrough_instance(
            slide,
            request,
            metadata,
            study_uid,
            job.instance_number,
            job.scene_idx,
            job.series_idx,
            job.level_idx,
            job.z,
            job.c,
            job.t,
            job.level,
        )
    } else {
        export_instance(
            slide,
            request,
            metadata,
            study_uid,
            job.instance_number,
            job.scene_idx,
            job.series_idx,
            job.level_idx,
            job.z,
            job.c,
            job.t,
            job.level,
        )
    }
}

fn configured_export_instance_worker_count() -> Result<Option<usize>, Error> {
    let value = match std::env::var(WSI_DICOM_EXPORT_INSTANCE_WORKERS_ENV) {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => return Ok(None),
        Err(err) => {
            return Err(Error::InvalidOptions {
                reason: format!(
                    "{WSI_DICOM_EXPORT_INSTANCE_WORKERS_ENV} is not valid UTF-8: {err}"
                ),
            });
        }
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let workers = trimmed
        .parse::<usize>()
        .map_err(|_| Error::InvalidOptions {
            reason: format!("{WSI_DICOM_EXPORT_INSTANCE_WORKERS_ENV} must be a positive integer"),
        })?;
    if workers == 0 {
        return Err(Error::InvalidOptions {
            reason: format!("{WSI_DICOM_EXPORT_INSTANCE_WORKERS_ENV} must be greater than zero"),
        });
    }
    Ok(Some(workers))
}

fn default_export_instance_worker_count(
    options: &ExportOptions,
    job_count: usize,
    rayon_threads: usize,
) -> usize {
    if job_count <= 1 {
        return 1;
    }
    if !options.encode_backend.cpu_batch_safe() {
        return 1;
    }
    job_count.min(rayon_threads.saturating_sub(1).max(1)).max(1)
}

fn resolve_source_aware_profile_options(
    source_path: &Path,
    mut options: ExportOptions,
    level_filter: Option<u32>,
    max_levels: Option<u32>,
    source_aware_transfer_syntax: bool,
) -> Result<ExportOptions, Error> {
    if source_aware_transfer_syntax {
        let current_default =
            JpegDirectHtj2kProfile::default_for_transfer_syntax(options.transfer_syntax);
        let profile_is_default = options.jpeg_direct_htj2k_profile == current_default;
        let mut request =
            DefaultTransferSyntaxRequest::new(source_path.to_path_buf(), options.tile_size);
        request.level_filter = level_filter;
        request.max_levels = max_levels;
        options.transfer_syntax = default_transfer_syntax_for_source(request)?;
        if profile_is_default {
            options.jpeg_direct_htj2k_profile =
                JpegDirectHtj2kProfile::default_for_transfer_syntax(options.transfer_syntax);
        }
    }
    options.validate()?;
    Ok(options)
}

/// Profile the route selection and encode path for a bounded number of frames.
pub fn profile_dicom_routes(request: RouteProfileRequest) -> Result<RouteProfileReport, Error> {
    #[cfg(all(feature = "metal", target_os = "macos"))]
    load_persistent_auto_metal_input_route_cache_if_requested()?;
    if request.max_frames == 0 {
        return Err(Error::Unsupported {
            reason: "route profiling requires max_frames > 0".into(),
        });
    }
    let options = resolve_source_aware_profile_options(
        &request.source_path,
        request.options,
        Some(request.level),
        None,
        request.source_aware_transfer_syntax,
    )?;
    if options.transfer_syntax != TransferSyntax::JpegBaseline8Bit
        && !options.transfer_syntax.is_j2k_family()
    {
        return Err(Error::Unsupported {
            reason: "bounded route profiling currently supports JPEG Baseline, JPEG 2000, and HTJ2K transfer syntaxes"
                .into(),
        });
    }

    let slide = Slide::open(&request.source_path).map_err(|source| Error::SourceOpen {
        path: request.source_path.clone(),
        message: source.to_string(),
    })?;
    let jobs = dicom_route_profile_jobs(&slide, Some(request.level), None)?;
    if jobs.is_empty() {
        return Err(Error::Unsupported {
            reason: format!("route profiling level {} is not available", request.level),
        });
    }
    let started = Instant::now();
    let transfer_syntax_uid = options.transfer_syntax.uid();
    let mut metrics = ExportMetrics::default();
    let mut available_frames = 0u64;
    let mut remaining = request.max_frames;

    for job in &jobs {
        let location = job.location();
        let job_available_frames =
            route_profile_available_frames(&slide, &options, job.level, location)?;
        available_frames = available_frames.saturating_add(job_available_frames);
        if remaining == 0 || job_available_frames == 0 {
            continue;
        }
        let job_frames = remaining.min(job_available_frames);
        let job_metrics = if options.transfer_syntax == TransferSyntax::JpegBaseline8Bit {
            profile_jpeg_baseline_routes(&slide, options.clone(), job.level, location, job_frames)?
        } else {
            profile_lossless_j2k_routes(
                &slide,
                &request.source_path,
                options.clone(),
                job.level,
                location,
                job_frames,
                None,
            )?
        };
        remaining = remaining.saturating_sub(job_metrics.routes.total_frames);
        metrics.add_assign(job_metrics);
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    flush_persistent_auto_metal_input_route_cache_if_requested()?;

    Ok(RouteProfileReport {
        source_path: request.source_path,
        transfer_syntax_uid,
        level: request.level,
        requested_frames: request.max_frames,
        available_frames,
        metrics,
        elapsed_micros: duration_as_reported_micros(started.elapsed()),
    })
}

/// Profile route coverage across exportable slide planes without writing DICOM.
pub fn profile_dicom_route_coverage(
    request: RouteCoverageRequest,
) -> Result<RouteCoverageReport, Error> {
    #[cfg(all(feature = "metal", target_os = "macos"))]
    load_persistent_auto_metal_input_route_cache_if_requested()?;
    if request.max_frames_per_level == 0 {
        return Err(Error::Unsupported {
            reason: "route coverage profiling requires max_frames_per_level > 0".into(),
        });
    }
    if request.max_levels == Some(0) {
        return Err(Error::Unsupported {
            reason: "route coverage profiling requires max_levels > 0 when provided".into(),
        });
    }
    validate_max_level_elapsed(request.max_level_elapsed, "route coverage profiling")?;
    let source_path = match &request.target {
        RouteCoverageTarget::Source(source_path) => source_path.clone(),
        RouteCoverageTarget::Corpus(_) => {
            return Err(Error::Unsupported {
                reason: "route coverage profiling requires a source target".into(),
            });
        }
    };

    let options = resolve_source_aware_profile_options(
        &source_path,
        request.options,
        None,
        request.max_levels,
        request.source_aware_transfer_syntax,
    )?;

    if options.transfer_syntax != TransferSyntax::JpegBaseline8Bit
        && !options.transfer_syntax.is_j2k_family()
    {
        return Err(Error::Unsupported {
            reason: "route coverage profiling currently supports JPEG Baseline, JPEG 2000, and HTJ2K transfer syntaxes"
                .into(),
        });
    }

    let slide = Slide::open(&source_path).map_err(|source| Error::SourceOpen {
        path: source_path.clone(),
        message: source.to_string(),
    })?;
    let jobs = dicom_route_profile_jobs(&slide, None, request.max_levels)?;
    if jobs.is_empty() {
        return Err(Error::Unsupported {
            reason: "route coverage profiling requires at least one exportable level".into(),
        });
    }

    let started = Instant::now();
    let transfer_syntax_uid = options.transfer_syntax.uid();
    let mut jobs_by_level: BTreeMap<u32, Vec<DicomRouteProfileJob<'_>>> = BTreeMap::new();
    for job in jobs {
        jobs_by_level.entry(job.level_idx).or_default().push(job);
    }
    let level_count = jobs_by_level.len();
    let mut levels = Vec::with_capacity(level_count);
    let mut metrics = ExportMetrics::default();
    let mut available_frames = 0u64;

    for (level_ordinal, (level_idx, level_jobs)) in jobs_by_level.into_iter().enumerate() {
        let level_started = Instant::now();
        let mut level_available_frames = 0u64;
        for job in &level_jobs {
            level_available_frames = level_available_frames.saturating_add(
                route_profile_available_frames(&slide, &options, job.level, job.location())?,
            );
        }
        if matches!(request.progress, Some(RouteProgressSink::Stderr)) {
            eprintln!(
                "coverage level {}/{} start {} level={} available_frames={}",
                level_ordinal + 1,
                level_count,
                source_path.display(),
                level_idx,
                level_available_frames
            );
        }
        let level_deadline = RouteLevelDeadline::new(request.max_level_elapsed);
        let mut level_metrics = ExportMetrics::default();
        let mut remaining = request.max_frames_per_level;
        for job in &level_jobs {
            if remaining == 0 {
                break;
            }
            check_route_level_deadline(level_deadline, level_idx)?;
            let location = job.location();
            let job_available_frames =
                route_profile_available_frames(&slide, &options, job.level, location)?;
            if job_available_frames == 0 {
                continue;
            }
            let job_frames = remaining.min(job_available_frames);
            let job_metrics = if options.transfer_syntax == TransferSyntax::JpegBaseline8Bit {
                coverage_jpeg_baseline_routes(
                    &slide,
                    options.clone(),
                    job.level,
                    location,
                    job_frames,
                    level_deadline,
                )?
            } else {
                profile_lossless_j2k_routes(
                    &slide,
                    &source_path,
                    options.clone(),
                    job.level,
                    location,
                    job_frames,
                    level_deadline,
                )?
            };
            remaining = remaining.saturating_sub(job_metrics.routes.total_frames);
            level_metrics.add_assign(job_metrics);
        }
        if matches!(request.progress, Some(RouteProgressSink::Stderr)) {
            eprintln!(
                "coverage level {}/{} ok {} level={} frames={} route_passthrough={} route_gpu_transcode={} route_cpu_fallback={} elapsed_ms={:.3}",
                level_ordinal + 1,
                level_count,
                source_path.display(),
                level_idx,
                level_metrics.routes.total_frames,
                level_metrics.route_passthrough_frames(),
                level_metrics.routes.gpu_transcode_frames,
                level_metrics.routes.cpu_fallback_frames,
                duration_as_reported_micros(level_started.elapsed()) as f64 / 1000.0
            );
        }
        metrics.add_assign(level_metrics);
        available_frames = available_frames.saturating_add(level_available_frames);
        levels.push(RouteProfileReport {
            source_path: source_path.clone(),
            transfer_syntax_uid,
            level: level_idx,
            requested_frames: request.max_frames_per_level,
            available_frames: level_available_frames,
            metrics: level_metrics,
            elapsed_micros: duration_as_reported_micros(level_started.elapsed()),
        });
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    flush_persistent_auto_metal_input_route_cache_if_requested()?;

    Ok(RouteCoverageReport {
        source_path,
        transfer_syntax_uid,
        requested_frames_per_level: request.max_frames_per_level,
        available_frames,
        complete_frame_coverage: metrics.routes.total_frames >= available_frames,
        levels,
        metrics,
        elapsed_micros: duration_as_reported_micros(started.elapsed()),
    })
}

/// Profile route coverage for every WSI-like file under a source root.
pub fn profile_dicom_route_corpus_coverage(
    request: RouteCoverageRequest,
) -> Result<RouteCorpusCoverageReport, Error> {
    if request.max_frames_per_level == 0 {
        return Err(Error::Unsupported {
            reason: "corpus route coverage profiling requires max_frames_per_level > 0".into(),
        });
    }
    if request.max_levels == Some(0) {
        return Err(Error::Unsupported {
            reason: "corpus route coverage profiling requires max_levels > 0 when provided".into(),
        });
    }
    validate_max_level_elapsed(request.max_level_elapsed, "corpus route coverage profiling")?;
    let started = Instant::now();
    let source_root = match &request.target {
        RouteCoverageTarget::Corpus(source_root) => source_root.clone(),
        RouteCoverageTarget::Source(_) => {
            return Err(Error::Unsupported {
                reason: "corpus route coverage profiling requires a corpus target".into(),
            });
        }
    };
    request.options.validate()?;
    if !request.source_aware_transfer_syntax
        && request.options.transfer_syntax != TransferSyntax::JpegBaseline8Bit
        && !request.options.transfer_syntax.is_j2k_family()
    {
        return Err(Error::Unsupported {
            reason: "corpus route coverage profiling currently supports JPEG Baseline, JPEG 2000, and HTJ2K transfer syntaxes"
                .into(),
        });
    }
    let sources =
        collect_wsi_candidate_paths(&source_root, request.max_sources, request.max_depth)?;
    let mut reports = Vec::new();
    let mut failures = Vec::new();
    let mut metrics = ExportMetrics::default();
    let mut available_frames = 0u64;

    for (source_idx, source_path) in sources.iter().enumerate() {
        let source_started = Instant::now();
        if matches!(request.progress, Some(RouteProgressSink::Stderr)) {
            eprintln!(
                "coverage-corpus source {}/{} start {}",
                source_idx + 1,
                sources.len(),
                source_path.display()
            );
        }
        match profile_dicom_route_coverage(RouteCoverageRequest {
            target: RouteCoverageTarget::Source(source_path.clone()),
            options: request.options.clone(),
            source_aware_transfer_syntax: request.source_aware_transfer_syntax,
            max_frames_per_level: request.max_frames_per_level,
            max_levels: request.max_levels,
            max_level_elapsed: request.max_level_elapsed,
            progress: request.progress,
            max_sources: request.max_sources,
            max_depth: request.max_depth,
        }) {
            Ok(report) => {
                metrics.add_assign(report.metrics);
                available_frames = available_frames.saturating_add(report.available_frames);
                if matches!(request.progress, Some(RouteProgressSink::Stderr)) {
                    eprintln!(
                        "coverage-corpus source {}/{} ok {} levels={} frames={} route_passthrough={} route_gpu_transcode={} route_cpu_fallback={} elapsed_ms={:.3}",
                        source_idx + 1,
                        sources.len(),
                        source_path.display(),
                        report.levels.len(),
                        report.metrics.routes.total_frames,
                        report.metrics.route_passthrough_frames(),
                        report.metrics.routes.gpu_transcode_frames,
                        report.metrics.routes.cpu_fallback_frames,
                        duration_as_reported_micros(source_started.elapsed()) as f64 / 1000.0
                    );
                }
                reports.push(report);
            }
            Err(err) => {
                if matches!(request.progress, Some(RouteProgressSink::Stderr)) {
                    eprintln!(
                        "coverage-corpus source {}/{} failed {} error={} elapsed_ms={:.3}",
                        source_idx + 1,
                        sources.len(),
                        source_path.display(),
                        err,
                        duration_as_reported_micros(source_started.elapsed()) as f64 / 1000.0
                    );
                }
                failures.push(RouteCorpusCoverageFailure {
                    source_path: source_path.clone(),
                    message: err.to_string(),
                });
            }
        }
    }
    let transfer_syntax_uids = corpus_transfer_syntax_uids(&reports);

    Ok(RouteCorpusCoverageReport {
        source_root,
        transfer_syntax_uid: common_corpus_transfer_syntax_uid(&transfer_syntax_uids),
        transfer_syntax_uids,
        requested_frames_per_level: request.max_frames_per_level,
        max_levels: request.max_levels,
        sources_considered: sources.len(),
        available_frames,
        complete_frame_coverage: failures.is_empty()
            && reports.iter().all(|report| report.complete_frame_coverage),
        reports,
        failures,
        metrics,
        elapsed_micros: duration_as_reported_micros(started.elapsed()),
    })
}

fn corpus_transfer_syntax_uids(reports: &[RouteCoverageReport]) -> Vec<&'static str> {
    let mut transfer_syntax_uids = reports
        .iter()
        .map(|report| report.transfer_syntax_uid)
        .collect::<Vec<_>>();
    transfer_syntax_uids.sort_unstable();
    transfer_syntax_uids.dedup();
    transfer_syntax_uids
}

fn common_corpus_transfer_syntax_uid(
    transfer_syntax_uids: &[&'static str],
) -> Option<&'static str> {
    let first = transfer_syntax_uids.first().copied()?;
    transfer_syntax_uids
        .iter()
        .all(|uid| *uid == first)
        .then_some(first)
}

fn collect_wsi_candidate_paths(
    root: &Path,
    max_sources: usize,
    max_depth: usize,
) -> Result<Vec<PathBuf>, Error> {
    let root_metadata = fs::symlink_metadata(root).map_err(|source| Error::Io {
        path: root.to_path_buf(),
        source,
    })?;
    if root_metadata.file_type().is_symlink() {
        return Err(Error::Unsupported {
            reason: format!("corpus coverage refuses symlink root {}", root.display()),
        });
    }
    if root_metadata.is_file() {
        return Ok(if is_wsi_candidate_path(root) {
            vec![root.to_path_buf()]
        } else {
            Vec::new()
        });
    }
    if !root_metadata.is_dir() {
        return Err(Error::Unsupported {
            reason: format!(
                "corpus coverage root is not a file or directory: {}",
                root.display()
            ),
        });
    }

    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut candidates = Vec::new();
    while let Some((dir, depth)) = pending.pop() {
        if depth > max_depth {
            return Err(Error::Unsupported {
                reason: format!(
                    "corpus coverage directory depth exceeds max_depth={} at {}",
                    max_depth,
                    dir.display()
                ),
            });
        }
        let entries = fs::read_dir(&dir).map_err(|source| Error::Io {
            path: dir.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| Error::Io {
                path: dir.clone(),
                source,
            })?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            if file_type.is_symlink() {
                return Err(Error::Unsupported {
                    reason: format!(
                        "corpus coverage refuses symlink traversal at {}",
                        path.display()
                    ),
                });
            } else if file_type.is_dir() {
                pending.push((path, depth + 1));
            } else if file_type.is_file() && is_wsi_candidate_path(&path) {
                candidates.push(path);
                if candidates.len() > max_sources {
                    return Err(Error::Unsupported {
                        reason: format!(
                            "corpus coverage found more than max_sources={} candidate files",
                            max_sources
                        ),
                    });
                }
            }
        }
    }
    candidates.sort();
    Ok(candidates)
}

fn is_wsi_candidate_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("svs" | "tif" | "tiff" | "ndpi" | "scn" | "dcm" | "mrxs" | "vms" | "vmu")
    )
}

struct GeneratedJpegDirectHtj2kOutcome {
    direct: Result<jpeg_direct_htj2k::BatchOutcome, Error>,
    input_decode_duration: Duration,
    compose_duration: Duration,
    jpeg_encode_duration: Duration,
}

fn jpeg_direct_htj2k_result_is_ok(
    direct_results: &[Option<Result<jpeg_direct_htj2k::BatchOutcome, Error>>],
    generated_results: &[Option<GeneratedJpegDirectHtj2kOutcome>],
    idx: usize,
) -> bool {
    direct_results[idx].as_ref().is_some_and(Result::is_ok)
        || generated_results[idx]
            .as_ref()
            .is_some_and(|outcome| outcome.direct.is_ok())
}

#[allow(clippy::too_many_arguments)]
fn try_write_existing_lossless_j2k_frame(
    idx: usize,
    planned_frame: &LosslessJ2kPlannedFrame,
    direct_j2k_results: &mut [Option<Result<j2k_direct_htj2k::BatchOutcome, Error>>],
    generated_jpeg_direct_results: &mut [Option<GeneratedJpegDirectHtj2kOutcome>],
    direct_jpeg_results: &mut [Option<Result<jpeg_direct_htj2k::BatchOutcome, Error>>],
    options: &ExportOptions,
    metrics: &mut ExportMetrics,
    pixel_profile: &mut Option<PixelProfile>,
    pixel_data: &mut impl PixelDataSink,
    j2k_passthrough_lossy: &mut bool,
) -> Result<bool, Error> {
    if let Some(passthrough) = planned_frame.passthrough.as_ref() {
        let profile = passthrough.profile;
        ensure_consistent_pixel_profile(
            pixel_profile,
            profile,
            "pixel profile changed across frames",
        )?;
        *j2k_passthrough_lossy |= passthrough.is_lossy();
        write_existing_lossless_j2k_codestream(pixel_data, metrics, &passthrough.codestream)?;
        metrics.record_j2k_passthrough_frame();
        metrics.record_pixel_profile(profile);
        return Ok(true);
    }

    if let Some(Ok(direct)) = direct_j2k_results[idx].take() {
        j2k_direct_htj2k::record_success(
            metrics,
            pixel_profile,
            &direct,
            "pixel profile changed across frames",
        )?;
        write_existing_lossless_j2k_codestream(pixel_data, metrics, &direct.codestream)?;
        return Ok(true);
    }

    if let Some(generated) = generated_jpeg_direct_results[idx].take() {
        metrics.record_jpeg_decode_fallback();
        metrics.record_input_decode_duration(generated.input_decode_duration);
        metrics.record_compose_duration(generated.compose_duration);
        metrics.record_jpeg_cpu_encode(generated.jpeg_encode_duration);
        match generated.direct {
            Ok(direct) => {
                write_jpeg_direct_htj2k_frame(
                    planned_frame,
                    options,
                    metrics,
                    pixel_profile,
                    pixel_data,
                    &direct,
                )?;
                return Ok(true);
            }
            Err(_) => metrics.record_jpeg_direct_htj2k_rejected_frame(),
        }
    }

    if let Some(direct_result) = direct_jpeg_results[idx].take() {
        match direct_result {
            Ok(direct) => {
                write_jpeg_direct_htj2k_frame(
                    planned_frame,
                    options,
                    metrics,
                    pixel_profile,
                    pixel_data,
                    &direct,
                )?;
                return Ok(true);
            }
            Err(_) => metrics.record_jpeg_direct_htj2k_rejected_frame(),
        }
    } else if planned_frame.source_jpeg_direct_rejected {
        metrics.record_jpeg_direct_htj2k_rejected_frame();
    }

    if let Some(reason) = planned_frame.source_jpeg_retile_rejection {
        metrics.record_jpeg_retile_rejected_frame(reason);
    }

    Ok(false)
}

fn write_existing_lossless_j2k_codestream(
    pixel_data: &mut impl PixelDataSink,
    metrics: &mut ExportMetrics,
    codestream: &[u8],
) -> Result<(), Error> {
    let byte_started = Instant::now();
    pixel_data.push_frame(codestream)?;
    metrics.record_write_duration(byte_started.elapsed());
    Ok(())
}

fn write_jpeg_direct_htj2k_frame(
    planned_frame: &LosslessJ2kPlannedFrame,
    options: &ExportOptions,
    metrics: &mut ExportMetrics,
    pixel_profile: &mut Option<PixelProfile>,
    pixel_data: &mut impl PixelDataSink,
    direct: &jpeg_direct_htj2k::BatchOutcome,
) -> Result<(), Error> {
    jpeg_direct_htj2k::record_route_success(
        metrics,
        pixel_profile,
        direct,
        options.jpeg_direct_htj2k_profile,
        planned_frame.source_jpeg_retiled,
        planned_frame.source_jpeg_retile_duration,
        "pixel profile changed across frames",
    )?;
    write_existing_lossless_j2k_codestream(pixel_data, metrics, &direct.codestream)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn generated_jpeg_direct_htj2k_allowed_for_route(
    transfer_syntax: TransferSyntax,
    _metal_input: &MetalInputTileReader,
) -> bool {
    jpeg_direct_htj2k::transfer_syntax(transfer_syntax)
}

fn j2k_direct_htj2k_result_is_ok(
    direct_results: &[Option<Result<j2k_direct_htj2k::BatchOutcome, Error>>],
    idx: usize,
) -> bool {
    direct_results[idx].as_ref().is_some_and(Result::is_ok)
}

fn generated_jpeg_direct_htj2k_indices(
    planned: &[LosslessJ2kPlannedFrame],
    transfer_syntax: TransferSyntax,
    mut direct_jpeg_succeeded: impl FnMut(usize) -> bool,
) -> Vec<usize> {
    let row_has_jpeg_source = planned.iter().any(|planned_frame| {
        planned_frame.source_jpeg.is_some() || planned_frame.source_jpeg_direct_rejected
    });
    planned
        .iter()
        .enumerate()
        .filter_map(|(idx, planned_frame)| {
            jpeg_direct_htj2k::generated_candidate(
                transfer_syntax,
                row_has_jpeg_source,
                direct_jpeg_succeeded(idx),
                planned_frame.source_jpeg_direct_rejected,
                planned_frame.source_raw_probe_failed,
                planned_frame.passthrough.is_some(),
            )
            .then_some(idx)
        })
        .collect()
}

fn profile_lossless_j2k_routes(
    slide: &Slide,
    _source_path: &Path,
    options: ExportOptions,
    level: &wsi_rs::Level,
    location: JpegBaselineFrameLocation,
    max_frames: u64,
    deadline: Option<RouteLevelDeadline>,
) -> Result<ExportMetrics, Error> {
    let level_idx = location.level_idx;
    let tile_size = j2k_route_tile_size(&options, level)?;
    let (matrix_columns, matrix_rows) = level.dimensions;
    let grid = TileGrid::square(matrix_columns, matrix_rows, tile_size)?;
    let tiles_down = grid.tiles_down;
    let route_scope_frames = grid.frame_count_u64()?.min(max_frames);
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let route_scope_frames_usize =
        usize::try_from(route_scope_frames).map_err(|_| Error::Unsupported {
            reason: "route profile frame count exceeds platform addressable memory".into(),
        })?;
    let effective_backend = effective_lossless_j2k_encode_backend(&options, route_scope_frames);
    let mut j2k_encoder = DicomJ2kEncoder::new(
        effective_backend,
        j2k_encode_transfer_syntax(options.transfer_syntax),
        options.codec_validation,
    )
    .with_j2k_decomposition_levels(options.j2k_decomposition_levels)
    .with_gpu_encode_tuning(
        options.gpu_encode_inflight_tiles,
        hybrid_lane::effective_lossless_gpu_encode_memory_mib(&options, route_scope_frames),
    );
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let metal_input_backend =
        lossless_j2k_metal_input_preference(effective_backend, options.source_device_decode);
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let mut metal_input = MetalInputTileReader::new_for_lossless_j2k(
        metal_input_backend,
        lossless_j2k_auto_allows_metal_input(
            metal_input_backend,
            options.transfer_syntax,
            max_frames,
            options.source_device_decode,
        ),
        auto_metal_input_route_cache_key(
            _source_path,
            options.clone(),
            location,
            route_scope_frames,
        ),
        options.source_device_decode,
    )
    .with_row_batch_tuning(
        options.gpu_row_batch_rows,
        hybrid_lane::effective_lossless_gpu_row_batch_target_tiles(&options, route_scope_frames),
    )
    .with_pipeline_depth(effective_gpu_pipeline_depth(&options));
    #[cfg(all(feature = "metal", target_os = "macos"))]
    if lossless_j2k_auto_should_start_cpu_only(
        effective_backend,
        options.transfer_syntax,
        route_scope_frames,
        options.source_device_decode,
    ) || metal_input.auto_route_decision() == AutoLosslessJ2kRouteDecision::CpuOnly
    {
        j2k_encoder.force_cpu_only_for_auto();
    }
    let mut metrics = ExportMetrics::default();
    #[cfg(all(feature = "metal", target_os = "macos"))]
    if metal_input.enabled() {
        metrics.record_gpu_pipeline_depth(effective_gpu_pipeline_depth(&options));
    }
    let mut pixel_profile = None;
    let mut remaining = max_frames;
    let allow_passthrough_probe =
        j2k_family_passthrough_probe_allowed(_source_path, options.transfer_syntax);
    let mut jpeg_direct_encoder =
        jpeg_direct_htj2k_supported_for_backend(options.transfer_syntax, effective_backend)
            .then(|| {
                jpeg_direct_htj2k::BatchEncoder::new(
                    options.transfer_syntax,
                    options.jpeg_direct_htj2k_profile,
                    effective_backend,
                )
            })
            .transpose()?;

    for row in 0..tiles_down {
        if remaining == 0 {
            break;
        }
        check_route_level_deadline(deadline, level_idx)?;
        let row_tile_count = grid.row_tile_count(row)?.min(remaining);
        let planned = plan_lossless_j2k_row(
            slide,
            location.scene_idx,
            location.series_idx,
            location.level_idx,
            location.z,
            location.c,
            location.t,
            row,
            0,
            row_tile_count,
            matrix_columns,
            matrix_rows,
            tile_size,
            options.transfer_syntax,
            allow_passthrough_probe,
        )?;
        let mut direct_jpeg_results =
            if let Some(jpeg_direct_encoder) = jpeg_direct_encoder.as_mut() {
                jpeg_direct_htj2k::encode_planned_batch_with_encoder(&planned, jpeg_direct_encoder)?
            } else {
                (0..planned.len()).map(|_| None).collect()
            };
        let mut direct_j2k_results = j2k_direct_htj2k::encode_planned_batch(
            &planned,
            options.transfer_syntax,
            options.codec_validation,
        )?;
        let mut generated_jpeg_direct_results: Vec<Option<GeneratedJpegDirectHtj2kOutcome>> =
            (0..planned.len()).map(|_| None).collect();
        #[cfg(all(feature = "metal", target_os = "macos"))]
        let generated_jpeg_direct_allowed = jpeg_direct_encoder.is_some()
            && generated_jpeg_direct_htj2k_allowed_for_route(options.transfer_syntax, &metal_input);
        #[cfg(not(all(feature = "metal", target_os = "macos")))]
        let generated_jpeg_direct_allowed = jpeg_direct_encoder.is_some();
        if generated_jpeg_direct_allowed {
            let generated_indices =
                generated_jpeg_direct_htj2k_indices(&planned, options.transfer_syntax, |idx| {
                    direct_jpeg_results[idx].as_ref().is_some_and(Result::is_ok)
                });
            scatter_indexed_results(
                &mut generated_jpeg_direct_results,
                encode_generated_jpeg_direct_htj2k_planned_batch(
                    slide,
                    jpeg_direct_encoder.as_mut().ok_or_else(|| Error::Encode {
                        message: "generated JPEG direct route missing HTJ2K encoder".into(),
                    })?,
                    location,
                    &planned,
                    &generated_indices,
                    tile_size,
                    options.jpeg_quality,
                    options.max_prepared_frame_bytes,
                )?,
            )?;
        }
        #[cfg(all(feature = "metal", target_os = "macos"))]
        {
            let mut routed_tiles: Vec<Option<RoutedLosslessJ2kTile>> =
                (0..planned.len()).map(|_| None).collect();
            let mut run_start = 0usize;
            while run_start < planned.len() {
                if planned[run_start].passthrough.is_some()
                    || jpeg_direct_htj2k_result_is_ok(
                        &direct_jpeg_results,
                        &generated_jpeg_direct_results,
                        run_start,
                    )
                    || j2k_direct_htj2k_result_is_ok(&direct_j2k_results, run_start)
                {
                    run_start += 1;
                    continue;
                }
                let mut run_end = run_start + 1;
                while run_end < planned.len()
                    && planned[run_end].passthrough.is_none()
                    && !jpeg_direct_htj2k_result_is_ok(
                        &direct_jpeg_results,
                        &generated_jpeg_direct_results,
                        run_end,
                    )
                    && !j2k_direct_htj2k_result_is_ok(&direct_j2k_results, run_end)
                {
                    run_end += 1;
                }
                if options.transfer_syntax.is_jpeg2000_passthrough_only() {
                    run_start = run_end;
                    continue;
                }
                if metal_input.auto_input_probe_pending() {
                    let probe_end =
                        (run_start + LOSSLESS_J2K_AUTO_ROUTE_PROBE_MAX_FRAMES).min(run_end);
                    let probe_run = probe_auto_metal_input_tile_run(
                        slide,
                        &mut metal_input,
                        &mut j2k_encoder,
                        level,
                        location.scene_idx,
                        location.series_idx,
                        location.level_idx,
                        location.z,
                        location.c,
                        location.t,
                        row,
                        &planned[run_start..probe_end],
                        route_scope_frames_usize,
                        matrix_columns,
                        matrix_rows,
                        tile_size,
                    )?;
                    let selected_gpu_input =
                        probe_run.route == AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode;
                    if selected_gpu_input {
                        metrics.record_gpu_input_decode_duration(probe_run.input_decode_duration);
                        metrics.record_gpu_compose_duration(probe_run.compose_duration);
                    } else {
                        metrics.record_input_decode_duration(probe_run.input_decode_duration);
                        metrics.record_compose_duration(probe_run.compose_duration);
                    }
                    metrics.record_gpu_batches(
                        probe_run.gpu_input_decode_batches,
                        probe_run.gpu_compose_batches,
                        probe_run.gpu_encode_batches,
                    );
                    metrics.record_gpu_encode_batch_stats(probe_run.gpu_encode_stats);
                    metrics.record_auto_route_probe(
                        u64::try_from(probe_end - run_start).map_err(|_| Error::Unsupported {
                            reason: "auto route probe frame count exceeds u64".into(),
                        })?,
                        probe_run.probe_cpu_duration,
                        probe_run.probe_gpu_duration,
                        probe_run.probe_gpu_batches,
                        selected_gpu_input,
                    );
                    for (slot, encoded) in routed_tiles[run_start..probe_end]
                        .iter_mut()
                        .zip(probe_run.tiles.into_iter())
                    {
                        *slot = encoded;
                    }
                    run_start = probe_end;
                    continue;
                }
                if metal_input.enabled() {
                    let metal_run = try_encode_metal_input_tile_run(
                        slide,
                        &mut metal_input,
                        &mut j2k_encoder,
                        level,
                        location.scene_idx,
                        location.series_idx,
                        location.level_idx,
                        location.z,
                        location.c,
                        location.t,
                        row,
                        planned[run_start].col,
                        (run_end - run_start) as u64,
                        matrix_columns,
                        matrix_rows,
                        tile_size,
                    )?;
                    metrics.record_gpu_input_decode_duration(metal_run.input_decode_duration);
                    metrics.record_gpu_compose_duration(metal_run.compose_duration);
                    metrics.record_gpu_batches(
                        metal_run.input_decode_batches,
                        metal_run.compose_batches,
                        metal_run.encode_batches,
                    );
                    metrics.record_gpu_encode_batch_stats(metal_run.gpu_encode_stats);
                    metrics.record_gpu_row_batch_config(
                        metal_run.row_batch_rows,
                        metal_run.row_batch_target_tiles,
                    );
                    for (slot, encoded) in routed_tiles[run_start..run_end]
                        .iter_mut()
                        .zip(metal_run.tiles.into_iter())
                    {
                        *slot = encoded.map(|(encoded, profile)| RoutedLosslessJ2kTile {
                            encoded: Ok(encoded),
                            profile,
                            used_gpu_input: true,
                        });
                    }
                }
                run_start = run_end;
            }
            let mut cpu_batch_results: Vec<Option<LosslessJ2kCpuBatchOutcome>> =
                (0..planned.len()).map(|_| None).collect();
            if let Some((
                transfer_syntax,
                codec_validation,
                j2k_decomposition_levels,
                reversible_transform,
            )) = (options.transfer_syntax != TransferSyntax::Jpeg2000)
                .then(|| j2k_encoder.cpu_batch_settings())
                .flatten()
            {
                let cpu_indices = lossless_j2k_cpu_fallback_indices(
                    &planned,
                    options.transfer_syntax,
                    tile_size,
                    |idx| {
                        routed_tiles[idx].is_some()
                            || jpeg_direct_htj2k_result_is_ok(
                                &direct_jpeg_results,
                                &generated_jpeg_direct_results,
                                idx,
                            )
                            || j2k_direct_htj2k_result_is_ok(&direct_j2k_results, idx)
                    },
                );
                scatter_indexed_results(
                    &mut cpu_batch_results,
                    encode_cpu_input_lossless_j2k_planned_batch(
                        slide,
                        level,
                        LosslessJ2kCpuBatchSettings {
                            transfer_syntax,
                            codec_validation,
                            j2k_decomposition_levels,
                            reversible_transform,
                            max_prepared_frame_bytes: options.max_prepared_frame_bytes,
                        },
                        location.scene_idx,
                        location.series_idx,
                        location.level_idx,
                        location.z,
                        location.c,
                        location.t,
                        &planned,
                        &cpu_indices,
                        tile_size,
                    )?,
                )?;
            }
            for (idx, planned_frame) in planned.into_iter().enumerate() {
                let encode_allowed = j2k_non_passthrough_encode_allowed(
                    &planned_frame,
                    options.transfer_syntax,
                    tile_size,
                );
                if let Some(passthrough) = planned_frame.passthrough.as_ref() {
                    let profile = passthrough.profile;
                    ensure_consistent_pixel_profile(
                        &mut pixel_profile,
                        profile,
                        "pixel profile changed across profiled frames",
                    )?;
                    metrics.record_j2k_passthrough_frame();
                    metrics.record_pixel_profile(profile);
                    remaining = remaining.saturating_sub(1);
                    continue;
                }
                if let Some(Ok(direct)) = direct_j2k_results[idx].take() {
                    j2k_direct_htj2k::record_success(
                        &mut metrics,
                        &mut pixel_profile,
                        &direct,
                        "pixel profile changed across profiled frames",
                    )?;
                    remaining = remaining.saturating_sub(1);
                    continue;
                }
                if let Some(generated) = generated_jpeg_direct_results[idx].take() {
                    metrics.record_jpeg_decode_fallback();
                    metrics.record_input_decode_duration(generated.input_decode_duration);
                    metrics.record_compose_duration(generated.compose_duration);
                    metrics.record_jpeg_cpu_encode(generated.jpeg_encode_duration);
                    match generated.direct {
                        Ok(direct) => {
                            jpeg_direct_htj2k::record_route_success(
                                &mut metrics,
                                &mut pixel_profile,
                                &direct,
                                options.jpeg_direct_htj2k_profile,
                                planned_frame.source_jpeg_retiled,
                                planned_frame.source_jpeg_retile_duration,
                                "pixel profile changed across profiled frames",
                            )?;
                            remaining = remaining.saturating_sub(1);
                            continue;
                        }
                        Err(_) => metrics.record_jpeg_direct_htj2k_rejected_frame(),
                    }
                }
                if let Some(direct_result) = direct_jpeg_results[idx].take() {
                    match direct_result {
                        Ok(direct) => {
                            jpeg_direct_htj2k::record_route_success(
                                &mut metrics,
                                &mut pixel_profile,
                                &direct,
                                options.jpeg_direct_htj2k_profile,
                                planned_frame.source_jpeg_retiled,
                                planned_frame.source_jpeg_retile_duration,
                                "pixel profile changed across profiled frames",
                            )?;
                            remaining = remaining.saturating_sub(1);
                            continue;
                        }
                        Err(_) => metrics.record_jpeg_direct_htj2k_rejected_frame(),
                    }
                } else if planned_frame.source_jpeg_direct_rejected {
                    metrics.record_jpeg_direct_htj2k_rejected_frame();
                }
                if let Some(reason) = planned_frame.source_jpeg_retile_rejection {
                    metrics.record_jpeg_retile_rejected_frame(reason);
                }
                if !encode_allowed {
                    metrics.record_j2k_passthrough_only_fallback_classification();
                    remaining = remaining.saturating_sub(1);
                    continue;
                }
                reject_lossy_j2k_lossless_fallback(&planned_frame, options.transfer_syntax, row)?;

                let routed_encoded = routed_tiles[idx].take();
                let (encoded, profile, used_gpu_input, input_decode_duration, compose_duration) =
                    match routed_encoded {
                        Some(routed) => (
                            routed.encoded,
                            routed.profile,
                            routed.used_gpu_input,
                            Duration::ZERO,
                            Duration::ZERO,
                        ),
                        None if cpu_batch_results[idx].is_some() => {
                            let outcome =
                                cpu_batch_results[idx].take().ok_or_else(|| Error::Encode {
                                    message:
                                        "CPU JPEG 2000 batch result missing for fallback frame"
                                            .into(),
                                })?;
                            (
                                outcome.encoded,
                                outcome.profile,
                                false,
                                outcome.input_decode_duration,
                                outcome.compose_duration,
                            )
                        }
                        None => {
                            j2k_encoder.set_reversible_transform(
                                j2k_fallback_reversible_transform(
                                    &planned_frame,
                                    options.transfer_syntax,
                                ),
                            );
                            let (encoded, profile, input_decode_duration, compose_duration) =
                                encode_cpu_input_tile(
                                    slide,
                                    &mut j2k_encoder,
                                    location,
                                    planned_frame.x,
                                    planned_frame.y,
                                    planned_frame.width,
                                    planned_frame.height,
                                    tile_size,
                                )?;
                            (
                                encoded,
                                profile,
                                false,
                                input_decode_duration,
                                compose_duration,
                            )
                        }
                    };
                let profile =
                    j2k_fallback_profile(&planned_frame, profile, options.transfer_syntax);
                if used_gpu_input {
                    metrics.record_gpu_input();
                } else {
                    metrics.record_cpu_input();
                    metrics.record_input_decode_duration(input_decode_duration);
                    metrics.record_compose_duration(compose_duration);
                }
                metrics.record_pixel_profile(profile);
                ensure_consistent_pixel_profile(
                    &mut pixel_profile,
                    profile,
                    "pixel profile changed across profiled frames",
                )?;
                let encoded = encoded?;
                metrics.record_encoded_frame(&encoded);
                metrics.record_transcode_route(used_gpu_input, encoded.used_device_encode);
                let _ = encoded.codestream_bytes()?;
                remaining = remaining.saturating_sub(1);
            }
        }
        #[cfg(not(all(feature = "metal", target_os = "macos")))]
        {
            let mut cpu_batch_results: Vec<Option<LosslessJ2kCpuBatchOutcome>> =
                (0..planned.len()).map(|_| None).collect();
            if let Some((
                transfer_syntax,
                codec_validation,
                j2k_decomposition_levels,
                reversible_transform,
            )) = (options.transfer_syntax != TransferSyntax::Jpeg2000)
                .then(|| j2k_encoder.cpu_batch_settings())
                .flatten()
            {
                let cpu_indices = lossless_j2k_cpu_fallback_indices(
                    &planned,
                    options.transfer_syntax,
                    tile_size,
                    |idx| {
                        jpeg_direct_htj2k_result_is_ok(
                            &direct_jpeg_results,
                            &generated_jpeg_direct_results,
                            idx,
                        ) || j2k_direct_htj2k_result_is_ok(&direct_j2k_results, idx)
                    },
                );
                scatter_indexed_results(
                    &mut cpu_batch_results,
                    encode_cpu_input_lossless_j2k_planned_batch(
                        slide,
                        level,
                        LosslessJ2kCpuBatchSettings {
                            transfer_syntax,
                            codec_validation,
                            j2k_decomposition_levels,
                            reversible_transform,
                            max_prepared_frame_bytes: options.max_prepared_frame_bytes,
                        },
                        location.scene_idx,
                        location.series_idx,
                        location.level_idx,
                        location.z,
                        location.c,
                        location.t,
                        &planned,
                        &cpu_indices,
                        tile_size,
                    )?,
                )?;
            }
            for (idx, planned_frame) in planned.into_iter().enumerate() {
                let encode_allowed = j2k_non_passthrough_encode_allowed(
                    &planned_frame,
                    options.transfer_syntax,
                    tile_size,
                );
                if let Some(passthrough) = planned_frame.passthrough.as_ref() {
                    let profile = passthrough.profile;
                    ensure_consistent_pixel_profile(
                        &mut pixel_profile,
                        profile,
                        "pixel profile changed across profiled frames",
                    )?;
                    metrics.record_j2k_passthrough_frame();
                    metrics.record_pixel_profile(profile);
                    remaining = remaining.saturating_sub(1);
                    continue;
                }
                if let Some(Ok(direct)) = direct_j2k_results[idx].take() {
                    j2k_direct_htj2k::record_success(
                        &mut metrics,
                        &mut pixel_profile,
                        &direct,
                        "pixel profile changed across profiled frames",
                    )?;
                    remaining = remaining.saturating_sub(1);
                    continue;
                }
                if let Some(generated) = generated_jpeg_direct_results[idx].take() {
                    metrics.record_jpeg_decode_fallback();
                    metrics.record_input_decode_duration(generated.input_decode_duration);
                    metrics.record_compose_duration(generated.compose_duration);
                    metrics.record_jpeg_cpu_encode(generated.jpeg_encode_duration);
                    match generated.direct {
                        Ok(direct) => {
                            jpeg_direct_htj2k::record_route_success(
                                &mut metrics,
                                &mut pixel_profile,
                                &direct,
                                options.jpeg_direct_htj2k_profile,
                                planned_frame.source_jpeg_retiled,
                                planned_frame.source_jpeg_retile_duration,
                                "pixel profile changed across profiled frames",
                            )?;
                            remaining = remaining.saturating_sub(1);
                            continue;
                        }
                        Err(_) => metrics.record_jpeg_direct_htj2k_rejected_frame(),
                    }
                }
                if let Some(direct_result) = direct_jpeg_results[idx].take() {
                    match direct_result {
                        Ok(direct) => {
                            jpeg_direct_htj2k::record_route_success(
                                &mut metrics,
                                &mut pixel_profile,
                                &direct,
                                options.jpeg_direct_htj2k_profile,
                                planned_frame.source_jpeg_retiled,
                                planned_frame.source_jpeg_retile_duration,
                                "pixel profile changed across profiled frames",
                            )?;
                            remaining = remaining.saturating_sub(1);
                            continue;
                        }
                        Err(_) => metrics.record_jpeg_direct_htj2k_rejected_frame(),
                    }
                } else if planned_frame.source_jpeg_direct_rejected {
                    metrics.record_jpeg_direct_htj2k_rejected_frame();
                }
                if let Some(reason) = planned_frame.source_jpeg_retile_rejection {
                    metrics.record_jpeg_retile_rejected_frame(reason);
                }
                if !encode_allowed {
                    metrics.record_j2k_passthrough_only_fallback_classification();
                    remaining = remaining.saturating_sub(1);
                    continue;
                }
                reject_lossy_j2k_lossless_fallback(&planned_frame, options.transfer_syntax, row)?;

                let (encoded, profile, input_decode_duration, compose_duration) =
                    if let Some(outcome) = cpu_batch_results[idx].take() {
                        (
                            outcome.encoded,
                            outcome.profile,
                            outcome.input_decode_duration,
                            outcome.compose_duration,
                        )
                    } else {
                        j2k_encoder.set_reversible_transform(j2k_fallback_reversible_transform(
                            &planned_frame,
                            options.transfer_syntax,
                        ));
                        encode_cpu_input_tile(
                            slide,
                            &mut j2k_encoder,
                            location,
                            planned_frame.x,
                            planned_frame.y,
                            planned_frame.width,
                            planned_frame.height,
                            tile_size,
                        )?
                    };
                let profile =
                    j2k_fallback_profile(&planned_frame, profile, options.transfer_syntax);
                metrics.record_input_decode_duration(input_decode_duration);
                metrics.record_compose_duration(compose_duration);
                metrics.record_cpu_input();
                metrics.record_pixel_profile(profile);
                ensure_consistent_pixel_profile(
                    &mut pixel_profile,
                    profile,
                    "pixel profile changed across profiled frames",
                )?;
                let encoded = encoded?;
                metrics.record_encoded_frame(&encoded);
                metrics.record_transcode_route(false, encoded.used_device_encode);
                let _ = encoded.codestream_bytes()?;
                remaining = remaining.saturating_sub(1);
            }
        }
    }

    Ok(metrics)
}

fn profile_jpeg_baseline_routes(
    slide: &Slide,
    options: ExportOptions,
    level: &wsi_rs::Level,
    location: JpegBaselineFrameLocation,
    max_frames: u64,
) -> Result<ExportMetrics, Error> {
    let geometry = jpeg_baseline_route_frame_geometry(slide, level, location, options.tile_size)?;
    let (matrix_columns, matrix_rows) = level.dimensions;
    let (tiles_across, tiles_down) = (geometry.tiles_across, geometry.tiles_down);
    let (frame_columns, frame_rows) = (geometry.frame_columns, geometry.frame_rows);
    let allow_raw_rgb_passthrough = raw_rgb_passthrough_has_no_geometry_fallback(level, geometry);
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let mut metal_input =
        MetalInputTileReader::new(options.encode_backend, options.source_device_decode);
    let mut metrics = ExportMetrics::default();
    let mut pixel_profile = None;
    let mut remaining = max_frames;
    let mut blank_jpeg_cache = None;

    for row in 0..tiles_down {
        if remaining == 0 {
            break;
        }
        let row_tile_count = tiles_across.min(remaining);
        let row_plan = plan_jpeg_baseline_row(
            slide,
            location,
            row,
            row_tile_count,
            matrix_columns,
            matrix_rows,
            frame_columns,
            frame_rows,
            allow_raw_rgb_passthrough,
            options.jpeg_quality,
            &mut blank_jpeg_cache,
            "JPEG Baseline profiled row frame count exceeds platform addressable memory",
            "JPEG Baseline profile tile x offset overflow",
            "JPEG Baseline profile tile y offset overflow",
        )?;
        record_jpeg_retile_rejections(&mut metrics, &row_plan.retile_rejections);
        let planned = row_plan.frames;

        let mut index = 0usize;
        while index < planned.len() {
            match &planned[index] {
                JpegBaselinePlannedFrame::Passthrough { profile, .. } => {
                    ensure_consistent_pixel_profile(
                        &mut pixel_profile,
                        *profile,
                        "JPEG passthrough pixel profile changed across profiled frames",
                    )?;
                    metrics.record_passthrough_frame();
                    metrics.record_pixel_profile(*profile);
                    remaining = remaining.saturating_sub(1);
                    index += 1;
                }
                JpegBaselinePlannedFrame::Retile {
                    profile,
                    retile_duration,
                    ..
                } => {
                    ensure_consistent_pixel_profile(
                        &mut pixel_profile,
                        *profile,
                        "JPEG retile pixel profile changed across profiled frames",
                    )?;
                    metrics.record_jpeg_retile_baseline_frame(*retile_duration);
                    metrics.record_pixel_profile(*profile);
                    remaining = remaining.saturating_sub(1);
                    index += 1;
                }
                JpegBaselinePlannedFrame::Blank {
                    profile,
                    encode_duration,
                    ..
                } => {
                    ensure_consistent_pixel_profile(
                        &mut pixel_profile,
                        *profile,
                        "blank JPEG Baseline pixel profile changed across profiled frames",
                    )?;
                    metrics.record_cpu_input();
                    metrics.record_pixel_profile(*profile);
                    metrics.record_transcode_route(false, false);
                    metrics.record_jpeg_decode_fallback();
                    metrics.record_jpeg_cpu_encode(*encode_duration);
                    remaining = remaining.saturating_sub(1);
                    index += 1;
                }
                JpegBaselinePlannedFrame::Fallback(_) => {
                    let (next_index, fallback_frames) = jpeg_baseline_fallback_run(&planned, index);
                    index = next_index;

                    #[cfg(all(feature = "metal", target_os = "macos"))]
                    let mut metal_run = try_encode_jpeg_baseline_metal_input_tile_run(
                        slide,
                        &mut metal_input,
                        level,
                        location,
                        row,
                        &fallback_frames,
                        frame_columns,
                        frame_rows,
                        options.jpeg_quality,
                        options.max_prepared_frame_bytes,
                    )?;
                    #[cfg(not(all(feature = "metal", target_os = "macos")))]
                    let mut metal_run =
                        empty_jpeg_baseline_metal_run_for_non_metal(fallback_frames.len());

                    metrics.record_gpu_input_decode_duration(metal_run.input_decode_duration);
                    metrics.record_jpeg_metal_batch_encode(
                        metal_run
                            .frames
                            .iter()
                            .filter(|frame| frame.is_some())
                            .count() as u64,
                        metal_run.encode_duration,
                    );
                    metrics.record_gpu_batches(
                        metal_run.input_decode_batches,
                        0,
                        metal_run.encode_batches,
                    );

                    let mut cpu_batch_results = encode_jpeg_baseline_cpu_metal_misses(
                        slide,
                        location,
                        &fallback_frames,
                        &metal_run,
                        options.encode_backend,
                        JpegBaselineCpuEncodeSettings {
                            frame_columns,
                            frame_rows,
                            jpeg_quality: options.jpeg_quality,
                            max_prepared_frame_bytes: options.max_prepared_frame_bytes,
                        },
                    )?;

                    for (idx, (_frame, metal_encoded)) in fallback_frames
                        .iter()
                        .copied()
                        .zip(metal_run.frames.iter_mut())
                        .enumerate()
                    {
                        let (
                            encoded,
                            profile,
                            input_decode_duration,
                            compose_duration,
                            encode_duration,
                        ) = if let Some((encoded, profile)) = metal_encoded.take() {
                            (
                                encoded,
                                profile,
                                Duration::ZERO,
                                Duration::ZERO,
                                Duration::ZERO,
                            )
                        } else {
                            if options.encode_backend == EncodeBackendPreference::RequireDevice {
                                return Err(Error::Unsupported {
                                        reason:
                                            "requested JPEG Baseline device encode backend is unavailable or unsupported"
                                                .into(),
                                    });
                            }
                            cpu_batch_results[idx].take().ok_or_else(|| Error::Encode {
                                message: "CPU JPEG batch result missing for non-Metal frame".into(),
                            })?
                        };
                        ensure_consistent_pixel_profile(
                            &mut pixel_profile,
                            profile,
                            "JPEG Baseline pixel profile changed across profiled frames",
                        )?;
                        if encoded.backend == JpegBackend::Metal {
                            metrics.record_gpu_input();
                        } else {
                            metrics.record_cpu_input();
                        }
                        metrics.record_pixel_profile(profile);
                        metrics.record_transcode_route(
                            encoded.backend == JpegBackend::Metal,
                            encoded.backend == JpegBackend::Metal,
                        );
                        metrics.record_jpeg_decode_fallback();
                        metrics.record_input_decode_duration(input_decode_duration);
                        metrics.record_compose_duration(compose_duration);
                        match encoded.backend {
                            JpegBackend::Cpu | JpegBackend::Auto => {
                                metrics.record_jpeg_cpu_encode(encode_duration);
                            }
                            JpegBackend::Metal => {}
                        }
                        remaining = remaining.saturating_sub(1);
                    }
                }
            }
        }
    }

    Ok(metrics)
}

fn coverage_jpeg_baseline_routes(
    slide: &Slide,
    options: ExportOptions,
    level: &wsi_rs::Level,
    location: JpegBaselineFrameLocation,
    max_frames: u64,
    deadline: Option<RouteLevelDeadline>,
) -> Result<ExportMetrics, Error> {
    let level_idx = location.level_idx;
    let geometry = jpeg_baseline_route_frame_geometry(slide, level, location, options.tile_size)?;
    let (tiles_across, tiles_down) = (geometry.tiles_across, geometry.tiles_down);
    let (frame_columns, frame_rows) = (geometry.frame_columns, geometry.frame_rows);
    let allow_raw_rgb_passthrough = raw_rgb_passthrough_has_no_geometry_fallback(level, geometry);
    let mut metrics = ExportMetrics::default();
    let mut pixel_profile = None;
    let mut remaining = max_frames;

    for row in 0..tiles_down {
        if remaining == 0 {
            break;
        }
        check_route_level_deadline(deadline, level_idx)?;
        let row_tile_count = tiles_across.min(remaining);
        for col in 0..row_tile_count {
            let mut raw_jpeg_retile_candidate = false;
            let raw =
                slide.read_raw_compressed_tile(&location.tile_request(col as i64, row as i64));

            match raw {
                Ok(raw) if raw_jpeg_matches_frame_geometry(&raw, frame_columns, frame_rows) => {
                    let profile = pixel_profile_from_raw_jpeg_tile(&raw)?;
                    if raw_jpeg_profile_can_passthrough(profile, allow_raw_rgb_passthrough) {
                        ensure_consistent_pixel_profile(
                            &mut pixel_profile,
                            profile,
                            "JPEG passthrough pixel profile changed across coverage frames",
                        )?;
                        metrics.record_passthrough_frame();
                        metrics.record_pixel_profile(profile);
                        remaining = remaining.saturating_sub(1);
                        continue;
                    }
                }
                Ok(raw) if raw.compression == Compression::Jpeg => {
                    raw_jpeg_retile_candidate = true;
                }
                Ok(_) | Err(_) => {}
            }

            if raw_jpeg_retile_candidate {
                match read_raw_jpeg_retile_display_tile(
                    slide,
                    location,
                    col,
                    row,
                    frame_columns,
                    frame_rows,
                )? {
                    RawJpegRetileProbe::Accepted(retiled) => {
                        let profile = pixel_profile_from_raw_jpeg_tile(&retiled.raw)?;
                        if raw_jpeg_profile_can_passthrough(profile, allow_raw_rgb_passthrough) {
                            ensure_consistent_pixel_profile(
                                &mut pixel_profile,
                                profile,
                                "JPEG retile pixel profile changed across coverage frames",
                            )?;
                            metrics.record_jpeg_retile_baseline_frame(retiled.duration);
                            metrics.record_pixel_profile(profile);
                            remaining = remaining.saturating_sub(1);
                            continue;
                        }
                        metrics.record_jpeg_retile_rejected_frame(
                            JpegRetileRejectionReason::ProfileUnsupported,
                        );
                    }
                    RawJpegRetileProbe::Rejected(reason) => {
                        metrics.record_jpeg_retile_rejected_frame(reason);
                    }
                }
            }

            metrics.record_jpeg_cpu_fallback_route_classification();
            remaining = remaining.saturating_sub(1);
        }
    }

    Ok(metrics)
}

fn ensure_consistent_pixel_profile(
    existing: &mut Option<PixelProfile>,
    profile: PixelProfile,
    mismatch_reason: &'static str,
) -> Result<(), Error> {
    if let Some(existing) = existing {
        if *existing != profile {
            return Err(Error::UnsupportedPixelData {
                reason: mismatch_reason.into(),
            });
        }
    } else {
        *existing = Some(profile);
    }
    Ok(())
}

#[cfg(not(all(feature = "metal", target_os = "macos")))]
fn empty_jpeg_baseline_metal_run_for_non_metal(tile_count: usize) -> JpegBaselineMetalEncodedRun {
    JpegBaselineMetalEncodedRun {
        frames: (0..tile_count).map(|_| None).collect(),
        input_decode_duration: Duration::ZERO,
        encode_duration: Duration::ZERO,
        input_decode_batches: 0,
        encode_batches: 0,
    }
}

fn missing_metal_frame_indices<T>(frames: &[Option<T>]) -> Vec<usize> {
    frames
        .iter()
        .enumerate()
        .filter_map(|(idx, frame)| frame.is_none().then_some(idx))
        .collect()
}

fn scatter_indexed_results<T>(
    slots: &mut [Option<T>],
    indexed_results: impl IntoIterator<Item = (usize, T)>,
) -> Result<(), Error> {
    for (idx, result) in indexed_results {
        let slot = slots.get_mut(idx).ok_or_else(|| Error::Unsupported {
            reason: format!("indexed batch result {idx} is outside result slots"),
        })?;
        *slot = Some(result);
    }
    Ok(())
}

fn jpeg_baseline_fallback_run(
    planned: &[JpegBaselinePlannedFrame],
    start: usize,
) -> (usize, Vec<JpegBaselineFallbackFrame>) {
    let mut index = start;
    let mut fallback_frames = Vec::new();
    while let Some(JpegBaselinePlannedFrame::Fallback(frame)) = planned.get(index) {
        fallback_frames.push(*frame);
        index += 1;
    }
    (index, fallback_frames)
}

struct JpegBaselineRowPlan {
    frames: Vec<JpegBaselinePlannedFrame>,
    retile_rejections: Vec<JpegRetileRejectionReason>,
}

#[derive(Clone, Copy)]
struct JpegBaselineCpuEncodeSettings {
    frame_columns: u32,
    frame_rows: u32,
    jpeg_quality: u8,
    max_prepared_frame_bytes: u64,
}

#[allow(clippy::too_many_arguments)]
fn plan_jpeg_baseline_row(
    slide: &Slide,
    location: JpegBaselineFrameLocation,
    row: u64,
    row_tile_count: u64,
    matrix_columns: u64,
    matrix_rows: u64,
    frame_columns: u32,
    frame_rows: u32,
    allow_raw_rgb_passthrough: bool,
    jpeg_quality: u8,
    blank_jpeg_cache: &mut Option<(Vec<u8>, Duration)>,
    row_frame_count_error: &'static str,
    fallback_x_overflow_reason: &'static str,
    fallback_y_overflow_reason: &'static str,
) -> Result<JpegBaselineRowPlan, Error> {
    let row_frame_capacity = usize::try_from(row_tile_count).map_err(|_| Error::Unsupported {
        reason: row_frame_count_error.into(),
    })?;
    let mut planned = Vec::with_capacity(row_frame_capacity);
    let mut retile_rejections = Vec::new();

    for col in 0..row_tile_count {
        let mut raw_jpeg_retile_candidate = false;
        let raw = slide.read_raw_compressed_tile(&location.tile_request(col as i64, row as i64));

        let empty_raw_tile = matches!(&raw, Err(err) if raw_compressed_error_is_empty_tile(err));
        match raw {
            Ok(raw) if raw_jpeg_matches_frame_geometry(&raw, frame_columns, frame_rows) => {
                let profile = pixel_profile_from_raw_jpeg_tile(&raw)?;
                if raw_jpeg_profile_can_passthrough(profile, allow_raw_rgb_passthrough) {
                    planned.push(JpegBaselinePlannedFrame::Passthrough {
                        uncompressed_bytes: uncompressed_frame_bytes(&raw)?,
                        data: raw.data,
                        profile,
                    });
                    continue;
                }
            }
            Ok(raw) if raw.compression == Compression::Jpeg => {
                raw_jpeg_retile_candidate = true;
            }
            Ok(_) | Err(_) => {}
        }

        if empty_raw_tile {
            planned.push(blank_jpeg_baseline_frame(
                frame_columns,
                frame_rows,
                jpeg_quality,
                blank_jpeg_cache,
            )?);
            continue;
        }

        if raw_jpeg_retile_candidate {
            match read_raw_jpeg_retile_display_tile(
                slide,
                location,
                col,
                row,
                frame_columns,
                frame_rows,
            )? {
                RawJpegRetileProbe::Accepted(retiled) => {
                    let profile = pixel_profile_from_raw_jpeg_tile(&retiled.raw)?;
                    if raw_jpeg_profile_can_passthrough(profile, allow_raw_rgb_passthrough) {
                        planned.push(JpegBaselinePlannedFrame::Retile {
                            uncompressed_bytes: uncompressed_frame_bytes(&retiled.raw)?,
                            data: retiled.raw.data,
                            profile,
                            retile_duration: retiled.duration,
                        });
                        continue;
                    }
                    retile_rejections.push(JpegRetileRejectionReason::ProfileUnsupported);
                }
                RawJpegRetileProbe::Rejected(reason) => {
                    retile_rejections.push(reason);
                }
            }
        }

        planned.push(JpegBaselinePlannedFrame::Fallback(
            jpeg_baseline_fallback_frame(
                col,
                row,
                matrix_columns,
                matrix_rows,
                frame_columns,
                frame_rows,
                fallback_x_overflow_reason,
                fallback_y_overflow_reason,
            )?,
        ));
    }

    Ok(JpegBaselineRowPlan {
        frames: planned,
        retile_rejections,
    })
}

fn record_jpeg_retile_rejections(
    metrics: &mut ExportMetrics,
    rejections: &[JpegRetileRejectionReason],
) {
    for &reason in rejections {
        metrics.record_jpeg_retile_rejected_frame(reason);
    }
}

#[allow(clippy::too_many_arguments)]
fn jpeg_baseline_fallback_frame(
    col: u64,
    row: u64,
    matrix_columns: u64,
    matrix_rows: u64,
    frame_columns: u32,
    frame_rows: u32,
    x_overflow_reason: &'static str,
    y_overflow_reason: &'static str,
) -> Result<JpegBaselineFallbackFrame, Error> {
    OutputFrameRect::clamped(
        col,
        row,
        FrameRectGrid {
            matrix_columns,
            matrix_rows,
            frame_columns,
            frame_rows,
        },
        FrameRectOverflowReasons {
            x: x_overflow_reason,
            y: y_overflow_reason,
        },
    )
}

fn encode_jpeg_baseline_cpu_metal_misses(
    slide: &Slide,
    location: JpegBaselineFrameLocation,
    fallback_frames: &[JpegBaselineFallbackFrame],
    metal_run: &JpegBaselineMetalEncodedRun,
    encode_backend: EncodeBackendPreference,
    settings: JpegBaselineCpuEncodeSettings,
) -> Result<Vec<Option<EncodedJpegBaselineFrame>>, Error> {
    let mut cpu_batch_results: Vec<Option<EncodedJpegBaselineFrame>> =
        (0..fallback_frames.len()).map(|_| None).collect();
    if encode_backend == EncodeBackendPreference::RequireDevice {
        return Ok(cpu_batch_results);
    }

    let cpu_indices = missing_metal_frame_indices(&metal_run.frames);
    let cpu_frames = cpu_indices
        .iter()
        .map(|&idx| fallback_frames[idx])
        .collect::<Vec<_>>();
    let cpu_encoded =
        encode_jpeg_baseline_cpu_input_tile_batch(slide, location, &cpu_frames, settings)?;
    for (idx, encoded) in cpu_indices.into_iter().zip(cpu_encoded) {
        cpu_batch_results[idx] = Some(encoded);
    }
    Ok(cpu_batch_results)
}

fn encode_jpeg_baseline_cpu_input_tile_batch(
    slide: &Slide,
    location: JpegBaselineFrameLocation,
    frames: &[JpegBaselineFallbackFrame],
    settings: JpegBaselineCpuEncodeSettings,
) -> Result<Vec<EncodedJpegBaselineFrame>, Error> {
    frames
        .par_iter()
        .map(|frame| encode_jpeg_baseline_cpu_input_tile(slide, location, *frame, settings))
        .collect()
}

fn encode_jpeg_baseline_cpu_input_tile(
    slide: &Slide,
    location: JpegBaselineFrameLocation,
    frame: JpegBaselineFallbackFrame,
    settings: JpegBaselineCpuEncodeSettings,
) -> Result<EncodedJpegBaselineFrame, Error> {
    let (prepared_bytes, profile, subsampling, input_decode_duration, compose_duration) =
        prepare_jpeg_baseline_cpu_input_tile(slide, location, frame, settings)?;
    let samples = match profile.components {
        1 => JpegSamples::Gray8 {
            data: &prepared_bytes,
            width: settings.frame_columns,
            height: settings.frame_rows,
        },
        3 => JpegSamples::Rgb8 {
            data: &prepared_bytes,
            width: settings.frame_columns,
            height: settings.frame_rows,
        },
        components => {
            return Err(Error::UnsupportedPixelData {
                reason: format!("JPEG Baseline supports 1 or 3 components, got {components}"),
            });
        }
    };
    let encode_started = Instant::now();
    let encoded = encode_jpeg_baseline_cpu_fragment(
        samples,
        settings.jpeg_quality,
        subsampling,
        jpeg_baseline_cpu_restart_interval(
            settings.frame_columns,
            settings.frame_rows,
            subsampling,
        ),
    )?;
    let encode_duration = encode_started.elapsed();

    Ok((
        encoded,
        profile,
        input_decode_duration,
        compose_duration,
        encode_duration,
    ))
}

fn prepare_jpeg_baseline_cpu_input_tile(
    slide: &Slide,
    location: JpegBaselineFrameLocation,
    frame: JpegBaselineFallbackFrame,
    settings: JpegBaselineCpuEncodeSettings,
) -> Result<(Vec<u8>, PixelProfile, JpegSubsampling, Duration, Duration), Error> {
    let prepared = read_and_prepare_region(
        slide,
        location,
        frame.x,
        frame.y,
        frame.width,
        frame.height,
        settings.frame_columns,
        settings.frame_rows,
        settings.max_prepared_frame_bytes,
    )?;
    let (profile, subsampling) = jpeg_baseline_output_profile(prepared.profile)?;
    Ok((
        prepared.bytes,
        profile,
        subsampling,
        prepared.input_decode_duration,
        prepared.compose_duration,
    ))
}

#[allow(clippy::too_many_arguments)]
fn read_and_prepare_region(
    slide: &Slide,
    location: JpegBaselineFrameLocation,
    x: u64,
    y: u64,
    width: u32,
    height: u32,
    output_width: u32,
    output_height: u32,
    max_prepared_frame_bytes: u64,
) -> Result<PreparedCpuRegion, Error> {
    let input_decode_started = Instant::now();
    let region = slide
        .read_region(&RegionRequest {
            scene: SceneId(location.scene_idx),
            series: SeriesId(location.series_idx),
            level: LevelIdx(location.level_idx),
            plane: PlaneIdx(PlaneSelection {
                z: location.z,
                c: location.c,
                t: location.t,
            }),
            origin_px: (x as i64, y as i64),
            size_px: (width, height),
        })
        .map_err(|source| Error::SlideRead {
            message: source.to_string(),
        })?;
    let input_decode_duration = input_decode_started.elapsed();

    let compose_started = Instant::now();
    let max_prepared_frame_bytes =
        usize::try_from(max_prepared_frame_bytes).map_err(|_| Error::UnsupportedPixelData {
            reason: "max_prepared_frame_bytes exceeds platform addressable memory".into(),
        })?;
    let prepared = prepare_tile_samples_with_limit(
        &region,
        output_width,
        output_height,
        max_prepared_frame_bytes,
    )?;
    let compose_duration = compose_started.elapsed();
    Ok(PreparedCpuRegion {
        bytes: prepared.bytes,
        profile: prepared.profile,
        input_decode_duration,
        compose_duration,
    })
}

fn jpeg_baseline_output_profile(
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

#[cfg(all(feature = "metal", target_os = "macos"))]
fn jpeg_baseline_auto_allows_metal_batch(
    preference: EncodeBackendPreference,
    frame_columns: u32,
    frame_rows: u32,
    frame_count: usize,
    source_device_decode: bool,
) -> bool {
    match preference {
        EncodeBackendPreference::CpuOnly => false,
        EncodeBackendPreference::PreferDevice | EncodeBackendPreference::RequireDevice => {
            frame_count > 0
        }
        EncodeBackendPreference::Auto => {
            source_device_decode && frame_columns == frame_rows && frame_count >= 4
        }
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn lossless_j2k_auto_allows_metal_input(
    preference: EncodeBackendPreference,
    transfer_syntax: TransferSyntax,
    frame_count: u64,
    _source_device_decode: bool,
) -> bool {
    if !transfer_syntax.is_lossless_j2k_family() {
        return false;
    }
    match preference {
        EncodeBackendPreference::CpuOnly => false,
        EncodeBackendPreference::PreferDevice | EncodeBackendPreference::RequireDevice => true,
        EncodeBackendPreference::Auto => {
            if frame_count < LOSSLESS_J2K_AUTO_ROUTE_MIN_FRAMES {
                return false;
            }
            matches!(
                transfer_syntax,
                TransferSyntax::Htj2kLossless | TransferSyntax::Htj2kLosslessRpcl
            )
        }
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn lossless_j2k_metal_input_preference(
    preference: EncodeBackendPreference,
    source_device_decode: bool,
) -> EncodeBackendPreference {
    if preference == EncodeBackendPreference::RequireDevice && !source_device_decode {
        EncodeBackendPreference::CpuOnly
    } else {
        preference
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn lossless_j2k_auto_should_start_cpu_only(
    preference: EncodeBackendPreference,
    transfer_syntax: TransferSyntax,
    frame_count: u64,
    source_device_decode: bool,
) -> bool {
    preference == EncodeBackendPreference::Auto
        && transfer_syntax.is_lossless_j2k_family()
        && !lossless_j2k_auto_allows_metal_input(
            preference,
            transfer_syntax,
            frame_count,
            source_device_decode,
        )
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn auto_metal_input_route_cache_key(
    source_path: &Path,
    options: ExportOptions,
    location: JpegBaselineFrameLocation,
    route_scope_frames: u64,
) -> Option<AutoMetalInputRouteCacheKey> {
    (options.encode_backend == EncodeBackendPreference::Auto
        && matches!(
            options.transfer_syntax,
            TransferSyntax::Htj2kLossless | TransferSyntax::Htj2kLosslessRpcl
        ))
    .then(|| AutoMetalInputRouteCacheKey {
        source_path: source_path.to_path_buf(),
        scene_idx: location.scene_idx,
        series_idx: location.series_idx,
        level: location.level_idx,
        z: location.z,
        c: location.c,
        t: location.t,
        tile_size: options.tile_size,
        transfer_syntax: options.transfer_syntax,
        route_scope_frames,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn try_encode_jpeg_baseline_metal_input_tile_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    level: &wsi_rs::Level,
    location: JpegBaselineFrameLocation,
    row: u64,
    frames: &[JpegBaselineFallbackFrame],
    frame_columns: u32,
    frame_rows: u32,
    jpeg_quality: u8,
    max_prepared_frame_bytes: u64,
) -> Result<JpegBaselineMetalEncodedRun, Error> {
    objc::rc::autoreleasepool(|| {
        if !jpeg_baseline_auto_allows_metal_batch(
            metal_input.preference,
            frame_columns,
            frame_rows,
            frames.len(),
            metal_input.source_device_decode,
        ) {
            return Ok(empty_jpeg_baseline_metal_run(frames.len()));
        }
        if !output_frame_maps_to_wsi_rs_tile(level, frame_columns, frame_rows) {
            if metal_input.preference == EncodeBackendPreference::RequireDevice {
                return Err(Error::Unsupported {
                    reason:
                        "requested JPEG Baseline Metal fallback requires the DICOM frame grid to align with wsi-rs source tiles"
                            .into(),
                });
            }
            return Ok(empty_jpeg_baseline_metal_run(frames.len()));
        }

        let row_i64 = i64::try_from(row).map_err(|_| Error::Unsupported {
            reason: "JPEG Baseline Metal tile row exceeds i64".into(),
        })?;
        let mut requests = Vec::with_capacity(frames.len());
        for frame in frames {
            requests.push(TileRequest {
                scene: location.scene_idx,
                series: location.series_idx,
                level: location.level_idx,
                plane: PlaneSelection {
                    z: location.z,
                    c: location.c,
                    t: location.t,
                },
                col: i64::try_from(frame.x / u64::from(frame_columns)).map_err(|_| {
                    Error::Unsupported {
                        reason: "JPEG Baseline Metal tile column exceeds i64".into(),
                    }
                })?,
                row: row_i64,
            });
        }
        let output = match metal_input.source_tile_output_preference() {
            Ok(output) => output,
            Err(err) if metal_input.preference == EncodeBackendPreference::RequireDevice => {
                return Err(err);
            }
            Err(_) => return Ok(empty_jpeg_baseline_metal_run(frames.len())),
        };

        let input_decode_started = Instant::now();
        let pixels = match slide.read_tiles(&requests, output) {
            Ok(pixels) if pixels.len() == frames.len() => pixels,
            Ok(pixels) if metal_input.preference == EncodeBackendPreference::RequireDevice => {
                return Err(Error::SlideRead {
                    message: format!(
                        "JPEG Baseline Metal input decode returned {} tile(s), expected {}",
                        pixels.len(),
                        frames.len()
                    ),
                });
            }
            Ok(_) => {
                return Ok(empty_jpeg_baseline_metal_run_with_input_duration(
                    frames.len(),
                    input_decode_started.elapsed(),
                ));
            }
            Err(err) if metal_input.preference == EncodeBackendPreference::RequireDevice => {
                return Err(Error::SlideRead {
                    message: format!("JPEG Baseline Metal input decode failed: {err}"),
                });
            }
            Err(_) => {
                return Ok(empty_jpeg_baseline_metal_run_with_input_duration(
                    frames.len(),
                    input_decode_started.elapsed(),
                ));
            }
        };
        let input_decode_duration = input_decode_started.elapsed();

        let tile_entries =
            jpeg_baseline_metal_tile_entries(pixels, frames, metal_input.preference)?;
        let batch_tiles: Vec<_> = tile_entries
            .iter()
            .filter_map(|entry| entry.as_ref().cloned())
            .collect();
        if batch_tiles.is_empty() {
            return Ok(empty_jpeg_baseline_metal_run_with_input_duration(
                frames.len(),
                input_decode_duration,
            ));
        }
        let bytes_per_pixel =
            u64::try_from(batch_tiles[0].format.bytes_per_pixel()).map_err(|_| {
                Error::UnsupportedPixelData {
                    reason: "JPEG Baseline Metal bytes-per-pixel exceeds u64".into(),
                }
            })?;
        let prepared_frame_bytes = u64::from(frame_columns)
            .checked_mul(u64::from(frame_rows))
            .and_then(|pixels| pixels.checked_mul(bytes_per_pixel))
            .ok_or_else(|| Error::UnsupportedPixelData {
                reason: "JPEG Baseline Metal prepared frame byte length overflow".into(),
            })?;
        if prepared_frame_bytes > max_prepared_frame_bytes {
            return Err(Error::UnsupportedPixelData {
                reason: format!(
                    "prepared tile buffer requires {prepared_frame_bytes} bytes, exceeding configured limit {max_prepared_frame_bytes}"
                ),
            });
        }

        let encode_started = Instant::now();
        let encoded = match encode_jpeg_baseline_metal_device_tile_batch(
            &batch_tiles,
            frame_columns,
            frame_rows,
            jpeg_quality,
            metal_input.jpeg_encode_session()?,
        ) {
            Ok(encoded) => encoded,
            Err(err) if metal_input.preference == EncodeBackendPreference::RequireDevice => {
                return Err(err);
            }
            Err(_) => return Ok(empty_jpeg_baseline_metal_run(frames.len())),
        };
        if encoded.len() != batch_tiles.len() {
            if metal_input.preference == EncodeBackendPreference::RequireDevice {
                return Err(Error::Encode {
                    message: format!(
                        "JPEG Baseline Metal encode returned {} frame(s), expected {}",
                        encoded.len(),
                        batch_tiles.len()
                    ),
                });
            }
            return Ok(empty_jpeg_baseline_metal_run_with_input_duration(
                frames.len(),
                input_decode_duration,
            ));
        }
        let encode_duration = encode_started.elapsed();
        let mut encoded = encoded.into_iter();
        let mut output_frames = Vec::with_capacity(frames.len());
        for entry in tile_entries {
            if entry.is_some() {
                output_frames.push(Some(encoded.next().ok_or_else(|| {
                    Error::Encode {
                        message:
                            "JPEG Baseline Metal encoded frame count did not match input tile count"
                                .into(),
                    }
                })?));
            } else {
                output_frames.push(None);
            }
        }
        Ok(JpegBaselineMetalEncodedRun {
            frames: output_frames,
            input_decode_duration,
            encode_duration,
            input_decode_batches: 1,
            encode_batches: 1,
        })
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn jpeg_baseline_metal_tile_entries(
    pixels: Vec<TilePixels>,
    frames: &[JpegBaselineFallbackFrame],
    preference: EncodeBackendPreference,
) -> Result<Vec<Option<wsi_rs::output::metal::MetalDeviceTile>>, Error> {
    let mut entries = Vec::with_capacity(frames.len());
    for (pixels, frame) in pixels.into_iter().zip(frames.iter()) {
        let TilePixels::Device(DeviceTile::Metal(tile)) = pixels else {
            if preference == EncodeBackendPreference::RequireDevice {
                return Err(Error::Unsupported {
                    reason:
                        "requested JPEG Baseline Metal input decode returned CPU pixels; set STATUMEN_JPEG_DEVICE_DECODE=1 or STATUMEN_JP2K_DEVICE_DECODE=1 for compressed WSI tiles"
                            .into(),
                });
            }
            entries.push(None);
            continue;
        };
        if tile.width != frame.width || tile.height != frame.height {
            if preference == EncodeBackendPreference::RequireDevice {
                return Err(Error::Unsupported {
                    reason: format!(
                        "JPEG Baseline Metal input geometry changed: expected {}x{}, got {}x{}",
                        frame.width, frame.height, tile.width, tile.height
                    ),
                });
            }
            entries.push(None);
            continue;
        }
        entries.push(Some(tile));
    }
    Ok(entries)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn encode_jpeg_baseline_metal_device_tile_batch(
    tiles: &[wsi_rs::output::metal::MetalDeviceTile],
    frame_columns: u32,
    frame_rows: u32,
    jpeg_quality: u8,
    session: &j2k_jpeg_metal::MetalBackendSession,
) -> Result<Vec<(EncodedJpeg, PixelProfile)>, Error> {
    let first = tiles.first().ok_or_else(|| Error::Unsupported {
        reason: "JPEG Baseline Metal tile batch is empty".into(),
    })?;
    let source_profile = pixel_profile_from_device_format(first.format)?;
    let (profile, subsampling) = jpeg_baseline_output_profile(source_profile)?;
    let mut requests = Vec::with_capacity(tiles.len());
    for tile in tiles {
        if pixel_profile_from_device_format(tile.format)? != source_profile {
            return Err(Error::UnsupportedPixelData {
                reason: "JPEG Baseline Metal tile batch changed pixel profile".into(),
            });
        }
        let wsi_rs::output::metal::MetalDeviceStorage::Buffer {
            buffer,
            byte_offset,
        } = &tile.storage;
        requests.push(JpegBaselineMetalEncodeTile {
            buffer,
            byte_offset: *byte_offset,
            width: tile.width,
            height: tile.height,
            pitch_bytes: tile.pitch_bytes,
            output_width: frame_columns,
            output_height: frame_rows,
            format: tile.format,
        });
    }
    let encoded = encode_jpeg_baseline_batch_from_metal_buffers(
        &requests,
        j2k_jpeg::JpegEncodeOptions {
            quality: jpeg_quality,
            subsampling,
            restart_interval: None,
            backend: JpegBackend::Metal,
        },
        session,
    )
    .map_err(|source| Error::Encode {
        message: format!("JPEG Baseline Metal encode failed: {source}"),
    })?;
    Ok(encoded
        .into_iter()
        .map(|encoded| (encoded, profile))
        .collect())
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn empty_jpeg_baseline_metal_run(tile_count: usize) -> JpegBaselineMetalEncodedRun {
    empty_jpeg_baseline_metal_run_with_input_duration(tile_count, Duration::ZERO)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn empty_jpeg_baseline_metal_run_with_input_duration(
    tile_count: usize,
    input_decode_duration: Duration,
) -> JpegBaselineMetalEncodedRun {
    JpegBaselineMetalEncodedRun {
        frames: (0..tile_count).map(|_| None).collect(),
        input_decode_duration,
        encode_duration: Duration::ZERO,
        input_decode_batches: u64::from(input_decode_duration > Duration::ZERO),
        encode_batches: 0,
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_cpu_input_tile(
    slide: &Slide,
    j2k_encoder: &mut DicomJ2kEncoder,
    location: JpegBaselineFrameLocation,
    x: u64,
    y: u64,
    width: u32,
    height: u32,
    tile_size: u32,
) -> Result<
    (
        Result<EncodedDicomJ2kFrame, Error>,
        PixelProfile,
        Duration,
        Duration,
    ),
    Error,
> {
    let prepared = prepare_cpu_input_lossless_j2k_tile(
        slide,
        location.scene_idx,
        location.series_idx,
        location.level_idx,
        location.z,
        location.c,
        location.t,
        x,
        y,
        width,
        height,
        tile_size,
        u64::MAX,
    )?;
    let samples = lossless_j2k_samples_from_prepared_region(&prepared, tile_size)?;
    Ok((
        j2k_encoder.encode(samples),
        prepared.profile,
        prepared.input_decode_duration,
        prepared.compose_duration,
    ))
}

#[allow(clippy::too_many_arguments)]
fn encode_generated_jpeg_direct_htj2k_planned_batch(
    slide: &Slide,
    direct_encoder: &mut jpeg_direct_htj2k::BatchEncoder,
    location: JpegBaselineFrameLocation,
    planned: &[LosslessJ2kPlannedFrame],
    indices: &[usize],
    tile_size: u32,
    jpeg_quality: u8,
    max_prepared_frame_bytes: u64,
) -> Result<Vec<(usize, GeneratedJpegDirectHtj2kOutcome)>, Error> {
    let frames = indices
        .iter()
        .map(|&idx| {
            let planned = &planned[idx];
            planned.rect()
        })
        .collect::<Vec<_>>();
    let jpeg_tiles = encode_jpeg_baseline_cpu_input_tile_batch(
        slide,
        location,
        &frames,
        JpegBaselineCpuEncodeSettings {
            frame_columns: tile_size,
            frame_rows: tile_size,
            jpeg_quality,
            max_prepared_frame_bytes,
        },
    )?;
    let direct_frames = jpeg_tiles
        .iter()
        .map(
            |(encoded, profile, _input_decode, _compose, _jpeg_encode)| jpeg_direct_htj2k::Frame {
                data: encoded.data.clone(),
                profile: *profile,
            },
        )
        .collect::<Vec<_>>();
    let direct_tiles =
        jpeg_direct_htj2k::encode_frames_batch_with_encoder(&direct_frames, direct_encoder)?;

    Ok(indices
        .iter()
        .copied()
        .zip(jpeg_tiles.into_iter().zip(direct_tiles))
        .map(
            |(
                idx,
                (
                    (
                        _encoded,
                        _profile,
                        input_decode_duration,
                        compose_duration,
                        jpeg_encode_duration,
                    ),
                    direct,
                ),
            )| {
                (
                    idx,
                    GeneratedJpegDirectHtj2kOutcome {
                        direct,
                        input_decode_duration,
                        compose_duration,
                        jpeg_encode_duration,
                    },
                )
            },
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::encode::{
        dicom_j2k_decomposition_levels, encode_dicom_j2k_lossless, encode_dicom_lossless,
    };
    use crate::options::IccProfilePolicy;
    use crate::test_support::{
        dicom_fragment_payload_without_padding, encode_test_jpeg, find_command_for_test,
        read_binary_ppm_for_test, tiff_short_value, tiff_tag, write_tiled_jp2k_rgb_tiff,
        write_tiled_jp2k_ycbcr_tiff, write_tiled_jpeg_tiff,
    };
    use dicom_core::{DataElement, PrimitiveValue, VR};
    use dicom_dictionary_std::{tags, uids};
    use dicom_object::{FileMetaTableBuilder, InMemDicomObject};

    #[cfg(all(feature = "metal", target_os = "macos"))]
    mod auto_route_tests;
    mod export_integration_tests;
    mod external_htj2k_tests;
    mod fixture_tests;
    mod icc_profile_tests;
    mod j2k_encode_tests;
    mod jpeg_baseline_route_tests;
    mod options_builder_tests;
    mod route_profile_tests;
    mod support;
    mod unit_tests;

    use support::*;
}
