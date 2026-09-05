use std::fs;
#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
use j2k::J2kLosslessSamples;
#[cfg(test)]
use j2k::{J2kView, ReversibleTransform};
#[cfg(test)]
use j2k_core::CompressedTransferSyntax;
#[cfg(all(test, feature = "metal", target_os = "macos"))]
use j2k_core::PixelFormat as J2kPixelFormat;
#[cfg(test)]
use j2k_jpeg::JpegBackend;
#[cfg(all(test, feature = "metal", target_os = "macos"))]
use wsi_rs::DeviceTile;
#[cfg(test)]
use wsi_rs::EncodedTilePhotometricInterpretation;
#[cfg(test)]
use wsi_rs::LevelSourceKind;
use wsi_rs::Slide;
#[cfg(test)]
use wsi_rs::TileLayout;
#[cfg(all(test, feature = "metal", target_os = "macos"))]
use wsi_rs::TilePixels;
#[cfg(test)]
use wsi_rs::{Compression, RawCompressedTile, RegionRequest};

#[cfg(test)]
use crate::api::Export;
#[cfg(test)]
use crate::calibration::ColorManagement;
use crate::coordinate::InstanceCoordinate;
use crate::encode::DicomJ2kEncoder;
use crate::error::Error;
use crate::metadata::DicomMetadata;
#[cfg(test)]
use crate::metadata::MetadataSource;
#[cfg(test)]
use crate::options::SourcePixelSpacingMm;
#[cfg(test)]
use crate::options::{
    CodecValidation, EncodeBackendPreference, ExportOptions, JpegDirectHtj2kProfile,
};
use crate::options::{NormalizedExportOptions, TransferSyntax};
#[cfg(test)]
use crate::report::IccProfileSource;
use crate::report::{EncodedFrame, ExportMetrics, ExportReport};
#[cfg(test)]
use crate::report::{GpuEncodeMetrics, RouteCounters, WriteTimings};
#[cfg(test)]
use crate::report::{RouteCoverageReport, RouteProfileReport};
#[cfg(test)]
use crate::request::{DefaultTransferSyntaxRequest, FrameSamples};
use crate::request::{ExportRequest, J2kFrameEncodeRequest};
#[cfg(test)]
use crate::request::{RouteCoverageRequest, RouteCoverageTarget, RouteProfileRequest};
use crate::routing::open_slide;
#[cfg(test)]
use crate::routing::unsupported_j2k_route_error;
#[cfg(test)]
use crate::tile::prepare_tile_samples;
use crate::tile::PixelProfile;
use crate::uid::DicomExportIdentity;
#[cfg(test)]
use crate::writer::{extended_offset_table_metadata_bytes, PerFrameFunctionalGroupsPlan};

mod corpus_discovery;
mod defaults;
mod frame_region;
mod hybrid_lane;
mod icc_profile;
mod j2k_direct_htj2k;
mod j2k_policy;
mod jobs;
mod jpeg_baseline;
mod jpeg_baseline_frames;
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
pub(crate) mod metal_compose;
#[cfg(all(feature = "metal", target_os = "macos"))]
mod metal_input;
#[cfg(all(feature = "metal", target_os = "macos"))]
mod metal_route;
#[cfg(all(feature = "metal", target_os = "macos"))]
mod metal_row_batch;
mod profiling;
#[cfg(all(feature = "metal", target_os = "macos"))]
mod route_cache;
mod route_plan;
mod tile_grid;
mod transaction;

pub(crate) use self::defaults::default_transfer_syntax_for_open_slide;
pub use self::defaults::default_transfer_syntax_for_source;
pub use self::profiling::{
    profile_corpus_route_coverage, profile_dicom_route_corpus_coverage,
    profile_dicom_route_coverage, profile_dicom_routes, profile_slide_route_coverage,
};

#[cfg(test)]
use self::corpus_discovery::collect_wsi_candidate_paths;

