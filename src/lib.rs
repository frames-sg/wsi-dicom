#![forbid(unsafe_code)]

#[cfg(all(feature = "metal", target_os = "macos"))]
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(all(feature = "metal", target_os = "macos"))]
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use dicom_object::FileMetaTableBuilder;
use rayon::prelude::*;
use serde::{ser::SerializeStruct, Serialize};
#[cfg(all(feature = "metal", target_os = "macos"))]
use signinum_core::PixelFormat as SigninumPixelFormat;
use signinum_core::{
    Colorspace, CompressedPayloadKind, CompressedTransferSyntax, PassthroughRequirements,
};
use signinum_j2k::{J2kLosslessSamples, J2kView};
use signinum_jpeg::{
    encode_jpeg_baseline, EncodedJpeg, JpegBackend, JpegEncodeOptions, JpegSamples, JpegSubsampling,
};
#[cfg(all(feature = "metal", target_os = "macos"))]
use signinum_jpeg_metal::{
    encode_jpeg_baseline_batch_from_metal_buffers, JpegBaselineMetalEncodeTile,
};
use statumen::{
    Compression, EncodedTilePhotometricInterpretation, LevelIdx, LevelSourceKind, PlaneIdx,
    PlaneSelection, RawCompressedTile, RegionRequest, SceneId, SeriesId, Slide, TileLayout,
    TileRequest,
};
#[cfg(all(feature = "metal", target_os = "macos"))]
use statumen::{DeviceTile, TileOutputPreference, TilePixels};

mod encode;
mod error;
mod metadata;
mod options;
mod tile;
mod uid;
mod writer;

pub use error::WsiDicomError;
pub use metadata::{DicomMetadata, MetadataSource};
pub use options::{CodecValidation, DicomExportOptions, EncodeBackendPreference, TransferSyntax};

use encode::{DicomJ2kEncoder, EncodedDicomJ2kFrame};
#[cfg(all(feature = "metal", target_os = "macos"))]
use tile::pixel_profile_from_device_format;
use tile::{optical_path_groups, prepare_tile_samples, PixelProfile};
use uid::{deterministic_instance_path, uid_from_seed};
use writer::{
    build_dicom_object, write_dicom_object_with_spooled_pixel_data, LossyCompressionMetadata,
    PixelDataSpool,
};

pub(crate) const VL_WSI_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.1.77.1.6";

type EncodedJpegBaselineFrame = (EncodedJpeg, PixelProfile, Duration, Duration, Duration);

#[cfg(all(feature = "metal", target_os = "macos"))]
const STATUMEN_JPEG_DEVICE_DECODE_ENV: &str = "STATUMEN_JPEG_DEVICE_DECODE";

#[cfg(all(feature = "metal", target_os = "macos"))]
const STATUMEN_JP2K_DEVICE_DECODE_ENV: &str = "STATUMEN_JP2K_DEVICE_DECODE";

#[cfg(all(feature = "metal", target_os = "macos"))]
const WSI_DICOM_AUTO_ROUTE_CACHE_ENV: &str = "WSI_DICOM_AUTO_ROUTE_CACHE";

#[cfg(all(feature = "metal", target_os = "macos"))]
const LOSSLESS_J2K_AUTO_ROUTE_PROBE_MAX_FRAMES: usize = 4;

#[cfg(all(feature = "metal", target_os = "macos"))]
const LOSSLESS_J2K_AUTO_ROUTE_MIN_FRAMES: u64 = 16;

#[cfg(all(feature = "metal", target_os = "macos"))]
const LOSSLESS_J2K_AUTO_PARTIAL_GPU_MIN_FRAMES: usize = 32;

#[cfg(all(feature = "metal", target_os = "macos"))]
const LOSSLESS_J2K_AUTO_ROUTE_SPEEDUP_NUMERATOR: u128 = 92;

#[cfg(all(feature = "metal", target_os = "macos"))]
const LOSSLESS_J2K_AUTO_ROUTE_SPEEDUP_DENOMINATOR: u128 = 100;

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum AutoLosslessJ2kRouteDecision {
    Undecided,
    CpuOnly,
    CpuInputDeviceEncode,
    GpuInputDeviceEncode,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AutoMetalInputRouteCacheKey {
    source_path: PathBuf,
    level: u32,
    tile_size: u32,
    transfer_syntax: TransferSyntax,
    route_scope_frames: u64,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
static AUTO_METAL_INPUT_ROUTE_CACHE: OnceLock<
    Mutex<HashMap<AutoMetalInputRouteCacheKey, AutoLosslessJ2kRouteDecision>>,
> = OnceLock::new();

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Default)]
struct AutoMetalInputRouteCacheState {
    loaded_path: Option<PathBuf>,
    dirty: bool,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
static AUTO_METAL_INPUT_ROUTE_CACHE_STATE: OnceLock<Mutex<AutoMetalInputRouteCacheState>> =
    OnceLock::new();

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
struct PersistentAutoMetalInputRouteCacheEntry {
    source_path: PathBuf,
    level: u32,
    tile_size: u32,
    transfer_syntax_uid: String,
    #[serde(default)]
    route_scope_frames: u64,
    #[serde(default)]
    route: Option<AutoLosslessJ2kRouteDecision>,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn auto_metal_input_route_cache(
) -> &'static Mutex<HashMap<AutoMetalInputRouteCacheKey, AutoLosslessJ2kRouteDecision>> {
    AUTO_METAL_INPUT_ROUTE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn auto_metal_input_route_cache_state() -> &'static Mutex<AutoMetalInputRouteCacheState> {
    AUTO_METAL_INPUT_ROUTE_CACHE_STATE
        .get_or_init(|| Mutex::new(AutoMetalInputRouteCacheState::default()))
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn cached_auto_metal_input_decision(
    key: &AutoMetalInputRouteCacheKey,
) -> Option<AutoLosslessJ2kRouteDecision> {
    auto_metal_input_route_cache()
        .lock()
        .expect("auto Metal input route cache mutex poisoned")
        .get(key)
        .copied()
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn store_cached_auto_metal_input_decision(
    key: &AutoMetalInputRouteCacheKey,
    route: AutoLosslessJ2kRouteDecision,
) {
    if route == AutoLosslessJ2kRouteDecision::Undecided {
        return;
    }
    auto_metal_input_route_cache()
        .lock()
        .expect("auto Metal input route cache mutex poisoned")
        .insert(key.clone(), route);
    auto_metal_input_route_cache_state()
        .lock()
        .expect("auto Metal input route cache state mutex poisoned")
        .dirty = true;
}

#[cfg(all(test, feature = "metal", target_os = "macos"))]
fn clear_auto_metal_input_route_cache_for_tests() {
    auto_metal_input_route_cache()
        .lock()
        .expect("auto Metal input route cache mutex poisoned")
        .clear();
}

#[cfg(all(test, feature = "metal", target_os = "macos"))]
fn clear_auto_metal_input_route_cache_state_for_tests() {
    *auto_metal_input_route_cache_state()
        .lock()
        .expect("auto Metal input route cache state mutex poisoned") =
        AutoMetalInputRouteCacheState::default();
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn persistent_auto_metal_input_route_cache_path() -> Option<PathBuf> {
    std::env::var_os(WSI_DICOM_AUTO_ROUTE_CACHE_ENV)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn load_persistent_auto_metal_input_route_cache_if_requested() -> Result<(), WsiDicomError> {
    let Some(path) = persistent_auto_metal_input_route_cache_path() else {
        return Ok(());
    };
    {
        let state = auto_metal_input_route_cache_state()
            .lock()
            .expect("auto Metal input route cache state mutex poisoned");
        if state.loaded_path.as_ref() == Some(&path) {
            return Ok(());
        }
    }

    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(source) => {
            return Err(WsiDicomError::Io { path, source });
        }
    };

    if !bytes.is_empty() {
        let entries: Vec<PersistentAutoMetalInputRouteCacheEntry> = serde_json::from_slice(&bytes)
            .map_err(|source| WsiDicomError::Json {
                path: path.clone(),
                source,
            })?;
        let mut cache = auto_metal_input_route_cache()
            .lock()
            .expect("auto Metal input route cache mutex poisoned");
        for entry in entries {
            let Some(route) = entry
                .route
                .filter(|route| *route != AutoLosslessJ2kRouteDecision::Undecided)
            else {
                continue;
            };
            let transfer_syntax =
                transfer_syntax_from_uid(&entry.transfer_syntax_uid).ok_or_else(|| {
                    WsiDicomError::Unsupported {
                        reason: format!(
                            "auto route cache {} contains unsupported transfer syntax UID {}",
                            path.display(),
                            entry.transfer_syntax_uid
                        ),
                    }
                })?;
            cache.insert(
                AutoMetalInputRouteCacheKey {
                    source_path: entry.source_path,
                    level: entry.level,
                    tile_size: entry.tile_size,
                    transfer_syntax,
                    route_scope_frames: entry.route_scope_frames,
                },
                route,
            );
        }
    }

    let mut state = auto_metal_input_route_cache_state()
        .lock()
        .expect("auto Metal input route cache state mutex poisoned");
    state.loaded_path = Some(path);
    state.dirty = false;
    Ok(())
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn flush_persistent_auto_metal_input_route_cache_if_requested() -> Result<(), WsiDicomError> {
    let Some(path) = persistent_auto_metal_input_route_cache_path() else {
        return Ok(());
    };
    {
        let state = auto_metal_input_route_cache_state()
            .lock()
            .expect("auto Metal input route cache state mutex poisoned");
        if !state.dirty && state.loaded_path.as_ref() == Some(&path) {
            return Ok(());
        }
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|source| WsiDicomError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }

    let mut entries: Vec<_> = auto_metal_input_route_cache()
        .lock()
        .expect("auto Metal input route cache mutex poisoned")
        .iter()
        .map(|(key, route)| PersistentAutoMetalInputRouteCacheEntry {
            source_path: key.source_path.clone(),
            level: key.level,
            tile_size: key.tile_size,
            transfer_syntax_uid: key.transfer_syntax.uid().to_string(),
            route_scope_frames: key.route_scope_frames,
            route: Some(*route),
        })
        .collect();
    entries.sort_by(|left, right| {
        left.source_path
            .cmp(&right.source_path)
            .then(left.level.cmp(&right.level))
            .then(left.tile_size.cmp(&right.tile_size))
            .then(left.transfer_syntax_uid.cmp(&right.transfer_syntax_uid))
            .then(left.route_scope_frames.cmp(&right.route_scope_frames))
    });
    let bytes =
        serde_json::to_vec_pretty(&entries).map_err(|source| WsiDicomError::JsonSerialize {
            message: format!("auto route cache serialization failed: {source}"),
        })?;
    fs::write(&path, bytes).map_err(|source| WsiDicomError::Io {
        path: path.clone(),
        source,
    })?;

    let mut state = auto_metal_input_route_cache_state()
        .lock()
        .expect("auto Metal input route cache state mutex poisoned");
    state.loaded_path = Some(path);
    state.dirty = false;
    Ok(())
}

/// A validated request to export one vendor WSI into one DICOM output directory.
#[derive(Debug, Clone, PartialEq)]
pub struct DicomExportRequest {
    pub source_path: PathBuf,
    pub output_dir: PathBuf,
    pub options: DicomExportOptions,
    pub metadata: MetadataSource,
    pub level_filter: Option<u32>,
}

impl DicomExportRequest {
    pub fn new(
        source_path: PathBuf,
        output_dir: PathBuf,
        options: DicomExportOptions,
    ) -> Result<Self, WsiDicomError> {
        options.validate()?;
        Ok(Self {
            source_path,
            output_dir,
            options,
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
    }

    pub fn validate(&self) -> Result<(), WsiDicomError> {
        self.options.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DicomExportReport {
    pub output_dir: PathBuf,
    pub instances: Vec<DicomInstanceReport>,
    pub metrics: DicomExportMetrics,
}

/// Request to encode one already-composed tile into DICOM-ready J2K/HTJ2K frame bytes.
#[derive(Debug, Clone, Copy)]
pub struct DicomJ2kFrameEncodeRequest<'a> {
    pub samples: J2kLosslessSamples<'a>,
    pub transfer_syntax: TransferSyntax,
    pub encode_backend: EncodeBackendPreference,
    pub codec_validation: CodecValidation,
}

/// Finished compressed frame bytes ready for DICOM encapsulated Pixel Data insertion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DicomEncodedFrame {
    pub transfer_syntax_uid: &'static str,
    pub bytes: Vec<u8>,
    pub used_device_encode: bool,
    pub used_device_validation: bool,
    pub encode_micros: u128,
    pub validation_micros: u128,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DicomRouteProfileRequest {
    pub source_path: PathBuf,
    pub options: DicomExportOptions,
    pub level: u32,
    pub max_frames: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DicomRouteProfileReport {
    pub source_path: PathBuf,
    pub transfer_syntax_uid: &'static str,
    pub level: u32,
    pub requested_frames: u64,
    pub available_frames: u64,
    pub metrics: DicomExportMetrics,
    pub elapsed_micros: u128,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DicomRouteCoverageRequest {
    pub source_path: PathBuf,
    pub options: DicomExportOptions,
    pub max_frames_per_level: u64,
    pub max_levels: Option<u32>,
    pub max_level_elapsed: Option<Duration>,
    pub progress: Option<DicomRouteCoverageProgress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DicomRouteCoverageProgress {
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DicomRouteCoverageReport {
    pub source_path: PathBuf,
    pub transfer_syntax_uid: &'static str,
    pub requested_frames_per_level: u64,
    pub available_frames: u64,
    pub complete_frame_coverage: bool,
    pub levels: Vec<DicomRouteProfileReport>,
    pub metrics: DicomExportMetrics,
    pub elapsed_micros: u128,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DicomRouteCorpusCoverageRequest {
    pub source_root: PathBuf,
    pub options: DicomExportOptions,
    pub max_frames_per_level: u64,
    pub max_levels: Option<u32>,
    pub max_level_elapsed: Option<Duration>,
    pub progress: Option<DicomRouteCorpusCoverageProgress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DicomRouteCorpusCoverageProgress {
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DicomRouteCorpusCoverageFailure {
    pub source_path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DicomRouteCorpusCoverageReport {
    pub source_root: PathBuf,
    pub transfer_syntax_uid: &'static str,
    pub requested_frames_per_level: u64,
    pub max_levels: Option<u32>,
    pub sources_considered: usize,
    pub available_frames: u64,
    pub complete_frame_coverage: bool,
    pub reports: Vec<DicomRouteCoverageReport>,
    pub failures: Vec<DicomRouteCorpusCoverageFailure>,
    pub metrics: DicomExportMetrics,
    pub elapsed_micros: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DicomInstanceReport {
    pub path: PathBuf,
    pub sop_instance_uid: String,
    pub series_instance_uid: String,
    pub transfer_syntax_uid: &'static str,
    pub level: u32,
    pub z: u32,
    pub c: u32,
    pub t: u32,
    pub frame_count: u32,
    pub metrics: DicomExportMetrics,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DicomExportMetrics {
    pub total_frames: u64,
    pub cpu_input_frames: u64,
    pub gpu_input_decode_frames: u64,
    pub gpu_encode_frames: u64,
    pub gpu_validation_frames: u64,
    pub gray_frames: u64,
    pub rgb_like_frames: u64,
    pub other_component_frames: u64,
    pub unknown_pixel_profile_frames: u64,
    pub bits8_frames: u64,
    pub bits16_frames: u64,
    pub other_bit_depth_frames: u64,
    pub gpu_transcode_frames: u64,
    pub resident_gpu_transcode_frames: u64,
    pub partial_gpu_transcode_frames: u64,
    pub gpu_input_decode_batches: u64,
    pub gpu_compose_batches: u64,
    pub gpu_encode_batches: u64,
    pub auto_route_probe_frames: u64,
    pub auto_route_probe_gpu_batches: u64,
    pub auto_route_probe_cpu_micros: u128,
    pub auto_route_probe_gpu_micros: u128,
    pub auto_route_probe_selected_gpu_input_frames: u64,
    pub cpu_fallback_frames: u64,
    pub jpeg_passthrough_frames: u64,
    pub j2k_passthrough_frames: u64,
    pub jpeg_cpu_encode_frames: u64,
    pub jpeg_metal_encode_frames: u64,
    pub jpeg_decode_fallback_frames: u64,
    pub input_decode_micros: u128,
    pub compose_micros: u128,
    pub encode_micros: u128,
    pub validation_micros: u128,
    pub gpu_dispatch_micros: u128,
    pub gpu_encode_configured_inflight_tiles: u64,
    pub gpu_encode_effective_inflight_tiles: u64,
    pub gpu_encode_max_observed_inflight_tiles: u64,
    pub gpu_encode_configured_memory_mib: u64,
    pub gpu_encode_effective_memory_mib: u64,
    pub gpu_encode_wall_micros: u128,
    pub gpu_encode_hardware_micros: u128,
    pub gpu_encode_dispatch_overhead_micros: u128,
    pub write_micros: u128,
}

impl Serialize for DicomExportMetrics {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("DicomExportMetrics", 43)?;
        state.serialize_field("total_frames", &self.total_frames)?;
        state.serialize_field("cpu_input_frames", &self.cpu_input_frames)?;
        state.serialize_field("gpu_input_decode_frames", &self.gpu_input_decode_frames)?;
        state.serialize_field("gpu_encode_frames", &self.gpu_encode_frames)?;
        state.serialize_field("gpu_validation_frames", &self.gpu_validation_frames)?;
        state.serialize_field("gray_frames", &self.gray_frames)?;
        state.serialize_field("rgb_like_frames", &self.rgb_like_frames)?;
        state.serialize_field("other_component_frames", &self.other_component_frames)?;
        state.serialize_field(
            "unknown_pixel_profile_frames",
            &self.unknown_pixel_profile_frames,
        )?;
        state.serialize_field("bits8_frames", &self.bits8_frames)?;
        state.serialize_field("bits16_frames", &self.bits16_frames)?;
        state.serialize_field("other_bit_depth_frames", &self.other_bit_depth_frames)?;
        state.serialize_field("gpu_transcode_frames", &self.gpu_transcode_frames)?;
        state.serialize_field(
            "resident_gpu_transcode_frames",
            &self.resident_gpu_transcode_frames,
        )?;
        state.serialize_field(
            "partial_gpu_transcode_frames",
            &self.partial_gpu_transcode_frames,
        )?;
        state.serialize_field("gpu_input_decode_batches", &self.gpu_input_decode_batches)?;
        state.serialize_field("gpu_compose_batches", &self.gpu_compose_batches)?;
        state.serialize_field("gpu_encode_batches", &self.gpu_encode_batches)?;
        state.serialize_field("auto_route_probe_frames", &self.auto_route_probe_frames)?;
        state.serialize_field(
            "auto_route_probe_gpu_batches",
            &self.auto_route_probe_gpu_batches,
        )?;
        state.serialize_field(
            "auto_route_probe_cpu_micros",
            &self.auto_route_probe_cpu_micros,
        )?;
        state.serialize_field(
            "auto_route_probe_gpu_micros",
            &self.auto_route_probe_gpu_micros,
        )?;
        state.serialize_field(
            "auto_route_probe_selected_gpu_input_frames",
            &self.auto_route_probe_selected_gpu_input_frames,
        )?;
        state.serialize_field("cpu_fallback_frames", &self.cpu_fallback_frames)?;
        state.serialize_field("jpeg_passthrough_frames", &self.jpeg_passthrough_frames)?;
        state.serialize_field("j2k_passthrough_frames", &self.j2k_passthrough_frames)?;
        state.serialize_field("jpeg_cpu_encode_frames", &self.jpeg_cpu_encode_frames)?;
        state.serialize_field("jpeg_metal_encode_frames", &self.jpeg_metal_encode_frames)?;
        state.serialize_field(
            "jpeg_decode_fallback_frames",
            &self.jpeg_decode_fallback_frames,
        )?;
        state.serialize_field("input_decode_micros", &self.input_decode_micros)?;
        state.serialize_field("compose_micros", &self.compose_micros)?;
        state.serialize_field("encode_micros", &self.encode_micros)?;
        state.serialize_field("validation_micros", &self.validation_micros)?;
        state.serialize_field("gpu_dispatch_micros", &self.gpu_dispatch_micros)?;
        state.serialize_field(
            "gpu_encode_configured_inflight_tiles",
            &self.gpu_encode_configured_inflight_tiles,
        )?;
        state.serialize_field(
            "gpu_encode_effective_inflight_tiles",
            &self.gpu_encode_effective_inflight_tiles,
        )?;
        state.serialize_field(
            "gpu_encode_max_observed_inflight_tiles",
            &self.gpu_encode_max_observed_inflight_tiles,
        )?;
        state.serialize_field(
            "gpu_encode_configured_memory_mib",
            &self.gpu_encode_configured_memory_mib,
        )?;
        state.serialize_field(
            "gpu_encode_effective_memory_mib",
            &self.gpu_encode_effective_memory_mib,
        )?;
        state.serialize_field("gpu_encode_wall_micros", &self.gpu_encode_wall_micros)?;
        state.serialize_field(
            "gpu_encode_effective_parallelism",
            &self.gpu_encode_effective_parallelism(),
        )?;
        state.serialize_field(
            "gpu_encode_hardware_micros",
            &self.gpu_encode_hardware_micros,
        )?;
        state.serialize_field(
            "gpu_encode_dispatch_overhead_micros",
            &self.gpu_encode_dispatch_overhead_micros,
        )?;
        state.serialize_field("write_micros", &self.write_micros)?;
        state.end()
    }
}

impl DicomExportMetrics {
    pub fn route_passthrough_frames(&self) -> u64 {
        self.jpeg_passthrough_frames
            .saturating_add(self.j2k_passthrough_frames)
    }

    pub fn route_unclassified_frames(&self) -> u64 {
        self.total_frames
            .saturating_sub(self.route_passthrough_frames())
            .saturating_sub(self.gpu_transcode_frames)
            .saturating_sub(self.cpu_fallback_frames)
    }

    fn add_assign(&mut self, other: Self) {
        self.total_frames = self.total_frames.saturating_add(other.total_frames);
        self.cpu_input_frames = self.cpu_input_frames.saturating_add(other.cpu_input_frames);
        self.gpu_input_decode_frames = self
            .gpu_input_decode_frames
            .saturating_add(other.gpu_input_decode_frames);
        self.gpu_encode_frames = self
            .gpu_encode_frames
            .saturating_add(other.gpu_encode_frames);
        self.gpu_validation_frames = self
            .gpu_validation_frames
            .saturating_add(other.gpu_validation_frames);
        self.gray_frames = self.gray_frames.saturating_add(other.gray_frames);
        self.rgb_like_frames = self.rgb_like_frames.saturating_add(other.rgb_like_frames);
        self.other_component_frames = self
            .other_component_frames
            .saturating_add(other.other_component_frames);
        self.unknown_pixel_profile_frames = self
            .unknown_pixel_profile_frames
            .saturating_add(other.unknown_pixel_profile_frames);
        self.bits8_frames = self.bits8_frames.saturating_add(other.bits8_frames);
        self.bits16_frames = self.bits16_frames.saturating_add(other.bits16_frames);
        self.other_bit_depth_frames = self
            .other_bit_depth_frames
            .saturating_add(other.other_bit_depth_frames);
        self.gpu_transcode_frames = self
            .gpu_transcode_frames
            .saturating_add(other.gpu_transcode_frames);
        self.resident_gpu_transcode_frames = self
            .resident_gpu_transcode_frames
            .saturating_add(other.resident_gpu_transcode_frames);
        self.partial_gpu_transcode_frames = self
            .partial_gpu_transcode_frames
            .saturating_add(other.partial_gpu_transcode_frames);
        self.gpu_input_decode_batches = self
            .gpu_input_decode_batches
            .saturating_add(other.gpu_input_decode_batches);
        self.gpu_compose_batches = self
            .gpu_compose_batches
            .saturating_add(other.gpu_compose_batches);
        self.gpu_encode_batches = self
            .gpu_encode_batches
            .saturating_add(other.gpu_encode_batches);
        self.auto_route_probe_frames = self
            .auto_route_probe_frames
            .saturating_add(other.auto_route_probe_frames);
        self.auto_route_probe_gpu_batches = self
            .auto_route_probe_gpu_batches
            .saturating_add(other.auto_route_probe_gpu_batches);
        self.auto_route_probe_cpu_micros = self
            .auto_route_probe_cpu_micros
            .saturating_add(other.auto_route_probe_cpu_micros);
        self.auto_route_probe_gpu_micros = self
            .auto_route_probe_gpu_micros
            .saturating_add(other.auto_route_probe_gpu_micros);
        self.auto_route_probe_selected_gpu_input_frames = self
            .auto_route_probe_selected_gpu_input_frames
            .saturating_add(other.auto_route_probe_selected_gpu_input_frames);
        self.cpu_fallback_frames = self
            .cpu_fallback_frames
            .saturating_add(other.cpu_fallback_frames);
        self.jpeg_passthrough_frames = self
            .jpeg_passthrough_frames
            .saturating_add(other.jpeg_passthrough_frames);
        self.j2k_passthrough_frames = self
            .j2k_passthrough_frames
            .saturating_add(other.j2k_passthrough_frames);
        self.jpeg_cpu_encode_frames = self
            .jpeg_cpu_encode_frames
            .saturating_add(other.jpeg_cpu_encode_frames);
        self.jpeg_metal_encode_frames = self
            .jpeg_metal_encode_frames
            .saturating_add(other.jpeg_metal_encode_frames);
        self.jpeg_decode_fallback_frames = self
            .jpeg_decode_fallback_frames
            .saturating_add(other.jpeg_decode_fallback_frames);
        self.input_decode_micros = self
            .input_decode_micros
            .saturating_add(other.input_decode_micros);
        self.compose_micros = self.compose_micros.saturating_add(other.compose_micros);
        self.encode_micros = self.encode_micros.saturating_add(other.encode_micros);
        self.validation_micros = self
            .validation_micros
            .saturating_add(other.validation_micros);
        self.gpu_dispatch_micros = self
            .gpu_dispatch_micros
            .saturating_add(other.gpu_dispatch_micros);
        self.gpu_encode_configured_inflight_tiles = self
            .gpu_encode_configured_inflight_tiles
            .max(other.gpu_encode_configured_inflight_tiles);
        self.gpu_encode_effective_inflight_tiles = self
            .gpu_encode_effective_inflight_tiles
            .max(other.gpu_encode_effective_inflight_tiles);
        self.gpu_encode_max_observed_inflight_tiles = self
            .gpu_encode_max_observed_inflight_tiles
            .max(other.gpu_encode_max_observed_inflight_tiles);
        self.gpu_encode_configured_memory_mib = self
            .gpu_encode_configured_memory_mib
            .max(other.gpu_encode_configured_memory_mib);
        self.gpu_encode_effective_memory_mib = self
            .gpu_encode_effective_memory_mib
            .max(other.gpu_encode_effective_memory_mib);
        self.gpu_encode_wall_micros = self
            .gpu_encode_wall_micros
            .saturating_add(other.gpu_encode_wall_micros);
        self.gpu_encode_hardware_micros = self
            .gpu_encode_hardware_micros
            .saturating_add(other.gpu_encode_hardware_micros);
        self.gpu_encode_dispatch_overhead_micros = self
            .gpu_encode_dispatch_overhead_micros
            .saturating_add(other.gpu_encode_dispatch_overhead_micros);
        self.write_micros = self.write_micros.saturating_add(other.write_micros);
    }

    fn record_cpu_input(&mut self) {
        self.total_frames = self.total_frames.saturating_add(1);
        self.cpu_input_frames = self.cpu_input_frames.saturating_add(1);
    }

    fn record_gpu_input(&mut self) {
        self.total_frames = self.total_frames.saturating_add(1);
        self.gpu_input_decode_frames = self.gpu_input_decode_frames.saturating_add(1);
    }

    fn record_passthrough_frame(&mut self) {
        self.total_frames = self.total_frames.saturating_add(1);
        self.jpeg_passthrough_frames = self.jpeg_passthrough_frames.saturating_add(1);
    }

    fn record_j2k_passthrough_frame(&mut self) {
        self.total_frames = self.total_frames.saturating_add(1);
        self.j2k_passthrough_frames = self.j2k_passthrough_frames.saturating_add(1);
    }

    fn record_pixel_profile(&mut self, profile: PixelProfile) {
        match profile.components {
            1 => self.gray_frames = self.gray_frames.saturating_add(1),
            3 => self.rgb_like_frames = self.rgb_like_frames.saturating_add(1),
            _ => {
                self.other_component_frames = self.other_component_frames.saturating_add(1);
            }
        }
        match profile.bits_allocated {
            8 => self.bits8_frames = self.bits8_frames.saturating_add(1),
            16 => self.bits16_frames = self.bits16_frames.saturating_add(1),
            _ => self.other_bit_depth_frames = self.other_bit_depth_frames.saturating_add(1),
        }
    }

    fn record_unknown_pixel_profile(&mut self) {
        self.unknown_pixel_profile_frames = self.unknown_pixel_profile_frames.saturating_add(1);
    }

    fn record_transcode_route(&mut self, used_gpu_input: bool, used_gpu_encode: bool) {
        if used_gpu_input || used_gpu_encode {
            self.gpu_transcode_frames = self.gpu_transcode_frames.saturating_add(1);
            if used_gpu_input && used_gpu_encode {
                self.resident_gpu_transcode_frames =
                    self.resident_gpu_transcode_frames.saturating_add(1);
            } else {
                self.partial_gpu_transcode_frames =
                    self.partial_gpu_transcode_frames.saturating_add(1);
            }
        } else {
            self.cpu_fallback_frames = self.cpu_fallback_frames.saturating_add(1);
        }
    }

    fn record_gpu_batches(
        &mut self,
        input_decode_batches: u64,
        compose_batches: u64,
        encode_batches: u64,
    ) {
        self.gpu_input_decode_batches = self
            .gpu_input_decode_batches
            .saturating_add(input_decode_batches);
        self.gpu_compose_batches = self.gpu_compose_batches.saturating_add(compose_batches);
        self.gpu_encode_batches = self.gpu_encode_batches.saturating_add(encode_batches);
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    fn record_gpu_encode_batch_stats(&mut self, stats: encode::DicomJ2kGpuEncodeBatchStats) {
        self.gpu_encode_configured_inflight_tiles = self
            .gpu_encode_configured_inflight_tiles
            .max(stats.configured_inflight_tiles.unwrap_or(0) as u64);
        self.gpu_encode_effective_inflight_tiles = self
            .gpu_encode_effective_inflight_tiles
            .max(stats.effective_inflight_tiles as u64);
        self.gpu_encode_max_observed_inflight_tiles = self
            .gpu_encode_max_observed_inflight_tiles
            .max(stats.max_observed_inflight_tiles as u64);
        self.gpu_encode_configured_memory_mib = self
            .gpu_encode_configured_memory_mib
            .max(stats.configured_memory_mib.unwrap_or(0));
        self.gpu_encode_effective_memory_mib = self
            .gpu_encode_effective_memory_mib
            .max(stats.effective_memory_mib);
        self.gpu_encode_wall_micros = self
            .gpu_encode_wall_micros
            .saturating_add(duration_as_reported_micros(stats.encode_wall_duration));
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    fn record_auto_route_probe(
        &mut self,
        frames: u64,
        cpu_duration: Duration,
        gpu_duration: Duration,
        gpu_batches: u64,
        selected_gpu_input: bool,
    ) {
        self.auto_route_probe_frames = self.auto_route_probe_frames.saturating_add(frames);
        self.auto_route_probe_gpu_batches = self
            .auto_route_probe_gpu_batches
            .saturating_add(gpu_batches);
        self.auto_route_probe_cpu_micros = self
            .auto_route_probe_cpu_micros
            .saturating_add(duration_as_reported_micros(cpu_duration));
        self.auto_route_probe_gpu_micros = self
            .auto_route_probe_gpu_micros
            .saturating_add(duration_as_reported_micros(gpu_duration));
        if selected_gpu_input {
            self.auto_route_probe_selected_gpu_input_frames = self
                .auto_route_probe_selected_gpu_input_frames
                .saturating_add(frames);
        }
    }

    fn record_jpeg_decode_fallback(&mut self) {
        self.jpeg_decode_fallback_frames = self.jpeg_decode_fallback_frames.saturating_add(1);
    }

    fn record_jpeg_cpu_fallback_route_classification(&mut self) {
        self.total_frames = self.total_frames.saturating_add(1);
        self.cpu_fallback_frames = self.cpu_fallback_frames.saturating_add(1);
        self.jpeg_decode_fallback_frames = self.jpeg_decode_fallback_frames.saturating_add(1);
        self.record_unknown_pixel_profile();
    }

    fn record_j2k_passthrough_only_fallback_classification(&mut self) {
        self.total_frames = self.total_frames.saturating_add(1);
        self.cpu_fallback_frames = self.cpu_fallback_frames.saturating_add(1);
        self.record_unknown_pixel_profile();
    }

    fn record_jpeg_cpu_encode(&mut self, duration: Duration) {
        self.jpeg_cpu_encode_frames = self.jpeg_cpu_encode_frames.saturating_add(1);
        self.record_encode_duration(duration);
    }

    fn record_jpeg_metal_batch_encode(&mut self, frames: u64, duration: Duration) {
        self.jpeg_metal_encode_frames = self.jpeg_metal_encode_frames.saturating_add(frames);
        self.record_encode_duration(duration);
        self.record_gpu_dispatch_duration(duration);
    }

    fn record_encoded_frame(&mut self, encoded: &encode::EncodedDicomJ2kFrame) {
        if encoded.used_device_encode {
            self.gpu_encode_frames = self.gpu_encode_frames.saturating_add(1);
            self.record_gpu_dispatch_duration(encoded.encode_duration);
            self.record_gpu_encode_hardware_duration(
                encoded.device_gpu_duration,
                encoded.encode_duration,
            );
        }
        if encoded.used_device_validation {
            self.gpu_validation_frames = self.gpu_validation_frames.saturating_add(1);
            self.record_gpu_dispatch_duration(encoded.validation_duration);
        }
        self.record_encode_duration(encoded.encode_duration);
        self.record_validation_duration(encoded.validation_duration);
    }

    fn record_input_decode_duration(&mut self, duration: Duration) {
        self.input_decode_micros = self
            .input_decode_micros
            .saturating_add(duration_as_reported_micros(duration));
    }

    fn record_gpu_input_decode_duration(&mut self, duration: Duration) {
        self.record_input_decode_duration(duration);
        self.record_gpu_dispatch_duration(duration);
    }

    fn record_compose_duration(&mut self, duration: Duration) {
        self.compose_micros = self
            .compose_micros
            .saturating_add(duration_as_reported_micros(duration));
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    fn record_gpu_compose_duration(&mut self, duration: Duration) {
        self.record_compose_duration(duration);
        self.record_gpu_dispatch_duration(duration);
    }

    fn record_encode_duration(&mut self, duration: Duration) {
        self.encode_micros = self
            .encode_micros
            .saturating_add(duration_as_reported_micros(duration));
    }

    fn record_validation_duration(&mut self, duration: Duration) {
        self.validation_micros = self
            .validation_micros
            .saturating_add(duration_as_reported_micros(duration));
    }

    fn record_gpu_dispatch_duration(&mut self, duration: Duration) {
        self.gpu_dispatch_micros = self
            .gpu_dispatch_micros
            .saturating_add(duration_as_reported_micros(duration));
    }

    fn record_gpu_encode_hardware_duration(
        &mut self,
        gpu_duration: Option<Duration>,
        dispatch_duration: Duration,
    ) {
        let Some(gpu_duration) = gpu_duration else {
            return;
        };
        let hardware_micros = duration_as_reported_micros(gpu_duration);
        self.gpu_encode_hardware_micros = self
            .gpu_encode_hardware_micros
            .saturating_add(hardware_micros);
        let overhead = dispatch_duration.saturating_sub(gpu_duration);
        self.gpu_encode_dispatch_overhead_micros = self
            .gpu_encode_dispatch_overhead_micros
            .saturating_add(duration_as_reported_micros(overhead));
    }

    fn record_write_duration(&mut self, duration: Duration) {
        self.write_micros = self
            .write_micros
            .saturating_add(duration_as_reported_micros(duration));
    }

    pub fn gpu_encode_effective_parallelism(&self) -> f64 {
        if self.gpu_encode_wall_micros == 0 {
            0.0
        } else {
            self.gpu_encode_hardware_micros as f64 / self.gpu_encode_wall_micros as f64
        }
    }
}

fn duration_as_reported_micros(duration: Duration) -> u128 {
    match duration.as_micros() {
        0 if duration > Duration::ZERO => 1,
        micros => micros,
    }
}

#[derive(Clone, Copy)]
struct RouteLevelDeadline {
    started: Instant,
    max_elapsed: Duration,
}

impl RouteLevelDeadline {
    fn new(max_elapsed: Option<Duration>) -> Option<Self> {
        max_elapsed.map(|max_elapsed| Self {
            started: Instant::now(),
            max_elapsed,
        })
    }
}

fn validate_max_level_elapsed(
    max_level_elapsed: Option<Duration>,
    context: &str,
) -> Result<(), WsiDicomError> {
    if max_level_elapsed == Some(Duration::ZERO) {
        return Err(WsiDicomError::Unsupported {
            reason: format!("{context} requires max_level_elapsed > 0 when provided"),
        });
    }
    Ok(())
}

fn check_route_level_deadline(
    deadline: Option<RouteLevelDeadline>,
    level_idx: u32,
) -> Result<(), WsiDicomError> {
    let Some(deadline) = deadline else {
        return Ok(());
    };
    let elapsed = deadline.started.elapsed();
    if elapsed > deadline.max_elapsed {
        return Err(WsiDicomError::Unsupported {
            reason: format!(
                "route coverage level {level_idx} timed out after {:.3} ms (max_level_elapsed {:.3} ms)",
                duration_as_reported_micros(elapsed) as f64 / 1000.0,
                duration_as_reported_micros(deadline.max_elapsed) as f64 / 1000.0
            ),
        });
    }
    Ok(())
}

fn pyramid_label(scene_idx: usize, series_idx: usize, z: u32, c: u32, t: u32) -> String {
    format!("WSI pyramid s{scene_idx} ser{series_idx} z{z} c{c} t{t}")
}

fn level_pixel_spacing_mm(slide: &Slide, level: &statumen::Level) -> Option<(f64, f64)> {
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

fn require_pixel_spacing_mm(
    pixel_spacing_mm: Option<(f64, f64)>,
) -> Result<(f64, f64), WsiDicomError> {
    pixel_spacing_mm.ok_or_else(|| WsiDicomError::Metadata {
        reason: "VL WSI VOLUME export requires pixel spacing metadata".into(),
    })
}

fn route_profile_available_frames(
    slide: &Slide,
    options: &DicomExportOptions,
    level: &statumen::Level,
    location: JpegBaselineFrameLocation,
) -> Result<u64, WsiDicomError> {
    if options.transfer_syntax == TransferSyntax::JpegBaseline8Bit {
        let geometry =
            jpeg_baseline_route_frame_geometry(slide, level, location, options.tile_size)?;
        return geometry
            .tiles_across
            .checked_mul(geometry.tiles_down)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "route profile JPEG frame count overflow".into(),
            });
    }
    let (matrix_columns, matrix_rows) = level.dimensions;
    let tile_size = j2k_route_tile_size(options, level)?;
    matrix_columns
        .div_ceil(u64::from(tile_size))
        .checked_mul(matrix_rows.div_ceil(u64::from(tile_size)))
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "route profile frame count overflow".into(),
        })
}

fn j2k_route_tile_size(
    options: &DicomExportOptions,
    level: &statumen::Level,
) -> Result<u32, WsiDicomError> {
    if options.transfer_syntax.is_jpeg2000_passthrough_only() {
        let native_square = match level.tile_layout {
            TileLayout::Regular {
                tile_width,
                tile_height,
                ..
            }
            | TileLayout::WholeLevel {
                virtual_tile_width: tile_width,
                virtual_tile_height: tile_height,
                ..
            } if tile_width == tile_height && tile_width > 0 => Some(tile_width),
            TileLayout::Regular { .. }
            | TileLayout::WholeLevel { .. }
            | TileLayout::Irregular { .. } => None,
        };
        if let Some(tile_size) = native_square {
            return Ok(tile_size);
        }
    }
    if options.tile_size == 0 {
        return Err(WsiDicomError::InvalidOptions {
            reason: "tile_size must be greater than zero".into(),
        });
    }
    Ok(options.tile_size)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn transfer_syntax_from_uid(uid: &str) -> Option<TransferSyntax> {
    [
        TransferSyntax::JpegBaseline8Bit,
        TransferSyntax::Jpeg2000,
        TransferSyntax::Jpeg2000Lossless,
        TransferSyntax::Htj2kLossless,
        TransferSyntax::Htj2kLosslessRpcl,
        TransferSyntax::ExplicitVrLittleEndian,
    ]
    .into_iter()
    .find(|transfer_syntax| transfer_syntax.uid() == uid)
}

fn j2k_passthrough_frame(
    raw: RawCompressedTile,
    frame_columns: u32,
    frame_rows: u32,
    transfer_syntax: TransferSyntax,
) -> Result<Option<J2kPassthroughFrame>, WsiDicomError> {
    if raw.width != frame_columns || raw.height != frame_rows {
        return Ok(None);
    }
    if !matches!(
        raw.compression,
        Compression::Jp2kRgb | Compression::Jp2kYcbcr
    ) {
        return Ok(None);
    }
    if raw.bits_allocated > u8::MAX as u16 || raw.samples_per_pixel > u8::MAX as u16 {
        return Ok(None);
    }
    let (passthrough_syntax, photometric_interpretation) = {
        let view = match J2kView::parse(&raw.data) {
            Ok(view) => view,
            Err(_) => return Ok(None),
        };
        if transfer_syntax == TransferSyntax::Htj2kLosslessRpcl
            && !j2k_codestream_is_rpcl(&raw.data)
        {
            return Ok(None);
        }
        let Some(candidate) = view.passthrough_candidate() else {
            return Ok(None);
        };
        let candidate_syntax = candidate.transfer_syntax();
        let Some(source_syntax) = required_passthrough_syntax(transfer_syntax, candidate_syntax)
        else {
            return Ok(None);
        };
        let Some(photometric_interpretation) =
            j2k_passthrough_photometric_interpretation(raw.photometric_interpretation, view.info())
        else {
            return Ok(None);
        };
        let requirements =
            PassthroughRequirements::new(source_syntax, CompressedPayloadKind::Jpeg2000Codestream)
                .with_dimensions((frame_columns, frame_rows))
                .with_components(raw.samples_per_pixel as u8)
                .with_bit_depth(raw.bits_allocated as u8);
        if candidate.copy_bytes_if_eligible(&requirements).is_err() {
            return Ok(None);
        }
        (candidate_syntax, photometric_interpretation)
    };

    Ok(Some(J2kPassthroughFrame {
        codestream: raw.data,
        profile: PixelProfile {
            components: raw.samples_per_pixel as u8,
            bits_allocated: raw.bits_allocated,
            photometric_interpretation,
        },
        transfer_syntax: passthrough_syntax,
    }))
}

fn j2k_passthrough_photometric_interpretation(
    raw_photometric: EncodedTilePhotometricInterpretation,
    info: &signinum_core::Info,
) -> Option<&'static str> {
    match (info.components, raw_photometric) {
        (1, EncodedTilePhotometricInterpretation::Monochrome2) => Some("MONOCHROME2"),
        (3, EncodedTilePhotometricInterpretation::Rgb) => Some("RGB"),
        (3, EncodedTilePhotometricInterpretation::YbrFull422) => match info.colorspace {
            Colorspace::Rct => Some("YBR_RCT"),
            Colorspace::Ict => Some("YBR_ICT"),
            Colorspace::YCbCr => Some("YBR_FULL_422"),
            Colorspace::Rgb | Colorspace::SRgb => Some("RGB"),
            _ => None,
        },
        _ => None,
    }
}

fn j2k_codestream_is_rpcl(data: &[u8]) -> bool {
    const MARKER_SOC: u16 = 0xFF4F;
    const MARKER_COD: u16 = 0xFF52;
    const MARKER_SOT: u16 = 0xFF90;
    const MARKER_SOD: u16 = 0xFF93;
    const MARKER_EOC: u16 = 0xFFD9;
    const PROGRESSION_RPCL: u8 = 2;

    let mut offset = 0usize;
    if read_be_u16(data, offset) == Some(MARKER_SOC) {
        offset += 2;
    }
    while offset + 4 <= data.len() {
        while offset < data.len() && data[offset] == 0xFF {
            offset += 1;
        }
        if offset >= data.len() {
            return false;
        }
        let marker = 0xFF00 | u16::from(data[offset]);
        offset += 1;
        match marker {
            MARKER_COD => {
                let Some(length) = read_be_u16(data, offset).map(usize::from) else {
                    return false;
                };
                if length < 4 || offset + length > data.len() {
                    return false;
                }
                let payload = &data[offset + 2..offset + length];
                return payload.get(1) == Some(&PROGRESSION_RPCL);
            }
            MARKER_SOT | MARKER_SOD | MARKER_EOC => return false,
            _ => {
                let Some(length) = read_be_u16(data, offset).map(usize::from) else {
                    return false;
                };
                if length < 2 || offset + length > data.len() {
                    return false;
                }
                offset += length;
            }
        }
    }
    false
}

fn read_be_u16(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn source_path_has_extension(path: &Path, ext: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(ext))
}

fn j2k_family_passthrough_probe_allowed(
    source_path: &Path,
    transfer_syntax: TransferSyntax,
) -> bool {
    match transfer_syntax {
        TransferSyntax::Jpeg2000 | TransferSyntax::Jpeg2000Lossless => true,
        TransferSyntax::Htj2kLossless | TransferSyntax::Htj2kLosslessRpcl => {
            source_path_has_extension(source_path, "dcm")
        }
        _ => false,
    }
}

fn level_is_synthetic_downsample(
    slide: &Slide,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
) -> Result<bool, WsiDicomError> {
    slide
        .level_source_kind(scene_idx, series_idx, level_idx)
        .map(|kind| kind == LevelSourceKind::SyntheticDownsample)
        .map_err(|err| WsiDicomError::SlideRead {
            message: format!("failed to inspect level source kind: {err}"),
        })
}

fn required_passthrough_syntax(
    transfer_syntax: TransferSyntax,
    candidate_syntax: CompressedTransferSyntax,
) -> Option<CompressedTransferSyntax> {
    match transfer_syntax {
        TransferSyntax::Jpeg2000 => match candidate_syntax {
            CompressedTransferSyntax::Jpeg2000Lossless
            | CompressedTransferSyntax::Jpeg2000Lossy => Some(candidate_syntax),
            _ => None,
        },
        TransferSyntax::Jpeg2000Lossless => Some(CompressedTransferSyntax::Jpeg2000Lossless),
        TransferSyntax::Htj2kLossless => Some(CompressedTransferSyntax::HtJpeg2000Lossless),
        TransferSyntax::Htj2kLosslessRpcl => Some(CompressedTransferSyntax::HtJpeg2000Lossless),
        TransferSyntax::JpegBaseline8Bit | TransferSyntax::ExplicitVrLittleEndian => None,
    }
}

/// Encode one composed tile into finished compressed DICOM frame bytes.
pub fn encode_dicom_j2k_frame(
    request: DicomJ2kFrameEncodeRequest<'_>,
) -> Result<DicomEncodedFrame, WsiDicomError> {
    if !request.transfer_syntax.is_lossless_j2k_family() {
        return Err(WsiDicomError::Unsupported {
            reason: "single-frame DICOM J2K encode requires a JPEG 2000 or HTJ2K transfer syntax"
                .into(),
        });
    }

    let mut encoder = DicomJ2kEncoder::new(
        request.encode_backend,
        request.transfer_syntax,
        request.codec_validation,
    );
    let encoded = encoder.encode(request.samples)?;
    let bytes = encoded.codestream_bytes()?.to_vec();

    Ok(DicomEncodedFrame {
        transfer_syntax_uid: request.transfer_syntax.uid(),
        bytes,
        used_device_encode: encoded.used_device_encode,
        used_device_validation: encoded.used_device_validation,
        encode_micros: encoded.encode_duration.as_micros(),
        validation_micros: encoded.validation_duration.as_micros(),
    })
}

/// Export a statumen-readable WSI into DICOM VL Whole Slide Microscopy files.
pub fn export_dicom(request: DicomExportRequest) -> Result<DicomExportReport, WsiDicomError> {
    request.validate()?;
    #[cfg(all(feature = "metal", target_os = "macos"))]
    load_persistent_auto_metal_input_route_cache_if_requested()?;
    if request.options.transfer_syntax != TransferSyntax::JpegBaseline8Bit
        && !request.options.transfer_syntax.is_j2k_family()
    {
        return Err(WsiDicomError::Unsupported {
            reason: "only JPEG Baseline passthrough, JPEG 2000, JPEG 2000 Lossless, and HTJ2K Lossless transfer syntaxes are implemented"
                .into(),
        });
    }
    let metadata = request.metadata.resolve()?;
    fs::create_dir_all(&request.output_dir).map_err(|source| WsiDicomError::Io {
        path: request.output_dir.clone(),
        source,
    })?;

    let slide = Slide::open(&request.source_path).map_err(|source| WsiDicomError::SourceOpen {
        path: request.source_path.clone(),
        message: source.to_string(),
    })?;

    let study_uid = metadata
        .study_instance_uid
        .clone()
        .unwrap_or_else(|| uid_from_seed(&format!("study:{}", request.source_path.display())));
    let mut instances = Vec::new();
    let mut metrics = DicomExportMetrics::default();

    for (scene_idx, scene) in slide.dataset().scenes.iter().enumerate() {
        for (series_idx, series) in scene.series.iter().enumerate() {
            for (level_idx, level) in series.levels.iter().enumerate() {
                let level_idx =
                    u32::try_from(level_idx).map_err(|_| WsiDicomError::Unsupported {
                        reason: "export level index exceeds u32".into(),
                    })?;
                if request
                    .level_filter
                    .is_some_and(|requested_level| requested_level != level_idx)
                {
                    continue;
                }
                if level_is_synthetic_downsample(&slide, scene_idx, series_idx, level_idx)? {
                    continue;
                }
                for z in 0..series.axes.z {
                    for t in 0..series.axes.t {
                        let channel_groups = optical_path_groups(series.axes.c);
                        for c in channel_groups {
                            let instance_number = instances.len() as u32 + 1;
                            let report = if request.options.transfer_syntax
                                == TransferSyntax::JpegBaseline8Bit
                            {
                                export_jpeg_passthrough_instance(
                                    &slide,
                                    &request,
                                    &metadata,
                                    &study_uid,
                                    instance_number,
                                    scene_idx,
                                    series_idx,
                                    level_idx,
                                    z,
                                    c,
                                    t,
                                    level,
                                )?
                            } else {
                                export_instance(
                                    &slide,
                                    &request,
                                    &metadata,
                                    &study_uid,
                                    instance_number,
                                    scene_idx,
                                    series_idx,
                                    level_idx,
                                    z,
                                    c,
                                    t,
                                    level,
                                )?
                            };
                            metrics.add_assign(report.metrics);
                            instances.push(report);
                        }
                    }
                }
            }
        }
    }

    if instances.is_empty() {
        return Err(WsiDicomError::Unsupported {
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

    Ok(DicomExportReport {
        output_dir: request.output_dir,
        instances,
        metrics,
    })
}

/// Profile the route selection and encode path for a bounded number of frames.
pub fn profile_dicom_routes(
    request: DicomRouteProfileRequest,
) -> Result<DicomRouteProfileReport, WsiDicomError> {
    request.options.validate()?;
    #[cfg(all(feature = "metal", target_os = "macos"))]
    load_persistent_auto_metal_input_route_cache_if_requested()?;
    if request.max_frames == 0 {
        return Err(WsiDicomError::Unsupported {
            reason: "route profiling requires max_frames > 0".into(),
        });
    }
    if request.options.transfer_syntax != TransferSyntax::JpegBaseline8Bit
        && !request.options.transfer_syntax.is_j2k_family()
    {
        return Err(WsiDicomError::Unsupported {
            reason: "bounded route profiling currently supports JPEG Baseline, JPEG 2000, and HTJ2K transfer syntaxes"
                .into(),
        });
    }

    let slide = Slide::open(&request.source_path).map_err(|source| WsiDicomError::SourceOpen {
        path: request.source_path.clone(),
        message: source.to_string(),
    })?;
    let level = slide
        .dataset()
        .scenes
        .first()
        .and_then(|scene| scene.series.first())
        .and_then(|series| series.levels.get(request.level as usize))
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: format!("route profiling level {} is not available", request.level),
        })?;
    if level_is_synthetic_downsample(&slide, 0, 0, request.level)? {
        return Err(WsiDicomError::Unsupported {
            reason: format!(
                "route profiling skips synthetic downsample level {}; profile a physical source level instead",
                request.level
            ),
        });
    }

    let started = Instant::now();
    let transfer_syntax_uid = request.options.transfer_syntax.uid();
    let available_frames = route_profile_available_frames(
        &slide,
        &request.options,
        level,
        JpegBaselineFrameLocation::first_series_level(request.level),
    )?;
    let metrics = if request.options.transfer_syntax == TransferSyntax::JpegBaseline8Bit {
        profile_jpeg_baseline_routes(
            &slide,
            request.options,
            level,
            request.level,
            request.max_frames,
        )?
    } else {
        profile_lossless_j2k_routes(
            &slide,
            &request.source_path,
            request.options,
            level,
            request.level,
            request.max_frames,
            None,
        )?
    };

    #[cfg(all(feature = "metal", target_os = "macos"))]
    flush_persistent_auto_metal_input_route_cache_if_requested()?;

    Ok(DicomRouteProfileReport {
        source_path: request.source_path,
        transfer_syntax_uid,
        level: request.level,
        requested_frames: request.max_frames,
        available_frames,
        metrics,
        elapsed_micros: duration_as_reported_micros(started.elapsed()),
    })
}

/// Profile route coverage across all levels in the first scene/series without writing DICOM.
pub fn profile_dicom_route_coverage(
    request: DicomRouteCoverageRequest,
) -> Result<DicomRouteCoverageReport, WsiDicomError> {
    request.options.validate()?;
    #[cfg(all(feature = "metal", target_os = "macos"))]
    load_persistent_auto_metal_input_route_cache_if_requested()?;
    if request.max_frames_per_level == 0 {
        return Err(WsiDicomError::Unsupported {
            reason: "route coverage profiling requires max_frames_per_level > 0".into(),
        });
    }
    if request.max_levels == Some(0) {
        return Err(WsiDicomError::Unsupported {
            reason: "route coverage profiling requires max_levels > 0 when provided".into(),
        });
    }
    validate_max_level_elapsed(request.max_level_elapsed, "route coverage profiling")?;
    if request.options.transfer_syntax != TransferSyntax::JpegBaseline8Bit
        && !request.options.transfer_syntax.is_j2k_family()
    {
        return Err(WsiDicomError::Unsupported {
            reason: "route coverage profiling currently supports JPEG Baseline, JPEG 2000, and HTJ2K transfer syntaxes"
                .into(),
        });
    }

    let slide = Slide::open(&request.source_path).map_err(|source| WsiDicomError::SourceOpen {
        path: request.source_path.clone(),
        message: source.to_string(),
    })?;
    let series = slide
        .dataset()
        .scenes
        .first()
        .and_then(|scene| scene.series.first())
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "route coverage profiling requires at least one scene and series".into(),
        })?;
    if series.levels.is_empty() {
        return Err(WsiDicomError::Unsupported {
            reason: "route coverage profiling requires at least one level".into(),
        });
    }

    let started = Instant::now();
    let transfer_syntax_uid = request.options.transfer_syntax.uid();
    let level_limit = match request.max_levels {
        Some(max_levels) => {
            usize::try_from(max_levels).map_err(|_| WsiDicomError::Unsupported {
                reason: "route coverage max_levels exceeds platform addressable memory".into(),
            })?
        }
        None => series.levels.len(),
    }
    .min(series.levels.len());
    let mut levels = Vec::with_capacity(level_limit);
    let mut metrics = DicomExportMetrics::default();
    let mut available_frames = 0u64;

    for (level_idx, level) in series.levels.iter().take(level_limit).enumerate() {
        let level_started = Instant::now();
        let level_idx = u32::try_from(level_idx).map_err(|_| WsiDicomError::Unsupported {
            reason: "route coverage level index exceeds u32".into(),
        })?;
        if level_is_synthetic_downsample(&slide, 0, 0, level_idx)? {
            if matches!(request.progress, Some(DicomRouteCoverageProgress::Stderr)) {
                eprintln!(
                    "coverage level {}/{} skip {} level={} reason=synthetic_downsample",
                    usize::try_from(level_idx).unwrap_or(usize::MAX) + 1,
                    level_limit,
                    request.source_path.display(),
                    level_idx
                );
            }
            continue;
        }
        let level_available_frames = route_profile_available_frames(
            &slide,
            &request.options,
            level,
            JpegBaselineFrameLocation::first_series_level(level_idx),
        )?;
        if matches!(request.progress, Some(DicomRouteCoverageProgress::Stderr)) {
            eprintln!(
                "coverage level {}/{} start {} level={} available_frames={}",
                usize::try_from(level_idx).unwrap_or(usize::MAX) + 1,
                level_limit,
                request.source_path.display(),
                level_idx,
                level_available_frames
            );
        }
        let level_deadline = RouteLevelDeadline::new(request.max_level_elapsed);
        let level_metrics = if request.options.transfer_syntax == TransferSyntax::JpegBaseline8Bit {
            coverage_jpeg_baseline_routes(
                &slide,
                request.options.clone(),
                level,
                level_idx,
                request.max_frames_per_level,
                level_deadline,
            )?
        } else {
            profile_lossless_j2k_routes(
                &slide,
                &request.source_path,
                request.options.clone(),
                level,
                level_idx,
                request.max_frames_per_level,
                level_deadline,
            )?
        };
        if matches!(request.progress, Some(DicomRouteCoverageProgress::Stderr)) {
            eprintln!(
                "coverage level {}/{} ok {} level={} frames={} route_passthrough={} route_gpu_transcode={} route_cpu_fallback={} elapsed_ms={:.3}",
                usize::try_from(level_idx).unwrap_or(usize::MAX) + 1,
                level_limit,
                request.source_path.display(),
                level_idx,
                level_metrics.total_frames,
                level_metrics.route_passthrough_frames(),
                level_metrics.gpu_transcode_frames,
                level_metrics.cpu_fallback_frames,
                duration_as_reported_micros(level_started.elapsed()) as f64 / 1000.0
            );
        }
        metrics.add_assign(level_metrics);
        available_frames = available_frames.saturating_add(level_available_frames);
        levels.push(DicomRouteProfileReport {
            source_path: request.source_path.clone(),
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

    Ok(DicomRouteCoverageReport {
        source_path: request.source_path,
        transfer_syntax_uid,
        requested_frames_per_level: request.max_frames_per_level,
        available_frames,
        complete_frame_coverage: metrics.total_frames >= available_frames,
        levels,
        metrics,
        elapsed_micros: duration_as_reported_micros(started.elapsed()),
    })
}

/// Profile route coverage for every WSI-like file under a source root.
pub fn profile_dicom_route_corpus_coverage(
    request: DicomRouteCorpusCoverageRequest,
) -> Result<DicomRouteCorpusCoverageReport, WsiDicomError> {
    request.options.validate()?;
    if request.max_frames_per_level == 0 {
        return Err(WsiDicomError::Unsupported {
            reason: "corpus route coverage profiling requires max_frames_per_level > 0".into(),
        });
    }
    if request.max_levels == Some(0) {
        return Err(WsiDicomError::Unsupported {
            reason: "corpus route coverage profiling requires max_levels > 0 when provided".into(),
        });
    }
    validate_max_level_elapsed(request.max_level_elapsed, "corpus route coverage profiling")?;
    let started = Instant::now();
    let transfer_syntax_uid = request.options.transfer_syntax.uid();
    let sources = collect_wsi_candidate_paths(&request.source_root)?;
    let mut reports = Vec::new();
    let mut failures = Vec::new();
    let mut metrics = DicomExportMetrics::default();
    let mut available_frames = 0u64;

    for (source_idx, source_path) in sources.iter().enumerate() {
        let source_started = Instant::now();
        if matches!(
            request.progress,
            Some(DicomRouteCorpusCoverageProgress::Stderr)
        ) {
            eprintln!(
                "coverage-corpus source {}/{} start {}",
                source_idx + 1,
                sources.len(),
                source_path.display()
            );
        }
        match profile_dicom_route_coverage(DicomRouteCoverageRequest {
            source_path: source_path.clone(),
            options: request.options.clone(),
            max_frames_per_level: request.max_frames_per_level,
            max_levels: request.max_levels,
            max_level_elapsed: request.max_level_elapsed,
            progress: request.progress.map(|_| DicomRouteCoverageProgress::Stderr),
        }) {
            Ok(report) => {
                metrics.add_assign(report.metrics);
                available_frames = available_frames.saturating_add(report.available_frames);
                if matches!(
                    request.progress,
                    Some(DicomRouteCorpusCoverageProgress::Stderr)
                ) {
                    eprintln!(
                        "coverage-corpus source {}/{} ok {} levels={} frames={} route_passthrough={} route_gpu_transcode={} route_cpu_fallback={} elapsed_ms={:.3}",
                        source_idx + 1,
                        sources.len(),
                        source_path.display(),
                        report.levels.len(),
                        report.metrics.total_frames,
                        report.metrics.route_passthrough_frames(),
                        report.metrics.gpu_transcode_frames,
                        report.metrics.cpu_fallback_frames,
                        duration_as_reported_micros(source_started.elapsed()) as f64 / 1000.0
                    );
                }
                reports.push(report);
            }
            Err(err) => {
                if matches!(
                    request.progress,
                    Some(DicomRouteCorpusCoverageProgress::Stderr)
                ) {
                    eprintln!(
                        "coverage-corpus source {}/{} failed {} error={} elapsed_ms={:.3}",
                        source_idx + 1,
                        sources.len(),
                        source_path.display(),
                        err,
                        duration_as_reported_micros(source_started.elapsed()) as f64 / 1000.0
                    );
                }
                failures.push(DicomRouteCorpusCoverageFailure {
                    source_path: source_path.clone(),
                    message: err.to_string(),
                });
            }
        }
    }

    Ok(DicomRouteCorpusCoverageReport {
        source_root: request.source_root,
        transfer_syntax_uid,
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

fn collect_wsi_candidate_paths(root: &Path) -> Result<Vec<PathBuf>, WsiDicomError> {
    if root.is_file() {
        return Ok(if is_wsi_candidate_path(root) {
            vec![root.to_path_buf()]
        } else {
            Vec::new()
        });
    }
    if !root.is_dir() {
        return Err(WsiDicomError::Unsupported {
            reason: format!(
                "corpus coverage root is not a file or directory: {}",
                root.display()
            ),
        });
    }

    let mut pending = vec![root.to_path_buf()];
    let mut candidates = Vec::new();
    while let Some(dir) = pending.pop() {
        let entries = fs::read_dir(&dir).map_err(|source| WsiDicomError::Io {
            path: dir.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| WsiDicomError::Io {
                path: dir.clone(),
                source,
            })?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|source| WsiDicomError::Io {
                path: path.clone(),
                source,
            })?;
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() && is_wsi_candidate_path(&path) {
                candidates.push(path);
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

struct LosslessJ2kPlannedFrame {
    col: u64,
    x: u64,
    y: u64,
    width: u32,
    height: u32,
    passthrough: Option<J2kPassthroughFrame>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
struct LosslessJ2kCpuBatchSettings {
    transfer_syntax: TransferSyntax,
    codec_validation: CodecValidation,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
struct LosslessJ2kCpuBatchFrame {
    x: u64,
    y: u64,
    width: u32,
    height: u32,
}

#[allow(dead_code)]
struct LosslessJ2kCpuBatchOutcome {
    encoded: Result<EncodedDicomJ2kFrame, WsiDicomError>,
    profile: PixelProfile,
    input_decode_duration: Duration,
    compose_duration: Duration,
}

#[derive(Clone)]
struct J2kPassthroughFrame {
    codestream: Vec<u8>,
    profile: PixelProfile,
    transfer_syntax: CompressedTransferSyntax,
}

impl J2kPassthroughFrame {
    fn is_lossy(&self) -> bool {
        matches!(
            self.transfer_syntax,
            CompressedTransferSyntax::Jpeg2000Lossy | CompressedTransferSyntax::HtJpeg2000Lossy
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn plan_lossless_j2k_row(
    slide: &Slide,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    row: u64,
    start_col: u64,
    tile_count: u64,
    matrix_columns: u64,
    matrix_rows: u64,
    tile_size: u32,
    transfer_syntax: TransferSyntax,
    allow_passthrough_probe: bool,
) -> Result<Vec<LosslessJ2kPlannedFrame>, WsiDicomError> {
    let tile_count = usize::try_from(tile_count).map_err(|_| WsiDicomError::Unsupported {
        reason: "J2K row planning tile count exceeds platform addressable memory".into(),
    })?;
    let row_i64 = i64::try_from(row).map_err(|_| WsiDicomError::Unsupported {
        reason: "J2K row planning tile row exceeds i64".into(),
    })?;
    let mut planned = Vec::with_capacity(tile_count);
    for offset in 0..tile_count {
        let col = start_col
            .checked_add(
                u64::try_from(offset).map_err(|_| WsiDicomError::Unsupported {
                    reason: "J2K row planning tile offset exceeds u64".into(),
                })?,
            )
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "J2K row planning tile column overflow".into(),
            })?;
        let col_i64 = i64::try_from(col).map_err(|_| WsiDicomError::Unsupported {
            reason: "J2K row planning tile column exceeds i64".into(),
        })?;
        let x =
            col.checked_mul(u64::from(tile_size))
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "J2K row planning tile x offset overflow".into(),
                })?;
        let y =
            row.checked_mul(u64::from(tile_size))
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "J2K row planning tile y offset overflow".into(),
                })?;
        let width = (matrix_columns - x).min(u64::from(tile_size)) as u32;
        let height = (matrix_rows - y).min(u64::from(tile_size)) as u32;
        let passthrough = if allow_passthrough_probe {
            let tile_request = TileRequest {
                scene: scene_idx,
                series: series_idx,
                level: level_idx,
                plane: PlaneSelection { z, c, t },
                col: col_i64,
                row: row_i64,
            };
            match slide.read_raw_compressed_tile(&tile_request) {
                Ok(raw) => j2k_passthrough_frame(raw, tile_size, tile_size, transfer_syntax)?,
                Err(_) => None,
            }
        } else {
            None
        };
        planned.push(LosslessJ2kPlannedFrame {
            col,
            x,
            y,
            width,
            height,
            passthrough,
        });
    }
    Ok(planned)
}

fn profile_lossless_j2k_routes(
    slide: &Slide,
    _source_path: &Path,
    options: DicomExportOptions,
    level: &statumen::Level,
    level_idx: u32,
    max_frames: u64,
    deadline: Option<RouteLevelDeadline>,
) -> Result<DicomExportMetrics, WsiDicomError> {
    let tile_size = j2k_route_tile_size(&options, level)?;
    let (matrix_columns, matrix_rows) = level.dimensions;
    let tiles_across = matrix_columns.div_ceil(u64::from(tile_size));
    let tiles_down = matrix_rows.div_ceil(u64::from(tile_size));
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let route_scope_frames = tiles_across
        .checked_mul(tiles_down)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "route profile frame count overflow".into(),
        })?
        .min(max_frames);
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let route_scope_frames_usize =
        usize::try_from(route_scope_frames).map_err(|_| WsiDicomError::Unsupported {
            reason: "route profile frame count exceeds platform addressable memory".into(),
        })?;
    let mut j2k_encoder = DicomJ2kEncoder::new(
        options.encode_backend,
        options.transfer_syntax,
        options.codec_validation,
    )
    .with_gpu_encode_tuning(
        options.gpu_encode_inflight_tiles,
        options.gpu_encode_memory_mib,
    );
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let mut metal_input = MetalInputTileReader::new_for_lossless_j2k(
        options.encode_backend,
        lossless_j2k_auto_allows_metal_input(
            options.encode_backend,
            options.transfer_syntax,
            max_frames,
            options.source_device_decode,
        ),
        auto_metal_input_route_cache_key(
            _source_path,
            options.clone(),
            level_idx,
            route_scope_frames,
        ),
        options.source_device_decode,
    );
    #[cfg(all(feature = "metal", target_os = "macos"))]
    if lossless_j2k_auto_should_start_cpu_only(
        options.encode_backend,
        options.transfer_syntax,
        route_scope_frames,
        options.source_device_decode,
    ) || metal_input.auto_route_decision() == AutoLosslessJ2kRouteDecision::CpuOnly
    {
        j2k_encoder.force_cpu_only_for_auto();
    }
    let mut metrics = DicomExportMetrics::default();
    let mut pixel_profile = None;
    let mut remaining = max_frames;
    let allow_passthrough_probe =
        j2k_family_passthrough_probe_allowed(_source_path, options.transfer_syntax);

    for row in 0..tiles_down {
        if remaining == 0 {
            break;
        }
        check_route_level_deadline(deadline, level_idx)?;
        let row_tile_count = tiles_across.min(remaining);
        let planned = plan_lossless_j2k_row(
            slide,
            0,
            0,
            level_idx,
            0,
            0,
            0,
            row,
            0,
            row_tile_count,
            matrix_columns,
            matrix_rows,
            tile_size,
            options.transfer_syntax,
            allow_passthrough_probe,
        )?;
        #[cfg(all(feature = "metal", target_os = "macos"))]
        {
            let mut routed_tiles: Vec<Option<RoutedLosslessJ2kTile>> =
                (0..planned.len()).map(|_| None).collect();
            let mut run_start = 0usize;
            while run_start < planned.len() {
                if planned[run_start].passthrough.is_some() {
                    run_start += 1;
                    continue;
                }
                let mut run_end = run_start + 1;
                while run_end < planned.len() && planned[run_end].passthrough.is_none() {
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
                        0,
                        0,
                        level_idx,
                        0,
                        0,
                        0,
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
                        u64::try_from(probe_end - run_start).map_err(|_| {
                            WsiDicomError::Unsupported {
                                reason: "auto route probe frame count exceeds u64".into(),
                            }
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
                        0,
                        0,
                        level_idx,
                        0,
                        0,
                        0,
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
            if let Some((transfer_syntax, codec_validation)) = j2k_encoder.cpu_batch_settings() {
                let cpu_indices = planned
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, planned_frame)| {
                        (planned_frame.passthrough.is_none()
                            && !options.transfer_syntax.is_jpeg2000_passthrough_only()
                            && routed_tiles[idx].is_none())
                        .then_some(idx)
                    })
                    .collect::<Vec<_>>();
                for (idx, outcome) in encode_cpu_input_lossless_j2k_planned_batch(
                    slide,
                    LosslessJ2kCpuBatchSettings {
                        transfer_syntax,
                        codec_validation,
                    },
                    0,
                    0,
                    level_idx,
                    0,
                    0,
                    0,
                    &planned,
                    &cpu_indices,
                    tile_size,
                )? {
                    cpu_batch_results[idx] = Some(outcome);
                }
            }
            for (idx, planned_frame) in planned.into_iter().enumerate() {
                if let Some(passthrough) = planned_frame.passthrough {
                    let profile = passthrough.profile;
                    if let Some(existing) = pixel_profile {
                        if existing != profile {
                            return Err(WsiDicomError::UnsupportedPixelData {
                                reason: "pixel profile changed across profiled frames".into(),
                            });
                        }
                    } else {
                        pixel_profile = Some(profile);
                    }
                    let byte_started = Instant::now();
                    metrics.record_write_duration(byte_started.elapsed());
                    metrics.record_j2k_passthrough_frame();
                    metrics.record_pixel_profile(profile);
                    remaining = remaining.saturating_sub(1);
                    continue;
                }
                if options.transfer_syntax.is_jpeg2000_passthrough_only() {
                    metrics.record_j2k_passthrough_only_fallback_classification();
                    remaining = remaining.saturating_sub(1);
                    continue;
                }

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
                            let outcome = cpu_batch_results[idx]
                                .take()
                                .expect("checked CPU batch outcome presence");
                            (
                                outcome.encoded,
                                outcome.profile,
                                false,
                                outcome.input_decode_duration,
                                outcome.compose_duration,
                            )
                        }
                        None => {
                            let (encoded, profile, input_decode_duration, compose_duration) =
                                encode_cpu_input_tile(
                                    slide,
                                    &mut j2k_encoder,
                                    0,
                                    0,
                                    level_idx,
                                    0,
                                    0,
                                    0,
                                    row,
                                    planned_frame.col,
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
                if used_gpu_input {
                    metrics.record_gpu_input();
                } else {
                    metrics.record_cpu_input();
                    metrics.record_input_decode_duration(input_decode_duration);
                    metrics.record_compose_duration(compose_duration);
                }
                metrics.record_pixel_profile(profile);
                if let Some(existing) = pixel_profile {
                    if existing != profile {
                        return Err(WsiDicomError::UnsupportedPixelData {
                            reason: "pixel profile changed across profiled frames".into(),
                        });
                    }
                } else {
                    pixel_profile = Some(profile);
                }
                let encoded = encoded?;
                metrics.record_encoded_frame(&encoded);
                metrics.record_transcode_route(used_gpu_input, encoded.used_device_encode);
                let byte_started = Instant::now();
                let _ = encoded.codestream_bytes()?.len();
                metrics.record_write_duration(byte_started.elapsed());
                remaining = remaining.saturating_sub(1);
            }
        }
        #[cfg(not(all(feature = "metal", target_os = "macos")))]
        {
            let mut cpu_batch_results: Vec<Option<LosslessJ2kCpuBatchOutcome>> =
                (0..planned.len()).map(|_| None).collect();
            if let Some((transfer_syntax, codec_validation)) = j2k_encoder.cpu_batch_settings() {
                let cpu_indices = planned
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, planned_frame)| {
                        (planned_frame.passthrough.is_none()
                            && !options.transfer_syntax.is_jpeg2000_passthrough_only())
                        .then_some(idx)
                    })
                    .collect::<Vec<_>>();
                for (idx, outcome) in encode_cpu_input_lossless_j2k_planned_batch(
                    slide,
                    LosslessJ2kCpuBatchSettings {
                        transfer_syntax,
                        codec_validation,
                    },
                    0,
                    0,
                    level_idx,
                    0,
                    0,
                    0,
                    &planned,
                    &cpu_indices,
                    tile_size,
                )? {
                    cpu_batch_results[idx] = Some(outcome);
                }
            }
            for (idx, planned_frame) in planned.into_iter().enumerate() {
                if let Some(passthrough) = planned_frame.passthrough {
                    let profile = passthrough.profile;
                    if let Some(existing) = pixel_profile {
                        if existing != profile {
                            return Err(WsiDicomError::UnsupportedPixelData {
                                reason: "pixel profile changed across profiled frames".into(),
                            });
                        }
                    } else {
                        pixel_profile = Some(profile);
                    }
                    let byte_started = Instant::now();
                    metrics.record_write_duration(byte_started.elapsed());
                    metrics.record_j2k_passthrough_frame();
                    metrics.record_pixel_profile(profile);
                    remaining = remaining.saturating_sub(1);
                    continue;
                }
                if options.transfer_syntax.is_jpeg2000_passthrough_only() {
                    metrics.record_j2k_passthrough_only_fallback_classification();
                    remaining = remaining.saturating_sub(1);
                    continue;
                }

                let (encoded, profile, input_decode_duration, compose_duration) =
                    if let Some(outcome) = cpu_batch_results[idx].take() {
                        (
                            outcome.encoded,
                            outcome.profile,
                            outcome.input_decode_duration,
                            outcome.compose_duration,
                        )
                    } else {
                        encode_cpu_input_tile(
                            slide,
                            &mut j2k_encoder,
                            0,
                            0,
                            level_idx,
                            0,
                            0,
                            0,
                            row,
                            planned_frame.col,
                            planned_frame.x,
                            planned_frame.y,
                            planned_frame.width,
                            planned_frame.height,
                            tile_size,
                        )?
                    };
                metrics.record_input_decode_duration(input_decode_duration);
                metrics.record_compose_duration(compose_duration);
                metrics.record_cpu_input();
                metrics.record_pixel_profile(profile);
                if let Some(existing) = pixel_profile {
                    if existing != profile {
                        return Err(WsiDicomError::UnsupportedPixelData {
                            reason: "pixel profile changed across profiled frames".into(),
                        });
                    }
                } else {
                    pixel_profile = Some(profile);
                }
                let encoded = encoded?;
                metrics.record_encoded_frame(&encoded);
                metrics.record_transcode_route(false, encoded.used_device_encode);
                let byte_started = Instant::now();
                let _ = encoded.codestream_bytes()?.len();
                metrics.record_write_duration(byte_started.elapsed());
                remaining = remaining.saturating_sub(1);
            }
        }
    }

    Ok(metrics)
}

fn profile_jpeg_baseline_routes(
    slide: &Slide,
    options: DicomExportOptions,
    level: &statumen::Level,
    level_idx: u32,
    max_frames: u64,
) -> Result<DicomExportMetrics, WsiDicomError> {
    let geometry = jpeg_baseline_route_frame_geometry(
        slide,
        level,
        JpegBaselineFrameLocation::first_series_level(level_idx),
        options.tile_size,
    )?;
    let (matrix_columns, matrix_rows) = level.dimensions;
    let (tiles_across, tiles_down) = (geometry.tiles_across, geometry.tiles_down);
    let (frame_columns, frame_rows) = (geometry.frame_columns, geometry.frame_rows);
    let allow_raw_rgb_passthrough = raw_rgb_passthrough_has_no_geometry_fallback(level, geometry);
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let mut metal_input =
        MetalInputTileReader::new(options.encode_backend, options.source_device_decode);
    let mut metrics = DicomExportMetrics::default();
    let mut pixel_profile = None;
    let mut remaining = max_frames;

    for row in 0..tiles_down {
        if remaining == 0 {
            break;
        }
        let row_tile_count = tiles_across.min(remaining);
        let row_frame_capacity =
            usize::try_from(row_tile_count).map_err(|_| WsiDicomError::Unsupported {
                reason:
                    "JPEG Baseline profiled row frame count exceeds platform addressable memory"
                        .into(),
            })?;
        let mut planned = Vec::with_capacity(row_frame_capacity);
        for col in 0..row_tile_count {
            let raw = slide.read_raw_compressed_tile(&TileRequest {
                scene: 0,
                series: 0,
                level: level_idx,
                plane: PlaneSelection { z: 0, c: 0, t: 0 },
                col: col as i64,
                row: row as i64,
            });

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
                Ok(raw) if raw.compression == Compression::Jpeg => {}
                Ok(_) | Err(_) => {}
            }

            let x = col.checked_mul(u64::from(frame_columns)).ok_or_else(|| {
                WsiDicomError::Unsupported {
                    reason: "JPEG Baseline profile tile x offset overflow".into(),
                }
            })?;
            let y = row.checked_mul(u64::from(frame_rows)).ok_or_else(|| {
                WsiDicomError::Unsupported {
                    reason: "JPEG Baseline profile tile y offset overflow".into(),
                }
            })?;
            let width = (matrix_columns - x).min(u64::from(frame_columns)) as u32;
            let height = (matrix_rows - y).min(u64::from(frame_rows)) as u32;
            planned.push(JpegBaselinePlannedFrame::Fallback(
                JpegBaselineFallbackFrame {
                    x,
                    y,
                    width,
                    height,
                },
            ));
        }

        let mut index = 0usize;
        while index < planned.len() {
            match &planned[index] {
                JpegBaselinePlannedFrame::Passthrough { data, profile, .. } => {
                    ensure_jpeg_baseline_profile(
                        &mut pixel_profile,
                        *profile,
                        "JPEG passthrough pixel profile changed across profiled frames",
                    )?;
                    let byte_started = Instant::now();
                    let _ = data.len();
                    metrics.record_write_duration(byte_started.elapsed());
                    metrics.record_passthrough_frame();
                    metrics.record_pixel_profile(*profile);
                    remaining = remaining.saturating_sub(1);
                    index += 1;
                }
                JpegBaselinePlannedFrame::Fallback(_) => {
                    let start = index;
                    while index < planned.len()
                        && matches!(planned[index], JpegBaselinePlannedFrame::Fallback(_))
                    {
                        index += 1;
                    }
                    let fallback_frames: Vec<_> = planned[start..index]
                        .iter()
                        .map(|frame| match frame {
                            JpegBaselinePlannedFrame::Fallback(frame) => *frame,
                            JpegBaselinePlannedFrame::Passthrough { .. } => {
                                unreachable!("fallback run contains only fallback frames")
                            }
                        })
                        .collect();

                    #[cfg(all(feature = "metal", target_os = "macos"))]
                    let mut metal_run = try_encode_jpeg_baseline_metal_input_tile_run(
                        slide,
                        &mut metal_input,
                        level,
                        0,
                        0,
                        level_idx,
                        0,
                        0,
                        0,
                        row,
                        &fallback_frames,
                        frame_columns,
                        frame_rows,
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

                    let mut cpu_batch_results: Vec<Option<EncodedJpegBaselineFrame>> =
                        (0..fallback_frames.len()).map(|_| None).collect();
                    if options.encode_backend != EncodeBackendPreference::RequireDevice {
                        let cpu_indices = metal_run
                            .frames
                            .iter()
                            .enumerate()
                            .filter_map(|(idx, frame)| frame.is_none().then_some(idx))
                            .collect::<Vec<_>>();
                        let cpu_frames = cpu_indices
                            .iter()
                            .map(|&idx| fallback_frames[idx])
                            .collect::<Vec<_>>();
                        let cpu_encoded = encode_jpeg_baseline_cpu_input_tile_batch(
                            slide,
                            0,
                            0,
                            level_idx,
                            0,
                            0,
                            0,
                            &cpu_frames,
                            frame_columns,
                            frame_rows,
                        )?;
                        for (idx, encoded) in cpu_indices.into_iter().zip(cpu_encoded) {
                            cpu_batch_results[idx] = Some(encoded);
                        }
                    }

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
                                return Err(WsiDicomError::Unsupported {
                                        reason:
                                            "requested JPEG Baseline device encode backend is unavailable or unsupported"
                                                .into(),
                                    });
                            }
                            cpu_batch_results[idx]
                                .take()
                                .expect("CPU JPEG batch encoded every non-Metal frame")
                        };
                        ensure_jpeg_baseline_profile(
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
                        let byte_started = Instant::now();
                        let _ = encoded.data.len();
                        metrics.record_write_duration(byte_started.elapsed());
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
    options: DicomExportOptions,
    level: &statumen::Level,
    level_idx: u32,
    max_frames: u64,
    deadline: Option<RouteLevelDeadline>,
) -> Result<DicomExportMetrics, WsiDicomError> {
    let geometry = jpeg_baseline_route_frame_geometry(
        slide,
        level,
        JpegBaselineFrameLocation::first_series_level(level_idx),
        options.tile_size,
    )?;
    let (tiles_across, tiles_down) = (geometry.tiles_across, geometry.tiles_down);
    let (frame_columns, frame_rows) = (geometry.frame_columns, geometry.frame_rows);
    let allow_raw_rgb_passthrough = raw_rgb_passthrough_has_no_geometry_fallback(level, geometry);
    let mut metrics = DicomExportMetrics::default();
    let mut pixel_profile = None;
    let mut remaining = max_frames;

    for row in 0..tiles_down {
        if remaining == 0 {
            break;
        }
        check_route_level_deadline(deadline, level_idx)?;
        let row_tile_count = tiles_across.min(remaining);
        for col in 0..row_tile_count {
            let raw = slide.read_raw_compressed_tile(&TileRequest {
                scene: 0,
                series: 0,
                level: level_idx,
                plane: PlaneSelection { z: 0, c: 0, t: 0 },
                col: col as i64,
                row: row as i64,
            });

            match raw {
                Ok(raw) if raw_jpeg_matches_frame_geometry(&raw, frame_columns, frame_rows) => {
                    let profile = pixel_profile_from_raw_jpeg_tile(&raw)?;
                    if raw_jpeg_profile_can_passthrough(profile, allow_raw_rgb_passthrough) {
                        ensure_jpeg_baseline_profile(
                            &mut pixel_profile,
                            profile,
                            "JPEG passthrough pixel profile changed across coverage frames",
                        )?;
                        let byte_started = Instant::now();
                        let _ = raw.data.len();
                        metrics.record_write_duration(byte_started.elapsed());
                        metrics.record_passthrough_frame();
                        metrics.record_pixel_profile(profile);
                        remaining = remaining.saturating_sub(1);
                        continue;
                    }
                }
                Ok(raw) if raw.compression == Compression::Jpeg => {}
                Ok(_) | Err(_) => {}
            }

            metrics.record_jpeg_cpu_fallback_route_classification();
            remaining = remaining.saturating_sub(1);
        }
    }

    Ok(metrics)
}

#[allow(clippy::too_many_arguments)]
fn export_instance(
    slide: &Slide,
    request: &DicomExportRequest,
    metadata: &DicomMetadata,
    study_uid: &str,
    instance_number: u32,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    level: &statumen::Level,
) -> Result<DicomInstanceReport, WsiDicomError> {
    let tile_size = j2k_route_tile_size(&request.options, level)?;
    let (matrix_columns, matrix_rows) = level.dimensions;
    let tiles_across = matrix_columns.div_ceil(u64::from(tile_size));
    let tiles_down = matrix_rows.div_ceil(u64::from(tile_size));
    let frame_count = tiles_across
        .checked_mul(tiles_down)
        .and_then(|count| u32::try_from(count).ok())
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "frame count exceeds u32".into(),
        })?;

    let series_uid = uid_from_seed(&format!(
        "series:{}:{}:{}:{}:{}:{}",
        request.source_path.display(),
        scene_idx,
        series_idx,
        z,
        c,
        t
    ));
    let sop_instance_uid = uid_from_seed(&format!(
        "instance:{}:{}:{}:{}:{}:{}",
        request.source_path.display(),
        scene_idx,
        series_idx,
        level_idx,
        z,
        c
    ));
    let frame_of_reference_uid = uid_from_seed(&format!(
        "frame-of-reference:{}:{}:{}",
        request.source_path.display(),
        scene_idx,
        series_idx
    ));
    let pyramid_uid = uid_from_seed(&format!(
        "pyramid:{}:{}:{}:{}:{}:{}",
        request.source_path.display(),
        scene_idx,
        series_idx,
        z,
        c,
        t
    ));
    let dimension_organization_uid = uid_from_seed(&format!(
        "dimension-organization:{}:{}:{}:{}:{}:{}",
        request.source_path.display(),
        scene_idx,
        series_idx,
        z,
        c,
        t
    ));
    let pyramid_label = pyramid_label(scene_idx, series_idx, z, c, t);
    let pixel_spacing_mm = require_pixel_spacing_mm(level_pixel_spacing_mm(slide, level))?;

    let path = deterministic_instance_path(&request.output_dir, level_idx, z, c, t);
    let spool_path = path.with_extension("pixeldata.tmp");
    let mut pixel_spool = PixelDataSpool::create(spool_path, frame_count as usize)?;
    let mut pixel_profile = None;
    let mut j2k_encoder = DicomJ2kEncoder::new(
        request.options.encode_backend,
        request.options.transfer_syntax,
        request.options.codec_validation,
    )
    .with_gpu_encode_tuning(
        request.options.gpu_encode_inflight_tiles,
        request.options.gpu_encode_memory_mib,
    );
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let mut metal_input = MetalInputTileReader::new_for_lossless_j2k(
        request.options.encode_backend,
        lossless_j2k_auto_allows_metal_input(
            request.options.encode_backend,
            request.options.transfer_syntax,
            u64::from(frame_count),
            request.options.source_device_decode,
        ),
        auto_metal_input_route_cache_key(
            &request.source_path,
            request.options.clone(),
            level_idx,
            u64::from(frame_count),
        ),
        request.options.source_device_decode,
    );
    #[cfg(all(feature = "metal", target_os = "macos"))]
    if lossless_j2k_auto_should_start_cpu_only(
        request.options.encode_backend,
        request.options.transfer_syntax,
        u64::from(frame_count),
        request.options.source_device_decode,
    ) || metal_input.auto_route_decision() == AutoLosslessJ2kRouteDecision::CpuOnly
    {
        j2k_encoder.force_cpu_only_for_auto();
    }
    let mut metrics = DicomExportMetrics::default();
    let mut j2k_passthrough_lossy = false;
    let allow_passthrough_probe =
        j2k_family_passthrough_probe_allowed(&request.source_path, request.options.transfer_syntax);

    for row in 0..tiles_down {
        let planned = plan_lossless_j2k_row(
            slide,
            scene_idx,
            series_idx,
            level_idx,
            z,
            c,
            t,
            row,
            0,
            tiles_across,
            matrix_columns,
            matrix_rows,
            tile_size,
            request.options.transfer_syntax,
            allow_passthrough_probe,
        )?;
        #[cfg(all(feature = "metal", target_os = "macos"))]
        {
            let mut routed_tiles: Vec<Option<RoutedLosslessJ2kTile>> =
                (0..planned.len()).map(|_| None).collect();
            let mut run_start = 0usize;
            while run_start < planned.len() {
                if planned[run_start].passthrough.is_some() {
                    run_start += 1;
                    continue;
                }
                let mut run_end = run_start + 1;
                while run_end < planned.len() && planned[run_end].passthrough.is_none() {
                    run_end += 1;
                }
                if request
                    .options
                    .transfer_syntax
                    .is_jpeg2000_passthrough_only()
                {
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
                        scene_idx,
                        series_idx,
                        level_idx,
                        z,
                        c,
                        t,
                        row,
                        &planned[run_start..probe_end],
                        frame_count as usize,
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
                        u64::try_from(probe_end - run_start).map_err(|_| {
                            WsiDicomError::Unsupported {
                                reason: "auto route probe frame count exceeds u64".into(),
                            }
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
                        scene_idx,
                        series_idx,
                        level_idx,
                        z,
                        c,
                        t,
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
            if let Some((transfer_syntax, codec_validation)) = j2k_encoder.cpu_batch_settings() {
                let cpu_indices = planned
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, planned_frame)| {
                        (planned_frame.passthrough.is_none()
                            && !request
                                .options
                                .transfer_syntax
                                .is_jpeg2000_passthrough_only()
                            && routed_tiles[idx].is_none())
                        .then_some(idx)
                    })
                    .collect::<Vec<_>>();
                for (idx, outcome) in encode_cpu_input_lossless_j2k_planned_batch(
                    slide,
                    LosslessJ2kCpuBatchSettings {
                        transfer_syntax,
                        codec_validation,
                    },
                    scene_idx,
                    series_idx,
                    level_idx,
                    z,
                    c,
                    t,
                    &planned,
                    &cpu_indices,
                    tile_size,
                )? {
                    cpu_batch_results[idx] = Some(outcome);
                }
            }

            for (idx, planned_frame) in planned.into_iter().enumerate() {
                if let Some(passthrough) = planned_frame.passthrough {
                    let profile = passthrough.profile;
                    if let Some(existing) = pixel_profile {
                        if existing != profile {
                            return Err(WsiDicomError::UnsupportedPixelData {
                                reason: "pixel profile changed across frames".into(),
                            });
                        }
                    } else {
                        pixel_profile = Some(profile);
                    }
                    j2k_passthrough_lossy |= passthrough.is_lossy();
                    let byte_started = Instant::now();
                    pixel_spool.push_frame(&passthrough.codestream)?;
                    metrics.record_write_duration(byte_started.elapsed());
                    metrics.record_j2k_passthrough_frame();
                    metrics.record_pixel_profile(profile);
                    continue;
                }
                if request
                    .options
                    .transfer_syntax
                    .is_jpeg2000_passthrough_only()
                {
                    return Err(WsiDicomError::Unsupported {
                        reason: format!(
                            "JPEG 2000 transfer syntax export is passthrough-only; frame row={} col={} was not eligible for compressed-frame passthrough",
                            row, planned_frame.col
                        ),
                    });
                }

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
                            let outcome = cpu_batch_results[idx]
                                .take()
                                .expect("checked CPU batch outcome presence");
                            (
                                outcome.encoded,
                                outcome.profile,
                                false,
                                outcome.input_decode_duration,
                                outcome.compose_duration,
                            )
                        }
                        None => {
                            let (encoded, profile, input_decode_duration, compose_duration) =
                                encode_cpu_input_tile(
                                    slide,
                                    &mut j2k_encoder,
                                    scene_idx,
                                    series_idx,
                                    level_idx,
                                    z,
                                    c,
                                    t,
                                    row,
                                    planned_frame.col,
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
                if used_gpu_input {
                    metrics.record_gpu_input();
                } else {
                    metrics.record_cpu_input();
                    metrics.record_input_decode_duration(input_decode_duration);
                    metrics.record_compose_duration(compose_duration);
                }
                metrics.record_pixel_profile(profile);

                if let Some(existing) = pixel_profile {
                    if existing != profile {
                        return Err(WsiDicomError::UnsupportedPixelData {
                            reason: "pixel profile changed across frames".into(),
                        });
                    }
                } else {
                    pixel_profile = Some(profile);
                }

                let encoded = encoded.map_err(|err| match err {
                    WsiDicomError::Encode { message } => WsiDicomError::FrameEncode {
                        level: level_idx,
                        row,
                        col: planned_frame.col,
                        message,
                    },
                    other => other,
                })?;
                metrics.record_encoded_frame(&encoded);
                metrics.record_transcode_route(used_gpu_input, encoded.used_device_encode);
                let byte_started = Instant::now();
                pixel_spool.push_frame(encoded.codestream_bytes()?)?;
                metrics.record_write_duration(byte_started.elapsed());
            }
        }
        #[cfg(not(all(feature = "metal", target_os = "macos")))]
        {
            let mut cpu_batch_results: Vec<Option<LosslessJ2kCpuBatchOutcome>> =
                (0..planned.len()).map(|_| None).collect();
            if let Some((transfer_syntax, codec_validation)) = j2k_encoder.cpu_batch_settings() {
                let cpu_indices = planned
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, planned_frame)| {
                        (planned_frame.passthrough.is_none()
                            && !request
                                .options
                                .transfer_syntax
                                .is_jpeg2000_passthrough_only())
                        .then_some(idx)
                    })
                    .collect::<Vec<_>>();
                for (idx, outcome) in encode_cpu_input_lossless_j2k_planned_batch(
                    slide,
                    LosslessJ2kCpuBatchSettings {
                        transfer_syntax,
                        codec_validation,
                    },
                    scene_idx,
                    series_idx,
                    level_idx,
                    z,
                    c,
                    t,
                    &planned,
                    &cpu_indices,
                    tile_size,
                )? {
                    cpu_batch_results[idx] = Some(outcome);
                }
            }
            for (idx, planned_frame) in planned.into_iter().enumerate() {
                if let Some(passthrough) = planned_frame.passthrough {
                    let profile = passthrough.profile;
                    if let Some(existing) = pixel_profile {
                        if existing != profile {
                            return Err(WsiDicomError::UnsupportedPixelData {
                                reason: "pixel profile changed across frames".into(),
                            });
                        }
                    } else {
                        pixel_profile = Some(profile);
                    }
                    j2k_passthrough_lossy |= passthrough.is_lossy();
                    let byte_started = Instant::now();
                    pixel_spool.push_frame(&passthrough.codestream)?;
                    metrics.record_write_duration(byte_started.elapsed());
                    metrics.record_j2k_passthrough_frame();
                    metrics.record_pixel_profile(profile);
                    continue;
                }
                if request
                    .options
                    .transfer_syntax
                    .is_jpeg2000_passthrough_only()
                {
                    return Err(WsiDicomError::Unsupported {
                        reason: format!(
                            "JPEG 2000 transfer syntax export is passthrough-only; frame row={} col={} was not eligible for compressed-frame passthrough",
                            row, planned_frame.col
                        ),
                    });
                }

                let (encoded, profile, input_decode_duration, compose_duration) =
                    if let Some(outcome) = cpu_batch_results[idx].take() {
                        (
                            outcome.encoded,
                            outcome.profile,
                            outcome.input_decode_duration,
                            outcome.compose_duration,
                        )
                    } else {
                        encode_cpu_input_tile(
                            slide,
                            &mut j2k_encoder,
                            scene_idx,
                            series_idx,
                            level_idx,
                            z,
                            c,
                            t,
                            row,
                            planned_frame.col,
                            planned_frame.x,
                            planned_frame.y,
                            planned_frame.width,
                            planned_frame.height,
                            tile_size,
                        )?
                    };
                metrics.record_input_decode_duration(input_decode_duration);
                metrics.record_compose_duration(compose_duration);
                metrics.record_cpu_input();
                metrics.record_pixel_profile(profile);

                if let Some(existing) = pixel_profile {
                    if existing != profile {
                        return Err(WsiDicomError::UnsupportedPixelData {
                            reason: "pixel profile changed across frames".into(),
                        });
                    }
                } else {
                    pixel_profile = Some(profile);
                }

                let encoded = encoded.map_err(|err| match err {
                    WsiDicomError::Encode { message } => WsiDicomError::FrameEncode {
                        level: level_idx,
                        row,
                        col: planned_frame.col,
                        message,
                    },
                    other => other,
                })?;
                metrics.record_encoded_frame(&encoded);
                metrics.record_transcode_route(false, encoded.used_device_encode);
                let byte_started = Instant::now();
                pixel_spool.push_frame(encoded.codestream_bytes()?)?;
                metrics.record_write_duration(byte_started.elapsed());
            }
        }
    }

    let profile = pixel_profile.ok_or_else(|| WsiDicomError::Unsupported {
        reason: "slide level produced no frames".into(),
    })?;
    let object = build_dicom_object(
        metadata,
        study_uid,
        &series_uid,
        &sop_instance_uid,
        &frame_of_reference_uid,
        &pyramid_uid,
        &dimension_organization_uid,
        &pyramid_label,
        (series_idx + 1) as u32,
        instance_number,
        level_idx,
        tile_size,
        tile_size,
        matrix_columns,
        matrix_rows,
        frame_count,
        profile,
        Some(pixel_spacing_mm),
        pixel_spool.offsets(),
        pixel_spool.lengths(),
        j2k_passthrough_lossy.then_some(LossyCompressionMetadata {
            method: "ISO_15444_1",
            ratio: None,
        }),
    )?;
    let write_started = Instant::now();
    write_dicom_object_with_spooled_pixel_data(
        &path,
        object,
        FileMetaTableBuilder::new()
            .media_storage_sop_class_uid(VL_WSI_SOP_CLASS_UID)
            .media_storage_sop_instance_uid(&sop_instance_uid)
            .transfer_syntax(request.options.transfer_syntax.uid()),
        &mut pixel_spool,
    )?;
    metrics.record_write_duration(write_started.elapsed());

    Ok(DicomInstanceReport {
        path,
        sop_instance_uid,
        series_instance_uid: series_uid,
        transfer_syntax_uid: request.options.transfer_syntax.uid(),
        level: level_idx,
        z,
        c,
        t,
        frame_count,
        metrics,
    })
}

#[allow(clippy::too_many_arguments)]
fn export_jpeg_passthrough_instance(
    slide: &Slide,
    request: &DicomExportRequest,
    metadata: &DicomMetadata,
    study_uid: &str,
    instance_number: u32,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    level: &statumen::Level,
) -> Result<DicomInstanceReport, WsiDicomError> {
    let tile_size = request.options.tile_size;
    let (matrix_columns, matrix_rows) = level.dimensions;
    let geometry = jpeg_baseline_route_frame_geometry(
        slide,
        level,
        JpegBaselineFrameLocation {
            scene_idx,
            series_idx,
            level_idx,
            z,
            c,
            t,
        },
        tile_size,
    )?;
    let (tiles_across, tiles_down) = (geometry.tiles_across, geometry.tiles_down);
    let (frame_columns, frame_rows) = (geometry.frame_columns, geometry.frame_rows);
    let frame_count = tiles_across
        .checked_mul(tiles_down)
        .and_then(|count| u32::try_from(count).ok())
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "frame count exceeds u32".into(),
        })?;

    let series_uid = uid_from_seed(&format!(
        "series:{}:{}:{}:{}:{}:{}",
        request.source_path.display(),
        scene_idx,
        series_idx,
        z,
        c,
        t
    ));
    let sop_instance_uid = uid_from_seed(&format!(
        "instance:{}:{}:{}:{}:{}:{}",
        request.source_path.display(),
        scene_idx,
        series_idx,
        level_idx,
        z,
        c
    ));
    let frame_of_reference_uid = uid_from_seed(&format!(
        "frame-of-reference:{}:{}:{}",
        request.source_path.display(),
        scene_idx,
        series_idx
    ));
    let pyramid_uid = uid_from_seed(&format!(
        "pyramid:{}:{}:{}:{}:{}:{}",
        request.source_path.display(),
        scene_idx,
        series_idx,
        z,
        c,
        t
    ));
    let dimension_organization_uid = uid_from_seed(&format!(
        "dimension-organization:{}:{}:{}:{}:{}:{}",
        request.source_path.display(),
        scene_idx,
        series_idx,
        z,
        c,
        t
    ));
    let pyramid_label = pyramid_label(scene_idx, series_idx, z, c, t);
    let pixel_spacing_mm = require_pixel_spacing_mm(level_pixel_spacing_mm(slide, level))?;

    let path = deterministic_instance_path(&request.output_dir, level_idx, z, c, t);
    let spool_path = path.with_extension("pixeldata.tmp");
    let mut pixel_spool = PixelDataSpool::create(spool_path, frame_count as usize)?;
    let mut pixel_profile = None;
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let mut metal_input = MetalInputTileReader::new(
        request.options.encode_backend,
        request.options.source_device_decode,
    );
    let mut metrics = DicomExportMetrics::default();
    let mut compressed_bytes = 0u64;
    let mut uncompressed_bytes = 0u64;
    let allow_raw_rgb_passthrough = raw_rgb_passthrough_has_no_geometry_fallback(level, geometry);
    let row_frame_capacity =
        usize::try_from(tiles_across).map_err(|_| WsiDicomError::Unsupported {
            reason: "JPEG Baseline row frame count exceeds platform addressable memory".into(),
        })?;

    for row in 0..tiles_down {
        let mut planned = Vec::with_capacity(row_frame_capacity);
        for col in 0..tiles_across {
            let raw = slide.read_raw_compressed_tile(&TileRequest {
                scene: scene_idx,
                series: series_idx,
                level: level_idx,
                plane: PlaneSelection { z, c, t },
                col: col as i64,
                row: row as i64,
            });

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
                    // Source exposed JPEG bytes, but not with this DICOM frame geometry.
                    // Fall through to decode/re-encode so every frame has fixed Rows/Columns.
                }
                Ok(_) | Err(_) => {}
            }

            let x = col.checked_mul(u64::from(frame_columns)).ok_or_else(|| {
                WsiDicomError::Unsupported {
                    reason: "JPEG Baseline tile x offset overflow".into(),
                }
            })?;
            let y = row.checked_mul(u64::from(frame_rows)).ok_or_else(|| {
                WsiDicomError::Unsupported {
                    reason: "JPEG Baseline tile y offset overflow".into(),
                }
            })?;
            let width = (matrix_columns - x).min(u64::from(frame_columns)) as u32;
            let height = (matrix_rows - y).min(u64::from(frame_rows)) as u32;
            planned.push(JpegBaselinePlannedFrame::Fallback(
                JpegBaselineFallbackFrame {
                    x,
                    y,
                    width,
                    height,
                },
            ));
        }

        let mut index = 0usize;
        while index < planned.len() {
            match &planned[index] {
                JpegBaselinePlannedFrame::Passthrough {
                    data,
                    profile,
                    uncompressed_bytes: frame_uncompressed_bytes,
                } => {
                    ensure_jpeg_baseline_profile(
                        &mut pixel_profile,
                        *profile,
                        "JPEG passthrough pixel profile changed across frames",
                    )?;
                    compressed_bytes = compressed_bytes
                        .saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
                    uncompressed_bytes =
                        uncompressed_bytes.saturating_add(*frame_uncompressed_bytes);
                    let byte_started = Instant::now();
                    pixel_spool.push_frame(data)?;
                    metrics.record_write_duration(byte_started.elapsed());
                    metrics.record_passthrough_frame();
                    metrics.record_pixel_profile(*profile);
                    index += 1;
                }
                JpegBaselinePlannedFrame::Fallback(_) => {
                    let start = index;
                    while index < planned.len()
                        && matches!(planned[index], JpegBaselinePlannedFrame::Fallback(_))
                    {
                        index += 1;
                    }
                    let fallback_frames: Vec<_> = planned[start..index]
                        .iter()
                        .map(|frame| match frame {
                            JpegBaselinePlannedFrame::Fallback(frame) => *frame,
                            JpegBaselinePlannedFrame::Passthrough { .. } => {
                                unreachable!("fallback run contains only fallback frames")
                            }
                        })
                        .collect();

                    #[cfg(all(feature = "metal", target_os = "macos"))]
                    let mut metal_run = try_encode_jpeg_baseline_metal_input_tile_run(
                        slide,
                        &mut metal_input,
                        level,
                        scene_idx,
                        series_idx,
                        level_idx,
                        z,
                        c,
                        t,
                        row,
                        &fallback_frames,
                        frame_columns,
                        frame_rows,
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

                    let mut cpu_batch_results: Vec<Option<EncodedJpegBaselineFrame>> =
                        (0..fallback_frames.len()).map(|_| None).collect();
                    if request.options.encode_backend != EncodeBackendPreference::RequireDevice {
                        let cpu_indices = metal_run
                            .frames
                            .iter()
                            .enumerate()
                            .filter_map(|(idx, frame)| frame.is_none().then_some(idx))
                            .collect::<Vec<_>>();
                        let cpu_frames = cpu_indices
                            .iter()
                            .map(|&idx| fallback_frames[idx])
                            .collect::<Vec<_>>();
                        let cpu_encoded = encode_jpeg_baseline_cpu_input_tile_batch(
                            slide,
                            scene_idx,
                            series_idx,
                            level_idx,
                            z,
                            c,
                            t,
                            &cpu_frames,
                            frame_columns,
                            frame_rows,
                        )?;
                        for (idx, encoded) in cpu_indices.into_iter().zip(cpu_encoded) {
                            cpu_batch_results[idx] = Some(encoded);
                        }
                    }

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
                            if request.options.encode_backend
                                == EncodeBackendPreference::RequireDevice
                            {
                                return Err(WsiDicomError::Unsupported {
                                        reason:
                                            "requested JPEG Baseline device encode backend is unavailable or unsupported"
                                                .into(),
                                    });
                            }
                            cpu_batch_results[idx]
                                .take()
                                .expect("CPU JPEG batch encoded every non-Metal frame")
                        };
                        ensure_jpeg_baseline_profile(
                            &mut pixel_profile,
                            profile,
                            "JPEG Baseline pixel profile changed across frames",
                        )?;
                        compressed_bytes = compressed_bytes
                            .saturating_add(u64::try_from(encoded.data.len()).unwrap_or(u64::MAX));
                        uncompressed_bytes = uncompressed_bytes.saturating_add(
                            jpeg_baseline_fallback_uncompressed_bytes(
                                frame_columns,
                                frame_rows,
                                profile,
                            )?,
                        );
                        let byte_started = Instant::now();
                        pixel_spool.push_frame(&encoded.data)?;
                        metrics.record_write_duration(byte_started.elapsed());
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
                    }
                }
            }
        }
    }

    let profile = pixel_profile.ok_or_else(|| WsiDicomError::Unsupported {
        reason: "slide level produced no frames".into(),
    })?;
    let object = build_dicom_object(
        metadata,
        study_uid,
        &series_uid,
        &sop_instance_uid,
        &frame_of_reference_uid,
        &pyramid_uid,
        &dimension_organization_uid,
        &pyramid_label,
        (series_idx + 1) as u32,
        instance_number,
        level_idx,
        frame_columns,
        frame_rows,
        matrix_columns,
        matrix_rows,
        frame_count,
        profile,
        Some(pixel_spacing_mm),
        pixel_spool.offsets(),
        pixel_spool.lengths(),
        Some(LossyCompressionMetadata {
            method: "ISO_10918_1",
            ratio: (compressed_bytes > 0)
                .then_some(uncompressed_bytes as f64 / compressed_bytes as f64),
        }),
    )?;
    let write_started = Instant::now();
    write_dicom_object_with_spooled_pixel_data(
        &path,
        object,
        FileMetaTableBuilder::new()
            .media_storage_sop_class_uid(VL_WSI_SOP_CLASS_UID)
            .media_storage_sop_instance_uid(&sop_instance_uid)
            .transfer_syntax(request.options.transfer_syntax.uid()),
        &mut pixel_spool,
    )?;
    metrics.record_write_duration(write_started.elapsed());

    Ok(DicomInstanceReport {
        path,
        sop_instance_uid,
        series_instance_uid: series_uid,
        transfer_syntax_uid: request.options.transfer_syntax.uid(),
        level: level_idx,
        z,
        c,
        t,
        frame_count,
        metrics,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JpegBaselineFrameGeometry {
    frame_columns: u32,
    frame_rows: u32,
    tiles_across: u64,
    tiles_down: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JpegBaselineFrameLocation {
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
}

impl JpegBaselineFrameLocation {
    fn first_series_level(level_idx: u32) -> Self {
        Self {
            scene_idx: 0,
            series_idx: 0,
            level_idx,
            z: 0,
            c: 0,
            t: 0,
        }
    }

    fn tile_request(self, col: i64, row: i64) -> TileRequest {
        TileRequest {
            scene: self.scene_idx,
            series: self.series_idx,
            level: self.level_idx,
            plane: PlaneSelection {
                z: self.z,
                c: self.c,
                t: self.t,
            },
            col,
            row,
        }
    }
}

#[derive(Clone, Copy)]
struct JpegBaselineFallbackFrame {
    x: u64,
    y: u64,
    width: u32,
    height: u32,
}

enum JpegBaselinePlannedFrame {
    Passthrough {
        data: Vec<u8>,
        profile: PixelProfile,
        uncompressed_bytes: u64,
    },
    Fallback(JpegBaselineFallbackFrame),
}

struct JpegBaselineMetalEncodedRun {
    frames: Vec<Option<(EncodedJpeg, PixelProfile)>>,
    input_decode_duration: Duration,
    encode_duration: Duration,
    input_decode_batches: u64,
    encode_batches: u64,
}

fn jpeg_baseline_frame_geometry(
    level: &statumen::Level,
    fallback_tile_size: u32,
) -> Result<JpegBaselineFrameGeometry, WsiDicomError> {
    let (matrix_columns, matrix_rows) = level.dimensions;
    let (frame_columns, frame_rows, tiles_across, tiles_down) = match level.tile_layout {
        TileLayout::WholeLevel {
            virtual_tile_width,
            virtual_tile_height,
            ..
        } => {
            if virtual_tile_width == 0 || virtual_tile_height == 0 {
                return Err(WsiDicomError::Unsupported {
                    reason:
                        "JPEG Baseline WholeLevel export requires nonzero virtual tile geometry"
                            .into(),
                });
            }
            (
                virtual_tile_width,
                virtual_tile_height,
                matrix_columns.div_ceil(u64::from(virtual_tile_width)),
                matrix_rows.div_ceil(u64::from(virtual_tile_height)),
            )
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
        TileLayout::Regular { .. } | TileLayout::Irregular { .. } => (
            fallback_tile_size,
            fallback_tile_size,
            matrix_columns.div_ceil(u64::from(fallback_tile_size)),
            matrix_rows.div_ceil(u64::from(fallback_tile_size)),
        ),
    };
    if frame_columns == 0 || frame_rows == 0 {
        return Err(WsiDicomError::Unsupported {
            reason: "JPEG Baseline frame geometry must be nonzero".into(),
        });
    }
    if frame_columns > u16::MAX as u32 || frame_rows > u16::MAX as u32 {
        return Err(WsiDicomError::Unsupported {
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
    })
}

fn jpeg_baseline_route_frame_geometry(
    slide: &Slide,
    level: &statumen::Level,
    location: JpegBaselineFrameLocation,
    fallback_tile_size: u32,
) -> Result<JpegBaselineFrameGeometry, WsiDicomError> {
    if let Some(geometry) =
        jpeg_baseline_native_regular_passthrough_geometry(slide, level, location)?
    {
        return Ok(geometry);
    }
    jpeg_baseline_frame_geometry(level, fallback_tile_size)
}

fn jpeg_baseline_native_regular_passthrough_geometry(
    slide: &Slide,
    level: &statumen::Level,
    location: JpegBaselineFrameLocation,
) -> Result<Option<JpegBaselineFrameGeometry>, WsiDicomError> {
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
        return Err(WsiDicomError::Unsupported {
            reason: "JPEG Baseline Regular export requires nonzero tile geometry".into(),
        });
    }

    let geometry = JpegBaselineFrameGeometry {
        frame_columns: tile_width,
        frame_rows: tile_height,
        tiles_across,
        tiles_down,
    };
    let raw = match slide.read_raw_compressed_tile(&location.tile_request(0, 0)) {
        Ok(raw) => raw,
        Err(_) => return Ok(None),
    };
    if !raw_jpeg_matches_frame_geometry(&raw, tile_width, tile_height) {
        return Ok(None);
    }
    let profile = pixel_profile_from_raw_jpeg_tile(&raw)?;
    if raw_jpeg_profile_can_passthrough(
        profile,
        raw_rgb_passthrough_has_no_geometry_fallback(level, geometry),
    ) {
        Ok(Some(geometry))
    } else {
        Ok(None)
    }
}

fn pixel_profile_from_raw_jpeg_tile(
    raw: &RawCompressedTile,
) -> Result<PixelProfile, WsiDicomError> {
    if raw.compression != Compression::Jpeg {
        return Err(WsiDicomError::Unsupported {
            reason: format!(
                "JPEG passthrough requires JPEG compression, got {:?}",
                raw.compression
            ),
        });
    }
    if raw.bits_allocated != 8 {
        return Err(WsiDicomError::UnsupportedPixelData {
            reason: format!(
                "JPEG passthrough requires 8-bit samples, got {}",
                raw.bits_allocated
            ),
        });
    }
    let photometric_interpretation = match raw.photometric_interpretation {
        EncodedTilePhotometricInterpretation::Monochrome2 => "MONOCHROME2",
        EncodedTilePhotometricInterpretation::Rgb => "RGB",
        EncodedTilePhotometricInterpretation::YbrFull422 => "YBR_FULL_422",
    };
    let components =
        u8::try_from(raw.samples_per_pixel).map_err(|_| WsiDicomError::UnsupportedPixelData {
            reason: format!(
                "JPEG passthrough component count exceeds u8: {}",
                raw.samples_per_pixel
            ),
        })?;
    Ok(PixelProfile {
        components,
        bits_allocated: raw.bits_allocated,
        photometric_interpretation,
    })
}

fn raw_jpeg_profile_can_passthrough(
    profile: PixelProfile,
    allow_raw_rgb_passthrough: bool,
) -> bool {
    profile.photometric_interpretation != "RGB" || allow_raw_rgb_passthrough
}

fn raw_jpeg_matches_frame_geometry(
    raw: &RawCompressedTile,
    frame_columns: u32,
    frame_rows: u32,
) -> bool {
    raw.compression == Compression::Jpeg && raw.width == frame_columns && raw.height == frame_rows
}

fn raw_rgb_passthrough_has_no_geometry_fallback(
    level: &statumen::Level,
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
                && full_frame_grid
        }
        TileLayout::WholeLevel {
            virtual_tile_width,
            virtual_tile_height,
            ..
        } => {
            virtual_tile_width == geometry.frame_columns
                && virtual_tile_height == geometry.frame_rows
        }
        TileLayout::Irregular { .. } => false,
    }
}

fn uncompressed_frame_bytes(raw: &RawCompressedTile) -> Result<u64, WsiDicomError> {
    u64::from(raw.width)
        .checked_mul(u64::from(raw.height))
        .and_then(|pixels| pixels.checked_mul(u64::from(raw.samples_per_pixel)))
        .and_then(|samples| samples.checked_mul(u64::from(raw.bits_allocated / 8)))
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "JPEG passthrough uncompressed frame byte count overflow".into(),
        })
}

fn ensure_jpeg_baseline_profile(
    existing: &mut Option<PixelProfile>,
    profile: PixelProfile,
    mismatch_reason: &'static str,
) -> Result<(), WsiDicomError> {
    if let Some(existing) = existing {
        if *existing != profile {
            return Err(WsiDicomError::UnsupportedPixelData {
                reason: mismatch_reason.into(),
            });
        }
    } else {
        *existing = Some(profile);
    }
    Ok(())
}

fn jpeg_baseline_fallback_uncompressed_bytes(
    frame_columns: u32,
    frame_rows: u32,
    profile: PixelProfile,
) -> Result<u64, WsiDicomError> {
    u64::from(frame_columns)
        .checked_mul(u64::from(frame_rows))
        .and_then(|pixels| pixels.checked_mul(u64::from(profile.components)))
        .and_then(|samples| samples.checked_mul(u64::from(profile.bits_allocated / 8)))
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "JPEG Baseline uncompressed frame byte count overflow".into(),
        })
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

#[allow(clippy::too_many_arguments)]
fn encode_jpeg_baseline_cpu_input_tile_batch(
    slide: &Slide,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    frames: &[JpegBaselineFallbackFrame],
    frame_columns: u32,
    frame_rows: u32,
) -> Result<Vec<EncodedJpegBaselineFrame>, WsiDicomError> {
    frames
        .par_iter()
        .map(|frame| {
            encode_jpeg_baseline_cpu_input_tile(
                slide,
                scene_idx,
                series_idx,
                level_idx,
                z,
                c,
                t,
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                frame_columns,
                frame_rows,
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn encode_jpeg_baseline_cpu_input_tile(
    slide: &Slide,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    x: u64,
    y: u64,
    width: u32,
    height: u32,
    frame_columns: u32,
    frame_rows: u32,
) -> Result<EncodedJpegBaselineFrame, WsiDicomError> {
    let (prepared_bytes, profile, subsampling, input_decode_duration, compose_duration) =
        prepare_jpeg_baseline_cpu_input_tile(
            slide,
            scene_idx,
            series_idx,
            level_idx,
            z,
            c,
            t,
            x,
            y,
            width,
            height,
            frame_columns,
            frame_rows,
        )?;
    let samples = match profile.components {
        1 => JpegSamples::Gray8 {
            data: &prepared_bytes,
            width: frame_columns,
            height: frame_rows,
        },
        3 => JpegSamples::Rgb8 {
            data: &prepared_bytes,
            width: frame_columns,
            height: frame_rows,
        },
        components => {
            return Err(WsiDicomError::UnsupportedPixelData {
                reason: format!("JPEG Baseline supports 1 or 3 components, got {components}"),
            });
        }
    };
    let encode_started = Instant::now();
    let encoded = encode_jpeg_baseline(
        samples,
        JpegEncodeOptions {
            quality: 90,
            subsampling,
            restart_interval: jpeg_baseline_cpu_restart_interval(
                frame_columns,
                frame_rows,
                subsampling,
            ),
            backend: JpegBackend::Cpu,
        },
    )
    .map_err(|source| WsiDicomError::Encode {
        message: source.to_string(),
    })?;
    let encode_duration = encode_started.elapsed();

    Ok((
        encoded,
        profile,
        input_decode_duration,
        compose_duration,
        encode_duration,
    ))
}

#[allow(clippy::too_many_arguments)]
fn prepare_jpeg_baseline_cpu_input_tile(
    slide: &Slide,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    x: u64,
    y: u64,
    width: u32,
    height: u32,
    frame_columns: u32,
    frame_rows: u32,
) -> Result<(Vec<u8>, PixelProfile, JpegSubsampling, Duration, Duration), WsiDicomError> {
    let input_decode_started = Instant::now();
    let region = slide
        .read_region(&RegionRequest {
            scene: SceneId(scene_idx),
            series: SeriesId(series_idx),
            level: LevelIdx(level_idx),
            plane: PlaneIdx(PlaneSelection { z, c, t }),
            origin_px: (x as i64, y as i64),
            size_px: (width, height),
        })
        .map_err(|source| WsiDicomError::SlideRead {
            message: source.to_string(),
        })?;
    let input_decode_duration = input_decode_started.elapsed();

    let compose_started = Instant::now();
    let prepared = prepare_tile_samples(&region, frame_columns, frame_rows)?;
    let (profile, subsampling) = jpeg_baseline_output_profile(prepared.profile)?;
    let compose_duration = compose_started.elapsed();
    Ok((
        prepared.bytes,
        profile,
        subsampling,
        input_decode_duration,
        compose_duration,
    ))
}

fn jpeg_baseline_cpu_restart_interval(
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

fn jpeg_baseline_output_profile(
    source: PixelProfile,
) -> Result<(PixelProfile, JpegSubsampling), WsiDicomError> {
    if source.bits_allocated != 8 {
        return Err(WsiDicomError::UnsupportedPixelData {
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
        components => Err(WsiDicomError::UnsupportedPixelData {
            reason: format!("JPEG Baseline supports 1 or 3 components, got {components}"),
        }),
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn jpeg_baseline_auto_allows_metal_batch(
    preference: EncodeBackendPreference,
    _frame_columns: u32,
    _frame_rows: u32,
    frame_count: usize,
) -> bool {
    match preference {
        EncodeBackendPreference::CpuOnly => false,
        EncodeBackendPreference::PreferDevice | EncodeBackendPreference::RequireDevice => {
            frame_count > 0
        }
        EncodeBackendPreference::Auto => false,
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn lossless_j2k_auto_allows_metal_input(
    preference: EncodeBackendPreference,
    transfer_syntax: TransferSyntax,
    frame_count: u64,
    source_device_decode: bool,
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
            transfer_syntax == TransferSyntax::Htj2kLosslessRpcl
                && (source_device_decode || statumen_device_decode_opted_in())
        }
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
    options: DicomExportOptions,
    level: u32,
    route_scope_frames: u64,
) -> Option<AutoMetalInputRouteCacheKey> {
    (options.encode_backend == EncodeBackendPreference::Auto
        && options.transfer_syntax == TransferSyntax::Htj2kLosslessRpcl)
        .then(|| AutoMetalInputRouteCacheKey {
            source_path: source_path.to_path_buf(),
            level,
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
    level: &statumen::Level,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    row: u64,
    frames: &[JpegBaselineFallbackFrame],
    frame_columns: u32,
    frame_rows: u32,
) -> Result<JpegBaselineMetalEncodedRun, WsiDicomError> {
    objc::rc::autoreleasepool(|| {
        if !jpeg_baseline_auto_allows_metal_batch(
            metal_input.preference,
            frame_columns,
            frame_rows,
            frames.len(),
        ) {
            return Ok(empty_jpeg_baseline_metal_run(frames.len()));
        }
        if !output_frame_maps_to_statumen_tile(level, frame_columns, frame_rows) {
            if metal_input.preference == EncodeBackendPreference::RequireDevice {
                return Err(WsiDicomError::Unsupported {
                    reason:
                        "requested JPEG Baseline Metal fallback requires the DICOM frame grid to align with statumen source tiles"
                            .into(),
                });
            }
            return Ok(empty_jpeg_baseline_metal_run(frames.len()));
        }

        let row_i64 = i64::try_from(row).map_err(|_| WsiDicomError::Unsupported {
            reason: "JPEG Baseline Metal tile row exceeds i64".into(),
        })?;
        let mut requests = Vec::with_capacity(frames.len());
        for frame in frames {
            requests.push(TileRequest {
                scene: scene_idx,
                series: series_idx,
                level: level_idx,
                plane: PlaneSelection { z, c, t },
                col: i64::try_from(frame.x / u64::from(frame_columns)).map_err(|_| {
                    WsiDicomError::Unsupported {
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
                return Err(WsiDicomError::SlideRead {
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
                return Err(WsiDicomError::SlideRead {
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

        let mut tiles = Vec::with_capacity(frames.len());
        for (pixels, frame) in pixels.into_iter().zip(frames.iter()) {
            let TilePixels::Device(DeviceTile::Metal(tile)) = pixels else {
                if metal_input.preference == EncodeBackendPreference::RequireDevice {
                    return Err(WsiDicomError::Unsupported {
                        reason:
                            "requested JPEG Baseline Metal input decode returned CPU pixels; set STATUMEN_JPEG_DEVICE_DECODE=1 or STATUMEN_JP2K_DEVICE_DECODE=1 for compressed WSI tiles"
                                .into(),
                    });
                }
                return Ok(empty_jpeg_baseline_metal_run_with_input_duration(
                    frames.len(),
                    input_decode_duration,
                ));
            };
            if tile.width != frame.width || tile.height != frame.height {
                if metal_input.preference == EncodeBackendPreference::RequireDevice {
                    return Err(WsiDicomError::Unsupported {
                        reason: format!(
                            "JPEG Baseline Metal input geometry changed: expected {}x{}, got {}x{}",
                            frame.width, frame.height, tile.width, tile.height
                        ),
                    });
                }
                return Ok(empty_jpeg_baseline_metal_run_with_input_duration(
                    frames.len(),
                    input_decode_duration,
                ));
            }
            tiles.push(tile);
        }

        let encode_started = Instant::now();
        let encoded = match encode_jpeg_baseline_metal_device_tile_batch(
            &tiles,
            frame_columns,
            frame_rows,
            metal_input.jpeg_encode_session()?,
        ) {
            Ok(encoded) => encoded,
            Err(err) if metal_input.preference == EncodeBackendPreference::RequireDevice => {
                return Err(err);
            }
            Err(_) => return Ok(empty_jpeg_baseline_metal_run(frames.len())),
        };
        let encode_duration = encode_started.elapsed();
        Ok(JpegBaselineMetalEncodedRun {
            frames: encoded.into_iter().map(Some).collect(),
            input_decode_duration,
            encode_duration,
            input_decode_batches: 1,
            encode_batches: 1,
        })
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn encode_jpeg_baseline_metal_device_tile_batch(
    tiles: &[statumen::output::metal::MetalDeviceTile],
    frame_columns: u32,
    frame_rows: u32,
    session: &signinum_jpeg_metal::MetalBackendSession,
) -> Result<Vec<(EncodedJpeg, PixelProfile)>, WsiDicomError> {
    let first = tiles.first().ok_or_else(|| WsiDicomError::Unsupported {
        reason: "JPEG Baseline Metal tile batch is empty".into(),
    })?;
    let source_profile = pixel_profile_from_device_format(first.format)?;
    let (profile, subsampling) = jpeg_baseline_output_profile(source_profile)?;
    let mut requests = Vec::with_capacity(tiles.len());
    for tile in tiles {
        if pixel_profile_from_device_format(tile.format)? != source_profile {
            return Err(WsiDicomError::UnsupportedPixelData {
                reason: "JPEG Baseline Metal tile batch changed pixel profile".into(),
            });
        }
        let statumen::output::metal::MetalDeviceStorage::Buffer {
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
        JpegEncodeOptions {
            quality: 90,
            subsampling,
            restart_interval: None,
            backend: JpegBackend::Metal,
        },
        session,
    )
    .map_err(|source| WsiDicomError::Encode {
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
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    _row: u64,
    _col: u64,
    x: u64,
    y: u64,
    width: u32,
    height: u32,
    tile_size: u32,
) -> Result<
    (
        Result<EncodedDicomJ2kFrame, WsiDicomError>,
        PixelProfile,
        Duration,
        Duration,
    ),
    WsiDicomError,
> {
    let (prepared_bytes, profile, input_decode_duration, compose_duration) =
        prepare_cpu_input_lossless_j2k_tile(
            slide, scene_idx, series_idx, level_idx, z, c, t, x, y, width, height, tile_size,
        )?;
    let samples = J2kLosslessSamples::new(
        &prepared_bytes,
        tile_size,
        tile_size,
        profile.components,
        profile.bits_allocated as u8,
        false,
    )
    .map_err(|source| WsiDicomError::Encode {
        message: source.to_string(),
    })?;
    Ok((
        j2k_encoder.encode(samples),
        profile,
        input_decode_duration,
        compose_duration,
    ))
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
fn encode_cpu_input_lossless_j2k_tile_batch(
    slide: &Slide,
    settings: LosslessJ2kCpuBatchSettings,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    frames: &[LosslessJ2kCpuBatchFrame],
    tile_size: u32,
) -> Result<Vec<LosslessJ2kCpuBatchOutcome>, WsiDicomError> {
    frames
        .par_iter()
        .map(|frame| {
            let (prepared_bytes, profile, input_decode_duration, compose_duration) =
                prepare_cpu_input_lossless_j2k_tile(
                    slide,
                    scene_idx,
                    series_idx,
                    level_idx,
                    z,
                    c,
                    t,
                    frame.x,
                    frame.y,
                    frame.width,
                    frame.height,
                    tile_size,
                )?;
            let samples = J2kLosslessSamples::new(
                &prepared_bytes,
                tile_size,
                tile_size,
                profile.components,
                profile.bits_allocated as u8,
                false,
            )
            .map_err(|source| WsiDicomError::Encode {
                message: source.to_string(),
            })?;
            Ok(LosslessJ2kCpuBatchOutcome {
                encoded: encode::encode_lossless_cpu(
                    samples,
                    settings.transfer_syntax,
                    settings.codec_validation,
                ),
                profile,
                input_decode_duration,
                compose_duration,
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn encode_cpu_input_lossless_j2k_planned_batch(
    slide: &Slide,
    settings: LosslessJ2kCpuBatchSettings,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    planned: &[LosslessJ2kPlannedFrame],
    indices: &[usize],
    tile_size: u32,
) -> Result<Vec<(usize, LosslessJ2kCpuBatchOutcome)>, WsiDicomError> {
    let frames = indices
        .iter()
        .map(|&idx| {
            let planned = &planned[idx];
            LosslessJ2kCpuBatchFrame {
                x: planned.x,
                y: planned.y,
                width: planned.width,
                height: planned.height,
            }
        })
        .collect::<Vec<_>>();
    let outcomes = encode_cpu_input_lossless_j2k_tile_batch(
        slide, settings, scene_idx, series_idx, level_idx, z, c, t, &frames, tile_size,
    )?;
    Ok(indices.iter().copied().zip(outcomes).collect())
}

#[allow(clippy::too_many_arguments)]
fn prepare_cpu_input_lossless_j2k_tile(
    slide: &Slide,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    x: u64,
    y: u64,
    width: u32,
    height: u32,
    tile_size: u32,
) -> Result<(Vec<u8>, PixelProfile, Duration, Duration), WsiDicomError> {
    let input_decode_started = Instant::now();
    let region = slide
        .read_region(&RegionRequest {
            scene: SceneId(scene_idx),
            series: SeriesId(series_idx),
            level: LevelIdx(level_idx),
            plane: PlaneIdx(PlaneSelection { z, c, t }),
            origin_px: (x as i64, y as i64),
            size_px: (width, height),
        })
        .map_err(|source| WsiDicomError::SlideRead {
            message: source.to_string(),
        })?;
    let input_decode_duration = input_decode_started.elapsed();
    let compose_started = Instant::now();
    let prepared = prepare_tile_samples(&region, tile_size, tile_size)?;
    let compose_duration = compose_started.elapsed();
    Ok((
        prepared.bytes,
        prepared.profile,
        input_decode_duration,
        compose_duration,
    ))
}

#[cfg(all(feature = "metal", target_os = "macos"))]
struct MetalEncodedTileRun {
    tiles: Vec<Option<(EncodedDicomJ2kFrame, PixelProfile)>>,
    input_decode_duration: Duration,
    compose_duration: Duration,
    input_decode_batches: u64,
    compose_batches: u64,
    encode_batches: u64,
    gpu_encode_stats: encode::DicomJ2kGpuEncodeBatchStats,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
struct RoutedLosslessJ2kTile {
    encoded: Result<EncodedDicomJ2kFrame, WsiDicomError>,
    profile: PixelProfile,
    used_gpu_input: bool,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
struct CpuEncodedTileRun {
    tiles: Vec<(Result<EncodedDicomJ2kFrame, WsiDicomError>, PixelProfile)>,
    input_decode_duration: Duration,
    compose_duration: Duration,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
struct AutoMetalInputProbeRun {
    tiles: Vec<Option<RoutedLosslessJ2kTile>>,
    input_decode_duration: Duration,
    compose_duration: Duration,
    gpu_input_decode_batches: u64,
    gpu_compose_batches: u64,
    gpu_encode_batches: u64,
    gpu_encode_stats: encode::DicomJ2kGpuEncodeBatchStats,
    probe_cpu_duration: Duration,
    probe_gpu_duration: Duration,
    probe_gpu_batches: u64,
    route: AutoLosslessJ2kRouteDecision,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
struct MetalInputTileReader {
    preference: EncodeBackendPreference,
    source_device_decode: bool,
    auto_device_decode_allowed: bool,
    auto_decision: AutoLosslessJ2kRouteDecision,
    auto_cache_key: Option<AutoMetalInputRouteCacheKey>,
    device: Option<metal::Device>,
    sessions: Option<statumen::output::metal::MetalBackendSessions>,
    jpeg_encode_session: Option<signinum_jpeg_metal::MetalBackendSession>,
    strip_composer: Option<MetalStripComposer>,
    whole_level_cache: MetalSourceTileCache,
    private_jpeg_decode: bool,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
impl MetalInputTileReader {
    fn new(preference: EncodeBackendPreference, source_device_decode: bool) -> Self {
        Self::new_with_auto_device_decode(preference, false, source_device_decode)
    }

    fn new_with_auto_device_decode(
        preference: EncodeBackendPreference,
        auto_device_decode_allowed: bool,
        source_device_decode: bool,
    ) -> Self {
        Self::new_with_auto_device_decode_and_cache_key(
            preference,
            auto_device_decode_allowed,
            None,
            source_device_decode,
        )
    }

    fn new_for_lossless_j2k(
        preference: EncodeBackendPreference,
        auto_device_decode_allowed: bool,
        auto_cache_key: Option<AutoMetalInputRouteCacheKey>,
        source_device_decode: bool,
    ) -> Self {
        let mut reader = Self::new_with_auto_device_decode_and_cache_key(
            preference,
            auto_device_decode_allowed,
            auto_cache_key,
            source_device_decode,
        );
        if source_device_decode {
            reader.enable_private_jpeg_decode();
        }
        reader
    }

    fn new_with_auto_device_decode_and_cache_key(
        preference: EncodeBackendPreference,
        auto_device_decode_allowed: bool,
        auto_cache_key: Option<AutoMetalInputRouteCacheKey>,
        source_device_decode: bool,
    ) -> Self {
        let cached_decision =
            if preference == EncodeBackendPreference::Auto && auto_device_decode_allowed {
                auto_cache_key
                    .as_ref()
                    .and_then(cached_auto_metal_input_decision)
            } else {
                None
            };
        let auto_decision = cached_decision.unwrap_or(AutoLosslessJ2kRouteDecision::Undecided);
        let auto_device_decode_allowed = auto_device_decode_allowed
            && matches!(
                auto_decision,
                AutoLosslessJ2kRouteDecision::Undecided
                    | AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode
            );
        Self {
            preference,
            source_device_decode,
            auto_device_decode_allowed,
            auto_decision,
            auto_cache_key,
            device: None,
            sessions: None,
            jpeg_encode_session: None,
            strip_composer: None,
            whole_level_cache: MetalSourceTileCache::default(),
            private_jpeg_decode: false,
        }
    }

    fn enable_private_jpeg_decode(&mut self) {
        self.private_jpeg_decode = true;
    }

    fn enabled(&self) -> bool {
        match self.preference {
            EncodeBackendPreference::CpuOnly => false,
            EncodeBackendPreference::Auto => {
                self.auto_device_decode_allowed
                    && matches!(
                        self.auto_decision,
                        AutoLosslessJ2kRouteDecision::Undecided
                            | AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode
                    )
            }
            EncodeBackendPreference::PreferDevice | EncodeBackendPreference::RequireDevice => true,
        }
    }

    fn auto_input_probe_pending(&self) -> bool {
        self.preference == EncodeBackendPreference::Auto
            && self.auto_device_decode_allowed
            && self.auto_decision == AutoLosslessJ2kRouteDecision::Undecided
    }

    fn auto_route_decision(&self) -> AutoLosslessJ2kRouteDecision {
        self.auto_decision
    }

    fn record_auto_route_probe_decision(&mut self, route: AutoLosslessJ2kRouteDecision) {
        if self.preference != EncodeBackendPreference::Auto {
            return;
        }
        self.auto_decision = route;
        self.auto_device_decode_allowed =
            route == AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode;
        if let Some(key) = &self.auto_cache_key {
            store_cached_auto_metal_input_decision(key, route);
        }
    }

    fn sessions(&mut self) -> Result<statumen::output::metal::MetalBackendSessions, WsiDicomError> {
        if self.sessions.is_none() {
            let device =
                metal::Device::system_default().ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "Metal is unavailable for WSI input tile decode".into(),
                })?;
            self.device = Some(device.clone());
            let sessions = statumen::output::metal::MetalBackendSessions::new(
                signinum_jpeg_metal::MetalBackendSession::new(device.clone()),
                signinum_j2k_metal::MetalBackendSession::new(device),
            );
            self.sessions = Some(if self.private_jpeg_decode {
                sessions.with_private_jpeg_decode()
            } else {
                sessions
            });
        }
        Ok(self
            .sessions
            .as_ref()
            .expect("Metal input sessions initialized")
            .clone())
    }

    fn source_tile_output_preference(&mut self) -> Result<TileOutputPreference, WsiDicomError> {
        let sessions = self.sessions()?;
        Ok(match (self.preference, self.source_device_decode) {
            (EncodeBackendPreference::RequireDevice, true) => {
                TileOutputPreference::require_device_auto_with_metal_and_compressed_decode(sessions)
            }
            (_, true) => {
                TileOutputPreference::prefer_device_auto_with_metal_and_compressed_decode(sessions)
            }
            _ => TileOutputPreference::prefer_device_auto_with_metal(sessions),
        })
    }

    fn strip_composer(&mut self) -> Result<&MetalStripComposer, WsiDicomError> {
        if self.strip_composer.is_none() {
            let _ = self.sessions()?;
            let device = self
                .device
                .as_ref()
                .expect("Metal input device initialized")
                .clone();
            self.strip_composer = Some(MetalStripComposer::new(device)?);
        }
        Ok(self
            .strip_composer
            .as_ref()
            .expect("Metal strip composer initialized"))
    }

    fn jpeg_encode_session(
        &mut self,
    ) -> Result<&signinum_jpeg_metal::MetalBackendSession, WsiDicomError> {
        if self.jpeg_encode_session.is_none() {
            let _ = self.sessions()?;
            let device = self
                .device
                .as_ref()
                .expect("Metal input device initialized")
                .clone();
            self.jpeg_encode_session = Some(signinum_jpeg_metal::MetalBackendSession::new(device));
        }
        Ok(self
            .jpeg_encode_session
            .as_ref()
            .expect("JPEG Baseline Metal encode session initialized"))
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn statumen_device_decode_opted_in() -> bool {
    env_flag_enabled(STATUMEN_JPEG_DEVICE_DECODE_ENV)
        || env_flag_enabled(STATUMEN_JP2K_DEVICE_DECODE_ENV)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn env_flag_enabled(name: &str) -> bool {
    std::env::var(name)
        .map(|value| {
            matches!(
                value.as_str(),
                "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
            )
        })
        .unwrap_or(false)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
const METAL_WHOLE_LEVEL_SOURCE_TILE_CACHE_CAPACITY: usize = 512;

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct MetalSourceTileKey {
    scene: usize,
    series: usize,
    level: u32,
    z: u32,
    c: u32,
    t: u32,
    col: i64,
    row: i64,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
struct MetalSourceTileCache {
    capacity: usize,
    entries: HashMap<MetalSourceTileKey, statumen::output::metal::MetalDeviceTile>,
    order: VecDeque<MetalSourceTileKey>,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
impl Default for MetalSourceTileCache {
    fn default() -> Self {
        Self {
            capacity: METAL_WHOLE_LEVEL_SOURCE_TILE_CACHE_CAPACITY,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
impl MetalSourceTileCache {
    fn get(&mut self, key: MetalSourceTileKey) -> Option<statumen::output::metal::MetalDeviceTile> {
        let tile = self.entries.get(&key)?.clone();
        self.touch(key);
        Some(tile)
    }

    fn insert(&mut self, key: MetalSourceTileKey, tile: statumen::output::metal::MetalDeviceTile) {
        if self.capacity == 0 {
            return;
        }
        self.entries.insert(key, tile);
        self.touch(key);
        while self.entries.len() > self.capacity {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
    }

    fn touch(&mut self, key: MetalSourceTileKey) {
        self.order.retain(|existing| existing != &key);
        self.order.push_back(key);
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
struct PackedMetalStrips {
    buffer: metal::Buffer,
    first_col: i64,
    first_row: i64,
    tiles_across: u32,
    tile_width: u32,
    tile_height: u32,
    slot_stride: usize,
    tile_slot_bytes: usize,
    format: SigninumPixelFormat,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, Copy)]
struct MetalComposeTileRequest {
    src_origin_x: u32,
    src_origin_y: u32,
    valid_width: u32,
    valid_height: u32,
    output_width: u32,
    output_height: u32,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
struct MetalComposeTileDispatch {
    request: MetalComposeTileRequest,
    params: MetalComposeStripsParams,
    dst_buffer: metal::Buffer,
    dst_stride: usize,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
struct MetalStripComposer {
    device: metal::Device,
    queue: metal::CommandQueue,
    pipeline: metal::ComputePipelineState,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
impl MetalStripComposer {
    fn new(device: metal::Device) -> Result<Self, WsiDicomError> {
        let options = metal::CompileOptions::new();
        let library = device
            .new_library_with_source(WSI_COMPOSE_STRIPS_METAL, &options)
            .map_err(|message| WsiDicomError::Encode {
                message: format!("Metal strip compose shader failed to compile: {message}"),
            })?;
        let function = library
            .get_function("wsi_compose_strips", None)
            .map_err(|message| WsiDicomError::Encode {
                message: format!("Metal strip compose function unavailable: {message}"),
            })?;
        let pipeline = device
            .new_compute_pipeline_state_with_function(&function)
            .map_err(|message| WsiDicomError::Encode {
                message: format!("Metal strip compose pipeline unavailable: {message}"),
            })?;
        let queue = device.new_command_queue();
        Ok(Self {
            device,
            queue,
            pipeline,
        })
    }

    fn pack_tiles(
        &self,
        tiles: &[statumen::output::metal::MetalDeviceTile],
        layout: WholeLevelStripLayout,
        first_col: i64,
        first_row: i64,
        tiles_across: usize,
    ) -> Result<PackedMetalStrips, WsiDicomError> {
        let first = tiles.first().ok_or_else(|| WsiDicomError::Unsupported {
            reason: "Metal WholeLevel composition requires at least one source tile".into(),
        })?;
        let format = first.format;
        let bytes_per_pixel = format.bytes_per_pixel();
        let slot_stride = (layout.width as usize)
            .checked_mul(bytes_per_pixel)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel source slot stride overflow".into(),
            })?;
        let tile_height_usize =
            usize::try_from(layout.height).map_err(|_| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel source tile height exceeds platform addressable memory"
                    .into(),
            })?;
        let tile_slot_bytes = slot_stride.checked_mul(tile_height_usize).ok_or_else(|| {
            WsiDicomError::Unsupported {
                reason: "Metal WholeLevel source tile slot byte length overflow".into(),
            }
        })?;
        let total_bytes =
            tile_slot_bytes
                .checked_mul(tiles.len())
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "Metal packed WholeLevel tile byte length overflow".into(),
                })?;
        let tiles_across_u32 =
            u32::try_from(tiles_across).map_err(|_| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel source tile columns exceed u32".into(),
            })?;
        if tiles_across == 0 || !tiles.len().is_multiple_of(tiles_across) {
            return Err(WsiDicomError::Unsupported {
                reason: "Metal WholeLevel source tile grid is not rectangular".into(),
            });
        }
        let total_bytes_u64 =
            u64::try_from(total_bytes).map_err(|_| WsiDicomError::Unsupported {
                reason: "Metal packed WholeLevel tile byte length exceeds u64".into(),
            })?;
        let packed = self.device.new_buffer(
            total_bytes_u64,
            metal::MTLResourceOptions::StorageModeShared,
        );
        let command_buffer = self.queue.new_command_buffer();
        let blit = command_buffer.new_blit_command_encoder();

        for (idx, tile) in tiles.iter().enumerate() {
            if tile.format != format {
                return Err(WsiDicomError::Unsupported {
                    reason: "Metal WholeLevel composition requires uniform source tile format"
                        .into(),
                });
            }
            if tile.width == 0
                || tile.height == 0
                || tile.width > layout.width
                || tile.height > layout.height
            {
                return Err(WsiDicomError::Unsupported {
                    reason: format!(
                        "Metal WholeLevel source tile geometry exceeds virtual tile: got {}x{}, expected <= {}x{}",
                        tile.width, tile.height, layout.width, layout.height
                    ),
                });
            }
            let row_bytes = (tile.width as usize)
                .checked_mul(bytes_per_pixel)
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "Metal WholeLevel source tile row byte length overflow".into(),
                })?;
            if tile.pitch_bytes < row_bytes {
                return Err(WsiDicomError::Unsupported {
                    reason: "Metal WholeLevel source tile pitch is smaller than row bytes".into(),
                });
            }
            let statumen::output::metal::MetalDeviceStorage::Buffer {
                buffer,
                byte_offset,
            } = &tile.storage;
            let slot_offset =
                idx.checked_mul(tile_slot_bytes)
                    .ok_or_else(|| WsiDicomError::Unsupported {
                        reason: "Metal packed WholeLevel destination offset overflow".into(),
                    })?;
            for source_row in 0..tile.height as usize {
                let source_offset = byte_offset
                    .checked_add(source_row.checked_mul(tile.pitch_bytes).ok_or_else(|| {
                        WsiDicomError::Unsupported {
                            reason: "Metal WholeLevel source row offset overflow".into(),
                        }
                    })?)
                    .ok_or_else(|| WsiDicomError::Unsupported {
                        reason: "Metal WholeLevel source row offset overflow".into(),
                    })?;
                let destination_offset = slot_offset
                    .checked_add(source_row.checked_mul(slot_stride).ok_or_else(|| {
                        WsiDicomError::Unsupported {
                            reason: "Metal WholeLevel destination row offset overflow".into(),
                        }
                    })?)
                    .ok_or_else(|| WsiDicomError::Unsupported {
                        reason: "Metal WholeLevel destination row offset overflow".into(),
                    })?;
                blit.copy_from_buffer(
                    buffer,
                    u64::try_from(source_offset).map_err(|_| WsiDicomError::Unsupported {
                        reason: "Metal WholeLevel source row offset exceeds u64".into(),
                    })?,
                    &packed,
                    u64::try_from(destination_offset).map_err(|_| WsiDicomError::Unsupported {
                        reason: "Metal WholeLevel destination row offset exceeds u64".into(),
                    })?,
                    u64::try_from(row_bytes).map_err(|_| WsiDicomError::Unsupported {
                        reason: "Metal WholeLevel source row byte length exceeds u64".into(),
                    })?,
                );
            }
        }

        blit.end_encoding();
        command_buffer.commit();
        command_buffer.wait_until_completed();

        Ok(PackedMetalStrips {
            buffer: packed,
            first_col,
            first_row,
            tiles_across: tiles_across_u32,
            tile_width: layout.width,
            tile_height: layout.height,
            slot_stride,
            tile_slot_bytes,
            format,
        })
    }

    #[allow(dead_code, clippy::too_many_arguments)]
    fn compose_tile(
        &self,
        packed: &PackedMetalStrips,
        src_origin_x: u32,
        src_origin_y: u32,
        valid_width: u32,
        valid_height: u32,
        output_width: u32,
        output_height: u32,
    ) -> Result<statumen::output::metal::MetalDeviceTile, WsiDicomError> {
        let mut tiles = self.compose_tiles(
            packed,
            &[MetalComposeTileRequest {
                src_origin_x,
                src_origin_y,
                valid_width,
                valid_height,
                output_width,
                output_height,
            }],
        )?;
        tiles.pop().ok_or_else(|| WsiDicomError::Unsupported {
            reason: "Metal composed tile batch unexpectedly returned no tiles".into(),
        })
    }

    fn compose_tiles(
        &self,
        packed: &PackedMetalStrips,
        requests: &[MetalComposeTileRequest],
    ) -> Result<Vec<statumen::output::metal::MetalDeviceTile>, WsiDicomError> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }
        let first_col =
            u32::try_from(packed.first_col).map_err(|_| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel first source tile column exceeds u32".into(),
            })?;
        let first_row =
            u32::try_from(packed.first_row).map_err(|_| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel first source tile row exceeds u32".into(),
            })?;
        let bytes_per_pixel = packed.format.bytes_per_pixel();
        let bytes_per_pixel_u32 =
            u32::try_from(bytes_per_pixel).map_err(|_| WsiDicomError::Unsupported {
                reason: "Metal composed tile bytes-per-pixel exceeds u32".into(),
            })?;
        let src_slot_stride =
            u32::try_from(packed.slot_stride).map_err(|_| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel source slot stride exceeds u32".into(),
            })?;
        let src_tile_slot_bytes =
            u32::try_from(packed.tile_slot_bytes).map_err(|_| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel source tile slot byte length exceeds u32".into(),
            })?;
        let mut dispatches = Vec::with_capacity(requests.len());
        for request in requests {
            let dst_stride = (request.output_width as usize)
                .checked_mul(bytes_per_pixel)
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "Metal composed tile stride overflow".into(),
                })?;
            let dst_bytes = dst_stride
                .checked_mul(request.output_height as usize)
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "Metal composed tile byte length overflow".into(),
                })?;
            let dst_bytes_u64 =
                u64::try_from(dst_bytes).map_err(|_| WsiDicomError::Unsupported {
                    reason: "Metal composed tile byte length exceeds u64".into(),
                })?;
            let dst_buffer = self
                .device
                .new_buffer(dst_bytes_u64, metal::MTLResourceOptions::StorageModeShared);
            let params = MetalComposeStripsParams {
                src_origin_x: request.src_origin_x,
                src_origin_y: request.src_origin_y,
                valid_width: request.valid_width,
                valid_height: request.valid_height,
                output_width: request.output_width,
                output_height: request.output_height,
                bytes_per_pixel: bytes_per_pixel_u32,
                src_tile_width: packed.tile_width,
                src_tile_height: packed.tile_height,
                src_slot_stride,
                src_tile_slot_bytes,
                src_first_col: first_col,
                src_first_row: first_row,
                src_tiles_across: packed.tiles_across,
                dst_stride: u32::try_from(dst_stride).map_err(|_| WsiDicomError::Unsupported {
                    reason: "Metal composed tile pitch exceeds u32".into(),
                })?,
            };
            dispatches.push(MetalComposeTileDispatch {
                request: *request,
                params,
                dst_buffer,
                dst_stride,
            });
        }

        let command_buffer = self.queue.new_command_buffer();
        let encoder = command_buffer.new_compute_command_encoder();
        encoder.set_compute_pipeline_state(&self.pipeline);
        encoder.set_buffer(0, Some(&packed.buffer), 0);
        let width = self.pipeline.thread_execution_width().max(1);
        let max_threads = self.pipeline.max_total_threads_per_threadgroup().max(width);
        let height = (max_threads / width).max(1);
        for dispatch in &dispatches {
            encoder.set_buffer(1, Some(&dispatch.dst_buffer), 0);
            encoder.set_bytes(
                2,
                core::mem::size_of::<MetalComposeStripsParams>() as u64,
                (&raw const dispatch.params).cast(),
            );
            encoder.dispatch_threads(
                metal::MTLSize {
                    width: u64::from(dispatch.request.output_width),
                    height: u64::from(dispatch.request.output_height),
                    depth: 1,
                },
                metal::MTLSize {
                    width,
                    height,
                    depth: 1,
                },
            );
        }
        encoder.end_encoding();
        command_buffer.commit();
        command_buffer.wait_until_completed();

        Ok(dispatches
            .into_iter()
            .map(|dispatch| statumen::output::metal::MetalDeviceTile {
                width: dispatch.request.output_width,
                height: dispatch.request.output_height,
                pitch_bytes: dispatch.dst_stride,
                format: packed.format,
                storage: statumen::output::metal::MetalDeviceStorage::Buffer {
                    buffer: dispatch.dst_buffer,
                    byte_offset: 0,
                },
            })
            .collect())
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[repr(C)]
#[derive(Clone, Copy)]
struct MetalComposeStripsParams {
    src_origin_x: u32,
    src_origin_y: u32,
    valid_width: u32,
    valid_height: u32,
    output_width: u32,
    output_height: u32,
    bytes_per_pixel: u32,
    src_tile_width: u32,
    src_tile_height: u32,
    src_slot_stride: u32,
    src_tile_slot_bytes: u32,
    src_first_col: u32,
    src_first_row: u32,
    src_tiles_across: u32,
    dst_stride: u32,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
const WSI_COMPOSE_STRIPS_METAL: &str = r#"
#include <metal_stdlib>
using namespace metal;

struct MetalComposeStripsParams {
    uint src_origin_x;
    uint src_origin_y;
    uint valid_width;
    uint valid_height;
    uint output_width;
    uint output_height;
    uint bytes_per_pixel;
    uint src_tile_width;
    uint src_tile_height;
    uint src_slot_stride;
    uint src_tile_slot_bytes;
    uint src_first_col;
    uint src_first_row;
    uint src_tiles_across;
    uint dst_stride;
};

kernel void wsi_compose_strips(
    device const uchar *src [[buffer(0)]],
    device uchar *dst [[buffer(1)]],
    constant MetalComposeStripsParams &params [[buffer(2)]],
    uint2 gid [[thread_position_in_grid]]
) {
    if (gid.x >= params.output_width || gid.y >= params.output_height) {
        return;
    }

    const uint dst_idx = gid.y * params.dst_stride + gid.x * params.bytes_per_pixel;
    const bool inside = gid.x < params.valid_width && gid.y < params.valid_height;
    if (!inside) {
        for (uint byte_idx = 0u; byte_idx < params.bytes_per_pixel; ++byte_idx) {
            dst[dst_idx + byte_idx] = uchar(0);
        }
        return;
    }

    const uint global_x = params.src_origin_x + gid.x;
    const uint global_y = params.src_origin_y + gid.y;
    const uint source_col = global_x / params.src_tile_width;
    const uint source_row = global_y / params.src_tile_height;
    const uint in_tile_x = global_x - source_col * params.src_tile_width;
    const uint in_tile_y = global_y - source_row * params.src_tile_height;
    const uint packed_col = source_col - params.src_first_col;
    const uint packed_row = source_row - params.src_first_row;
    const uint tile_idx = packed_row * params.src_tiles_across + packed_col;
    const uint src_idx = tile_idx * params.src_tile_slot_bytes
        + in_tile_y * params.src_slot_stride
        + in_tile_x * params.bytes_per_pixel;
    for (uint byte_idx = 0u; byte_idx < params.bytes_per_pixel; ++byte_idx) {
        dst[dst_idx + byte_idx] = src[src_idx + byte_idx];
    }
}
"#;

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn try_encode_metal_input_tile_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    level: &statumen::Level,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    row: u64,
    start_col: u64,
    tile_count: u64,
    matrix_columns: u64,
    matrix_rows: u64,
    tile_size: u32,
) -> Result<MetalEncodedTileRun, WsiDicomError> {
    // Long NDPI exports create thousands of autoreleased Metal/ObjC temporaries.
    // Drain them per run so later rows do not encode zero-filled composed buffers.
    objc::rc::autoreleasepool(|| {
        let tile_count = usize::try_from(tile_count).map_err(|_| WsiDicomError::Unsupported {
            reason: "tile batch size exceeds platform addressable memory".into(),
        })?;

        if !metal_input.enabled() {
            return Ok(empty_metal_tile_run(tile_count));
        }

        if output_tile_maps_to_statumen_tile(level, tile_size) {
            return try_encode_metal_aligned_tile_run(
                slide,
                metal_input,
                j2k_encoder,
                level,
                scene_idx,
                series_idx,
                level_idx,
                z,
                c,
                t,
                row,
                start_col,
                tile_count,
                matrix_columns,
                matrix_rows,
                tile_size,
            );
        }

        if let Some(source_layout) = regular_tiled_source_layout(level) {
            return try_encode_metal_whole_level_strip_run(
                slide,
                metal_input,
                j2k_encoder,
                source_layout,
                scene_idx,
                series_idx,
                level_idx,
                z,
                c,
                t,
                row,
                start_col,
                tile_count,
                matrix_columns,
                matrix_rows,
                tile_size,
            );
        }

        if let Some(strip_layout) = whole_level_strip_layout(level) {
            return try_encode_metal_whole_level_strip_run(
                slide,
                metal_input,
                j2k_encoder,
                strip_layout,
                scene_idx,
                series_idx,
                level_idx,
                z,
                c,
                t,
                row,
                start_col,
                tile_count,
                matrix_columns,
                matrix_rows,
                tile_size,
            );
        }

        if metal_input.preference == EncodeBackendPreference::RequireDevice {
            return Err(WsiDicomError::Unsupported {
                reason:
                    "requested Metal input tile decode requires a DICOM tile grid that can be sourced from aligned statumen tiles, regular tiled composition, or WholeLevel strip tiles"
                        .into(),
            });
        }
        Ok(empty_metal_tile_run(tile_count))
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn probe_auto_metal_input_tile_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    level: &statumen::Level,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    row: u64,
    planned: &[LosslessJ2kPlannedFrame],
    route_scope_frames: usize,
    matrix_columns: u64,
    matrix_rows: u64,
    tile_size: u32,
) -> Result<AutoMetalInputProbeRun, WsiDicomError> {
    let first = planned.first().ok_or_else(|| WsiDicomError::Unsupported {
        reason: "auto Metal input route probe requires at least one tile".into(),
    })?;
    let tile_count = u64::try_from(planned.len()).map_err(|_| WsiDicomError::Unsupported {
        reason: "auto Metal input route probe tile count exceeds u64".into(),
    })?;

    let metal_run = try_encode_metal_input_tile_run(
        slide,
        metal_input,
        j2k_encoder,
        level,
        scene_idx,
        series_idx,
        level_idx,
        z,
        c,
        t,
        row,
        first.col,
        tile_count,
        matrix_columns,
        matrix_rows,
        tile_size,
    )?;
    let mut cpu_probe_encoder = j2k_encoder.cpu_only_peer();
    let cpu_run = encode_cpu_input_planned_tile_run(
        slide,
        &mut cpu_probe_encoder,
        scene_idx,
        series_idx,
        level_idx,
        z,
        c,
        t,
        row,
        planned,
        tile_size,
    )?;
    let partial_gpu_run =
        if cpu_input_device_encode_auto_probe_allowed(&cpu_run, route_scope_frames) {
            let mut partial_probe_encoder = j2k_encoder.require_device_peer();
            Some(encode_cpu_input_planned_tile_run(
                slide,
                &mut partial_probe_encoder,
                scene_idx,
                series_idx,
                level_idx,
                z,
                c,
                t,
                row,
                planned,
                tile_size,
            )?)
        } else {
            None
        };

    let resident_gpu_complete = metal_run.tiles.iter().all(Option::is_some);
    let partial_gpu_complete = partial_gpu_run.as_ref().is_some_and(|partial_gpu_run| {
        partial_gpu_run
            .tiles
            .iter()
            .all(|(encoded, _)| matches!(encoded, Ok(encoded) if encoded.used_device_encode))
    });
    let cpu_complete = cpu_run.tiles.iter().all(|(encoded, _)| encoded.is_ok());
    let resident_gpu_duration = metal_encoded_tile_run_total_duration(&metal_run);
    let partial_gpu_duration = partial_gpu_run
        .as_ref()
        .map(cpu_encoded_tile_run_total_duration)
        .unwrap_or(Duration::ZERO);
    let cpu_duration = cpu_encoded_tile_run_total_duration(&cpu_run);
    let route = select_auto_lossless_j2k_probe_route(
        AutoLosslessJ2kRouteCandidate {
            complete: cpu_complete,
            duration: cpu_duration,
        },
        AutoLosslessJ2kRouteCandidate {
            complete: partial_gpu_complete,
            duration: partial_gpu_duration,
        },
        AutoLosslessJ2kRouteCandidate {
            complete: resident_gpu_complete,
            duration: resident_gpu_duration,
        },
    );
    metal_input.record_auto_route_probe_decision(route);
    if route == AutoLosslessJ2kRouteDecision::CpuOnly {
        j2k_encoder.force_cpu_only_for_auto();
    }

    let probe_gpu_batches = metal_run
        .input_decode_batches
        .saturating_add(metal_run.compose_batches)
        .saturating_add(metal_run.encode_batches);
    let metal_input_decode_duration = metal_run.input_decode_duration;
    let metal_compose_duration = metal_run.compose_duration;
    let metal_input_decode_batches = metal_run.input_decode_batches;
    let metal_compose_batches = metal_run.compose_batches;
    let metal_encode_batches = metal_run.encode_batches;
    let metal_gpu_encode_stats = metal_run.gpu_encode_stats;
    let cpu_input_decode_duration = cpu_run.input_decode_duration;
    let cpu_compose_duration = cpu_run.compose_duration;
    match route {
        AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode => Ok(AutoMetalInputProbeRun {
            tiles: metal_run
                .tiles
                .into_iter()
                .map(|entry| {
                    entry.map(|(encoded, profile)| RoutedLosslessJ2kTile {
                        encoded: Ok(encoded),
                        profile,
                        used_gpu_input: true,
                    })
                })
                .collect(),
            input_decode_duration: metal_input_decode_duration,
            compose_duration: metal_compose_duration,
            gpu_input_decode_batches: metal_input_decode_batches,
            gpu_compose_batches: metal_compose_batches,
            gpu_encode_batches: metal_encode_batches,
            gpu_encode_stats: metal_gpu_encode_stats,
            probe_cpu_duration: cpu_duration,
            probe_gpu_duration: resident_gpu_duration,
            probe_gpu_batches,
            route,
        }),
        AutoLosslessJ2kRouteDecision::CpuInputDeviceEncode => {
            let partial_gpu_run = partial_gpu_run.ok_or_else(|| WsiDicomError::Unsupported {
                reason: "auto route selected CPU-input device encode without a completed probe"
                    .into(),
            })?;
            Ok(AutoMetalInputProbeRun {
                tiles: partial_gpu_run
                    .tiles
                    .into_iter()
                    .map(|(encoded, profile)| {
                        Some(RoutedLosslessJ2kTile {
                            encoded,
                            profile,
                            used_gpu_input: false,
                        })
                    })
                    .collect(),
                input_decode_duration: partial_gpu_run.input_decode_duration,
                compose_duration: partial_gpu_run.compose_duration,
                gpu_input_decode_batches: 0,
                gpu_compose_batches: 0,
                gpu_encode_batches: 0,
                gpu_encode_stats: encode::DicomJ2kGpuEncodeBatchStats::default(),
                probe_cpu_duration: cpu_duration,
                probe_gpu_duration: resident_gpu_duration,
                probe_gpu_batches,
                route,
            })
        }
        AutoLosslessJ2kRouteDecision::CpuOnly | AutoLosslessJ2kRouteDecision::Undecided => {
            Ok(AutoMetalInputProbeRun {
                tiles: cpu_run
                    .tiles
                    .into_iter()
                    .map(|(encoded, profile)| {
                        Some(RoutedLosslessJ2kTile {
                            encoded,
                            profile,
                            used_gpu_input: false,
                        })
                    })
                    .collect(),
                input_decode_duration: cpu_input_decode_duration,
                compose_duration: cpu_compose_duration,
                gpu_input_decode_batches: 0,
                gpu_compose_batches: 0,
                gpu_encode_batches: 0,
                gpu_encode_stats: encode::DicomJ2kGpuEncodeBatchStats::default(),
                probe_cpu_duration: cpu_duration,
                probe_gpu_duration: resident_gpu_duration,
                probe_gpu_batches,
                route,
            })
        }
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn encode_cpu_input_planned_tile_run(
    slide: &Slide,
    j2k_encoder: &mut DicomJ2kEncoder,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    row: u64,
    planned: &[LosslessJ2kPlannedFrame],
    tile_size: u32,
) -> Result<CpuEncodedTileRun, WsiDicomError> {
    let mut tiles = Vec::with_capacity(planned.len());
    let mut input_decode_duration = Duration::ZERO;
    let mut compose_duration = Duration::ZERO;
    for planned_frame in planned {
        let (encoded, profile, frame_input_decode_duration, frame_compose_duration) =
            encode_cpu_input_tile(
                slide,
                j2k_encoder,
                scene_idx,
                series_idx,
                level_idx,
                z,
                c,
                t,
                row,
                planned_frame.col,
                planned_frame.x,
                planned_frame.y,
                planned_frame.width,
                planned_frame.height,
                tile_size,
            )?;
        input_decode_duration = input_decode_duration.saturating_add(frame_input_decode_duration);
        compose_duration = compose_duration.saturating_add(frame_compose_duration);
        tiles.push((encoded, profile));
    }
    Ok(CpuEncodedTileRun {
        tiles,
        input_decode_duration,
        compose_duration,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn cpu_encoded_tile_run_total_duration(run: &CpuEncodedTileRun) -> Duration {
    run.tiles.iter().fold(
        run.input_decode_duration
            .saturating_add(run.compose_duration),
        |duration, (encoded, _)| match encoded {
            Ok(encoded) => duration
                .saturating_add(encoded.encode_duration)
                .saturating_add(encoded.validation_duration),
            Err(_) => duration,
        },
    )
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn cpu_input_device_encode_auto_allowed(run: &CpuEncodedTileRun) -> bool {
    run.tiles.iter().all(|(_, profile)| {
        matches!(profile.components, 1 | 3) && matches!(profile.bits_allocated, 8 | 16)
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn cpu_input_device_encode_auto_probe_allowed(run: &CpuEncodedTileRun, frame_count: usize) -> bool {
    frame_count >= LOSSLESS_J2K_AUTO_PARTIAL_GPU_MIN_FRAMES
        && cpu_input_device_encode_auto_allowed(run)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn metal_encoded_tile_run_total_duration(run: &MetalEncodedTileRun) -> Duration {
    run.tiles.iter().fold(
        run.input_decode_duration
            .saturating_add(run.compose_duration),
        |duration, encoded| match encoded {
            Some((encoded, _)) => duration
                .saturating_add(encoded.encode_duration)
                .saturating_add(encoded.validation_duration),
            None => duration,
        },
    )
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AutoLosslessJ2kRouteCandidate {
    complete: bool,
    duration: Duration,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn select_auto_lossless_j2k_probe_route(
    cpu_only: AutoLosslessJ2kRouteCandidate,
    cpu_input_device_encode: AutoLosslessJ2kRouteCandidate,
    gpu_input_device_encode: AutoLosslessJ2kRouteCandidate,
) -> AutoLosslessJ2kRouteDecision {
    if !cpu_only.complete {
        return [
            (
                AutoLosslessJ2kRouteDecision::CpuInputDeviceEncode,
                cpu_input_device_encode,
            ),
            (
                AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode,
                gpu_input_device_encode,
            ),
        ]
        .into_iter()
        .filter(|(_, candidate)| candidate.complete)
        .min_by_key(|(_, candidate)| candidate.duration)
        .map(|(route, _)| route)
        .unwrap_or(AutoLosslessJ2kRouteDecision::CpuOnly);
    }

    let mut selected = (AutoLosslessJ2kRouteDecision::CpuOnly, cpu_only.duration);
    for (route, candidate) in [
        (
            AutoLosslessJ2kRouteDecision::CpuInputDeviceEncode,
            cpu_input_device_encode,
        ),
        (
            AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode,
            gpu_input_device_encode,
        ),
    ] {
        if candidate.complete
            && route_beats_cpu_baseline(candidate.duration, cpu_only.duration)
            && candidate.duration < selected.1
        {
            selected = (route, candidate.duration);
        }
    }
    selected.0
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn route_beats_cpu_baseline(route_duration: Duration, cpu_duration: Duration) -> bool {
    route_duration
        .as_nanos()
        .saturating_mul(LOSSLESS_J2K_AUTO_ROUTE_SPEEDUP_DENOMINATOR)
        < cpu_duration
            .as_nanos()
            .saturating_mul(LOSSLESS_J2K_AUTO_ROUTE_SPEEDUP_NUMERATOR)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn try_encode_metal_aligned_tile_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    level: &statumen::Level,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    row: u64,
    start_col: u64,
    tile_count: usize,
    matrix_columns: u64,
    matrix_rows: u64,
    tile_size: u32,
) -> Result<MetalEncodedTileRun, WsiDicomError> {
    if !output_tile_maps_to_statumen_tile(level, tile_size) {
        if metal_input.preference == EncodeBackendPreference::RequireDevice {
            return Err(WsiDicomError::Unsupported {
                reason:
                    "requested Metal input tile decode requires the DICOM tile grid to align with statumen source tiles"
                        .into(),
            });
        }
        return Ok(empty_metal_tile_run(tile_count));
    }

    let row_i64 = i64::try_from(row).map_err(|_| WsiDicomError::Unsupported {
        reason: "tile row exceeds i64".into(),
    })?;
    let start_col_i64 = i64::try_from(start_col).map_err(|_| WsiDicomError::Unsupported {
        reason: "tile column exceeds i64".into(),
    })?;
    let mut requests = Vec::with_capacity(tile_count);
    for offset in 0..tile_count {
        let col = start_col_i64
            .checked_add(
                i64::try_from(offset).map_err(|_| WsiDicomError::Unsupported {
                    reason: "tile batch offset exceeds i64".into(),
                })?,
            )
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile column overflow".into(),
            })?;
        requests.push(TileRequest {
            scene: scene_idx,
            series: series_idx,
            level: level_idx,
            plane: PlaneSelection { z, c, t },
            col,
            row: row_i64,
        });
    }

    let input_decode_started = Instant::now();
    let pixels = match slide.read_tiles(&requests, metal_input.source_tile_output_preference()?) {
        Ok(pixels) => pixels,
        Err(err) if metal_input.preference == EncodeBackendPreference::RequireDevice => {
            return Err(WsiDicomError::SlideRead {
                message: format!("Metal input tile batch decode failed: {err}"),
            });
        }
        Err(_) => return Ok(empty_metal_tile_run(tile_count)),
    };
    let input_decode_duration = input_decode_started.elapsed();

    if pixels.len() != tile_count {
        if metal_input.preference == EncodeBackendPreference::RequireDevice {
            return Err(WsiDicomError::SlideRead {
                message: format!(
                    "Metal input tile batch returned {} tile(s), expected {}",
                    pixels.len(),
                    tile_count
                ),
            });
        }
        return Ok(empty_metal_tile_run(tile_count));
    }

    let mut tile_entries = Vec::with_capacity(tile_count);
    for (offset, pixels) in pixels.into_iter().enumerate() {
        let col = start_col
            .checked_add(
                u64::try_from(offset).map_err(|_| WsiDicomError::Unsupported {
                    reason: "tile batch offset exceeds u64".into(),
                })?,
            )
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile column overflow".into(),
            })?;
        let x =
            col.checked_mul(u64::from(tile_size))
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "tile x offset overflow".into(),
                })?;
        let y =
            row.checked_mul(u64::from(tile_size))
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "tile y offset overflow".into(),
                })?;
        let width = (matrix_columns - x).min(u64::from(tile_size)) as u32;
        let height = (matrix_rows - y).min(u64::from(tile_size)) as u32;

        let TilePixels::Device(DeviceTile::Metal(tile)) = pixels else {
            if metal_input.preference == EncodeBackendPreference::RequireDevice {
                return Err(WsiDicomError::Unsupported {
                    reason:
                        "requested Metal input tile decode returned CPU pixels; set STATUMEN_JPEG_DEVICE_DECODE=1 or STATUMEN_JP2K_DEVICE_DECODE=1 for compressed WSI tiles"
                            .into(),
                });
            }
            tile_entries.push(None);
            continue;
        };

        if tile.width != width || tile.height != height {
            if metal_input.preference == EncodeBackendPreference::RequireDevice {
                return Err(WsiDicomError::Unsupported {
                    reason: format!(
                        "Metal input tile geometry changed: expected {}x{}, got {}x{}",
                        width, height, tile.width, tile.height
                    ),
                });
            }
            tile_entries.push(None);
            continue;
        }

        let profile = pixel_profile_from_device_format(tile.format)?;
        tile_entries.push(Some((tile, profile)));
    }

    let batch_tiles: Vec<_> = tile_entries
        .iter()
        .filter_map(|entry| entry.as_ref().map(|(tile, _)| tile.clone()))
        .collect();
    let encode_batches = metal_j2k_encode_batch_count(&batch_tiles, tile_size, tile_size);
    let batch_encoded = j2k_encoder.encode_metal_tiles(&batch_tiles, tile_size, tile_size)?;
    let gpu_encode_stats = batch_encoded.gpu_encode_stats;
    let mut batch_encoded = batch_encoded.frames.into_iter();
    let mut encoded = Vec::with_capacity(tile_count);
    for entry in tile_entries {
        let Some((_tile, profile)) = entry else {
            encoded.push(None);
            continue;
        };
        match batch_encoded
            .next()
            .expect("Metal batch encode result count matches input tile count")
        {
            Some(codestream) => encoded.push(Some((codestream, profile))),
            None if metal_input.preference == EncodeBackendPreference::RequireDevice => {
                return Err(WsiDicomError::Unsupported {
                    reason:
                        "requested JPEG 2000 Metal tile encode did not dispatch all required stages"
                            .into(),
                });
            }
            None => encoded.push(None),
        }
    }

    Ok(MetalEncodedTileRun {
        tiles: encoded,
        input_decode_duration,
        compose_duration: Duration::ZERO,
        input_decode_batches: 1,
        compose_batches: 0,
        encode_batches,
        gpu_encode_stats,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, Copy)]
struct WholeLevelStripLayout {
    width: u32,
    height: u32,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn try_encode_metal_whole_level_strip_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    strip_layout: WholeLevelStripLayout,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    row: u64,
    start_col: u64,
    tile_count: usize,
    matrix_columns: u64,
    matrix_rows: u64,
    tile_size: u32,
) -> Result<MetalEncodedTileRun, WsiDicomError> {
    let preference = metal_input.preference;
    let tile_size_u64 = u64::from(tile_size);
    let x_start =
        start_col
            .checked_mul(tile_size_u64)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile x offset overflow".into(),
            })?;
    let y = row
        .checked_mul(tile_size_u64)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "tile y offset overflow".into(),
        })?;
    let requested_batch_width = u64::try_from(tile_count)
        .map_err(|_| WsiDicomError::Unsupported {
            reason: "tile batch size exceeds u64".into(),
        })?
        .checked_mul(tile_size_u64)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "tile batch width overflow".into(),
        })?;
    let batch_width = matrix_columns
        .saturating_sub(x_start)
        .min(requested_batch_width);
    let valid_height = (matrix_rows - y).min(tile_size_u64) as u32;
    let source_tile_width = u64::from(strip_layout.width);
    let source_tile_height = u64::from(strip_layout.height);
    let first_source_col = x_start / source_tile_width;
    let first_source_row = y / source_tile_height;
    let source_col_count = x_start
        .checked_add(batch_width)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "source tile x end overflow".into(),
        })?
        .div_ceil(source_tile_width)
        .saturating_sub(first_source_col);
    let source_row_count = y
        .checked_add(u64::from(valid_height))
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "source tile y end overflow".into(),
        })?
        .div_ceil(source_tile_height)
        .saturating_sub(first_source_row);
    let first_source_col_i64 =
        i64::try_from(first_source_col).map_err(|_| WsiDicomError::Unsupported {
            reason: "source tile column exceeds i64".into(),
        })?;
    let first_source_row_i64 =
        i64::try_from(first_source_row).map_err(|_| WsiDicomError::Unsupported {
            reason: "source tile row exceeds i64".into(),
        })?;
    let source_col_count_usize =
        usize::try_from(source_col_count).map_err(|_| WsiDicomError::Unsupported {
            reason: "source tile column count exceeds platform addressable memory".into(),
        })?;
    let source_row_count_usize =
        usize::try_from(source_row_count).map_err(|_| WsiDicomError::Unsupported {
            reason: "source tile row count exceeds platform addressable memory".into(),
        })?;
    let source_tile_count = source_col_count_usize
        .checked_mul(source_row_count_usize)
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "source tile batch size overflow".into(),
        })?;
    let mut source_keys = Vec::with_capacity(source_tile_count);
    for source_row_offset in 0..source_row_count_usize {
        let source_row = first_source_row_i64
            .checked_add(i64::try_from(source_row_offset).map_err(|_| {
                WsiDicomError::Unsupported {
                    reason: "source tile row offset exceeds i64".into(),
                }
            })?)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "source tile row overflow".into(),
            })?;
        for source_col_offset in 0..source_col_count_usize {
            let source_col = first_source_col_i64
                .checked_add(i64::try_from(source_col_offset).map_err(|_| {
                    WsiDicomError::Unsupported {
                        reason: "source tile column offset exceeds i64".into(),
                    }
                })?)
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "source tile column overflow".into(),
                })?;
            source_keys.push(MetalSourceTileKey {
                scene: scene_idx,
                series: series_idx,
                level: level_idx,
                z,
                c,
                t,
                col: source_col,
                row: source_row,
            });
        }
    }

    if source_keys.is_empty() {
        if preference == EncodeBackendPreference::RequireDevice {
            return Err(WsiDicomError::Unsupported {
                reason: "Metal WholeLevel tile source batch is empty".into(),
            });
        }
        return Ok(empty_metal_tile_run(tile_count));
    }

    let mut source_tiles = vec![None; source_tile_count];
    let mut missing_requests = Vec::new();
    let mut missing_keys = Vec::new();
    let mut missing_indices = Vec::new();
    for (index, key) in source_keys.iter().copied().enumerate() {
        if let Some(tile) = metal_input.whole_level_cache.get(key) {
            source_tiles[index] = Some(tile);
        } else {
            missing_requests.push(TileRequest {
                scene: key.scene,
                series: key.series,
                level: key.level,
                plane: PlaneSelection {
                    z: key.z,
                    c: key.c,
                    t: key.t,
                },
                col: key.col,
                row: key.row,
            });
            missing_keys.push(key);
            missing_indices.push(index);
        }
    }

    let mut input_decode_duration = Duration::ZERO;
    if !missing_requests.is_empty() {
        let input_decode_started = Instant::now();
        let pixels = match slide.read_tiles(
            &missing_requests,
            metal_input.source_tile_output_preference()?,
        ) {
            Ok(pixels) => pixels,
            Err(err) if preference == EncodeBackendPreference::RequireDevice => {
                return Err(WsiDicomError::SlideRead {
                    message: format!("Metal WholeLevel tile batch decode failed: {err}"),
                });
            }
            Err(_) => return Ok(empty_metal_tile_run(tile_count)),
        };
        input_decode_duration = input_decode_started.elapsed();
        if pixels.len() != missing_requests.len() {
            if preference == EncodeBackendPreference::RequireDevice {
                return Err(WsiDicomError::SlideRead {
                    message: format!(
                        "Metal WholeLevel tile batch returned {} tile(s), expected {}",
                        pixels.len(),
                        missing_requests.len()
                    ),
                });
            }
            return Ok(empty_metal_tile_run(tile_count));
        }
        for ((index, key), pixels) in missing_indices
            .into_iter()
            .zip(missing_keys.into_iter())
            .zip(pixels.into_iter())
        {
            let TilePixels::Device(DeviceTile::Metal(tile)) = pixels else {
                if preference == EncodeBackendPreference::RequireDevice {
                    return Err(WsiDicomError::Unsupported {
                        reason:
                            "requested Metal WholeLevel tile decode returned CPU pixels; set STATUMEN_JPEG_DEVICE_DECODE=1 or STATUMEN_JP2K_DEVICE_DECODE=1 for compressed WSI tiles"
                                .into(),
                    });
                }
                return Ok(empty_metal_tile_run(tile_count));
            };
            if tile.width == 0
                || tile.height == 0
                || tile.width > strip_layout.width
                || tile.height > strip_layout.height
            {
                if preference == EncodeBackendPreference::RequireDevice {
                    return Err(WsiDicomError::Unsupported {
                        reason: format!(
                            "Metal WholeLevel tile geometry changed: expected <= {}x{}, got {}x{}",
                            strip_layout.width, strip_layout.height, tile.width, tile.height
                        ),
                    });
                }
                return Ok(empty_metal_tile_run(tile_count));
            }
            metal_input.whole_level_cache.insert(key, tile.clone());
            source_tiles[index] = Some(tile);
        }
    }
    let source_tiles: Vec<_> = source_tiles
        .into_iter()
        .map(|tile| {
            tile.ok_or_else(|| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel source tile cache returned incomplete row window".into(),
            })
        })
        .collect::<Result<_, _>>()?;

    let compose_started = Instant::now();
    let composer = metal_input.strip_composer()?;
    let packed = composer.pack_tiles(
        &source_tiles,
        strip_layout,
        first_source_col_i64,
        first_source_row_i64,
        source_col_count_usize,
    )?;
    let profile = pixel_profile_from_device_format(packed.format)?;
    let src_origin_y = u32::try_from(y).map_err(|_| WsiDicomError::Unsupported {
        reason: "Metal WholeLevel tile source y offset exceeds u32".into(),
    })?;
    let mut compose_requests = Vec::with_capacity(tile_count);
    for offset in 0..tile_count {
        let col = start_col
            .checked_add(
                u64::try_from(offset).map_err(|_| WsiDicomError::Unsupported {
                    reason: "tile batch offset exceeds u64".into(),
                })?,
            )
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile column overflow".into(),
            })?;
        let x = col
            .checked_mul(tile_size_u64)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "tile x offset overflow".into(),
            })?;
        let valid_width = (matrix_columns - x).min(tile_size_u64) as u32;
        let src_origin_x = u32::try_from(x).map_err(|_| WsiDicomError::Unsupported {
            reason: "Metal WholeLevel tile source x offset exceeds u32".into(),
        })?;
        compose_requests.push(MetalComposeTileRequest {
            src_origin_x,
            src_origin_y,
            valid_width,
            valid_height,
            output_width: tile_size,
            output_height: tile_size,
        });
    }
    let composed_tiles = composer.compose_tiles(&packed, &compose_requests)?;
    let compose_duration = compose_started.elapsed();

    let mut encoded = Vec::with_capacity(tile_count);
    let encode_batches = metal_j2k_encode_batch_count(&composed_tiles, tile_size, tile_size);
    let batch_encoded = j2k_encoder.encode_metal_tiles(&composed_tiles, tile_size, tile_size)?;
    let gpu_encode_stats = batch_encoded.gpu_encode_stats;
    for frame in batch_encoded.frames {
        match frame {
            Some(codestream) => encoded.push(Some((codestream, profile))),
            None if preference == EncodeBackendPreference::RequireDevice => {
                return Err(WsiDicomError::Unsupported {
                    reason:
                        "requested JPEG 2000 Metal tile encode did not dispatch all required stages"
                            .into(),
                });
            }
            None => encoded.push(None),
        }
    }

    Ok(MetalEncodedTileRun {
        tiles: encoded,
        input_decode_duration,
        compose_duration,
        input_decode_batches: u64::from(input_decode_duration > Duration::ZERO),
        compose_batches: 1,
        encode_batches,
        gpu_encode_stats,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn empty_metal_tile_run(tile_count: usize) -> MetalEncodedTileRun {
    MetalEncodedTileRun {
        tiles: (0..tile_count).map(|_| None).collect(),
        input_decode_duration: Duration::ZERO,
        compose_duration: Duration::ZERO,
        input_decode_batches: 0,
        compose_batches: 0,
        encode_batches: 0,
        gpu_encode_stats: encode::DicomJ2kGpuEncodeBatchStats::default(),
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn metal_j2k_encode_batch_count(
    tiles: &[statumen::output::metal::MetalDeviceTile],
    output_width: u32,
    output_height: u32,
) -> u64 {
    let mut batches = 0u64;
    let mut start = 0usize;
    while start < tiles.len() {
        batches = batches.saturating_add(1);
        let padded =
            encode::metal_tile_is_padded_contiguous(&tiles[start], output_width, output_height);
        let mut end = start + 1;
        while end < tiles.len()
            && encode::metal_tile_is_padded_contiguous(&tiles[end], output_width, output_height)
                == padded
        {
            end += 1;
        }
        start = end;
    }
    batches
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn regular_tiled_source_layout(level: &statumen::Level) -> Option<WholeLevelStripLayout> {
    let TileLayout::Regular {
        tile_width,
        tile_height,
        ..
    } = level.tile_layout
    else {
        return None;
    };
    if tile_width == 0 || tile_height == 0 {
        return None;
    }
    Some(WholeLevelStripLayout {
        width: tile_width,
        height: tile_height,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn whole_level_strip_layout(level: &statumen::Level) -> Option<WholeLevelStripLayout> {
    let TileLayout::WholeLevel {
        virtual_tile_width,
        virtual_tile_height,
        ..
    } = level.tile_layout
    else {
        return None;
    };
    if virtual_tile_width == 0 || virtual_tile_height == 0 {
        return None;
    }
    Some(WholeLevelStripLayout {
        width: virtual_tile_width,
        height: virtual_tile_height,
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn output_tile_maps_to_statumen_tile(level: &statumen::Level, tile_size: u32) -> bool {
    output_frame_maps_to_statumen_tile(level, tile_size, tile_size)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn output_frame_maps_to_statumen_tile(
    level: &statumen::Level,
    frame_columns: u32,
    frame_rows: u32,
) -> bool {
    matches!(
        level.tile_layout,
        TileLayout::Regular {
            tile_width,
            tile_height,
            ..
        } if tile_width == frame_columns && tile_height == frame_rows
    ) || matches!(
        level.tile_layout,
        TileLayout::WholeLevel {
            virtual_tile_width,
            virtual_tile_height,
            ..
        } if virtual_tile_width == frame_columns && virtual_tile_height == frame_rows
    )
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::PathBuf;

    use super::*;
    use crate::encode::{
        dicom_j2k_decomposition_levels, encode_dicom_j2k_lossless, encode_dicom_lossless,
    };
    use dicom_core::{DataElement, PrimitiveValue, VR};
    use dicom_dictionary_std::{tags, uids};
    use dicom_object::{FileMetaTableBuilder, InMemDicomObject};

    #[cfg(all(feature = "metal", target_os = "macos"))]
    static DEVICE_DECODE_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn default_options_use_htj2k_lossless_rpcl_and_auto_backend() {
        let options = DicomExportOptions::default();

        assert_eq!(options.tile_size, 512);
        assert_eq!(options.transfer_syntax.uid(), "1.2.840.10008.1.2.4.202");
        assert_eq!(options.encode_backend, EncodeBackendPreference::Auto);
        assert_eq!(options.codec_validation, CodecValidation::Disabled);
        assert!(!options.source_device_decode);
        assert_eq!(options.gpu_encode_inflight_tiles, None);
        assert_eq!(options.gpu_encode_memory_mib, None);
    }

    #[test]
    fn options_reject_zero_gpu_encode_tuning_overrides() {
        let err = DicomExportOptions {
            gpu_encode_inflight_tiles: Some(0),
            ..DicomExportOptions::default()
        }
        .validate()
        .unwrap_err();
        assert!(
            err.to_string().contains("gpu_encode_inflight_tiles"),
            "unexpected error: {err}"
        );

        let err = DicomExportOptions {
            gpu_encode_memory_mib: Some(0),
            ..DicomExportOptions::default()
        }
        .validate()
        .unwrap_err();
        assert!(
            err.to_string().contains("gpu_encode_memory_mib"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn transfer_syntax_uids_include_htj2k_lossless_profiles() {
        assert_eq!(TransferSyntax::Jpeg2000.uid(), "1.2.840.10008.1.2.4.91");
        assert_eq!(
            TransferSyntax::Jpeg2000Lossless.uid(),
            "1.2.840.10008.1.2.4.90"
        );
        assert_eq!(
            TransferSyntax::Htj2kLossless.uid(),
            "1.2.840.10008.1.2.4.201"
        );
        assert_eq!(
            TransferSyntax::Htj2kLosslessRpcl.uid(),
            "1.2.840.10008.1.2.4.202"
        );
    }

    #[test]
    fn export_request_rejects_zero_tile_size() {
        let err = DicomExportRequest {
            source_path: PathBuf::from("source.svs"),
            output_dir: PathBuf::from("out"),
            options: DicomExportOptions {
                tile_size: 0,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        }
        .validate()
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("tile_size must be greater than zero"));
    }

    #[test]
    fn export_request_keeps_source_and_output_paths() {
        let request = DicomExportRequest::new(
            PathBuf::from("source.ndpi"),
            PathBuf::from("dicom-out"),
            DicomExportOptions::default(),
        )
        .unwrap();

        assert_eq!(request.source_path, PathBuf::from("source.ndpi"));
        assert_eq!(request.output_dir, PathBuf::from("dicom-out"));
    }

    #[cfg(not(any(feature = "cuda", all(feature = "metal", target_os = "macos"))))]
    #[test]
    fn auto_and_prefer_device_fall_back_to_facade_cpu_when_no_device_backend_is_enabled() {
        let bytes = vec![0; 16 * 16];
        let samples = J2kLosslessSamples::new(&bytes, 16, 16, 1, 8, false).expect("valid samples");

        let auto = encode_dicom_j2k_lossless(samples, EncodeBackendPreference::Auto)
            .expect("auto backend should fall back to CPU");
        assert_j2k_facade_roundtrip(samples, &auto);

        let prefer = encode_dicom_j2k_lossless(samples, EncodeBackendPreference::PreferDevice)
            .expect("prefer-device backend should fall back to CPU");
        assert_j2k_facade_roundtrip(samples, &prefer);

        let require =
            encode_dicom_j2k_lossless(samples, EncodeBackendPreference::RequireDevice).unwrap_err();
        assert!(require.to_string().contains("device encode backend"));
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    fn require_device_uses_metal_j2k_encode_for_wsi_sized_tile() {
        let mut bytes = Vec::with_capacity(128 * 128 * 3);
        for y in 0..128u32 {
            for x in 0..128u32 {
                bytes.push(((x * 3 + y * 5) & 0xFF) as u8);
                bytes.push(((x * 7 + y * 11) & 0xFF) as u8);
                bytes.push(((x * 13 + y * 17) & 0xFF) as u8);
            }
        }
        let samples =
            J2kLosslessSamples::new(&bytes, 128, 128, 3, 8, false).expect("valid RGB samples");

        let codestream = encode_dicom_j2k_lossless(samples, EncodeBackendPreference::RequireDevice)
            .expect("Metal backend should encode WSI-sized DICOM tile");

        assert_j2k_facade_roundtrip(samples, &codestream);
    }

    #[test]
    fn encode_dicom_j2k_frame_returns_finished_dicom_frame_bytes() {
        let bytes: Vec<u8> = (0..64).map(|value| ((value * 13) & 0xFF) as u8).collect();
        let samples = J2kLosslessSamples::new(&bytes, 8, 8, 1, 8, false).expect("valid samples");

        let frame = encode_dicom_j2k_frame(DicomJ2kFrameEncodeRequest {
            samples,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::RoundTrip,
        })
        .unwrap();

        assert_eq!(
            frame.transfer_syntax_uid,
            TransferSyntax::Htj2kLosslessRpcl.uid()
        );
        assert_eq!(frame.bytes[..2], [0xFF, 0x4F]);
        assert!(!frame.used_device_encode);
        assert!(!frame.used_device_validation);
        assert_transfer_syntax_codestream(TransferSyntax::Htj2kLosslessRpcl, &frame.bytes);
        assert_j2k_facade_roundtrip(samples, &frame.bytes);
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    fn encode_dicom_j2k_frame_can_return_metal_finished_bytes_when_required() {
        let mut bytes = Vec::with_capacity(128 * 128 * 3);
        for y in 0..128u32 {
            for x in 0..128u32 {
                bytes.push(((x * 5 + y * 3) & 0xFF) as u8);
                bytes.push(((x * 11 + y * 7) & 0xFF) as u8);
                bytes.push(((x * 17 + y * 13) & 0xFF) as u8);
            }
        }
        let samples =
            J2kLosslessSamples::new(&bytes, 128, 128, 3, 8, false).expect("valid RGB samples");

        let frame = encode_dicom_j2k_frame(DicomJ2kFrameEncodeRequest {
            samples,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            encode_backend: EncodeBackendPreference::RequireDevice,
            codec_validation: CodecValidation::RoundTrip,
        })
        .expect("Metal backend should return finished DICOM frame bytes");

        assert_eq!(
            frame.transfer_syntax_uid,
            TransferSyntax::Htj2kLosslessRpcl.uid()
        );
        assert!(frame.used_device_encode);
        assert!(frame.used_device_validation);
        assert!(frame.validation_micros > 0);
        assert!(!frame.bytes.is_empty());
        assert_transfer_syntax_codestream(TransferSyntax::Htj2kLosslessRpcl, &frame.bytes);
        assert_j2k_facade_roundtrip(samples, &frame.bytes);
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    fn encode_dicom_j2k_frame_can_skip_runtime_codec_validation() {
        let mut bytes = Vec::with_capacity(128 * 128 * 3);
        for y in 0..128u32 {
            for x in 0..128u32 {
                bytes.push(((x * 19 + y * 23) & 0xFF) as u8);
                bytes.push(((x * 29 + y * 31) & 0xFF) as u8);
                bytes.push(((x * 37 + y * 41) & 0xFF) as u8);
            }
        }
        let samples =
            J2kLosslessSamples::new(&bytes, 128, 128, 3, 8, false).expect("valid RGB samples");

        let frame = encode_dicom_j2k_frame(DicomJ2kFrameEncodeRequest {
            samples,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            encode_backend: EncodeBackendPreference::RequireDevice,
            codec_validation: CodecValidation::Disabled,
        })
        .expect("Metal backend should return finished DICOM frame bytes");

        assert!(frame.used_device_encode);
        assert!(!frame.used_device_validation);
        assert_eq!(frame.validation_micros, 0);
        assert_transfer_syntax_codestream(TransferSyntax::Htj2kLosslessRpcl, &frame.bytes);
        assert_j2k_facade_roundtrip(samples, &frame.bytes);
    }

    #[test]
    fn dicom_j2k_decomposition_uses_validated_lossless_safe_profile() {
        let gray = vec![0; 128 * 128];
        let gray_samples =
            J2kLosslessSamples::new(&gray, 128, 128, 1, 8, false).expect("valid gray");
        assert_eq!(dicom_j2k_decomposition_levels(gray_samples), 1);

        let rgb = vec![0; 128 * 128 * 3];
        let rgb_samples = J2kLosslessSamples::new(&rgb, 128, 128, 3, 8, false).expect("valid rgb");
        assert_eq!(dicom_j2k_decomposition_levels(rgb_samples), 1);
    }

    #[test]
    fn dicom_j2k_cpu_encode_round_trips_gray8_tile() {
        let bytes: Vec<u8> = (0..64).map(|value| ((value * 5) & 0xFF) as u8).collect();
        let samples = J2kLosslessSamples::new(&bytes, 8, 8, 1, 8, false).expect("valid samples");

        let codestream =
            encode_dicom_j2k_lossless(samples, EncodeBackendPreference::CpuOnly).unwrap();

        assert_j2k_facade_roundtrip(samples, &codestream);
    }

    #[test]
    fn dicom_htj2k_cpu_encode_round_trips_gray8_tile() {
        let bytes: Vec<u8> = (0..64).map(|value| ((value * 7) & 0xFF) as u8).collect();
        let samples = J2kLosslessSamples::new(&bytes, 8, 8, 1, 8, false).expect("valid samples");

        let codestream = crate::encode::encode_dicom_lossless(
            samples,
            TransferSyntax::Htj2kLossless,
            EncodeBackendPreference::CpuOnly,
            CodecValidation::RoundTrip,
        )
        .unwrap();

        assert!(codestream.windows(2).any(|window| window == [0xFF, 0x50]));
        assert_j2k_facade_roundtrip(samples, &codestream);
    }

    #[test]
    fn dicom_htj2k_rpcl_encode_writes_tlm_marker() {
        let bytes: Vec<u8> = (0..64).map(|value| ((value * 11) & 0xFF) as u8).collect();
        let samples = J2kLosslessSamples::new(&bytes, 8, 8, 1, 8, false).expect("valid samples");

        let codestream = crate::encode::encode_dicom_lossless(
            samples,
            TransferSyntax::Htj2kLosslessRpcl,
            EncodeBackendPreference::CpuOnly,
            CodecValidation::RoundTrip,
        )
        .unwrap();

        let cod_offset = codestream
            .windows(2)
            .position(|window| window == [0xFF, 0x52])
            .expect("COD marker");
        assert_eq!(codestream[cod_offset + 5], 0x02);
        assert!(codestream.windows(2).any(|window| window == [0xFF, 0x55]));
        assert_j2k_facade_roundtrip(samples, &codestream);
    }

    #[test]
    fn raw_j2k_lossless_tile_can_passthrough_when_geometry_matches() {
        let bytes: Vec<u8> = (0..2 * 2 * 3)
            .map(|value| ((value * 19) & 0xFF) as u8)
            .collect();
        let samples = J2kLosslessSamples::new(&bytes, 2, 2, 3, 8, false).expect("valid samples");
        let codestream = encode_dicom_lossless(
            samples,
            TransferSyntax::Jpeg2000Lossless,
            EncodeBackendPreference::CpuOnly,
            CodecValidation::RoundTrip,
        )
        .unwrap();
        let raw = RawCompressedTile {
            compression: Compression::Jp2kRgb,
            width: 2,
            height: 2,
            bits_allocated: 8,
            samples_per_pixel: 3,
            photometric_interpretation: EncodedTilePhotometricInterpretation::Rgb,
            data: codestream.clone(),
        };

        let passed = j2k_passthrough_frame(raw, 2, 2, TransferSyntax::Jpeg2000Lossless)
            .unwrap()
            .expect("J2K passthrough");

        assert_eq!(passed.codestream, codestream);
        assert_eq!(
            passed.profile,
            PixelProfile {
                components: 3,
                bits_allocated: 8,
                photometric_interpretation: "RGB",
            }
        );
        assert_eq!(
            passed.transfer_syntax,
            CompressedTransferSyntax::Jpeg2000Lossless
        );
    }

    #[test]
    fn raw_j2k_ycbcr_tile_can_passthrough_to_general_jpeg2000() {
        let bytes: Vec<u8> = (0..2 * 2 * 3)
            .map(|value| ((value * 17) & 0xFF) as u8)
            .collect();
        let samples = J2kLosslessSamples::new(&bytes, 2, 2, 3, 8, false).expect("valid samples");
        let codestream = encode_dicom_lossless(
            samples,
            TransferSyntax::Jpeg2000Lossless,
            EncodeBackendPreference::CpuOnly,
            CodecValidation::RoundTrip,
        )
        .unwrap();
        let raw = RawCompressedTile {
            compression: Compression::Jp2kYcbcr,
            width: 2,
            height: 2,
            bits_allocated: 8,
            samples_per_pixel: 3,
            photometric_interpretation: EncodedTilePhotometricInterpretation::YbrFull422,
            data: codestream.clone(),
        };

        let passed = j2k_passthrough_frame(raw, 2, 2, TransferSyntax::Jpeg2000)
            .unwrap()
            .expect("general J2K passthrough");

        assert_eq!(passed.codestream, codestream);
        assert_eq!(
            passed.profile,
            PixelProfile {
                components: 3,
                bits_allocated: 8,
                photometric_interpretation: "YBR_RCT",
            }
        );
        assert_eq!(
            passed.transfer_syntax,
            CompressedTransferSyntax::Jpeg2000Lossless
        );
    }

    #[test]
    fn htj2k_passthrough_probe_is_limited_to_dicom_sources() {
        assert!(j2k_family_passthrough_probe_allowed(
            std::path::Path::new("source.svs"),
            TransferSyntax::Jpeg2000
        ));
        assert!(j2k_family_passthrough_probe_allowed(
            std::path::Path::new("source.svs"),
            TransferSyntax::Jpeg2000Lossless
        ));
        assert!(j2k_family_passthrough_probe_allowed(
            std::path::Path::new("source.dcm"),
            TransferSyntax::Htj2kLosslessRpcl
        ));
        assert!(j2k_family_passthrough_probe_allowed(
            std::path::Path::new("source.DCM"),
            TransferSyntax::Htj2kLossless
        ));
        assert!(!j2k_family_passthrough_probe_allowed(
            std::path::Path::new("source.svs"),
            TransferSyntax::Htj2kLosslessRpcl
        ));
        assert!(!j2k_family_passthrough_probe_allowed(
            std::path::Path::new("source.ndpi"),
            TransferSyntax::Htj2kLosslessRpcl
        ));
    }

    #[test]
    fn export_j2k_passthrough_does_not_touch_gpu_even_when_device_required() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes: Vec<u8> = (0..2 * 2 * 3)
            .map(|value| ((value * 41) & 0xFF) as u8)
            .collect();
        let samples = J2kLosslessSamples::new(&bytes, 2, 2, 3, 8, false).expect("valid samples");
        let codestream = encode_dicom_lossless(
            samples,
            TransferSyntax::Jpeg2000Lossless,
            EncodeBackendPreference::CpuOnly,
            CodecValidation::RoundTrip,
        )
        .unwrap();
        let source = tmp.path().join("source.svs");
        write_tiled_jp2k_rgb_tiff(&source, 2, 2, 2, 2, std::slice::from_ref(&codestream));

        let report = export_dicom(DicomExportRequest {
            source_path: source,
            output_dir: tmp.path().join("out"),
            options: DicomExportOptions {
                tile_size: 2,
                transfer_syntax: TransferSyntax::Jpeg2000Lossless,
                encode_backend: EncodeBackendPreference::RequireDevice,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();

        assert_eq!(report.metrics.total_frames, 1);
        assert_eq!(report.metrics.j2k_passthrough_frames, 1);
        assert_eq!(report.metrics.cpu_input_frames, 0);
        assert_eq!(report.metrics.gpu_input_decode_frames, 0);
        assert_eq!(report.metrics.gpu_encode_frames, 0);
        assert_eq!(report.metrics.gpu_input_decode_batches, 0);
        assert_eq!(report.metrics.gpu_compose_batches, 0);
        assert_eq!(report.metrics.gpu_encode_batches, 0);
        assert_eq!(report.metrics.cpu_fallback_frames, 0);

        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        let fragments = object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap();
        assert_eq!(fragments.len(), 1);
        assert_eq!(
            dicom_fragment_payload_without_padding(&fragments[0]),
            codestream
        );
    }

    #[test]
    fn export_general_j2k_passthrough_accepts_ycbcr_source_without_gpu_work() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes: Vec<u8> = (0..2 * 2 * 3)
            .map(|value| ((value * 13) & 0xFF) as u8)
            .collect();
        let samples = J2kLosslessSamples::new(&bytes, 2, 2, 3, 8, false).expect("valid samples");
        let codestream = encode_dicom_lossless(
            samples,
            TransferSyntax::Jpeg2000Lossless,
            EncodeBackendPreference::CpuOnly,
            CodecValidation::RoundTrip,
        )
        .unwrap();
        let source = tmp.path().join("source.svs");
        write_tiled_jp2k_ycbcr_tiff(&source, 2, 2, 2, 2, std::slice::from_ref(&codestream));

        let report = export_dicom(DicomExportRequest {
            source_path: source,
            output_dir: tmp.path().join("out"),
            options: DicomExportOptions {
                tile_size: 512,
                transfer_syntax: TransferSyntax::Jpeg2000,
                encode_backend: EncodeBackendPreference::RequireDevice,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: true,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();

        assert_eq!(report.metrics.total_frames, 1);
        assert_eq!(report.metrics.j2k_passthrough_frames, 1);
        assert_eq!(report.metrics.cpu_input_frames, 0);
        assert_eq!(report.metrics.gpu_input_decode_frames, 0);
        assert_eq!(report.metrics.gpu_encode_frames, 0);
        assert_eq!(report.metrics.gpu_input_decode_batches, 0);
        assert_eq!(report.metrics.gpu_compose_batches, 0);
        assert_eq!(report.metrics.gpu_encode_batches, 0);
        assert_eq!(report.metrics.cpu_fallback_frames, 0);

        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        assert_eq!(
            object.meta().transfer_syntax.as_str(),
            TransferSyntax::Jpeg2000.uid()
        );
        assert_eq!(
            object.element(tags::ROWS).unwrap().to_int::<u16>().unwrap(),
            2
        );
        assert_eq!(
            object
                .element(tags::COLUMNS)
                .unwrap()
                .to_int::<u16>()
                .unwrap(),
            2
        );
        assert_eq!(
            object
                .element(tags::PHOTOMETRIC_INTERPRETATION)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "YBR_RCT"
        );
        let fragments = object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap();
        assert_eq!(fragments.len(), 1);
        assert_eq!(
            dicom_fragment_payload_without_padding(&fragments[0]),
            codestream
        );
    }

    #[test]
    fn export_general_j2k_passthrough_only_rejects_mismatched_geometry_before_gpu_work() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes: Vec<u8> = (0..2 * 2 * 3)
            .map(|value| ((value * 7) & 0xFF) as u8)
            .collect();
        let samples = J2kLosslessSamples::new(&bytes, 2, 2, 3, 8, false).expect("valid samples");
        let codestream = encode_dicom_lossless(
            samples,
            TransferSyntax::Jpeg2000Lossless,
            EncodeBackendPreference::CpuOnly,
            CodecValidation::RoundTrip,
        )
        .unwrap();
        let source = tmp.path().join("source.svs");
        write_tiled_jp2k_ycbcr_tiff(&source, 2, 2, 2, 3, std::slice::from_ref(&codestream));

        let err = export_dicom(DicomExportRequest {
            source_path: source,
            output_dir: tmp.path().join("out"),
            options: DicomExportOptions {
                tile_size: 4,
                transfer_syntax: TransferSyntax::Jpeg2000,
                encode_backend: EncodeBackendPreference::RequireDevice,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: true,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap_err()
        .to_string();

        assert!(err.contains("passthrough-only"), "unexpected error: {err}");
        assert!(
            !err.contains("Metal"),
            "passthrough-only route should reject before device work: {err}"
        );
    }

    #[test]
    fn export_htj2k_rpcl_passthrough_does_not_touch_gpu_even_when_device_required() {
        let tmp = tempfile::tempdir().unwrap();
        let raw_source = tmp.path().join("source.dcm");
        write_source_dicom_with_dimensions(&raw_source, "1.2.826.0.1.3680043.10.999.43", 2, 2);

        let source_report = export_dicom(DicomExportRequest {
            source_path: raw_source,
            output_dir: tmp.path().join("source-dicom"),
            options: DicomExportOptions {
                tile_size: 2,
                transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();
        let source_object = dicom_object::open_file(&source_report.instances[0].path).unwrap();
        let source_fragments = source_object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap()
            .iter()
            .map(|fragment| dicom_fragment_payload_without_padding(fragment).to_vec())
            .collect::<Vec<_>>();
        assert_eq!(source_fragments.len(), 1);

        let report = export_dicom(DicomExportRequest {
            source_path: source_report.instances[0].path.clone(),
            output_dir: tmp.path().join("out"),
            options: DicomExportOptions {
                tile_size: 2,
                transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
                encode_backend: EncodeBackendPreference::RequireDevice,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();

        assert_eq!(report.metrics.total_frames, 1);
        assert_eq!(report.metrics.j2k_passthrough_frames, 1);
        assert_eq!(report.metrics.cpu_input_frames, 0);
        assert_eq!(report.metrics.gpu_input_decode_frames, 0);
        assert_eq!(report.metrics.gpu_encode_frames, 0);
        assert_eq!(report.metrics.gpu_input_decode_batches, 0);
        assert_eq!(report.metrics.gpu_compose_batches, 0);
        assert_eq!(report.metrics.gpu_encode_batches, 0);
        assert_eq!(report.metrics.cpu_fallback_frames, 0);

        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        assert_eq!(
            object.meta().transfer_syntax.trim_end_matches('\0'),
            TransferSyntax::Htj2kLosslessRpcl.uid()
        );
        let fragments = object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap();
        assert_eq!(fragments.len(), 1);
        assert_eq!(
            dicom_fragment_payload_without_padding(&fragments[0]),
            source_fragments[0]
        );
    }

    #[test]
    fn export_htj2k_rpcl_dicom_edge_passthrough_keeps_padded_source_frame() {
        let tmp = tempfile::tempdir().unwrap();
        let raw_source = tmp.path().join("source.dcm");
        write_source_dicom_with_dimensions(&raw_source, "1.2.826.0.1.3680043.10.999.53", 3, 2);

        let source_report = export_dicom(DicomExportRequest {
            source_path: raw_source,
            output_dir: tmp.path().join("source-dicom"),
            options: DicomExportOptions {
                tile_size: 2,
                transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();
        let source_object = dicom_object::open_file(&source_report.instances[0].path).unwrap();
        let source_fragments = source_object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap()
            .iter()
            .map(|fragment| dicom_fragment_payload_without_padding(fragment).to_vec())
            .collect::<Vec<_>>();
        assert_eq!(source_fragments.len(), 2);

        let report = export_dicom(DicomExportRequest {
            source_path: source_report.instances[0].path.clone(),
            output_dir: tmp.path().join("out"),
            options: DicomExportOptions {
                tile_size: 2,
                transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
                encode_backend: EncodeBackendPreference::RequireDevice,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();

        assert_eq!(report.metrics.total_frames, 2);
        assert_eq!(report.metrics.j2k_passthrough_frames, 2);
        assert_eq!(report.metrics.cpu_input_frames, 0);
        assert_eq!(report.metrics.gpu_input_decode_frames, 0);
        assert_eq!(report.metrics.gpu_encode_frames, 0);
        assert_eq!(report.metrics.cpu_fallback_frames, 0);
        assert_eq!(report.metrics.route_unclassified_frames(), 0);

        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        let fragments = object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap();
        assert_eq!(fragments.len(), 2);
        assert_eq!(
            dicom_fragment_payload_without_padding(&fragments[0]),
            source_fragments[0]
        );
        assert_eq!(
            dicom_fragment_payload_without_padding(&fragments[1]),
            source_fragments[1]
        );
    }

    #[test]
    fn raw_j2k_passthrough_rejects_geometry_or_syntax_mismatch() {
        let bytes: Vec<u8> = (0..2 * 2 * 3)
            .map(|value| ((value * 23) & 0xFF) as u8)
            .collect();
        let samples = J2kLosslessSamples::new(&bytes, 2, 2, 3, 8, false).expect("valid samples");
        let codestream = encode_dicom_lossless(
            samples,
            TransferSyntax::Jpeg2000Lossless,
            EncodeBackendPreference::CpuOnly,
            CodecValidation::RoundTrip,
        )
        .unwrap();
        let raw = RawCompressedTile {
            compression: Compression::Jp2kRgb,
            width: 2,
            height: 2,
            bits_allocated: 8,
            samples_per_pixel: 3,
            photometric_interpretation: EncodedTilePhotometricInterpretation::Rgb,
            data: codestream,
        };

        assert!(
            j2k_passthrough_frame(raw.clone(), 1, 2, TransferSyntax::Jpeg2000Lossless)
                .unwrap()
                .is_none()
        );
        assert!(
            j2k_passthrough_frame(raw, 2, 2, TransferSyntax::Htj2kLosslessRpcl)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn raw_htj2k_rpcl_tile_can_passthrough_when_geometry_matches() {
        let bytes: Vec<u8> = (0..2 * 2 * 3)
            .map(|value| ((value * 31) & 0xFF) as u8)
            .collect();
        let samples = J2kLosslessSamples::new(&bytes, 2, 2, 3, 8, false).expect("valid samples");
        let codestream = encode_dicom_lossless(
            samples,
            TransferSyntax::Htj2kLosslessRpcl,
            EncodeBackendPreference::CpuOnly,
            CodecValidation::RoundTrip,
        )
        .unwrap();
        let raw = RawCompressedTile {
            compression: Compression::Jp2kRgb,
            width: 2,
            height: 2,
            bits_allocated: 8,
            samples_per_pixel: 3,
            photometric_interpretation: EncodedTilePhotometricInterpretation::Rgb,
            data: codestream.clone(),
        };

        let passed = j2k_passthrough_frame(raw, 2, 2, TransferSyntax::Htj2kLosslessRpcl)
            .unwrap()
            .expect("HTJ2K RPCL passthrough");

        assert_eq!(passed.codestream, codestream);
        assert_eq!(
            passed.profile,
            PixelProfile {
                components: 3,
                bits_allocated: 8,
                photometric_interpretation: "RGB",
            }
        );
    }

    #[test]
    fn raw_htj2k_lrcp_tile_rejects_rpcl_passthrough() {
        let bytes: Vec<u8> = (0..2 * 2 * 3)
            .map(|value| ((value * 37) & 0xFF) as u8)
            .collect();
        let samples = J2kLosslessSamples::new(&bytes, 2, 2, 3, 8, false).expect("valid samples");
        let codestream = encode_dicom_lossless(
            samples,
            TransferSyntax::Htj2kLossless,
            EncodeBackendPreference::CpuOnly,
            CodecValidation::RoundTrip,
        )
        .unwrap();
        let raw = RawCompressedTile {
            compression: Compression::Jp2kRgb,
            width: 2,
            height: 2,
            bits_allocated: 8,
            samples_per_pixel: 3,
            photometric_interpretation: EncodedTilePhotometricInterpretation::Rgb,
            data: codestream,
        };

        assert!(
            j2k_passthrough_frame(raw, 2, 2, TransferSyntax::Htj2kLosslessRpcl)
                .unwrap()
                .is_none()
        );
    }

    fn assert_j2k_facade_roundtrip(samples: J2kLosslessSamples<'_>, codestream: &[u8]) {
        let mut decoder = signinum_j2k::J2kDecoder::new(codestream).expect("parse encoded J2K");
        let bytes_per_sample = if samples.bit_depth <= 8 {
            1usize
        } else {
            2usize
        };
        let stride = samples.width as usize * samples.components as usize * bytes_per_sample;
        let mut decoded = vec![0; stride * samples.height as usize];
        let fmt = match (samples.components, samples.bit_depth) {
            (1, 8) => signinum_j2k::PixelFormat::Gray8,
            (3, 8) => signinum_j2k::PixelFormat::Rgb8,
            (1, 16) => signinum_j2k::PixelFormat::Gray16,
            (3, 16) => signinum_j2k::PixelFormat::Rgb16,
            _ => panic!(
                "unsupported test sample profile: components={} bit_depth={}",
                samples.components, samples.bit_depth
            ),
        };
        decoder
            .decode_into(&mut decoded, stride, fmt)
            .expect("decode encoded J2K");

        assert_eq!(decoded, samples.data);
    }

    #[test]
    fn dicom_export_request_defaults_to_research_placeholder_metadata() {
        let request = DicomExportRequest::new(
            PathBuf::from("source.svs"),
            PathBuf::from("out"),
            DicomExportOptions::default(),
        )
        .unwrap();

        assert!(matches!(
            request.metadata,
            MetadataSource::ResearchPlaceholder
        ));
    }

    #[test]
    fn missing_pixel_spacing_is_rejected_before_frame_export() {
        let err = require_pixel_spacing_mm(None).unwrap_err();

        assert!(
            err.to_string().contains("pixel spacing"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn fhir_bundle_maps_patient_specimen_service_request_and_report() {
        let bundle = serde_json::json!({
            "resourceType": "Bundle",
            "entry": [
                {
                    "resource": {
                        "resourceType": "Patient",
                        "id": "pat-1",
                        "identifier": [{"value": "MRN123"}],
                        "name": [{"family": "Doe", "given": ["Jane", "Q"]}]
                    }
                },
                {
                    "resource": {
                        "resourceType": "Specimen",
                        "identifier": [{"value": "S-42"}],
                        "type": {"text": "colon biopsy"}
                    }
                },
                {
                    "resource": {
                        "resourceType": "ServiceRequest",
                        "identifier": [{"value": "ORDER-7"}],
                        "code": {"text": "Surgical pathology"}
                    }
                },
                {
                    "resource": {
                        "resourceType": "DiagnosticReport",
                        "identifier": [{"value": "DR-9"}],
                        "code": {"text": "Final pathology report"}
                    }
                }
            ]
        });

        let metadata = DicomMetadata::from_fhir_r4_bundle(&bundle).unwrap();

        assert_eq!(metadata.patient_id.as_deref(), Some("MRN123"));
        assert_eq!(metadata.patient_name.as_deref(), Some("Doe^Jane Q"));
        assert_eq!(metadata.specimen_identifier.as_deref(), Some("S-42"));
        assert_eq!(metadata.accession_number.as_deref(), Some("ORDER-7"));
        assert_eq!(
            metadata.study_description.as_deref(),
            Some("Final pathology report")
        );
    }

    #[test]
    fn export_dicom_writes_jpeg2000_lossless_vl_wsi_instances() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.dcm");
        let out = tmp.path().join("out");
        write_source_dicom(&source);

        let report = export_dicom(DicomExportRequest {
            source_path: source,
            output_dir: out.clone(),
            options: DicomExportOptions {
                tile_size: 2,
                transfer_syntax: TransferSyntax::Jpeg2000Lossless,
                encode_backend: EncodeBackendPreference::PreferDevice,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();

        assert_eq!(report.instances.len(), 1);
        assert_eq!(report.instances[0].frame_count, 2);
        assert_eq!(report.instances[0].metrics.total_frames, 2);
        assert_eq!(report.instances[0].metrics.cpu_input_frames, 2);
        assert_eq!(report.instances[0].metrics.gpu_input_decode_frames, 0);
        assert_eq!(report.metrics.total_frames, 2);
        assert_eq!(report.metrics.cpu_input_frames, 2);
        assert_eq!(report.metrics.gpu_input_decode_frames, 0);
        assert!(report.metrics.input_decode_micros > 0);
        assert!(report.metrics.encode_micros > 0);
        assert!(report.metrics.write_micros > 0);
        assert!(report.metrics.compose_micros > 0);
        if report.metrics.gpu_validation_frames == 0 {
            assert_eq!(report.metrics.validation_micros, 0);
        } else {
            assert!(report.metrics.validation_micros > 0);
        }
        assert_eq!(
            report.instances[0].transfer_syntax_uid,
            TransferSyntax::Jpeg2000Lossless.uid()
        );
        assert!(report.instances[0].path.starts_with(&out));

        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        assert_eq!(
            object.meta().transfer_syntax,
            TransferSyntax::Jpeg2000Lossless.uid()
        );
        assert_eq!(
            object
                .element(tags::SOP_CLASS_UID)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE
        );
        assert_eq!(
            object
                .element(tags::DIMENSION_ORGANIZATION_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "TILED_FULL"
        );
        assert!(object.element(tags::PYRAMID_UID).is_ok());
        assert_eq!(object.element(tags::PYRAMID_UID).unwrap().vr(), VR::UI);
        assert_eq!(object.element(tags::PYRAMID_LABEL).unwrap().vr(), VR::LO);
        assert_eq!(
            object.element(tags::FRAME_OF_REFERENCE_UID).unwrap().vr(),
            VR::UI
        );
        assert_eq!(
            object
                .element(tags::NUMBER_OF_FRAMES)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            2
        );
        assert_eq!(
            object
                .element(tags::TOTAL_PIXEL_MATRIX_COLUMNS)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            3
        );
        assert_eq!(
            object
                .element(tags::TOTAL_PIXEL_MATRIX_ROWS)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            2
        );
        assert_eq!(object.element(tags::SERIES_NUMBER).unwrap().vr(), VR::IS);
        assert_eq!(object.element(tags::INSTANCE_NUMBER).unwrap().vr(), VR::IS);
        assert_eq!(
            object
                .element(tags::ACQUISITION_DATE)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "19700101"
        );
        assert_eq!(
            object
                .element(tags::ACQUISITION_TIME)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "000000"
        );
        assert_eq!(
            object.element(tags::NUMBER_OF_OPTICAL_PATHS).unwrap().vr(),
            VR::UL
        );
        assert!(object.element(tags::EXTENDED_OFFSET_TABLE).is_ok());
        assert!(object.element(tags::EXTENDED_OFFSET_TABLE_LENGTHS).is_ok());
        assert_eq!(
            object
                .element(tags::PIXEL_DATA)
                .unwrap()
                .value()
                .fragments()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn external_openjpeg_decodes_jpeg2000_exported_frame_when_available() {
        let Some(opj_decompress) = find_command_for_test("opj_decompress") else {
            eprintln!("skipping external OpenJPEG parity smoke: opj_decompress not found");
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.dcm");
        let out = tmp.path().join("out");
        let expected = vec![
            255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 0, 255, 255, 255, 0, 255,
        ];
        write_source_dicom_with_pixels(
            &source,
            "1.2.826.0.1.3680043.10.999.91",
            3,
            2,
            expected.clone(),
        );

        let report = export_dicom(DicomExportRequest {
            source_path: source,
            output_dir: out,
            options: DicomExportOptions {
                tile_size: 3,
                transfer_syntax: TransferSyntax::Jpeg2000Lossless,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();
        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        let fragments = object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap();
        assert_eq!(fragments.len(), 1);

        let codestream_path = tmp.path().join("frame.j2k");
        let ppm_path = tmp.path().join("frame.ppm");
        std::fs::write(
            &codestream_path,
            dicom_fragment_payload_without_padding(&fragments[0]),
        )
        .unwrap();
        let status = std::process::Command::new(opj_decompress)
            .args(["-i"])
            .arg(&codestream_path)
            .args(["-o"])
            .arg(&ppm_path)
            .status()
            .unwrap();
        assert!(status.success(), "opj_decompress failed with {status}");

        let decoded = read_binary_ppm_for_test(&ppm_path);

        assert_eq!(decoded.0, 3);
        assert_eq!(decoded.1, 3);
        assert_eq!(&decoded.2[..expected.len()], expected.as_slice());
        assert_eq!(&decoded.2[expected.len()..], &[0; 9]);
    }

    #[test]
    fn external_dicom_validators_accept_jpeg_baseline_passthrough_when_available() {
        let tmp = tempfile::tempdir().unwrap();
        let jpeg = encode_test_jpeg(8, 8, [160, 20, 40]);
        let source = tmp.path().join("source.svs");
        write_tiled_jpeg_tiff(&source, 8, 8, 8, 8, std::slice::from_ref(&jpeg));

        let report = export_dicom(DicomExportRequest {
            source_path: source,
            output_dir: tmp.path().join("out"),
            options: DicomExportOptions {
                tile_size: 8,
                transfer_syntax: TransferSyntax::JpegBaseline8Bit,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();

        run_dicom_validators_for_test(&report.instances[0].path);
    }

    #[test]
    fn external_dicom_validators_accept_general_j2k_passthrough_when_available() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes: Vec<u8> = (0..2 * 2 * 3)
            .map(|value| ((value * 17) & 0xFF) as u8)
            .collect();
        let samples = J2kLosslessSamples::new(&bytes, 2, 2, 3, 8, false).expect("valid samples");
        let codestream = encode_dicom_lossless(
            samples,
            TransferSyntax::Jpeg2000Lossless,
            EncodeBackendPreference::CpuOnly,
            CodecValidation::RoundTrip,
        )
        .unwrap();
        let source = tmp.path().join("source.svs");
        write_tiled_jp2k_ycbcr_tiff(&source, 2, 2, 2, 2, std::slice::from_ref(&codestream));

        let report = export_dicom(DicomExportRequest {
            source_path: source,
            output_dir: tmp.path().join("out"),
            options: DicomExportOptions {
                tile_size: 512,
                transfer_syntax: TransferSyntax::Jpeg2000,
                encode_backend: EncodeBackendPreference::RequireDevice,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: true,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();

        run_dicom_validators_for_test(&report.instances[0].path);
    }

    #[test]
    fn external_htj2k_reference_decodes_htj2k_rpcl_exported_frame_when_available() {
        let Some(reference_decoder) = find_htj2k_reference_decoder_for_test() else {
            eprintln!(
                "skipping external HTJ2K parity smoke: grk_decompress or kdu_expand not found"
            );
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.dcm");
        let out = tmp.path().join("out");
        let expected = vec![
            255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 0, 255, 255, 255, 0, 255,
        ];
        write_source_dicom_with_pixels(
            &source,
            "1.2.826.0.1.3680043.10.999.93",
            3,
            2,
            expected.clone(),
        );

        let report = export_dicom(DicomExportRequest {
            source_path: source,
            output_dir: out,
            options: DicomExportOptions {
                tile_size: 3,
                transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();
        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        let fragments = object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap();
        assert_eq!(fragments.len(), 1);

        let codestream_path = tmp.path().join("frame.j2k");
        let ppm_path = tmp.path().join("frame.ppm");
        std::fs::write(
            &codestream_path,
            dicom_fragment_payload_without_padding(&fragments[0]),
        )
        .unwrap();
        reference_decoder.decode(&codestream_path, &ppm_path);

        let decoded = read_binary_ppm_for_test(&ppm_path);

        assert_eq!(decoded.0, 3);
        assert_eq!(decoded.1, 3);
        assert_eq!(&decoded.2[..expected.len()], expected.as_slice());
        assert_eq!(&decoded.2[expected.len()..], &[0; 9]);
    }

    #[test]
    fn external_dicom_validators_accept_htj2k_rpcl_when_available() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.dcm");
        let out = tmp.path().join("out");
        write_source_dicom_with_pixels(
            &source,
            "1.2.826.0.1.3680043.10.999.94",
            3,
            2,
            vec![
                255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 0, 255, 255, 255, 0, 255,
            ],
        );

        let report = export_dicom(DicomExportRequest {
            source_path: source,
            output_dir: out,
            options: DicomExportOptions {
                tile_size: 3,
                transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();

        run_dicom_validators_for_test(&report.instances[0].path);
    }

    #[test]
    fn export_dicom_writes_htj2k_lossless_vl_wsi_instances() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.dcm");
        let out = tmp.path().join("out");
        write_source_dicom(&source);

        let report = export_dicom(DicomExportRequest {
            source_path: source,
            output_dir: out.clone(),
            options: DicomExportOptions {
                tile_size: 2,
                transfer_syntax: TransferSyntax::Htj2kLossless,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();

        assert_eq!(report.instances.len(), 1);
        assert_eq!(report.metrics.total_frames, 2);
        assert_eq!(report.metrics.cpu_input_frames, 2);
        assert_eq!(report.metrics.gpu_input_decode_frames, 0);
        assert_eq!(report.metrics.gpu_encode_frames, 0);
        assert_eq!(report.metrics.gpu_validation_frames, 0);
        assert_eq!(
            report.instances[0].transfer_syntax_uid,
            TransferSyntax::Htj2kLossless.uid()
        );

        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        assert_eq!(
            object.meta().transfer_syntax.trim_end_matches('\0'),
            TransferSyntax::Htj2kLossless.uid()
        );
        assert_eq!(
            object
                .element(tags::PIXEL_DATA)
                .unwrap()
                .value()
                .fragments()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn profile_dicom_routes_limits_frames_without_writing_dicom() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.dcm");
        write_source_dicom(&source);

        let report = profile_dicom_routes(DicomRouteProfileRequest {
            source_path: source,
            options: DicomExportOptions {
                tile_size: 2,
                transfer_syntax: TransferSyntax::Htj2kLossless,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            level: 0,
            max_frames: 1,
        })
        .unwrap();

        assert_eq!(report.level, 0);
        assert_eq!(report.requested_frames, 1);
        assert_eq!(report.metrics.total_frames, 1);
        assert_eq!(report.metrics.cpu_input_frames, 1);
        assert_eq!(report.metrics.route_passthrough_frames(), 0);
        assert_eq!(report.metrics.gpu_transcode_frames, 0);
        assert_eq!(report.metrics.cpu_fallback_frames, 1);
        assert_eq!(report.metrics.route_unclassified_frames(), 0);
        assert!(report.elapsed_micros > 0);
    }

    #[test]
    fn profile_dicom_routes_reports_jpeg_baseline_passthrough_without_writing_dicom() {
        let tmp = tempfile::tempdir().unwrap();
        let jpeg_a = encode_test_jpeg(8, 8, [160, 20, 40]);
        let jpeg_b = encode_test_jpeg(8, 8, [20, 160, 40]);
        let source = tmp.path().join("source.svs");
        write_tiled_jpeg_tiff(&source, 16, 8, 8, 8, &[jpeg_a, jpeg_b]);

        let report = profile_dicom_routes(DicomRouteProfileRequest {
            source_path: source,
            options: DicomExportOptions {
                tile_size: 8,
                transfer_syntax: TransferSyntax::JpegBaseline8Bit,
                encode_backend: EncodeBackendPreference::RequireDevice,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            level: 0,
            max_frames: 2,
        })
        .unwrap();

        assert_eq!(report.level, 0);
        assert_eq!(report.requested_frames, 2);
        assert_eq!(report.metrics.total_frames, 2);
        assert_eq!(report.metrics.jpeg_passthrough_frames, 2);
        assert_eq!(report.metrics.rgb_like_frames, 2);
        assert_eq!(report.metrics.gray_frames, 0);
        assert_eq!(report.metrics.bits8_frames, 2);
        assert_eq!(report.metrics.bits16_frames, 0);
        assert_eq!(report.metrics.route_passthrough_frames(), 2);
        assert_eq!(report.metrics.jpeg_decode_fallback_frames, 0);
        assert_eq!(report.metrics.gpu_transcode_frames, 0);
        assert_eq!(report.metrics.cpu_fallback_frames, 0);
        assert_eq!(report.metrics.route_unclassified_frames(), 0);
        assert!(report.elapsed_micros > 0);
    }

    #[test]
    fn profile_jpeg_baseline_uses_native_source_tiles_for_passthrough() {
        let tmp = tempfile::tempdir().unwrap();
        let jpeg_a = encode_test_jpeg(8, 8, [160, 20, 40]);
        let jpeg_b = encode_test_jpeg(8, 8, [20, 160, 40]);
        let source = tmp.path().join("source.svs");
        write_tiled_jpeg_tiff(&source, 16, 8, 8, 8, &[jpeg_a, jpeg_b]);

        let report = profile_dicom_routes(DicomRouteProfileRequest {
            source_path: source,
            options: DicomExportOptions {
                tile_size: 16,
                transfer_syntax: TransferSyntax::JpegBaseline8Bit,
                encode_backend: EncodeBackendPreference::RequireDevice,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            level: 0,
            max_frames: 2,
        })
        .unwrap();

        assert_eq!(report.available_frames, 2);
        assert_eq!(report.metrics.total_frames, 2);
        assert_eq!(report.metrics.jpeg_passthrough_frames, 2);
        assert_eq!(report.metrics.jpeg_decode_fallback_frames, 0);
        assert_eq!(report.metrics.route_passthrough_frames(), 2);
        assert_eq!(report.metrics.cpu_fallback_frames, 0);
    }

    #[test]
    fn profile_dicom_routes_reports_jpeg_baseline_cpu_fallback_without_writing_dicom() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.dcm");
        write_source_dicom(&source);

        let report = profile_dicom_routes(DicomRouteProfileRequest {
            source_path: source,
            options: DicomExportOptions {
                tile_size: 2,
                transfer_syntax: TransferSyntax::JpegBaseline8Bit,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            level: 0,
            max_frames: 1,
        })
        .unwrap();

        assert_eq!(report.metrics.total_frames, 1);
        assert_eq!(report.metrics.jpeg_passthrough_frames, 0);
        assert_eq!(report.metrics.jpeg_decode_fallback_frames, 1);
        assert_eq!(report.metrics.jpeg_cpu_encode_frames, 1);
        assert_eq!(report.metrics.jpeg_metal_encode_frames, 0);
        assert_eq!(report.metrics.cpu_input_frames, 1);
        assert_eq!(report.metrics.gpu_input_decode_frames, 0);
        assert_eq!(report.metrics.route_passthrough_frames(), 0);
        assert_eq!(report.metrics.gpu_transcode_frames, 0);
        assert_eq!(report.metrics.cpu_fallback_frames, 1);
        assert_eq!(report.metrics.route_unclassified_frames(), 0);
        assert!(report.metrics.input_decode_micros > 0);
        assert!(report.metrics.compose_micros > 0);
        assert!(report.metrics.encode_micros > 0);
        assert!(report.elapsed_micros > 0);
    }

    #[test]
    fn profile_dicom_route_coverage_aggregates_all_levels_without_writing_dicom() {
        let tmp = tempfile::tempdir().unwrap();
        let source_dir = tmp.path().join("source");
        std::fs::create_dir_all(&source_dir).unwrap();
        let source_level0 = source_dir.join("level0.dcm");
        let source_level1 = source_dir.join("level1.dcm");
        write_source_dicom_with_dimensions(&source_level0, "1.2.826.0.1.3680043.10.999.31", 4, 4);
        write_source_dicom_with_dimensions(&source_level1, "1.2.826.0.1.3680043.10.999.32", 2, 2);

        let report = profile_dicom_route_coverage(DicomRouteCoverageRequest {
            source_path: source_level0,
            options: DicomExportOptions {
                tile_size: 2,
                transfer_syntax: TransferSyntax::Htj2kLossless,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            max_frames_per_level: 1,
            max_levels: None,
            max_level_elapsed: None,
            progress: None,
        })
        .unwrap();

        assert_eq!(report.requested_frames_per_level, 1);
        assert_eq!(report.levels.len(), 2);
        assert_eq!(report.levels[0].level, 0);
        assert_eq!(report.levels[1].level, 1);
        assert_eq!(report.levels[0].available_frames, 4);
        assert_eq!(report.levels[1].available_frames, 1);
        assert_eq!(report.available_frames, 5);
        assert!(!report.complete_frame_coverage);
        assert_eq!(report.metrics.total_frames, 2);
        assert_eq!(report.metrics.cpu_input_frames, 2);
        assert_eq!(report.metrics.cpu_fallback_frames, 2);
        assert_eq!(report.metrics.route_passthrough_frames(), 0);
        assert_eq!(report.metrics.gpu_transcode_frames, 0);
        assert_eq!(report.metrics.route_unclassified_frames(), 0);
        assert!(report.elapsed_micros > 0);
    }

    #[test]
    fn profile_dicom_route_coverage_can_limit_levels_for_bounded_real_checks() {
        let tmp = tempfile::tempdir().unwrap();
        let source_dir = tmp.path().join("source");
        std::fs::create_dir_all(&source_dir).unwrap();
        let source_level0 = source_dir.join("level0.dcm");
        let source_level1 = source_dir.join("level1.dcm");
        write_source_dicom_with_dimensions(&source_level0, "1.2.826.0.1.3680043.10.999.41", 4, 4);
        write_source_dicom_with_dimensions(&source_level1, "1.2.826.0.1.3680043.10.999.42", 2, 2);

        let report = profile_dicom_route_coverage(DicomRouteCoverageRequest {
            source_path: source_level0,
            options: DicomExportOptions {
                tile_size: 2,
                transfer_syntax: TransferSyntax::Htj2kLossless,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            max_frames_per_level: 1,
            max_levels: Some(1),
            max_level_elapsed: None,
            progress: None,
        })
        .unwrap();

        assert_eq!(report.levels.len(), 1);
        assert_eq!(report.levels[0].level, 0);
        assert_eq!(report.metrics.total_frames, 1);
        assert_eq!(report.metrics.cpu_fallback_frames, 1);
        assert_eq!(report.metrics.route_unclassified_frames(), 0);
    }

    #[test]
    fn profile_dicom_route_coverage_rejects_zero_level_elapsed_limit() {
        let err = profile_dicom_route_coverage(DicomRouteCoverageRequest {
            source_path: PathBuf::from("source.dcm"),
            options: DicomExportOptions {
                tile_size: 2,
                transfer_syntax: TransferSyntax::Htj2kLossless,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            max_frames_per_level: 1,
            max_levels: Some(1),
            max_level_elapsed: Some(Duration::ZERO),
            progress: None,
        })
        .unwrap_err();

        assert!(
            err.to_string().contains("max_level_elapsed"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn route_level_deadline_reports_elapsed_limit() {
        let deadline = RouteLevelDeadline {
            started: Instant::now() - Duration::from_millis(2),
            max_elapsed: Duration::from_millis(1),
        };
        let err = check_route_level_deadline(Some(deadline), 3).unwrap_err();

        assert!(
            err.to_string().contains("max_level_elapsed"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains("level 3"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn profile_dicom_route_corpus_coverage_rejects_zero_level_elapsed_limit() {
        let err = profile_dicom_route_corpus_coverage(DicomRouteCorpusCoverageRequest {
            source_root: PathBuf::from("slides"),
            options: DicomExportOptions {
                tile_size: 2,
                transfer_syntax: TransferSyntax::Htj2kLossless,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            max_frames_per_level: 1,
            max_levels: Some(1),
            max_level_elapsed: Some(Duration::ZERO),
            progress: None,
        })
        .unwrap_err();

        assert!(
            err.to_string().contains("max_level_elapsed"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn route_coverage_report_serializes_metrics_for_batch_aggregation() {
        let report = DicomRouteCoverageReport {
            source_path: PathBuf::from("source.svs"),
            transfer_syntax_uid: TransferSyntax::Htj2kLosslessRpcl.uid(),
            requested_frames_per_level: 8,
            available_frames: 64,
            complete_frame_coverage: false,
            levels: vec![DicomRouteProfileReport {
                source_path: PathBuf::from("source.svs"),
                transfer_syntax_uid: TransferSyntax::Htj2kLosslessRpcl.uid(),
                level: 2,
                requested_frames: 8,
                available_frames: 64,
                metrics: DicomExportMetrics {
                    total_frames: 8,
                    gpu_transcode_frames: 6,
                    resident_gpu_transcode_frames: 6,
                    cpu_fallback_frames: 2,
                    ..DicomExportMetrics::default()
                },
                elapsed_micros: 42_000,
            }],
            metrics: DicomExportMetrics {
                total_frames: 8,
                gpu_transcode_frames: 6,
                resident_gpu_transcode_frames: 6,
                cpu_fallback_frames: 2,
                gpu_dispatch_micros: 7_500,
                gpu_encode_configured_inflight_tiles: 8,
                gpu_encode_effective_inflight_tiles: 4,
                gpu_encode_max_observed_inflight_tiles: 4,
                gpu_encode_configured_memory_mib: 4096,
                gpu_encode_effective_memory_mib: 3277,
                gpu_encode_wall_micros: 5_000,
                gpu_encode_hardware_micros: 2_500,
                gpu_encode_dispatch_overhead_micros: 5_000,
                ..DicomExportMetrics::default()
            },
            elapsed_micros: 45_000,
        };

        let value = serde_json::to_value(&report).unwrap();

        assert_eq!(value["source_path"], "source.svs");
        assert_eq!(value["metrics"]["total_frames"], 8);
        assert_eq!(value["metrics"]["gpu_transcode_frames"], 6);
        assert_eq!(value["metrics"]["gpu_dispatch_micros"], 7_500);
        assert_eq!(value["metrics"]["gpu_encode_configured_inflight_tiles"], 8);
        assert_eq!(value["metrics"]["gpu_encode_effective_inflight_tiles"], 4);
        assert_eq!(
            value["metrics"]["gpu_encode_max_observed_inflight_tiles"],
            4
        );
        assert_eq!(value["metrics"]["gpu_encode_configured_memory_mib"], 4096);
        assert_eq!(value["metrics"]["gpu_encode_effective_memory_mib"], 3277);
        assert_eq!(value["metrics"]["gpu_encode_wall_micros"], 5_000);
        assert_eq!(value["metrics"]["gpu_encode_effective_parallelism"], 0.5);
        assert_eq!(value["metrics"]["gpu_encode_hardware_micros"], 2_500);
        assert_eq!(
            value["metrics"]["gpu_encode_dispatch_overhead_micros"],
            5_000
        );
        assert_eq!(value["metrics"]["cpu_fallback_frames"], 2);
        assert_eq!(value["available_frames"], 64);
        assert_eq!(value["complete_frame_coverage"], false);
        assert_eq!(value["levels"][0]["level"], 2);
        assert_eq!(value["levels"][0]["available_frames"], 64);
    }

    #[test]
    fn corpus_route_coverage_aggregates_sources_and_records_failures() {
        let tmp = tempfile::tempdir().unwrap();
        let source_dir = tmp.path().join("corpus");
        std::fs::create_dir_all(&source_dir).unwrap();
        let jpeg = encode_test_jpeg(8, 8, [120, 30, 90]);
        write_tiled_jpeg_tiff(&source_dir.join("good.svs"), 8, 8, 8, 8, &[jpeg]);
        std::fs::write(source_dir.join("bad.svs"), b"not a slide").unwrap();
        std::fs::write(source_dir.join("ignored.txt"), b"ignored").unwrap();

        let report = profile_dicom_route_corpus_coverage(DicomRouteCorpusCoverageRequest {
            source_root: source_dir,
            options: DicomExportOptions {
                tile_size: 8,
                transfer_syntax: TransferSyntax::JpegBaseline8Bit,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            max_frames_per_level: 1,
            max_levels: Some(1),
            max_level_elapsed: None,
            progress: None,
        })
        .unwrap();

        assert_eq!(report.sources_considered, 2);
        assert_eq!(report.reports.len(), 1);
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.available_frames, 1);
        assert_eq!(report.reports[0].available_frames, 1);
        assert!(report.failures[0]
            .source_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("bad.svs"));
        assert_eq!(report.metrics.total_frames, 1);
        assert_eq!(report.metrics.jpeg_passthrough_frames, 1);
        assert_eq!(report.metrics.cpu_fallback_frames, 0);
        assert_eq!(report.metrics.route_unclassified_frames(), 0);
    }

    #[test]
    fn profile_dicom_route_coverage_classifies_jpeg_fallback_without_decoding() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.dcm");
        write_source_dicom(&source);

        let report = profile_dicom_route_coverage(DicomRouteCoverageRequest {
            source_path: source,
            options: DicomExportOptions {
                tile_size: 2,
                transfer_syntax: TransferSyntax::JpegBaseline8Bit,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            max_frames_per_level: 1,
            max_levels: Some(1),
            max_level_elapsed: None,
            progress: None,
        })
        .unwrap();

        assert_eq!(report.metrics.total_frames, 1);
        assert_eq!(report.metrics.jpeg_passthrough_frames, 0);
        assert_eq!(report.metrics.jpeg_decode_fallback_frames, 1);
        assert_eq!(report.metrics.cpu_fallback_frames, 1);
        assert_eq!(report.metrics.cpu_input_frames, 0);
        assert_eq!(report.metrics.jpeg_cpu_encode_frames, 0);
        assert_eq!(report.metrics.input_decode_micros, 0);
        assert_eq!(report.metrics.encode_micros, 0);
        assert_eq!(report.metrics.route_unclassified_frames(), 0);
    }

    #[test]
    fn export_dicom_tags_sibling_levels_as_one_pyramid_series() {
        let tmp = tempfile::tempdir().unwrap();
        let source_dir = tmp.path().join("source");
        std::fs::create_dir_all(&source_dir).unwrap();
        let source_level0 = source_dir.join("level0.dcm");
        let source_level1 = source_dir.join("level1.dcm");
        let out = tmp.path().join("out");
        write_source_dicom_with_dimensions(&source_level0, "1.2.826.0.1.3680043.10.999.11", 4, 4);
        write_source_dicom_with_dimensions(&source_level1, "1.2.826.0.1.3680043.10.999.12", 2, 2);

        let report = export_dicom(DicomExportRequest {
            source_path: source_level0,
            output_dir: out,
            options: DicomExportOptions {
                tile_size: 2,
                encode_backend: EncodeBackendPreference::CpuOnly,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();

        assert_eq!(report.instances.len(), 2);

        let level0 = dicom_object::open_file(&report.instances[0].path).unwrap();
        let level1 = dicom_object::open_file(&report.instances[1].path).unwrap();
        let series_uid = level0
            .element(tags::SERIES_INSTANCE_UID)
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(
            level1
                .element(tags::SERIES_INSTANCE_UID)
                .unwrap()
                .to_str()
                .unwrap(),
            series_uid
        );
        let pyramid_uid = level0.element(tags::PYRAMID_UID).unwrap().to_str().unwrap();
        assert_eq!(
            level1.element(tags::PYRAMID_UID).unwrap().to_str().unwrap(),
            pyramid_uid
        );
        let frame_of_reference_uid = level0
            .element(tags::FRAME_OF_REFERENCE_UID)
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(
            level1
                .element(tags::FRAME_OF_REFERENCE_UID)
                .unwrap()
                .to_str()
                .unwrap(),
            frame_of_reference_uid
        );
        assert_eq!(
            level0
                .element(tags::IMAGE_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "ORIGINAL\\PRIMARY\\VOLUME\\NONE"
        );
        assert_eq!(
            level1
                .element(tags::IMAGE_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "DERIVED\\PRIMARY\\VOLUME\\RESAMPLED"
        );
        assert_eq!(
            level0
                .element(tags::INSTANCE_NUMBER)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            1
        );
        assert_eq!(
            level1
                .element(tags::INSTANCE_NUMBER)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            2
        );

        let slide = Slide::open(&report.instances[0].path).unwrap();
        let levels = &slide.dataset().scenes[0].series[0].levels;
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0].dimensions, (4, 4));
        assert_eq!(levels[1].dimensions, (2, 2));
    }

    #[test]
    fn export_dicom_can_limit_to_single_pyramid_level() {
        let tmp = tempfile::tempdir().unwrap();
        let source_dir = tmp.path().join("source");
        std::fs::create_dir_all(&source_dir).unwrap();
        let source_level0 = source_dir.join("level0.dcm");
        let source_level1 = source_dir.join("level1.dcm");
        write_source_dicom_with_dimensions(&source_level0, "1.2.826.0.1.3680043.10.999.21", 4, 4);
        write_source_dicom_with_dimensions(&source_level1, "1.2.826.0.1.3680043.10.999.22", 2, 2);

        let report = export_dicom(DicomExportRequest {
            source_path: source_level0,
            output_dir: tmp.path().join("out"),
            options: DicomExportOptions {
                tile_size: 2,
                encode_backend: EncodeBackendPreference::CpuOnly,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: Some(1),
        })
        .unwrap();

        assert_eq!(report.instances.len(), 1);
        assert_eq!(report.instances[0].level, 1);
        assert_eq!(report.instances[0].frame_count, 1);

        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        assert_eq!(
            object
                .element(tags::TOTAL_PIXEL_MATRIX_COLUMNS)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            2
        );
        assert_eq!(
            object
                .element(tags::IMAGE_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "DERIVED\\PRIMARY\\VOLUME\\RESAMPLED"
        );
    }

    #[test]
    fn export_dicom_passthrough_writes_jpeg_baseline_vl_wsi_instance() {
        let tmp = tempfile::tempdir().unwrap();
        let jpeg = encode_test_jpeg(8, 8, [160, 20, 40]);
        let source = tmp.path().join("source.svs");
        write_tiled_jpeg_tiff(&source, 8, 8, 8, 8, std::slice::from_ref(&jpeg));
        let out = tmp.path().join("out");

        let report = export_dicom(DicomExportRequest {
            source_path: source,
            output_dir: out,
            options: DicomExportOptions {
                tile_size: 8,
                transfer_syntax: TransferSyntax::JpegBaseline8Bit,
                encode_backend: EncodeBackendPreference::RequireDevice,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();

        assert_eq!(report.instances.len(), 1);
        assert_eq!(report.instances[0].frame_count, 1);
        assert_eq!(report.metrics.total_frames, 1);
        assert_eq!(report.metrics.cpu_input_frames, 0);
        assert_eq!(report.metrics.gpu_input_decode_frames, 0);
        assert_eq!(report.metrics.gpu_encode_frames, 0);
        assert_eq!(report.metrics.route_passthrough_frames(), 1);
        assert_eq!(report.metrics.gpu_transcode_frames, 0);
        assert_eq!(report.metrics.cpu_fallback_frames, 0);
        assert_eq!(report.metrics.route_unclassified_frames(), 0);
        assert_eq!(
            report.instances[0].transfer_syntax_uid,
            TransferSyntax::JpegBaseline8Bit.uid()
        );

        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        assert_eq!(
            object.meta().transfer_syntax.trim_end_matches('\0'),
            TransferSyntax::JpegBaseline8Bit.uid()
        );
        assert_eq!(object.element(tags::PYRAMID_UID).unwrap().vr(), VR::UI);
        assert_eq!(
            object.element(tags::FRAME_OF_REFERENCE_UID).unwrap().vr(),
            VR::UI
        );
        assert_eq!(
            object
                .element(tags::INSTANCE_NUMBER)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            1
        );
        assert_eq!(
            object
                .element(tags::LOSSY_IMAGE_COMPRESSION)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "01"
        );
        assert_eq!(
            object
                .element(tags::LOSSY_IMAGE_COMPRESSION_METHOD)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "ISO_10918_1"
        );
        assert!(
            object
                .element(tags::LOSSY_IMAGE_COMPRESSION_RATIO)
                .unwrap()
                .to_float32()
                .unwrap()
                > 0.0
        );
        let fragments = object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap();
        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0], jpeg);
    }

    #[test]
    fn export_dicom_jpeg_baseline_reencodes_non_passthrough_source() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.dcm");
        write_source_dicom(&source);

        let report = export_dicom(DicomExportRequest {
            source_path: source,
            output_dir: tmp.path().join("out"),
            options: DicomExportOptions {
                tile_size: 2,
                transfer_syntax: TransferSyntax::JpegBaseline8Bit,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();

        assert_eq!(report.instances.len(), 1);
        assert_eq!(report.instances[0].frame_count, 2);
        assert_eq!(report.metrics.total_frames, 2);
        assert_eq!(report.metrics.cpu_input_frames, 2);
        assert_eq!(report.metrics.gpu_encode_frames, 0);
        assert_eq!(report.metrics.route_passthrough_frames(), 0);
        assert_eq!(report.metrics.gpu_transcode_frames, 0);
        assert_eq!(report.metrics.cpu_fallback_frames, 2);
        assert_eq!(report.metrics.route_unclassified_frames(), 0);

        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        assert_eq!(
            object.meta().transfer_syntax.trim_end_matches('\0'),
            TransferSyntax::JpegBaseline8Bit.uid()
        );
        assert_eq!(
            object
                .element(tags::PHOTOMETRIC_INTERPRETATION)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "YBR_FULL_422"
        );
        let fragments = object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap();
        assert_eq!(fragments.len(), 2);
        for fragment in fragments {
            let fragment = dicom_fragment_jpeg_payload(fragment);
            assert!(fragment.starts_with(&[0xFF, 0xD8]));
            assert!(fragment.ends_with(&[0xFF, 0xD9]));
            let decoder = signinum_jpeg::Decoder::new(fragment).unwrap();
            let (_rgb, outcome) = decoder.decode(signinum_jpeg::PixelFormat::Rgb8).unwrap();
            assert_eq!((outcome.decoded.w, outcome.decoded.h), (2, 2));
        }
    }

    #[test]
    fn external_djpeg_decodes_jpeg_baseline_fallback_when_available() {
        let Some(djpeg) = find_command_for_test("djpeg") else {
            eprintln!("skipping external JPEG parity smoke: djpeg not found");
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.dcm");
        let out = tmp.path().join("out");
        let expected_pixel = [64u8, 128, 192];
        let expected = vec![expected_pixel; 4]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        write_source_dicom_with_pixels(
            &source,
            "1.2.826.0.1.3680043.10.999.92",
            2,
            2,
            expected.clone(),
        );

        let report = export_dicom(DicomExportRequest {
            source_path: source,
            output_dir: out,
            options: DicomExportOptions {
                tile_size: 2,
                transfer_syntax: TransferSyntax::JpegBaseline8Bit,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();
        assert_eq!(report.metrics.jpeg_decode_fallback_frames, 1);
        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        let fragments = object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap();
        assert_eq!(fragments.len(), 1);

        let jpeg_path = tmp.path().join("frame.jpg");
        let ppm_path = tmp.path().join("frame.ppm");
        std::fs::write(&jpeg_path, dicom_fragment_jpeg_payload(&fragments[0])).unwrap();
        let status = std::process::Command::new(djpeg)
            .args(["-outfile"])
            .arg(&ppm_path)
            .arg(&jpeg_path)
            .status()
            .unwrap();
        assert!(status.success(), "djpeg failed with {status}");

        let decoded = read_binary_ppm_for_test(&ppm_path);

        assert_eq!(decoded.0, 2);
        assert_eq!(decoded.1, 2);
        assert_eq!(decoded.2.len(), expected.len());
        for (actual, expected) in decoded.2.iter().zip(expected.iter()) {
            assert!(actual.abs_diff(*expected) <= 12);
        }
    }

    #[test]
    fn jpeg_baseline_whole_level_passthrough_uses_rectangular_virtual_frame_geometry() {
        let level = statumen::Level {
            dimensions: (130, 31),
            downsample: 1.0,
            tile_layout: TileLayout::WholeLevel {
                width: 130,
                height: 31,
                virtual_tile_width: 64,
                virtual_tile_height: 8,
            },
        };

        let geometry = jpeg_baseline_frame_geometry(&level, 512).unwrap();

        assert_eq!(geometry.frame_columns, 64);
        assert_eq!(geometry.frame_rows, 8);
        assert_eq!(geometry.tiles_across, 3);
        assert_eq!(geometry.tiles_down, 4);
    }

    #[test]
    fn jpeg_baseline_regular_fallback_uses_requested_tile_geometry() {
        let level = statumen::Level {
            dimensions: (17, 9),
            downsample: 1.0,
            tile_layout: TileLayout::Regular {
                tile_width: 8,
                tile_height: 8,
                tiles_across: 3,
                tiles_down: 2,
            },
        };

        let geometry = jpeg_baseline_frame_geometry(&level, 16).unwrap();

        assert_eq!(geometry.frame_columns, 16);
        assert_eq!(geometry.frame_rows, 16);
        assert_eq!(geometry.tiles_across, 2);
        assert_eq!(geometry.tiles_down, 1);
    }

    #[test]
    fn jpeg_baseline_raw_passthrough_requires_jpeg_compression_and_matching_geometry() {
        let mut raw = RawCompressedTile {
            compression: Compression::Jp2kRgb,
            width: 512,
            height: 512,
            bits_allocated: 8,
            samples_per_pixel: 3,
            photometric_interpretation: EncodedTilePhotometricInterpretation::Rgb,
            data: vec![0xFF, 0x4F],
        };

        assert!(!raw_jpeg_matches_frame_geometry(&raw, 512, 512));
        raw.compression = Compression::Jpeg;
        assert!(raw_jpeg_matches_frame_geometry(&raw, 512, 512));
        assert!(!raw_jpeg_matches_frame_geometry(&raw, 256, 512));
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    fn auto_metal_input_routing_ignores_device_decode_env_until_explicitly_preferred() {
        let _guard = DEVICE_DECODE_ENV_MUTEX.lock().unwrap();
        let old_jpeg = std::env::var_os(STATUMEN_JPEG_DEVICE_DECODE_ENV);
        let old_jp2k = std::env::var_os(STATUMEN_JP2K_DEVICE_DECODE_ENV);
        std::env::remove_var(STATUMEN_JPEG_DEVICE_DECODE_ENV);
        std::env::remove_var(STATUMEN_JP2K_DEVICE_DECODE_ENV);

        assert!(!statumen_device_decode_opted_in());
        assert!(!MetalInputTileReader::new(EncodeBackendPreference::Auto, false).enabled());
        assert!(!lossless_j2k_auto_allows_metal_input(
            EncodeBackendPreference::Auto,
            TransferSyntax::Htj2kLosslessRpcl,
            1,
            true
        ));
        assert!(!lossless_j2k_auto_allows_metal_input(
            EncodeBackendPreference::Auto,
            TransferSyntax::Htj2kLosslessRpcl,
            15,
            true
        ));
        assert!(lossless_j2k_auto_allows_metal_input(
            EncodeBackendPreference::Auto,
            TransferSyntax::Htj2kLosslessRpcl,
            16,
            true
        ));
        assert!(lossless_j2k_auto_should_start_cpu_only(
            EncodeBackendPreference::Auto,
            TransferSyntax::Htj2kLosslessRpcl,
            1,
            true
        ));
        assert!(!lossless_j2k_auto_should_start_cpu_only(
            EncodeBackendPreference::Auto,
            TransferSyntax::Htj2kLosslessRpcl,
            16,
            true
        ));
        assert!(lossless_j2k_auto_should_start_cpu_only(
            EncodeBackendPreference::Auto,
            TransferSyntax::Jpeg2000Lossless,
            64,
            true
        ));
        assert!(!lossless_j2k_auto_should_start_cpu_only(
            EncodeBackendPreference::PreferDevice,
            TransferSyntax::Htj2kLosslessRpcl,
            1,
            true
        ));
        assert!(!jpeg_baseline_auto_allows_metal_batch(
            EncodeBackendPreference::Auto,
            512,
            512,
            4
        ));

        std::env::set_var(STATUMEN_JP2K_DEVICE_DECODE_ENV, "1");
        assert!(statumen_device_decode_opted_in());
        assert!(!MetalInputTileReader::new(EncodeBackendPreference::Auto, false).enabled());
        assert!(!lossless_j2k_auto_allows_metal_input(
            EncodeBackendPreference::Auto,
            TransferSyntax::Htj2kLosslessRpcl,
            1,
            false
        ));
        assert!(lossless_j2k_auto_allows_metal_input(
            EncodeBackendPreference::Auto,
            TransferSyntax::Htj2kLosslessRpcl,
            30,
            false
        ));
        assert!(!lossless_j2k_auto_allows_metal_input(
            EncodeBackendPreference::Auto,
            TransferSyntax::Jpeg2000Lossless,
            1,
            false
        ));
        assert!(!jpeg_baseline_auto_allows_metal_batch(
            EncodeBackendPreference::Auto,
            512,
            512,
            1
        ));
        assert!(!jpeg_baseline_auto_allows_metal_batch(
            EncodeBackendPreference::Auto,
            256,
            512,
            4
        ));
        assert!(!jpeg_baseline_auto_allows_metal_batch(
            EncodeBackendPreference::Auto,
            512,
            512,
            2
        ));
        assert!(!jpeg_baseline_auto_allows_metal_batch(
            EncodeBackendPreference::Auto,
            512,
            512,
            4
        ));
        assert!(jpeg_baseline_auto_allows_metal_batch(
            EncodeBackendPreference::PreferDevice,
            64,
            64,
            1
        ));
        assert!(!jpeg_baseline_auto_allows_metal_batch(
            EncodeBackendPreference::CpuOnly,
            1024,
            1024,
            8
        ));

        match old_jpeg {
            Some(value) => std::env::set_var(STATUMEN_JPEG_DEVICE_DECODE_ENV, value),
            None => std::env::remove_var(STATUMEN_JPEG_DEVICE_DECODE_ENV),
        }
        match old_jp2k {
            Some(value) => std::env::set_var(STATUMEN_JP2K_DEVICE_DECODE_ENV, value),
            None => std::env::remove_var(STATUMEN_JP2K_DEVICE_DECODE_ENV),
        }
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    fn lossless_j2k_source_device_decode_enables_private_jpeg_handoff() {
        let reader = MetalInputTileReader::new_for_lossless_j2k(
            EncodeBackendPreference::PreferDevice,
            true,
            None,
            true,
        );
        assert!(reader.private_jpeg_decode);

        let reader = MetalInputTileReader::new_for_lossless_j2k(
            EncodeBackendPreference::PreferDevice,
            true,
            None,
            false,
        );
        assert!(!reader.private_jpeg_decode);

        let jpeg_baseline_reader =
            MetalInputTileReader::new(EncodeBackendPreference::PreferDevice, true);
        assert!(!jpeg_baseline_reader.private_jpeg_decode);
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    fn auto_lossless_j2k_probe_requires_material_speedup() {
        assert_eq!(
            select_auto_lossless_j2k_probe_route(
                auto_route_candidate(true, 1_000),
                auto_route_candidate(true, 920),
                auto_route_candidate(false, 1),
            ),
            AutoLosslessJ2kRouteDecision::CpuOnly
        );
        assert_eq!(
            select_auto_lossless_j2k_probe_route(
                auto_route_candidate(true, 1_000),
                auto_route_candidate(true, 910),
                auto_route_candidate(false, 1),
            ),
            AutoLosslessJ2kRouteDecision::CpuInputDeviceEncode
        );
        assert_eq!(
            select_auto_lossless_j2k_probe_route(
                auto_route_candidate(true, 1_000),
                auto_route_candidate(true, 780),
                auto_route_candidate(true, 700),
            ),
            AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode
        );
        assert_eq!(
            select_auto_lossless_j2k_probe_route(
                auto_route_candidate(false, 1_000),
                auto_route_candidate(true, 900),
                auto_route_candidate(true, 800),
            ),
            AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode
        );
        assert_eq!(
            select_auto_lossless_j2k_probe_route(
                auto_route_candidate(true, 1_000),
                auto_route_candidate(false, 1),
                auto_route_candidate(false, 1),
            ),
            AutoLosslessJ2kRouteDecision::CpuOnly
        );
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    fn auto_cpu_input_device_encode_allows_gray_and_rgb_profiles() {
        let gray_run = CpuEncodedTileRun {
            tiles: vec![(
                Err(WsiDicomError::Unsupported {
                    reason: "not encoded in this selector test".into(),
                }),
                PixelProfile {
                    components: 1,
                    bits_allocated: 8,
                    photometric_interpretation: "MONOCHROME2",
                },
            )],
            input_decode_duration: Duration::ZERO,
            compose_duration: Duration::ZERO,
        };
        let rgb_run = CpuEncodedTileRun {
            tiles: vec![(
                Err(WsiDicomError::Unsupported {
                    reason: "not encoded in this selector test".into(),
                }),
                PixelProfile {
                    components: 3,
                    bits_allocated: 8,
                    photometric_interpretation: "RGB",
                },
            )],
            input_decode_duration: Duration::ZERO,
            compose_duration: Duration::ZERO,
        };
        let cmyk_run = CpuEncodedTileRun {
            tiles: vec![(
                Err(WsiDicomError::Unsupported {
                    reason: "not encoded in this selector test".into(),
                }),
                PixelProfile {
                    components: 4,
                    bits_allocated: 8,
                    photometric_interpretation: "CMYK",
                },
            )],
            input_decode_duration: Duration::ZERO,
            compose_duration: Duration::ZERO,
        };

        assert!(cpu_input_device_encode_auto_allowed(&gray_run));
        assert!(cpu_input_device_encode_auto_allowed(&rgb_run));
        assert!(!cpu_input_device_encode_auto_allowed(&cmyk_run));
        assert!(!cpu_input_device_encode_auto_probe_allowed(
            &rgb_run,
            LOSSLESS_J2K_AUTO_PARTIAL_GPU_MIN_FRAMES - 1
        ));
        assert!(cpu_input_device_encode_auto_probe_allowed(
            &rgb_run,
            LOSSLESS_J2K_AUTO_PARTIAL_GPU_MIN_FRAMES
        ));
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    fn auto_route_candidate(complete: bool, micros: u64) -> AutoLosslessJ2kRouteCandidate {
        AutoLosslessJ2kRouteCandidate {
            complete,
            duration: Duration::from_micros(micros),
        }
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    fn auto_metal_input_route_cache_reuses_probe_decision() {
        let _guard = DEVICE_DECODE_ENV_MUTEX.lock().unwrap();
        clear_auto_metal_input_route_cache_for_tests();
        clear_auto_metal_input_route_cache_state_for_tests();
        let key = AutoMetalInputRouteCacheKey {
            source_path: PathBuf::from("slide.svs"),
            level: 2,
            tile_size: 512,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            route_scope_frames: 1,
        };
        let full_key = AutoMetalInputRouteCacheKey {
            source_path: PathBuf::from("slide.svs"),
            level: 2,
            tile_size: 512,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            route_scope_frames: 128,
        };
        let partial_key = AutoMetalInputRouteCacheKey {
            source_path: PathBuf::from("partial.svs"),
            level: 0,
            tile_size: 512,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            route_scope_frames: 16,
        };

        let mut reader = MetalInputTileReader::new_with_auto_device_decode_and_cache_key(
            EncodeBackendPreference::Auto,
            true,
            Some(key.clone()),
            false,
        );
        assert!(reader.enabled());
        assert!(reader.auto_input_probe_pending());
        reader.record_auto_route_probe_decision(AutoLosslessJ2kRouteDecision::CpuOnly);

        let cached_cpu_reader = MetalInputTileReader::new_with_auto_device_decode_and_cache_key(
            EncodeBackendPreference::Auto,
            true,
            Some(key.clone()),
            false,
        );
        assert!(!cached_cpu_reader.enabled());
        assert!(!cached_cpu_reader.auto_input_probe_pending());
        assert_eq!(
            cached_cpu_reader.auto_route_decision(),
            AutoLosslessJ2kRouteDecision::CpuOnly
        );

        let uncached_full_reader = MetalInputTileReader::new_with_auto_device_decode_and_cache_key(
            EncodeBackendPreference::Auto,
            true,
            Some(full_key),
            false,
        );
        assert!(uncached_full_reader.enabled());
        assert!(uncached_full_reader.auto_input_probe_pending());

        store_cached_auto_metal_input_decision(
            &key,
            AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode,
        );
        let cached_gpu_reader = MetalInputTileReader::new_with_auto_device_decode_and_cache_key(
            EncodeBackendPreference::Auto,
            true,
            Some(key),
            false,
        );
        assert!(cached_gpu_reader.enabled());
        assert!(!cached_gpu_reader.auto_input_probe_pending());
        assert_eq!(
            cached_gpu_reader.auto_route_decision(),
            AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode
        );

        store_cached_auto_metal_input_decision(
            &partial_key,
            AutoLosslessJ2kRouteDecision::CpuInputDeviceEncode,
        );
        let cached_partial_reader = MetalInputTileReader::new_with_auto_device_decode_and_cache_key(
            EncodeBackendPreference::Auto,
            true,
            Some(partial_key),
            false,
        );
        assert!(!cached_partial_reader.enabled());
        assert!(!cached_partial_reader.auto_input_probe_pending());
        assert_eq!(
            cached_partial_reader.auto_route_decision(),
            AutoLosslessJ2kRouteDecision::CpuInputDeviceEncode
        );

        clear_auto_metal_input_route_cache_for_tests();
        clear_auto_metal_input_route_cache_state_for_tests();
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    fn auto_metal_input_route_cache_can_persist_when_env_path_is_set() {
        let _guard = DEVICE_DECODE_ENV_MUTEX.lock().unwrap();
        clear_auto_metal_input_route_cache_for_tests();
        clear_auto_metal_input_route_cache_state_for_tests();
        let old_cache = std::env::var_os(WSI_DICOM_AUTO_ROUTE_CACHE_ENV);
        let tmp = tempfile::tempdir().unwrap();
        let cache_path = tmp.path().join("auto-route-cache.json");
        std::env::set_var(WSI_DICOM_AUTO_ROUTE_CACHE_ENV, &cache_path);

        let key = AutoMetalInputRouteCacheKey {
            source_path: PathBuf::from("slide.svs"),
            level: 2,
            tile_size: 512,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            route_scope_frames: 128,
        };
        store_cached_auto_metal_input_decision(
            &key,
            AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode,
        );
        flush_persistent_auto_metal_input_route_cache_if_requested().unwrap();

        clear_auto_metal_input_route_cache_for_tests();
        clear_auto_metal_input_route_cache_state_for_tests();
        load_persistent_auto_metal_input_route_cache_if_requested().unwrap();

        assert_eq!(
            cached_auto_metal_input_decision(&key),
            Some(AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode)
        );

        match old_cache {
            Some(value) => std::env::set_var(WSI_DICOM_AUTO_ROUTE_CACHE_ENV, value),
            None => std::env::remove_var(WSI_DICOM_AUTO_ROUTE_CACHE_ENV),
        }
        clear_auto_metal_input_route_cache_for_tests();
        clear_auto_metal_input_route_cache_state_for_tests();
    }

    #[test]
    #[ignore = "requires WSI_DICOM_NDPI_FIXTURE"]
    fn ndpi_fixture_exports_all_lossless_j2k_transfer_syntaxes_and_tile_sizes() {
        let Some(source) = std::env::var_os("WSI_DICOM_NDPI_FIXTURE").map(PathBuf::from) else {
            return;
        };
        let slide = Slide::open(&source).unwrap();
        let level = &slide.dataset().scenes[0].series[0].levels[0];
        let (matrix_columns, matrix_rows) = level.dimensions;
        assert!(matrix_columns > 0);
        assert!(matrix_rows > 0);

        for tile_size in [512, 1024, 2048] {
            let tile_size_u64 = u64::from(tile_size);
            let x = ((matrix_columns - 1) / tile_size_u64) * tile_size_u64;
            let y = ((matrix_rows - 1) / tile_size_u64) * tile_size_u64;
            let width = (matrix_columns - x).min(tile_size_u64) as u32;
            let height = (matrix_rows - y).min(tile_size_u64) as u32;
            let region = slide
                .read_region(&RegionRequest {
                    scene: SceneId(0),
                    series: SeriesId(0),
                    level: LevelIdx(0),
                    plane: PlaneIdx(PlaneSelection { z: 0, c: 0, t: 0 }),
                    origin_px: (x as i64, y as i64),
                    size_px: (width, height),
                })
                .unwrap();
            let prepared = prepare_tile_samples(&region, tile_size, tile_size).unwrap();
            let samples = J2kLosslessSamples::new(
                &prepared.bytes,
                tile_size,
                tile_size,
                prepared.profile.components,
                prepared.profile.bits_allocated as u8,
                false,
            )
            .unwrap();

            for transfer_syntax in [
                TransferSyntax::Jpeg2000Lossless,
                TransferSyntax::Htj2kLossless,
                TransferSyntax::Htj2kLosslessRpcl,
            ] {
                let codestream = encode_dicom_lossless(
                    samples,
                    transfer_syntax,
                    EncodeBackendPreference::RequireDevice,
                    CodecValidation::RoundTrip,
                )
                .unwrap();
                assert_transfer_syntax_codestream(transfer_syntax, &codestream);
                assert_j2k_facade_roundtrip(samples, &codestream);
            }
        }
    }

    #[test]
    #[ignore = "requires WSI_DICOM_NDPI_FIXTURE"]
    fn ndpi_fixture_exports_full_jpeg_baseline_passthrough_instance() {
        let Some(source) = std::env::var_os("WSI_DICOM_NDPI_FIXTURE").map(PathBuf::from) else {
            return;
        };
        let output_dir_env = std::env::var_os("WSI_DICOM_NDPI_JPEG_OUT")
            .or_else(|| std::env::var_os("WSI_DICOM_NDPI_LEVEL3_JPEG_OUT"))
            .map(PathBuf::from);
        let temp_dir = output_dir_env
            .is_none()
            .then(|| tempfile::tempdir().unwrap());
        let output_dir =
            output_dir_env.unwrap_or_else(|| temp_dir.as_ref().unwrap().path().to_path_buf());
        std::fs::create_dir_all(&output_dir).unwrap();

        let request = DicomExportRequest {
            source_path: source.clone(),
            output_dir: output_dir.clone(),
            options: DicomExportOptions {
                tile_size: 512,
                transfer_syntax: TransferSyntax::JpegBaseline8Bit,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        };
        let metadata = request.metadata.resolve().unwrap();
        let study_uid = metadata
            .study_instance_uid
            .clone()
            .unwrap_or_else(|| uid_from_seed(&format!("study:{}", source.display())));
        let slide = Slide::open(&source).unwrap();
        let (level_idx, geometry) = ndpi_jpeg_passthrough_level(&slide, request.options.tile_size);
        let level = &slide.dataset().scenes[0].series[0].levels[level_idx];
        let expected_frames = geometry.tiles_across * geometry.tiles_down;

        let report = export_jpeg_passthrough_instance(
            &slide,
            &request,
            &metadata,
            &study_uid,
            1,
            0,
            0,
            level_idx as u32,
            0,
            0,
            0,
            level,
        )
        .unwrap();

        assert_eq!(report.level, level_idx as u32);
        assert_eq!(report.frame_count, expected_frames as u32);
        assert_eq!(report.metrics.total_frames, expected_frames);
        assert_eq!(report.metrics.jpeg_passthrough_frames, expected_frames);
        assert_eq!(report.metrics.jpeg_decode_fallback_frames, 0);
        assert_eq!(report.metrics.jpeg_cpu_encode_frames, 0);
        assert_eq!(report.metrics.jpeg_metal_encode_frames, 0);
        assert_eq!(report.metrics.cpu_input_frames, 0);
        assert_eq!(report.metrics.gpu_input_decode_frames, 0);
        assert_eq!(report.metrics.gpu_encode_frames, 0);

        let object = dicom_object::open_file(&report.path).unwrap();
        assert_eq!(
            object.meta().transfer_syntax.trim_end_matches('\0'),
            TransferSyntax::JpegBaseline8Bit.uid()
        );
        assert_eq!(
            object.element(tags::ROWS).unwrap().to_int::<u32>().unwrap(),
            geometry.frame_rows
        );
        assert_eq!(
            object
                .element(tags::COLUMNS)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            geometry.frame_columns
        );
        assert_eq!(
            object
                .element(tags::TOTAL_PIXEL_MATRIX_COLUMNS)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            level.dimensions.0 as u32
        );
        assert_eq!(
            object
                .element(tags::TOTAL_PIXEL_MATRIX_ROWS)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            level.dimensions.1 as u32
        );
        assert_eq!(
            object
                .element(tags::NUMBER_OF_FRAMES)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            expected_frames as u32
        );
        assert_eq!(
            object
                .element(tags::PIXEL_DATA)
                .unwrap()
                .value()
                .fragments()
                .unwrap()
                .len(),
            expected_frames as usize
        );
    }

    fn ndpi_jpeg_passthrough_level(
        slide: &Slide,
        tile_size: u32,
    ) -> (usize, JpegBaselineFrameGeometry) {
        let levels = &slide.dataset().scenes[0].series[0].levels;
        let mut best = None;
        for (level_idx, level) in levels.iter().enumerate() {
            let Ok(geometry) = jpeg_baseline_frame_geometry(level, tile_size) else {
                continue;
            };
            let Ok(frame_count) = geometry
                .tiles_across
                .checked_mul(geometry.tiles_down)
                .ok_or(())
            else {
                continue;
            };
            let Ok(raw) = slide.read_raw_compressed_tile(&TileRequest {
                scene: 0,
                series: 0,
                level: level_idx as u32,
                plane: PlaneSelection { z: 0, c: 0, t: 0 },
                col: 0,
                row: 0,
            }) else {
                continue;
            };
            if !raw_jpeg_matches_frame_geometry(&raw, geometry.frame_columns, geometry.frame_rows) {
                continue;
            }
            let Ok(profile) = pixel_profile_from_raw_jpeg_tile(&raw) else {
                continue;
            };
            if !raw_jpeg_profile_can_passthrough(
                profile,
                raw_rgb_passthrough_has_no_geometry_fallback(level, geometry),
            ) {
                continue;
            }
            if best
                .map(|(_, _, best_frame_count)| frame_count < best_frame_count)
                .unwrap_or(true)
            {
                best = Some((level_idx, geometry, frame_count));
            }
        }
        best.map(|(level_idx, geometry, _)| (level_idx, geometry))
            .expect("NDPI fixture did not expose any full JPEG Baseline passthrough level")
    }

    #[test]
    #[ignore = "requires WSI_DICOM_NDPI_FIXTURE"]
    fn ndpi_fixture_exports_jpeg_baseline_passthrough_pyramid_subset_for_qupath() {
        let Some(source) = std::env::var_os("WSI_DICOM_NDPI_FIXTURE").map(PathBuf::from) else {
            return;
        };
        let output_dir = std::env::var_os("WSI_DICOM_NDPI_PYRAMID_OUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| tempfile::tempdir().unwrap().keep());
        std::fs::create_dir_all(&output_dir).unwrap();

        let request = DicomExportRequest {
            source_path: source.clone(),
            output_dir,
            options: DicomExportOptions {
                tile_size: 512,
                transfer_syntax: TransferSyntax::JpegBaseline8Bit,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        };
        let metadata = request.metadata.resolve().unwrap();
        let study_uid = metadata
            .study_instance_uid
            .clone()
            .unwrap_or_else(|| uid_from_seed(&format!("study:{}", source.display())));
        let slide = Slide::open(&source).unwrap();
        let levels = ndpi_jpeg_passthrough_levels(&slide, request.options.tile_size);
        assert!(
            levels.len() >= 2,
            "NDPI fixture must expose at least two JPEG passthrough levels for pyramid testing"
        );

        let mut reports = Vec::with_capacity(levels.len());
        for (instance_idx, (level_idx, geometry)) in levels.iter().copied().enumerate() {
            let level = &slide.dataset().scenes[0].series[0].levels[level_idx];
            let expected_frames = geometry.tiles_across * geometry.tiles_down;
            let report = export_jpeg_passthrough_instance(
                &slide,
                &request,
                &metadata,
                &study_uid,
                (instance_idx + 1) as u32,
                0,
                0,
                level_idx as u32,
                0,
                0,
                0,
                level,
            )
            .unwrap();

            assert_eq!(report.metrics.total_frames, expected_frames);
            assert_eq!(report.metrics.jpeg_passthrough_frames, expected_frames);
            assert_eq!(report.metrics.jpeg_decode_fallback_frames, 0);
            assert_eq!(report.metrics.jpeg_cpu_encode_frames, 0);
            assert_eq!(report.metrics.cpu_input_frames, 0);
            reports.push(report);
        }

        let first = dicom_object::open_file(&reports[0].path).unwrap();
        let series_uid = first
            .element(tags::SERIES_INSTANCE_UID)
            .unwrap()
            .to_str()
            .unwrap();
        let pyramid_uid = first.element(tags::PYRAMID_UID).unwrap().to_str().unwrap();
        for report in &reports[1..] {
            let object = dicom_object::open_file(&report.path).unwrap();
            assert_eq!(
                object
                    .element(tags::SERIES_INSTANCE_UID)
                    .unwrap()
                    .to_str()
                    .unwrap(),
                series_uid
            );
            assert_eq!(
                object.element(tags::PYRAMID_UID).unwrap().to_str().unwrap(),
                pyramid_uid
            );
        }
    }

    fn ndpi_jpeg_passthrough_levels(
        slide: &Slide,
        tile_size: u32,
    ) -> Vec<(usize, JpegBaselineFrameGeometry)> {
        let mut levels = Vec::new();
        for (level_idx, level) in slide.dataset().scenes[0].series[0]
            .levels
            .iter()
            .enumerate()
        {
            let Ok(geometry) = jpeg_baseline_frame_geometry(level, tile_size) else {
                continue;
            };
            let Ok(raw) = slide.read_raw_compressed_tile(&TileRequest {
                scene: 0,
                series: 0,
                level: level_idx as u32,
                plane: PlaneSelection { z: 0, c: 0, t: 0 },
                col: 0,
                row: 0,
            }) else {
                continue;
            };
            if !raw_jpeg_matches_frame_geometry(&raw, geometry.frame_columns, geometry.frame_rows) {
                continue;
            }
            let Ok(profile) = pixel_profile_from_raw_jpeg_tile(&raw) else {
                continue;
            };
            if raw_jpeg_profile_can_passthrough(
                profile,
                raw_rgb_passthrough_has_no_geometry_fallback(level, geometry),
            ) {
                levels.push((level_idx, geometry));
            }
        }
        levels
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    #[ignore = "requires WSI_DICOM_METAL_INPUT_FIXTURE"]
    fn fixture_first_mappable_tiles_use_batched_statumen_metal_input_decode_and_metal_encode() {
        let Some(source) = std::env::var_os("WSI_DICOM_METAL_INPUT_FIXTURE").map(PathBuf::from)
        else {
            return;
        };
        std::env::set_var("STATUMEN_JPEG_DEVICE_DECODE", "1");
        std::env::set_var("STATUMEN_JP2K_DEVICE_DECODE", "1");

        let slide = Slide::open(&source).unwrap();
        let level = &slide.dataset().scenes[0].series[0].levels[0];
        let tile_size = match level.tile_layout {
            TileLayout::Regular {
                tile_width,
                tile_height,
                ..
            } => {
                assert_eq!(tile_width, tile_height);
                tile_width
            }
            TileLayout::WholeLevel {
                virtual_tile_width,
                virtual_tile_height,
                ..
            } if virtual_tile_width == virtual_tile_height => virtual_tile_width,
            TileLayout::WholeLevel { .. } => 512,
            _ => {
                panic!("fixture first level must use a mappable Regular or WholeLevel tile layout")
            }
        };
        let tiles_across = level.dimensions.0.div_ceil(u64::from(tile_size));
        let tile_count = tiles_across.min(2);
        assert!(tile_count > 0);

        let mut metal_input =
            MetalInputTileReader::new(EncodeBackendPreference::RequireDevice, true);
        let mut encoder = DicomJ2kEncoder::new(
            EncodeBackendPreference::RequireDevice,
            TransferSyntax::Jpeg2000Lossless,
            CodecValidation::RoundTrip,
        );
        let encoded = try_encode_metal_input_tile_run(
            &slide,
            &mut metal_input,
            &mut encoder,
            level,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            tile_count,
            level.dimensions.0,
            level.dimensions.1,
            tile_size,
        )
        .unwrap();

        assert_eq!(encoded.tiles.len(), tile_count as usize);
        assert!(encoded.input_decode_duration > Duration::ZERO);
        for frame in encoded.tiles {
            let frame = frame.expect("fixture tile should decode and encode on Metal");
            assert!(frame.0.used_device_encode);
            assert!(frame.0.used_device_validation);
            assert_transfer_syntax_codestream(
                TransferSyntax::Jpeg2000Lossless,
                frame.0.codestream_bytes().expect("codestream bytes"),
            );
        }
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    #[ignore = "requires WSI_DICOM_APERIO_JP2K_FIXTURE and Metal JP2K device decode"]
    fn aperio_jp2k_aligned_metal_input_256_htj2k_rpcl_tile_matches_cpu() {
        assert_aperio_jp2k_metal_input_tile_matches_cpu(256);
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    #[ignore = "requires WSI_DICOM_APERIO_JP2K_FIXTURE and Metal JP2K device decode"]
    fn aperio_jp2k_regular_tiled_metal_input_composes_512_htj2k_rpcl_tile_matches_cpu() {
        assert_aperio_jp2k_metal_input_tile_matches_cpu(512);
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    fn assert_aperio_jp2k_metal_input_tile_matches_cpu(tile_size: u32) {
        let Some(source) = std::env::var_os("WSI_DICOM_APERIO_JP2K_FIXTURE").map(PathBuf::from)
        else {
            return;
        };
        std::env::set_var("STATUMEN_JP2K_DEVICE_DECODE", "1");

        let slide = Slide::open(&source).unwrap();
        let level = &slide.dataset().scenes[0].series[0].levels[0];
        let TileLayout::Regular {
            tile_width,
            tile_height,
            ..
        } = level.tile_layout
        else {
            panic!("fixture first level must use a regular tiled source layout");
        };
        if tile_size > tile_width || tile_size > tile_height {
            assert!(tile_width < tile_size || tile_height < tile_size);
        }
        let mut metal_input =
            MetalInputTileReader::new(EncodeBackendPreference::RequireDevice, true);
        let mut encoder = DicomJ2kEncoder::new(
            EncodeBackendPreference::RequireDevice,
            TransferSyntax::Htj2kLosslessRpcl,
            CodecValidation::RoundTrip,
        );

        let mut encoded = try_encode_metal_input_tile_run(
            &slide,
            &mut metal_input,
            &mut encoder,
            level,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            1,
            level.dimensions.0,
            level.dimensions.1,
            tile_size,
        )
        .unwrap();

        assert_eq!(encoded.tiles.len(), 1);
        assert!(encoded.input_decode_duration > Duration::ZERO);
        if tile_size > tile_width || tile_size > tile_height {
            assert!(encoded.compose_duration > Duration::ZERO);
        } else {
            assert_eq!(encoded.compose_duration, Duration::ZERO);
        }
        let (frame, profile) = encoded.tiles.remove(0).expect("resident Metal frame");
        assert!(frame.used_device_encode);
        assert!(frame.used_device_validation);
        assert!(frame.codestream_is_metal_buffer_backed());
        assert_transfer_syntax_codestream(
            TransferSyntax::Htj2kLosslessRpcl,
            frame.codestream_bytes().expect("codestream bytes"),
        );

        let cpu_region = slide
            .read_region(&RegionRequest {
                scene: SceneId(0),
                series: SeriesId(0),
                level: LevelIdx(0),
                plane: PlaneIdx(PlaneSelection { z: 0, c: 0, t: 0 }),
                origin_px: (0, 0),
                size_px: (tile_size, tile_size),
            })
            .unwrap();
        let expected = prepare_tile_samples(&cpu_region, tile_size, tile_size).unwrap();
        let actual = decode_j2k_frame_for_test(
            frame.codestream_bytes().expect("codestream bytes"),
            tile_size,
            tile_size,
            profile.components,
            profile.bits_allocated,
        );
        if actual != expected.bytes {
            let max_abs_diff = actual
                .iter()
                .zip(expected.bytes.iter())
                .map(|(actual, expected)| actual.abs_diff(*expected))
                .max()
                .unwrap_or(0);
            let mismatches = actual
                .iter()
                .zip(expected.bytes.iter())
                .filter(|(actual, expected)| actual != expected)
                .count();
            let first_mismatch = actual
                .iter()
                .zip(expected.bytes.iter())
                .position(|(actual, expected)| actual != expected)
                .expect("mismatch exists");
            let pixel = first_mismatch / usize::from(profile.components);
            let x = pixel % tile_size as usize;
            let y = pixel / tile_size as usize;
            let channel = first_mismatch % usize::from(profile.components);
            panic!(
                "Metal input tile mismatch for tile_size={tile_size} at x={x}, y={y}, channel={channel}: actual={}, expected={}, max_abs_diff={max_abs_diff}, mismatches={mismatches}, len={}",
                actual[first_mismatch],
                expected.bytes[first_mismatch],
                actual.len()
            );
        }
    }

    fn assert_transfer_syntax_codestream(transfer_syntax: TransferSyntax, codestream: &[u8]) {
        match transfer_syntax {
            TransferSyntax::Jpeg2000Lossless => {}
            TransferSyntax::Htj2kLossless => {
                assert!(codestream.windows(2).any(|window| window == [0xFF, 0x50]));
            }
            TransferSyntax::Htj2kLosslessRpcl => {
                let cod_offset = codestream
                    .windows(2)
                    .position(|window| window == [0xFF, 0x52])
                    .expect("COD marker");
                assert_eq!(codestream[cod_offset + 5], 0x02);
                assert!(codestream.windows(2).any(|window| window == [0xFF, 0x50]));
                assert!(codestream.windows(2).any(|window| window == [0xFF, 0x55]));
            }
            TransferSyntax::JpegBaseline8Bit
            | TransferSyntax::Jpeg2000
            | TransferSyntax::ExplicitVrLittleEndian => {
                panic!("non-JPEG 2000 transfer syntax in lossless J2K fixture test");
            }
        }
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    fn require_device_source_tile_preference_rejects_cpu_decode_fallback() {
        let mut metal_input =
            MetalInputTileReader::new(EncodeBackendPreference::RequireDevice, true);
        let Ok(output) = metal_input.source_tile_output_preference() else {
            return;
        };

        assert!(output.requires_device());
        assert!(output.compressed_device_decode_enabled());
    }

    #[test]
    #[ignore = "requires WSI_DICOM_APERIO_JP2K_FIXTURE"]
    fn real_aperio_jp2k_problem_tile_round_trips() {
        let Some(source) = std::env::var_os("WSI_DICOM_APERIO_JP2K_FIXTURE").map(PathBuf::from)
        else {
            return;
        };
        let slide = Slide::open(&source).unwrap();
        let region = slide
            .read_region(&RegionRequest {
                scene: SceneId(0),
                series: SeriesId(0),
                level: LevelIdx(0),
                plane: PlaneIdx(PlaneSelection { z: 0, c: 0, t: 0 }),
                origin_px: (24 * 512, 12 * 512),
                size_px: (512, 512),
            })
            .unwrap();
        let prepared = prepare_tile_samples(&region, 512, 512).unwrap();
        let samples = J2kLosslessSamples::new(
            &prepared.bytes,
            512,
            512,
            prepared.profile.components,
            prepared.profile.bits_allocated as u8,
            false,
        )
        .unwrap();

        let tile_out = std::env::var_os("WSI_DICOM_APERIO_JP2K_TILE_OUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target/aperio-jp2k-problem-tile.rgb"));
        if let Some(parent) = tile_out.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(tile_out, &prepared.bytes).unwrap();
        encode_dicom_j2k_lossless(samples, EncodeBackendPreference::CpuOnly).unwrap();
    }

    #[test]
    #[ignore = "requires WSI_DICOM_EXPORT_DIR"]
    fn exported_aperio_jp2k_dicom_instances_read_back() {
        let Some(output_dir) = std::env::var_os("WSI_DICOM_EXPORT_DIR").map(PathBuf::from) else {
            return;
        };
        let expected = [
            (
                "level-0000-z0000-c0000-t0000.dcm",
                15374u32,
                17497u32,
                1085u32,
            ),
            ("level-0001-z0000-c0000-t0000.dcm", 3843u32, 4374u32, 72u32),
            ("level-0002-z0000-c0000-t0000.dcm", 1921u32, 2187u32, 20u32),
        ];

        for (file_name, columns, rows, frames) in expected {
            let object = dicom_object::open_file(output_dir.join(file_name)).unwrap();
            assert_eq!(
                object.meta().media_storage_sop_class_uid,
                uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE
            );
            assert_eq!(object.meta().transfer_syntax, uids::JPEG2000_LOSSLESS);
            assert_eq!(
                object
                    .element(tags::SOP_CLASS_UID)
                    .unwrap()
                    .to_str()
                    .unwrap(),
                uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE
            );
            assert_eq!(
                object
                    .element(tags::TOTAL_PIXEL_MATRIX_COLUMNS)
                    .unwrap()
                    .to_int::<u32>()
                    .unwrap(),
                columns
            );
            assert_eq!(
                object
                    .element(tags::TOTAL_PIXEL_MATRIX_ROWS)
                    .unwrap()
                    .to_int::<u32>()
                    .unwrap(),
                rows
            );
            assert_eq!(
                object
                    .element(tags::NUMBER_OF_FRAMES)
                    .unwrap()
                    .to_int::<u32>()
                    .unwrap(),
                frames
            );
            assert_eq!(
                object
                    .element(tags::PIXEL_DATA)
                    .unwrap()
                    .value()
                    .fragments()
                    .unwrap()
                    .len(),
                frames as usize
            );
        }
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    fn decode_j2k_frame_for_test(
        codestream: &[u8],
        width: u32,
        height: u32,
        components: u8,
        bits_allocated: u16,
    ) -> Vec<u8> {
        let fmt = match (components, bits_allocated) {
            (1, 8) => signinum_j2k::PixelFormat::Gray8,
            (3, 8) => signinum_j2k::PixelFormat::Rgb8,
            (1, 16) => signinum_j2k::PixelFormat::Gray16,
            (3, 16) => signinum_j2k::PixelFormat::Rgb16,
            other => panic!("unsupported frame profile: {other:?}"),
        };
        let bytes_per_sample = if bits_allocated <= 8 { 1usize } else { 2usize };
        let stride = width as usize * components as usize * bytes_per_sample;
        let mut decoder = signinum_j2k::J2kDecoder::new(codestream).unwrap_or_else(|err| {
            if codestream.last() == Some(&0) {
                signinum_j2k::J2kDecoder::new(&codestream[..codestream.len() - 1])
                    .unwrap_or_else(|_| panic!("parse frame: {err}"))
            } else {
                panic!("parse frame: {err}");
            }
        });
        let mut decoded = vec![0; stride * height as usize];
        decoder.decode_into(&mut decoded, stride, fmt).unwrap();
        decoded
    }

    #[test]
    #[ignore = "requires WSI_DICOM_NDPI_FIXTURE and Metal device decode"]
    #[cfg(all(feature = "metal", target_os = "macos"))]
    fn ndpi_whole_level_metal_rows_do_not_turn_black_after_reused_encoder_state() {
        let Some(source) = std::env::var_os("WSI_DICOM_NDPI_FIXTURE").map(PathBuf::from) else {
            return;
        };
        std::env::set_var("STATUMEN_JPEG_DEVICE_DECODE", "1");
        let slide = Slide::open(&source).unwrap();
        let level = &slide.dataset().scenes[0].series[0].levels[0];
        let (matrix_columns, matrix_rows) = level.dimensions;
        let tile_size = 512u32;
        let tiles_across = matrix_columns.div_ceil(u64::from(tile_size));
        let target_row = 12u64.min(matrix_rows.div_ceil(u64::from(tile_size)).saturating_sub(1));
        let target_col = 0u64;
        let mut metal_input =
            MetalInputTileReader::new(EncodeBackendPreference::RequireDevice, true);
        let mut j2k_encoder = DicomJ2kEncoder::new(
            EncodeBackendPreference::RequireDevice,
            TransferSyntax::Htj2kLossless,
            CodecValidation::RoundTrip,
        );

        let mut target = None;
        for row in 0..=target_row {
            let mut metal_row = try_encode_metal_input_tile_run(
                &slide,
                &mut metal_input,
                &mut j2k_encoder,
                level,
                0,
                0,
                0,
                0,
                0,
                0,
                row,
                0,
                tiles_across,
                matrix_columns,
                matrix_rows,
                tile_size,
            )
            .unwrap();
            if row == target_row {
                target = metal_row.tiles[target_col as usize].take();
            }
        }
        let (encoded, profile) =
            target.expect("fixture frame should encode through Metal input path");
        assert_eq!(profile.components, 3);
        assert!(encoded.used_device_encode);
        assert!(encoded.used_device_validation);

        let x = target_col * u64::from(tile_size);
        let y = target_row * u64::from(tile_size);
        let valid_width = (matrix_columns - x).min(u64::from(tile_size)) as u32;
        let valid_height = (matrix_rows - y).min(u64::from(tile_size)) as u32;
        let cpu_region = slide
            .read_region(&RegionRequest {
                scene: SceneId(0),
                series: SeriesId(0),
                level: LevelIdx(0),
                plane: PlaneIdx(PlaneSelection { z: 0, c: 0, t: 0 }),
                origin_px: (x as i64, y as i64),
                size_px: (valid_width, valid_height),
            })
            .unwrap();
        let expected = prepare_tile_samples(&cpu_region, tile_size, tile_size).unwrap();
        let actual = decode_j2k_frame_for_test(
            encoded.codestream_bytes().expect("codestream bytes"),
            tile_size,
            tile_size,
            profile.components,
            profile.bits_allocated,
        );

        if actual != expected.bytes {
            let actual_nonzero = actual.iter().filter(|value| **value != 0).count();
            let expected_nonzero = expected.bytes.iter().filter(|value| **value != 0).count();
            panic!(
                "Metal WholeLevel frame mismatch at row {target_row}, col {target_col}: actual_nonzero={actual_nonzero}, expected_nonzero={expected_nonzero}, total={}",
                actual.len()
            );
        }
    }

    #[test]
    #[ignore = "requires WSI_DICOM_NDPI_FIXTURE and Metal device decode"]
    #[cfg(all(feature = "metal", target_os = "macos"))]
    fn ndpi_whole_level_metal_composes_multi_tile_run_in_one_batch() {
        let Some(source) = std::env::var_os("WSI_DICOM_NDPI_FIXTURE").map(PathBuf::from) else {
            return;
        };
        std::env::set_var("STATUMEN_JPEG_DEVICE_DECODE", "1");
        let slide = Slide::open(&source).unwrap();
        let level = &slide.dataset().scenes[0].series[0].levels[0];
        let Some(strip_layout) = whole_level_strip_layout(level) else {
            return;
        };
        let (matrix_columns, matrix_rows) = level.dimensions;
        let tile_size = 512u32;
        let tile_count = matrix_columns.div_ceil(u64::from(tile_size)).min(3);
        assert!(tile_count > 1);

        let mut metal_input =
            MetalInputTileReader::new(EncodeBackendPreference::RequireDevice, true);
        let mut j2k_encoder = DicomJ2kEncoder::new(
            EncodeBackendPreference::RequireDevice,
            TransferSyntax::Htj2kLossless,
            CodecValidation::RoundTrip,
        );

        let encoded = try_encode_metal_whole_level_strip_run(
            &slide,
            &mut metal_input,
            &mut j2k_encoder,
            strip_layout,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            tile_count as usize,
            matrix_columns,
            matrix_rows,
            tile_size,
        )
        .unwrap();

        assert_eq!(encoded.tiles.len(), tile_count as usize);
        assert_eq!(encoded.compose_batches, 1);
        assert!(encoded.compose_duration > Duration::ZERO);
        for frame in encoded.tiles {
            let (frame, _) = frame.expect("fixture frame should encode through Metal input path");
            assert!(frame.used_device_encode);
        }
    }

    #[test]
    #[cfg(all(feature = "metal", target_os = "macos"))]
    fn metal_strip_composer_returns_ordered_tiles_from_batched_compose() {
        let Some(device) = metal::Device::system_default() else {
            return;
        };
        let composer = MetalStripComposer::new(device.clone()).unwrap();
        let layout = WholeLevelStripLayout {
            width: 4,
            height: 4,
        };
        let source_a = [1u8; 16];
        let source_b = [2u8; 16];
        let tile_a = metal_test_tile(&device, &source_a, 4, 4, SigninumPixelFormat::Gray8);
        let tile_b = metal_test_tile(&device, &source_b, 4, 4, SigninumPixelFormat::Gray8);
        let packed = composer
            .pack_tiles(&[tile_a, tile_b], layout, 0, 0, 2)
            .expect("pack test tiles");

        let composed = composer
            .compose_tiles(
                &packed,
                &[
                    MetalComposeTileRequest {
                        src_origin_x: 0,
                        src_origin_y: 0,
                        valid_width: 4,
                        valid_height: 4,
                        output_width: 4,
                        output_height: 4,
                    },
                    MetalComposeTileRequest {
                        src_origin_x: 4,
                        src_origin_y: 0,
                        valid_width: 4,
                        valid_height: 4,
                        output_width: 4,
                        output_height: 4,
                    },
                ],
            )
            .expect("batched compose");

        assert_eq!(composed.len(), 2);
        assert_eq!(composed[0].width, 4);
        assert_eq!(composed[1].width, 4);
        assert_eq!(composed[0].height, 4);
        assert_eq!(composed[1].height, 4);
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    fn metal_test_tile(
        device: &metal::Device,
        bytes: &[u8],
        width: u32,
        height: u32,
        format: SigninumPixelFormat,
    ) -> statumen::output::metal::MetalDeviceTile {
        let buffer = device.new_buffer_with_data(
            bytes.as_ptr().cast(),
            bytes.len() as u64,
            metal::MTLResourceOptions::StorageModeShared,
        );
        statumen::output::metal::MetalDeviceTile {
            width,
            height,
            pitch_bytes: width as usize * format.bytes_per_pixel(),
            format,
            storage: statumen::output::metal::MetalDeviceStorage::Buffer {
                buffer,
                byte_offset: 0,
            },
        }
    }

    #[test]
    fn cpu_j2k_batch_matches_serial_ordered_frames() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.dcm");
        write_source_dicom_with_dimensions(&source, "1.2.826.0.1.3680043.10.999.71", 4, 2);
        let slide = Slide::open(&source).unwrap();
        let frames = [
            LosslessJ2kCpuBatchFrame {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            },
            LosslessJ2kCpuBatchFrame {
                x: 2,
                y: 0,
                width: 2,
                height: 2,
            },
        ];

        let batch = encode_cpu_input_lossless_j2k_tile_batch(
            &slide,
            LosslessJ2kCpuBatchSettings {
                transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
                codec_validation: CodecValidation::RoundTrip,
            },
            0,
            0,
            0,
            0,
            0,
            0,
            &frames,
            2,
        )
        .unwrap();

        let mut serial_encoder = DicomJ2kEncoder::new(
            EncodeBackendPreference::CpuOnly,
            TransferSyntax::Htj2kLosslessRpcl,
            CodecValidation::RoundTrip,
        );
        let serial = frames
            .iter()
            .enumerate()
            .map(|(idx, frame)| {
                encode_cpu_input_tile(
                    &slide,
                    &mut serial_encoder,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    idx as u64,
                    frame.x,
                    frame.y,
                    frame.width,
                    frame.height,
                    2,
                )
                .unwrap()
            })
            .collect::<Vec<_>>();

        assert_eq!(batch.len(), serial.len());
        for (batch, serial) in batch.iter().zip(serial.iter()) {
            assert_eq!(batch.profile, serial.1);
            assert_eq!(
                batch.encoded.as_ref().unwrap().codestream_bytes().unwrap(),
                serial.0.as_ref().unwrap().codestream_bytes().unwrap()
            );
        }
    }

    #[test]
    fn jpeg_baseline_cpu_batch_matches_serial_ordered_frames() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.dcm");
        write_source_dicom_with_dimensions(&source, "1.2.826.0.1.3680043.10.999.72", 4, 2);
        let slide = Slide::open(&source).unwrap();
        let frames = [
            JpegBaselineFallbackFrame {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            },
            JpegBaselineFallbackFrame {
                x: 2,
                y: 0,
                width: 2,
                height: 2,
            },
        ];

        let batch =
            encode_jpeg_baseline_cpu_input_tile_batch(&slide, 0, 0, 0, 0, 0, 0, &frames, 2, 2)
                .unwrap();
        let serial = frames
            .iter()
            .map(|frame| {
                encode_jpeg_baseline_cpu_input_tile(
                    &slide,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    frame.x,
                    frame.y,
                    frame.width,
                    frame.height,
                    2,
                    2,
                )
                .unwrap()
            })
            .collect::<Vec<_>>();

        assert_eq!(batch.len(), serial.len());
        for (batch, serial) in batch.iter().zip(serial.iter()) {
            assert_eq!(batch.0.data, serial.0.data);
            assert_eq!(batch.1, serial.1);
        }
    }

    #[test]
    fn jpeg_baseline_cpu_fallback_writes_restart_markers_for_large_frames() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.dcm");
        write_source_dicom_with_dimensions(&source, "1.2.826.0.1.3680043.10.999.73", 160, 64);

        let report = export_dicom(DicomExportRequest {
            source_path: source,
            output_dir: tmp.path().join("out"),
            options: DicomExportOptions {
                tile_size: 160,
                transfer_syntax: TransferSyntax::JpegBaseline8Bit,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();

        assert_eq!(report.metrics.jpeg_cpu_encode_frames, 1);
        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        let fragments = object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap();
        let payload = dicom_fragment_jpeg_payload(&fragments[0]);
        assert!(payload.windows(2).any(|window| window == [0xFF, 0xDD]));
        assert!(payload.windows(2).any(|window| window == [0xFF, 0xD0]));
    }

    fn write_source_dicom(path: &std::path::Path) {
        write_source_dicom_with_pixels(
            path,
            "1.2.826.0.1.3680043.10.999.1",
            3,
            2,
            vec![
                255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 0, 255, 255, 255, 0, 255,
            ],
        );
    }

    fn write_source_dicom_with_dimensions(
        path: &std::path::Path,
        sop_instance_uid: &str,
        width: u32,
        height: u32,
    ) {
        let mut pixels = Vec::with_capacity((width as usize) * (height as usize) * 3);
        for y in 0..height {
            for x in 0..width {
                pixels.push((x * 37 + y * 11) as u8);
                pixels.push((x * 17 + y * 29) as u8);
                pixels.push((x * 7 + y * 43) as u8);
            }
        }
        write_source_dicom_with_pixels(path, sop_instance_uid, width, height, pixels);
    }

    fn write_source_dicom_with_pixels(
        path: &std::path::Path,
        sop_instance_uid: &str,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) {
        assert_eq!(pixels.len(), (width as usize) * (height as usize) * 3);
        let mut object = InMemDicomObject::new_empty();
        object.put(DataElement::new(
            tags::SOP_CLASS_UID,
            VR::UI,
            uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE,
        ));
        object.put(DataElement::new(
            tags::SOP_INSTANCE_UID,
            VR::UI,
            sop_instance_uid,
        ));
        object.put(DataElement::new(
            tags::SERIES_INSTANCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999",
        ));
        object.put(DataElement::new(
            tags::IMAGE_TYPE,
            VR::CS,
            "ORIGINAL\\PRIMARY\\VOLUME\\NONE",
        ));
        object.put(DataElement::new(
            tags::ROWS,
            VR::US,
            PrimitiveValue::from(height as u16),
        ));
        object.put(DataElement::new(
            tags::COLUMNS,
            VR::US,
            PrimitiveValue::from(width as u16),
        ));
        object.put(DataElement::new(
            tags::TOTAL_PIXEL_MATRIX_ROWS,
            VR::UL,
            PrimitiveValue::from(height),
        ));
        object.put(DataElement::new(
            tags::TOTAL_PIXEL_MATRIX_COLUMNS,
            VR::UL,
            PrimitiveValue::from(width),
        ));
        object.put(DataElement::new(
            tags::PIXEL_SPACING,
            VR::DS,
            "0.0005\\0.0005",
        ));
        object.put(DataElement::new(
            tags::NUMBER_OF_FRAMES,
            VR::IS,
            PrimitiveValue::from(1u32),
        ));
        object.put(DataElement::new(
            tags::SAMPLES_PER_PIXEL,
            VR::US,
            PrimitiveValue::from(3u16),
        ));
        object.put(DataElement::new(
            tags::PHOTOMETRIC_INTERPRETATION,
            VR::CS,
            "RGB",
        ));
        object.put(DataElement::new(
            tags::PLANAR_CONFIGURATION,
            VR::US,
            PrimitiveValue::from(0u16),
        ));
        object.put(DataElement::new(
            tags::BITS_ALLOCATED,
            VR::US,
            PrimitiveValue::from(8u16),
        ));
        object.put(DataElement::new(
            tags::BITS_STORED,
            VR::US,
            PrimitiveValue::from(8u16),
        ));
        object.put(DataElement::new(
            tags::HIGH_BIT,
            VR::US,
            PrimitiveValue::from(7u16),
        ));
        object.put(DataElement::new(
            tags::PIXEL_REPRESENTATION,
            VR::US,
            PrimitiveValue::from(0u16),
        ));
        object.put(DataElement::new(
            tags::PIXEL_DATA,
            VR::OB,
            PrimitiveValue::from(pixels),
        ));
        object
            .with_meta(
                FileMetaTableBuilder::new()
                    .media_storage_sop_class_uid(uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE)
                    .media_storage_sop_instance_uid(sop_instance_uid)
                    .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN),
            )
            .unwrap()
            .write_to_file(path)
            .unwrap();
    }

    fn encode_test_jpeg(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
        let pixels = vec![rgb; (width * height) as usize]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        encode_jpeg_baseline(
            JpegSamples::Rgb8 {
                data: &pixels,
                width,
                height,
            },
            JpegEncodeOptions {
                quality: 90,
                subsampling: JpegSubsampling::Ybr422,
                restart_interval: None,
                backend: JpegBackend::Cpu,
            },
        )
        .unwrap()
        .data
    }

    fn dicom_fragment_jpeg_payload(fragment: &[u8]) -> &[u8] {
        if fragment.len() >= 3
            && fragment.last() == Some(&0)
            && fragment[fragment.len() - 3..fragment.len() - 1] == [0xFF, 0xD9]
        {
            &fragment[..fragment.len() - 1]
        } else {
            fragment
        }
    }

    fn dicom_fragment_payload_without_padding(fragment: &[u8]) -> &[u8] {
        if fragment.len().is_multiple_of(2) && fragment.last() == Some(&0) {
            &fragment[..fragment.len() - 1]
        } else {
            fragment
        }
    }

    enum Htj2kReferenceDecoder {
        Grok(String),
        Kakadu(String),
    }

    impl Htj2kReferenceDecoder {
        fn decode(&self, codestream_path: &std::path::Path, ppm_path: &std::path::Path) {
            let (command, args): (&str, &[&str]) = match self {
                Self::Grok(command) => (command.as_str(), &["-i", "-o"]),
                Self::Kakadu(command) => (command.as_str(), &["-i", "-o"]),
            };
            let status = std::process::Command::new(command)
                .arg(args[0])
                .arg(codestream_path)
                .arg(args[1])
                .arg(ppm_path)
                .status()
                .unwrap();
            assert!(status.success(), "{command} failed with {status}");
        }
    }

    fn find_htj2k_reference_decoder_for_test() -> Option<Htj2kReferenceDecoder> {
        find_command_for_test("grk_decompress")
            .map(Htj2kReferenceDecoder::Grok)
            .or_else(|| find_command_for_test("kdu_expand").map(Htj2kReferenceDecoder::Kakadu))
    }

    fn run_dicom_validators_for_test(path: &std::path::Path) {
        let mut ran = false;
        if let Some(dciodvfy) = find_command_for_test("dciodvfy") {
            run_dicom_validator_for_test("dciodvfy", &dciodvfy, &["-new"], &[path]);
            ran = true;
        } else {
            eprintln!("skipping dciodvfy validation: dciodvfy not found");
        }
        if let Some(dcentvfy) = find_command_for_test("dcentvfy") {
            run_dicom_validator_for_test("dcentvfy", &dcentvfy, &[], &[path]);
            ran = true;
        } else {
            eprintln!("skipping dcentvfy validation: dcentvfy not found");
        }
        if !ran {
            eprintln!("skipping external DICOM validator smoke: no DICOM validators found");
        }
    }

    fn run_dicom_validator_for_test(
        name: &str,
        command: &str,
        args: &[&str],
        paths: &[&std::path::Path],
    ) {
        let output = std::process::Command::new(command)
            .args(args)
            .args(paths)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let has_error = stdout
            .lines()
            .chain(stderr.lines())
            .any(|line| line.trim_start().starts_with("Error"));

        assert!(
            output.status.success() && !has_error,
            "{name} failed with status {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            stdout,
            stderr
        );
    }

    fn find_command_for_test(name: &str) -> Option<String> {
        std::env::var_os("PATH")
            .and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|path| path.join(name))
                    .find(|path| path.is_file())
            })
            .or_else(|| {
                let staged = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("target")
                    .join("dicom3tools-mac")
                    .join(name);
                staged.is_file().then_some(staged)
            })
            .map(|path| path.to_string_lossy().into_owned())
    }

    fn read_binary_ppm_for_test(path: &std::path::Path) -> (u32, u32, Vec<u8>) {
        let bytes = std::fs::read(path).unwrap();
        let mut cursor = 0usize;
        let magic = read_netpbm_token_for_test(&bytes, &mut cursor);
        assert_eq!(magic, "P6");
        let width = read_netpbm_token_for_test(&bytes, &mut cursor)
            .parse::<u32>()
            .unwrap();
        let height = read_netpbm_token_for_test(&bytes, &mut cursor)
            .parse::<u32>()
            .unwrap();
        let max_value = read_netpbm_token_for_test(&bytes, &mut cursor)
            .parse::<u32>()
            .unwrap();
        assert_eq!(max_value, 255);
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let expected_len = (width as usize) * (height as usize) * 3;
        assert_eq!(bytes.len() - cursor, expected_len);
        (width, height, bytes[cursor..].to_vec())
    }

    fn read_netpbm_token_for_test(bytes: &[u8], cursor: &mut usize) -> String {
        loop {
            while *cursor < bytes.len() && bytes[*cursor].is_ascii_whitespace() {
                *cursor += 1;
            }
            if *cursor >= bytes.len() || bytes[*cursor] != b'#' {
                break;
            }
            while *cursor < bytes.len() && bytes[*cursor] != b'\n' {
                *cursor += 1;
            }
        }
        let start = *cursor;
        while *cursor < bytes.len() && !bytes[*cursor].is_ascii_whitespace() {
            *cursor += 1;
        }
        std::str::from_utf8(&bytes[start..*cursor])
            .unwrap()
            .to_string()
    }

    fn write_tiled_jpeg_tiff(
        path: &std::path::Path,
        width: u32,
        height: u32,
        tile_width: u32,
        tile_height: u32,
        tiles: &[Vec<u8>],
    ) {
        write_tiled_compressed_tiff(path, width, height, tile_width, tile_height, 7, 6, tiles);
    }

    fn write_tiled_jp2k_rgb_tiff(
        path: &std::path::Path,
        width: u32,
        height: u32,
        tile_width: u32,
        tile_height: u32,
        tiles: &[Vec<u8>],
    ) {
        write_tiled_compressed_tiff(
            path,
            width,
            height,
            tile_width,
            tile_height,
            33004,
            2,
            tiles,
        );
    }

    fn write_tiled_jp2k_ycbcr_tiff(
        path: &std::path::Path,
        width: u32,
        height: u32,
        tile_width: u32,
        tile_height: u32,
        tiles: &[Vec<u8>],
    ) {
        write_tiled_compressed_tiff(
            path,
            width,
            height,
            tile_width,
            tile_height,
            33005,
            6,
            tiles,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn write_tiled_compressed_tiff(
        path: &std::path::Path,
        width: u32,
        height: u32,
        tile_width: u32,
        tile_height: u32,
        compression: u16,
        photometric: u16,
        tiles: &[Vec<u8>],
    ) {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"II");
        buf.extend_from_slice(&42u16.to_le_bytes());
        let first_ifd_pos = buf.len();
        buf.extend_from_slice(&0u32.to_le_bytes());

        let mut tile_offsets = Vec::with_capacity(tiles.len());
        let mut tile_byte_counts = Vec::with_capacity(tiles.len());
        for tile in tiles {
            tile_offsets.push(buf.len() as u32);
            tile_byte_counts.push(tile.len() as u32);
            buf.extend_from_slice(tile);
        }

        let tile_offsets_array_offset = buf.len() as u32;
        for value in &tile_offsets {
            buf.extend_from_slice(&value.to_le_bytes());
        }
        let tile_byte_counts_array_offset = buf.len() as u32;
        for value in &tile_byte_counts {
            buf.extend_from_slice(&value.to_le_bytes());
        }
        let x_resolution_offset = buf.len() as u32;
        buf.extend_from_slice(&40_000u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        let y_resolution_offset = buf.len() as u32;
        buf.extend_from_slice(&40_000u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());

        let ifd_offset = buf.len() as u32;
        buf[first_ifd_pos..first_ifd_pos + 4].copy_from_slice(&ifd_offset.to_le_bytes());
        let mut tags = vec![
            tiff_tag(256, 4, 1, width.to_le_bytes()),
            tiff_tag(257, 4, 1, height.to_le_bytes()),
            tiff_tag(258, 3, 1, tiff_short_value(8)),
            tiff_tag(259, 3, 1, tiff_short_value(compression)),
            tiff_tag(262, 3, 1, tiff_short_value(photometric)),
            tiff_tag(277, 3, 1, tiff_short_value(3)),
            tiff_tag(282, 5, 1, x_resolution_offset.to_le_bytes()),
            tiff_tag(283, 5, 1, y_resolution_offset.to_le_bytes()),
            tiff_tag(296, 3, 1, tiff_short_value(3)),
            tiff_tag(322, 4, 1, tile_width.to_le_bytes()),
            tiff_tag(323, 4, 1, tile_height.to_le_bytes()),
            tiff_tag(
                324,
                4,
                tile_offsets.len() as u32,
                if tile_offsets.len() == 1 {
                    tile_offsets[0].to_le_bytes()
                } else {
                    tile_offsets_array_offset.to_le_bytes()
                },
            ),
            tiff_tag(
                325,
                4,
                tile_byte_counts.len() as u32,
                if tile_byte_counts.len() == 1 {
                    tile_byte_counts[0].to_le_bytes()
                } else {
                    tile_byte_counts_array_offset.to_le_bytes()
                },
            ),
        ];
        tags.sort_by_key(|tag| tag.0);

        buf.extend_from_slice(&(tags.len() as u16).to_le_bytes());
        for (tag, typ, count, value) in tags {
            buf.extend_from_slice(&tag.to_le_bytes());
            buf.extend_from_slice(&typ.to_le_bytes());
            buf.extend_from_slice(&count.to_le_bytes());
            buf.extend_from_slice(&value);
        }
        buf.extend_from_slice(&0u32.to_le_bytes());

        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(&buf).unwrap();
        file.flush().unwrap();
    }

    fn tiff_short_value(value: u16) -> [u8; 4] {
        let mut bytes = [0u8; 4];
        bytes[..2].copy_from_slice(&value.to_le_bytes());
        bytes
    }

    fn tiff_tag(tag: u16, typ: u16, count: u32, value: [u8; 4]) -> (u16, u16, u32, [u8; 4]) {
        (tag, typ, count, value)
    }
}
