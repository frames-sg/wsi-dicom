use std::collections::BTreeMap;
#[cfg(all(feature = "metal", target_os = "macos"))]
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::Path;
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
    Compression, LevelIdx, PlaneSelection, RawCompressedTile, RegionRequest, SceneId, SeriesId,
    Slide,
};
#[cfg(all(feature = "metal", target_os = "macos"))]
use wsi_rs::{TileOutputPreference, TilePixels};

#[cfg(test)]
use crate::api::Export;
#[cfg(test)]
use crate::calibration::ColorManagement;
use crate::coordinate::InstanceCoordinate;
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
#[cfg(test)]
use crate::report::IccProfileSource;
use crate::report::{
    EncodedFrame, ExportMetrics, ExportReport, IccProfileReport, InstanceReport,
    JpegRetileRejectionReason, RouteCorpusCoverageFailure, RouteCorpusCoverageReport,
    RouteCoverageReport, RouteProfileReport,
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
    j2k_encode_transfer_syntax, j2k_family_passthrough_probe_allowed, j2k_route_tile_size,
    unsupported_j2k_route_error,
};
#[cfg(test)]
use crate::tile::prepare_tile_samples;
use crate::tile::{optical_path_groups, prepare_tile_samples_with_limit, PixelProfile};
#[cfg(all(feature = "metal", target_os = "macos"))]
use crate::tile::{pixel_profile_from_device_format, pixel_profile_from_wsi_device_format};
use crate::time::duration_as_reported_micros;
use crate::uid::DicomExportIdentity;
use crate::writer::{
    extended_offset_table_metadata_bytes, unique_spool_path,
    write_dicom_object_with_streamed_pixel_data, BufferedPixelDataSink, FrameGrid,
    LossyCompressionHistory, PerFrameFunctionalGroupsPlan, PixelDataSink, PixelDataSpool,
    StreamedDicomWritePlan,
};

mod corpus_discovery;
mod defaults;
mod frame_region;
mod hybrid_lane;
mod icc_profile;
mod j2k_direct_htj2k;
mod j2k_policy;
mod jobs;
mod jpeg_baseline;
mod jpeg_baseline_instance;
#[cfg(all(feature = "metal", target_os = "macos"))]
mod jpeg_baseline_metal;
mod jpeg_baseline_pipeline;
mod jpeg_direct_htj2k;
mod jpeg_passthrough;
mod jpeg_retile;
mod lossless_j2k_cpu;
mod lossless_j2k_direct_routes;
mod lossless_j2k_instance;
mod lossless_j2k_pipeline;
mod lossless_j2k_plan;
mod lossy_provenance;
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
mod transaction;

pub use self::defaults::default_transfer_syntax_for_source;
pub use self::profiling::{
    profile_dicom_route_corpus_coverage, profile_dicom_route_coverage, profile_dicom_routes,
};

use self::corpus_discovery::collect_wsi_candidate_paths;

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
    try_encode_metal_input_tile_run, AutoMetalInputProbeRequest, MetalEncodedRowRunKey,
    MetalEncodedTileRun, MetalInputTileReader, MetalInputTileRunRequest, MetalSourceTileKey,
    PendingMetalEncodedGridRun, PendingMetalEncodedTileRun, RoutedLosslessJ2kTile,
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
    clear_auto_metal_input_route_cache_state_for_tests,
    flush_persistent_auto_metal_input_route_cache_to_path,
    load_persistent_auto_metal_input_route_cache_from_path,
};