#[cfg(all(test, feature = "metal", target_os = "macos"))]
use self::metal_compose::{MetalComposeTileRequest, MetalStripComposer};
#[cfg(all(test, feature = "metal", target_os = "macos"))]
use self::metal_input::{
    cpu_input_device_encode_auto_allowed, cpu_input_device_encode_auto_probe_allowed,
    select_auto_lossless_j2k_probe_route, wsi_rs_device_decode_opted_in,
    AutoLosslessJ2kRouteCandidate, CpuEncodedTileRun,
};
#[cfg(all(test, feature = "metal", target_os = "macos"))]
use self::metal_input::{
    try_encode_metal_input_tile_run, MetalInputTileReader, MetalInputTileRunRequest,
};
#[cfg(all(test, feature = "metal", target_os = "macos"))]
use metal_route::whole_level_strip_layout;
#[cfg(all(test, feature = "metal", target_os = "macos"))]
use metal_row_batch::{try_encode_metal_whole_level_strip_run, WholeLevelStripLayout};
#[cfg(all(test, feature = "metal", target_os = "macos"))]
use route_cache::{
    cached_auto_metal_input_decision, clear_auto_metal_input_route_cache_for_tests,
    clear_auto_metal_input_route_cache_state_for_tests,
    flush_persistent_auto_metal_input_route_cache_to_path,
    load_persistent_auto_metal_input_route_cache_from_path, store_cached_auto_metal_input_decision,
    AutoLosslessJ2kRouteDecision, AutoMetalInputRouteCacheKey,
};
#[cfg(all(feature = "metal", target_os = "macos"))]
use route_cache::{
    flush_persistent_auto_metal_input_route_cache_if_requested,
    load_persistent_auto_metal_input_route_cache_if_requested,
};

pub(crate) use self::frame_region::FrameRectGrid;
#[cfg(test)]
use self::frame_region::OutputFrameRect;
#[cfg(test)]
use self::frame_region::PreparedCpuRegion;
use self::icc_profile::preflight_icc_profiles;
#[cfg(test)]
use self::j2k_policy::*;
#[cfg(test)]
use self::jobs::{
    default_export_instance_worker_count, metadata_frame_plan, DicomExportInstanceJob,
};
use self::jobs::{
    dicom_export_instance_jobs, export_dicom_instance_jobs, preflight_metadata_budgets,
    preflight_output_paths,
};
#[cfg(test)]
use self::jpeg_baseline::jpeg_baseline_frame_geometry;
pub(crate) use self::jpeg_baseline::{
    jpeg_baseline_route_frame_geometry, pixel_profile_from_raw_jpeg_tile,
    raw_jpeg_matches_frame_geometry, raw_jpeg_profile_can_passthrough,
    raw_rgb_passthrough_has_no_geometry_fallback, JpegBaselineFrameGeometry,
    JpegBaselineFrameLocation,
};
#[cfg(test)]
use self::jpeg_baseline::{JpegBaselineFallbackFrame, JpegBaselinePlannedFrame};
#[cfg(test)]
use self::jpeg_baseline_instance::export_jpeg_passthrough_instance;
#[cfg(all(test, feature = "metal", target_os = "macos"))]
use self::jpeg_baseline_metal::*;
#[cfg(test)]
use self::jpeg_baseline_pipeline::*;
pub(crate) use self::jpeg_passthrough::read_raw_jpeg_passthrough_tile;
#[cfg(test)]
use self::lossless_j2k_cpu::{
    encode_cpu_input_lossless_j2k_tile_batch, prepare_cpu_input_lossless_j2k_tile,
    LosslessJ2kCpuBatchFrame, LosslessJ2kCpuBatchSettings,
};
#[cfg(test)]
use self::lossless_j2k_direct_routes::*;
#[cfg(test)]
use self::lossless_j2k_plan::J2kPassthroughFrame;
pub(crate) use self::lossless_j2k_plan::{
    plan_lossless_j2k_frames, LosslessJ2kPlanRequest, LosslessJ2kPlannedFrame,
};
#[cfg(test)]
use self::profiling::{check_route_level_deadline, RouteLevelDeadline};
use self::transaction::{ExportTransaction, OutputDirectoryLock};

#[cfg(all(test, feature = "metal", target_os = "macos"))]
const WSI_RS_JPEG_DEVICE_DECODE_ENV: &str = "WSI_RS_JPEG_DEVICE_DECODE";

#[cfg(all(test, feature = "metal", target_os = "macos"))]
const WSI_RS_JP2K_DEVICE_DECODE_ENV: &str = "WSI_RS_JP2K_DEVICE_DECODE";

const DIRECT_JPEG_PASSTHROUGH_WRITE_CHUNK_FRAMES: usize = 2048;

#[derive(Clone, Copy)]
pub(super) struct InstanceExportContext<'a> {
    pub(super) options: &'a NormalizedExportOptions,
    pub(super) metadata: &'a DicomMetadata,
    pub(super) identity: &'a DicomExportIdentity,
    pub(super) instance_number: u32,
    pub(super) coordinate: InstanceCoordinate,
    pub(super) level: &'a wsi_rs::Level,
}

fn level_pixel_spacing_mm(
    slide: &Slide,
    level: &wsi_rs::Level,
    supplied: Option<crate::options::SourcePixelSpacingMm>,
) -> Result<Option<(f64, f64)>, Error> {
    let properties = &slide.dataset().properties;
    let source = properties
        .mpp()
        .map(|(mpp_x, mpp_y)| (mpp_y / 1000.0, mpp_x / 1000.0));
    let supplied = supplied.map(|spacing| (spacing.row, spacing.column));
    let base_spacing = match (source, supplied) {
        (Some(source), Some(supplied)) if !spacing_pairs_match(source, supplied) => {
            return Err(Error::Metadata {
                reason: format!(
                    "supplied source_pixel_spacing_mm row={} column={} conflicts with source pixel spacing row={} column={}",
                    supplied.0, supplied.1, source.0, source.1
                ),
            });
        }
        (Some(source), _) => source,
        (None, Some(supplied)) => supplied,
        (None, None) => return Ok(None),
    };
    let downsample = level.downsample;
    if !(base_spacing.0.is_finite() && base_spacing.1.is_finite() && downsample.is_finite()) {
        return Ok(None);
    }
    if base_spacing.0 <= 0.0 || base_spacing.1 <= 0.0 || downsample <= 0.0 {
        return Ok(None);
    }
    Ok(Some((
        base_spacing.0 * downsample,
        base_spacing.1 * downsample,
    )))
}

fn spacing_pairs_match(left: (f64, f64), right: (f64, f64)) -> bool {
    [left.0, left.1]
        .into_iter()
        .zip([right.0, right.1])
        .all(|(left, right)| {
            let scale = left.abs().max(right.abs()).max(f64::MIN_POSITIVE);
            (left - right).abs() <= scale * 1e-9
        })
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
    let options = validate_export_request(&request)?;
    let metadata = request.metadata.resolve()?;
    let slide = open_slide(&request.source_path)?;
    export_dicom_prepared(request, slide, metadata, options)
}

pub(crate) fn validate_export_request(
    request: &ExportRequest,
) -> Result<NormalizedExportOptions, Error> {
    request.validate()?;
    let options = NormalizedExportOptions::from_validated(&request.options);
    if options.semantics.transfer_syntax != TransferSyntax::JpegBaseline8Bit
        && !options.semantics.transfer_syntax.is_j2k_family()
    {
        return Err(Error::Unsupported {
            reason: "only JPEG Baseline passthrough, JPEG 2000, JPEG 2000 Lossless, and HTJ2K transfer syntaxes are implemented"
                .into(),
        });
    }
    Ok(options)
}

pub(crate) fn export_dicom_prepared(
    request: ExportRequest,
    slide: Slide,
    metadata: DicomMetadata,
    options: NormalizedExportOptions,
) -> Result<ExportReport, Error> {
    #[cfg(all(feature = "metal", target_os = "macos"))]
    load_persistent_auto_metal_input_route_cache_if_requested()?;
    let jobs = dicom_export_instance_jobs(&slide, &request)?;
    preflight_output_paths(&request, &options, &jobs)?;
    preflight_metadata_budgets(&slide, &options, &jobs)?;
    let effective_icc_digests = preflight_icc_profiles(&slide, &request, &metadata, &jobs)?;
    let identity = DicomExportIdentity::for_export(
        &request.source_path,
        &options,
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
    let mut staged_options = options;
    staged_options.semantics.overwrite = false;
    let mut instances = export_dicom_instance_jobs(
        &slide,
        &staged_request,
        &staged_options,
        &metadata,
        &identity,
        &jobs,
    )?;

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
    transaction.commit(&mut instances, options.semantics.overwrite)?;

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
        annotations: None,
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
        if slot.is_some() {
            return Err(Error::Unsupported {
                reason: format!("indexed batch result contains duplicate index {idx}"),
            });
        }
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
        write_tiled_aperio_jp2k_ycbcr_tiff, write_tiled_grayscale_jpeg_tiff,
        write_tiled_jp2k_rgb_tiff, write_tiled_jp2k_ycbcr_tiff, write_tiled_jpeg_tiff,
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