pub(crate) use self::frame_region::FrameRectGrid;
use self::frame_region::PreparedCpuRegion;
use self::frame_region::{FrameRectOverflowReasons, OutputFrameRect};
use self::icc_profile::{preflight_icc_profiles, resolve_icc_profile};
use self::j2k_policy::*;
use self::jobs::*;
#[cfg(all(feature = "metal", target_os = "macos"))]
use self::jpeg_baseline::encode_jpeg_baseline_metal_device_tile_batch;
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
#[cfg(all(feature = "metal", target_os = "macos"))]
use self::jpeg_baseline_metal::*;
use self::jpeg_baseline_pipeline::*;
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
use self::lossless_j2k_direct_routes::*;
use self::lossless_j2k_instance::export_instance;
#[cfg(all(feature = "metal", target_os = "macos"))]
use self::lossless_j2k_instance::{prepare_lossless_j2k_instance, PendingLosslessJ2kInstance};
use self::lossless_j2k_pipeline::*;
use self::lossless_j2k_plan::J2kPassthroughFrame;
pub(crate) use self::lossless_j2k_plan::{
    plan_lossless_j2k_frames, LosslessJ2kPlanRequest, LosslessJ2kPlannedFrame,
};
#[cfg(test)]
use self::profiling::{check_route_level_deadline, RouteLevelDeadline};
use self::tile_grid::{checked_frame_count_u32, TileGrid};
use self::transaction::{ExportTransaction, OutputDirectoryLock};

#[cfg(all(test, feature = "metal", target_os = "macos"))]
const WSI_RS_JPEG_DEVICE_DECODE_ENV: &str = "WSI_RS_JPEG_DEVICE_DECODE";

#[cfg(all(test, feature = "metal", target_os = "macos"))]
const WSI_RS_JP2K_DEVICE_DECODE_ENV: &str = "WSI_RS_JP2K_DEVICE_DECODE";

const DIRECT_JPEG_PASSTHROUGH_WRITE_CHUNK_FRAMES: usize = 2048;

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
    let slide = Slide::open(&request.source_path).map_err(|source| Error::SourceOpen {
        path: request.source_path.clone(),
        message: source.to_string(),
    })?;
    let jobs = dicom_export_instance_jobs(&slide, &request)?;
    preflight_output_paths(&request, &jobs)?;
    preflight_metadata_budgets(&slide, &request, &jobs)?;
    let effective_icc_digests = preflight_icc_profiles(&slide, &request, &metadata, &jobs)?;
    let identity = DicomExportIdentity::for_export(
        &request.source_path,
        &request.options,
        &metadata,
        request.level_filter,
        &effective_icc_digests,
    )?;

    fs::create_dir_all(&request.output_dir).map_err(|source| Error::Io {
        path: request.output_dir.clone(),
        source,
    })?;
    let _output_lock = OutputDirectoryLock::acquire(&request.output_dir)?;
    let transaction = ExportTransaction::begin(&request.output_dir)?;
    let mut staged_request = request.clone();
    staged_request.output_dir = transaction.staging_dir().to_path_buf();
    staged_request.options.overwrite = false;
    let mut instances =
        export_dicom_instance_jobs(&slide, &staged_request, &metadata, &identity, &jobs)?;

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
    transaction.commit(&mut instances, request.options.overwrite)?;

    #[cfg(all(feature = "metal", target_os = "macos"))]
    if let Err(err) = flush_persistent_auto_metal_input_route_cache_if_requested() {
        eprintln!(
            "wsi-dicom: export completed, but the optional auto-route cache could not be persisted: {err}"
        );
    }

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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::encode::{
        dicom_j2k_decomposition_levels, encode_dicom_j2k_lossless, encode_dicom_lossless,
    };
    use crate::test_support::{
        dicom_fragment_payload_without_padding, encode_test_gray_jpeg, encode_test_jpeg,
        find_command_for_test, read_binary_ppm_for_test, tiff_short_value, tiff_tag,
        write_tiled_grayscale_jpeg_tiff, write_tiled_jp2k_rgb_tiff, write_tiled_jp2k_ycbcr_tiff,
        write_tiled_jpeg_tiff,
    };
    use dicom_core::VR;
    use dicom_dictionary_std::{tags, uids};

    #[test]
    fn published_jpeg_backend_classification_treats_gpu_backends_as_device() {
        assert!(!jpeg_backend_uses_device(JpegBackend::Auto));
        assert!(!jpeg_backend_uses_device(JpegBackend::Cpu));
        assert!(jpeg_backend_uses_device(JpegBackend::Metal));
        assert!(jpeg_backend_uses_device(JpegBackend::Cuda));
    }

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
