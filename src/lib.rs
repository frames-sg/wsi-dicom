#![forbid(unsafe_code)]

#[cfg(all(feature = "metal", target_os = "macos"))]
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use dicom_object::FileMetaTableBuilder;
#[cfg(all(feature = "metal", target_os = "macos"))]
use signinum_core::PixelFormat as SigninumPixelFormat;
use signinum_j2k::J2kLosslessSamples;
#[cfg(all(feature = "metal", target_os = "macos"))]
use statumen::{DeviceTile, TileLayout, TileOutputPreference, TilePixels, TileRequest};
use statumen::{LevelIdx, PlaneIdx, PlaneSelection, RegionRequest, SceneId, SeriesId, Slide};

mod encode;
mod error;
mod metadata;
mod options;
mod tile;
mod uid;
mod writer;

pub use error::WsiDicomError;
pub use metadata::{DicomMetadata, MetadataSource};
pub use options::{DicomExportOptions, EncodeBackendPreference, TransferSyntax};

use encode::{DicomJ2kEncoder, EncodedDicomJ2kFrame};
#[cfg(all(feature = "metal", target_os = "macos"))]
use tile::pixel_profile_from_device_format;
use tile::{optical_path_groups, prepare_tile_samples, PixelProfile};
use uid::{deterministic_instance_path, uid_from_seed};
use writer::{build_dicom_object, write_dicom_object_with_spooled_pixel_data, PixelDataSpool};

pub(crate) const VL_WSI_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.1.77.1.6";

/// A validated request to export one vendor WSI into one DICOM output directory.
#[derive(Debug, Clone, PartialEq)]
pub struct DicomExportRequest {
    pub source_path: PathBuf,
    pub output_dir: PathBuf,
    pub options: DicomExportOptions,
    pub metadata: MetadataSource,
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
            metadata: MetadataSource::Strict(DicomMetadata::default()),
        })
    }

    pub fn validate(&self) -> Result<(), WsiDicomError> {
        self.options.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DicomExportReport {
    pub output_dir: PathBuf,
    pub instances: Vec<DicomInstanceReport>,
    pub metrics: DicomExportMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub input_decode_micros: u128,
    pub compose_micros: u128,
    pub encode_micros: u128,
    pub validation_micros: u128,
    pub write_micros: u128,
}

impl DicomExportMetrics {
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
        self.input_decode_micros = self
            .input_decode_micros
            .saturating_add(other.input_decode_micros);
        self.compose_micros = self.compose_micros.saturating_add(other.compose_micros);
        self.encode_micros = self.encode_micros.saturating_add(other.encode_micros);
        self.validation_micros = self
            .validation_micros
            .saturating_add(other.validation_micros);
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

    fn record_encoded_frame(&mut self, encoded: &encode::EncodedDicomJ2kFrame) {
        if encoded.used_device_encode {
            self.gpu_encode_frames = self.gpu_encode_frames.saturating_add(1);
        }
        if encoded.used_device_validation {
            self.gpu_validation_frames = self.gpu_validation_frames.saturating_add(1);
        }
        self.record_encode_duration(encoded.encode_duration);
        self.record_validation_duration(encoded.validation_duration);
    }

    fn record_input_decode_duration(&mut self, duration: Duration) {
        self.input_decode_micros = self
            .input_decode_micros
            .saturating_add(duration_as_reported_micros(duration));
    }

    fn record_compose_duration(&mut self, duration: Duration) {
        self.compose_micros = self
            .compose_micros
            .saturating_add(duration_as_reported_micros(duration));
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

    fn record_write_duration(&mut self, duration: Duration) {
        self.write_micros = self
            .write_micros
            .saturating_add(duration_as_reported_micros(duration));
    }
}

fn duration_as_reported_micros(duration: Duration) -> u128 {
    match duration.as_micros() {
        0 if duration > Duration::ZERO => 1,
        micros => micros,
    }
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

/// Export a statumen-readable WSI into DICOM VL Whole Slide Microscopy files.
pub fn export_dicom(request: DicomExportRequest) -> Result<DicomExportReport, WsiDicomError> {
    request.validate()?;
    if !request.options.transfer_syntax.is_lossless_j2k_family() {
        return Err(WsiDicomError::Unsupported {
            reason: "only JPEG 2000 Lossless and HTJ2K Lossless transfer syntaxes are implemented"
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
                for z in 0..series.axes.z {
                    for t in 0..series.axes.t {
                        let channel_groups = optical_path_groups(series.axes.c);
                        for c in channel_groups {
                            let instance_number = instances.len() as u32 + 1;
                            let report = export_instance(
                                &slide,
                                &request,
                                &metadata,
                                &study_uid,
                                instance_number,
                                scene_idx,
                                series_idx,
                                level_idx as u32,
                                z,
                                c,
                                t,
                                level,
                            )?;
                            metrics.add_assign(report.metrics);
                            instances.push(report);
                        }
                    }
                }
            }
        }
    }

    Ok(DicomExportReport {
        output_dir: request.output_dir,
        instances,
        metrics,
    })
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
    let tile_size = request.options.tile_size;
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
    let pixel_spacing_mm = level_pixel_spacing_mm(slide, level);

    let path = deterministic_instance_path(&request.output_dir, level_idx, z, c, t);
    let spool_path = path.with_extension("pixeldata.tmp");
    let mut pixel_spool = PixelDataSpool::create(spool_path, frame_count as usize)?;
    let mut pixel_profile = None;
    let mut j2k_encoder = DicomJ2kEncoder::new(
        request.options.encode_backend,
        request.options.transfer_syntax,
    );
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let mut metal_input = MetalInputTileReader::new(request.options.encode_backend);
    let mut metrics = DicomExportMetrics::default();

    for row in 0..tiles_down {
        #[cfg(all(feature = "metal", target_os = "macos"))]
        let mut metal_row = try_encode_metal_input_tile_run(
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
            0,
            tiles_across,
            matrix_columns,
            matrix_rows,
            tile_size,
        )?;
        #[cfg(all(feature = "metal", target_os = "macos"))]
        {
            metrics.record_input_decode_duration(metal_row.input_decode_duration);
            metrics.record_compose_duration(metal_row.compose_duration);
        }

        for col in 0..tiles_across {
            let x = col * u64::from(tile_size);
            let y = row * u64::from(tile_size);
            let width = (matrix_columns - x).min(u64::from(tile_size)) as u32;
            let height = (matrix_rows - y).min(u64::from(tile_size)) as u32;

            #[cfg(all(feature = "metal", target_os = "macos"))]
            let metal_encoded = metal_row.tiles[col as usize].take();
            #[cfg(not(all(feature = "metal", target_os = "macos")))]
            let metal_encoded: Option<(EncodedDicomJ2kFrame, PixelProfile)> = None;

            let (encoded, profile, used_gpu_input) = match metal_encoded {
                Some((encoded, profile)) => (Ok(encoded), profile, true),
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
                            col,
                            x,
                            y,
                            width,
                            height,
                            tile_size,
                        )?;
                    metrics.record_input_decode_duration(input_decode_duration);
                    metrics.record_compose_duration(compose_duration);
                    (encoded, profile, false)
                }
            };
            if used_gpu_input {
                metrics.record_gpu_input();
            } else {
                metrics.record_cpu_input();
            }

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
                    col,
                    message,
                },
                other => other,
            })?;
            metrics.record_encoded_frame(&encoded);
            pixel_spool.push_frame(&encoded.codestream)?;
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
        matrix_columns,
        matrix_rows,
        frame_count,
        profile,
        pixel_spacing_mm,
        pixel_spool.offsets(),
        pixel_spool.lengths(),
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
    let samples = J2kLosslessSamples::new(
        &prepared.bytes,
        tile_size,
        tile_size,
        prepared.profile.components,
        prepared.profile.bits_allocated as u8,
        false,
    )
    .map_err(|source| WsiDicomError::Encode {
        message: source.to_string(),
    })?;
    Ok((
        j2k_encoder.encode(samples),
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
}

#[cfg(all(feature = "metal", target_os = "macos"))]
struct MetalInputTileReader {
    preference: EncodeBackendPreference,
    device: Option<metal::Device>,
    sessions: Option<statumen::output::metal::MetalBackendSessions>,
    strip_composer: Option<MetalStripComposer>,
    whole_level_cache: MetalSourceTileCache,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
impl MetalInputTileReader {
    fn new(preference: EncodeBackendPreference) -> Self {
        Self {
            preference,
            device: None,
            sessions: None,
            strip_composer: None,
            whole_level_cache: MetalSourceTileCache::default(),
        }
    }

    fn enabled(&self) -> bool {
        self.preference != EncodeBackendPreference::CpuOnly
    }

    fn sessions(&mut self) -> Result<statumen::output::metal::MetalBackendSessions, WsiDicomError> {
        if self.sessions.is_none() {
            let device =
                metal::Device::system_default().ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "Metal is unavailable for WSI input tile decode".into(),
                })?;
            self.device = Some(device.clone());
            self.sessions = Some(statumen::output::metal::MetalBackendSessions::new(
                signinum_jpeg_metal::MetalBackendSession::new(device.clone()),
                signinum_j2k_metal::MetalBackendSession::new(device),
            ));
        }
        Ok(self
            .sessions
            .as_ref()
            .expect("Metal input sessions initialized")
            .clone())
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

    #[allow(clippy::too_many_arguments)]
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
        let first_col =
            u32::try_from(packed.first_col).map_err(|_| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel first source tile column exceeds u32".into(),
            })?;
        let first_row =
            u32::try_from(packed.first_row).map_err(|_| WsiDicomError::Unsupported {
                reason: "Metal WholeLevel first source tile row exceeds u32".into(),
            })?;
        let bytes_per_pixel = packed.format.bytes_per_pixel();
        let dst_stride = (output_width as usize)
            .checked_mul(bytes_per_pixel)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "Metal composed tile stride overflow".into(),
            })?;
        let dst_bytes = dst_stride
            .checked_mul(output_height as usize)
            .ok_or_else(|| WsiDicomError::Unsupported {
                reason: "Metal composed tile byte length overflow".into(),
            })?;
        let dst_bytes_u64 = u64::try_from(dst_bytes).map_err(|_| WsiDicomError::Unsupported {
            reason: "Metal composed tile byte length exceeds u64".into(),
        })?;
        let dst_buffer = self
            .device
            .new_buffer(dst_bytes_u64, metal::MTLResourceOptions::StorageModeShared);
        let params = MetalComposeStripsParams {
            src_origin_x,
            src_origin_y,
            valid_width,
            valid_height,
            output_width,
            output_height,
            bytes_per_pixel: u32::try_from(bytes_per_pixel).map_err(|_| {
                WsiDicomError::Unsupported {
                    reason: "Metal composed tile bytes-per-pixel exceeds u32".into(),
                }
            })?,
            src_tile_width: packed.tile_width,
            src_tile_height: packed.tile_height,
            src_slot_stride: u32::try_from(packed.slot_stride).map_err(|_| {
                WsiDicomError::Unsupported {
                    reason: "Metal WholeLevel source slot stride exceeds u32".into(),
                }
            })?,
            src_tile_slot_bytes: u32::try_from(packed.tile_slot_bytes).map_err(|_| {
                WsiDicomError::Unsupported {
                    reason: "Metal WholeLevel source tile slot byte length exceeds u32".into(),
                }
            })?,
            src_first_col: first_col,
            src_first_row: first_row,
            src_tiles_across: packed.tiles_across,
            dst_stride: u32::try_from(dst_stride).map_err(|_| WsiDicomError::Unsupported {
                reason: "Metal composed tile pitch exceeds u32".into(),
            })?,
        };

        let command_buffer = self.queue.new_command_buffer();
        let encoder = command_buffer.new_compute_command_encoder();
        encoder.set_compute_pipeline_state(&self.pipeline);
        encoder.set_buffer(0, Some(&packed.buffer), 0);
        encoder.set_buffer(1, Some(&dst_buffer), 0);
        encoder.set_bytes(
            2,
            core::mem::size_of::<MetalComposeStripsParams>() as u64,
            (&raw const params).cast(),
        );
        let width = self.pipeline.thread_execution_width().max(1);
        let max_threads = self.pipeline.max_total_threads_per_threadgroup().max(width);
        let height = (max_threads / width).max(1);
        encoder.dispatch_threads(
            metal::MTLSize {
                width: u64::from(output_width),
                height: u64::from(output_height),
                depth: 1,
            },
            metal::MTLSize {
                width,
                height,
                depth: 1,
            },
        );
        encoder.end_encoding();
        command_buffer.commit();
        command_buffer.wait_until_completed();

        Ok(statumen::output::metal::MetalDeviceTile {
            width: output_width,
            height: output_height,
            pitch_bytes: dst_stride,
            format: packed.format,
            storage: statumen::output::metal::MetalDeviceStorage::Buffer {
                buffer: dst_buffer,
                byte_offset: 0,
            },
        })
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
                    "requested Metal input tile decode requires a DICOM tile grid that can be sourced from aligned statumen tiles or WholeLevel strip tiles"
                        .into(),
            });
        }
        Ok(empty_metal_tile_run(tile_count))
    })
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
    let pixels = match slide.read_tiles(
        &requests,
        TileOutputPreference::prefer_device_auto_with_metal(metal_input.sessions()?),
    ) {
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
    let mut batch_encoded = j2k_encoder
        .encode_metal_tiles(&batch_tiles, tile_size, tile_size)?
        .into_iter();
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
            TileOutputPreference::prefer_device_auto_with_metal(metal_input.sessions()?),
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
    let mut composed_tiles = Vec::with_capacity(tile_count);
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
        let src_origin_y = u32::try_from(y).map_err(|_| WsiDicomError::Unsupported {
            reason: "Metal WholeLevel tile source y offset exceeds u32".into(),
        })?;
        let composed = composer.compose_tile(
            &packed,
            src_origin_x,
            src_origin_y,
            valid_width,
            valid_height,
            tile_size,
            tile_size,
        )?;
        composed_tiles.push(composed);
    }
    let compose_duration = compose_started.elapsed();

    let mut encoded = Vec::with_capacity(tile_count);
    for frame in j2k_encoder.encode_metal_tiles(&composed_tiles, tile_size, tile_size)? {
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
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn empty_metal_tile_run(tile_count: usize) -> MetalEncodedTileRun {
    MetalEncodedTileRun {
        tiles: (0..tile_count).map(|_| None).collect(),
        input_decode_duration: Duration::ZERO,
        compose_duration: Duration::ZERO,
    }
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
    matches!(
        level.tile_layout,
        TileLayout::Regular {
            tile_width,
            tile_height,
            ..
        } if tile_width == tile_size && tile_height == tile_size
    ) || matches!(
        level.tile_layout,
        TileLayout::WholeLevel {
            virtual_tile_width,
            virtual_tile_height,
            ..
        } if virtual_tile_width == tile_size && virtual_tile_height == tile_size
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::encode::{
        dicom_j2k_decomposition_levels, encode_dicom_j2k_lossless, encode_dicom_lossless,
    };
    use dicom_core::{DataElement, PrimitiveValue, VR};
    use dicom_dictionary_std::{tags, uids};
    use dicom_object::{FileMetaTableBuilder, InMemDicomObject};

    #[test]
    fn default_options_use_jpeg2000_lossless_and_auto_backend() {
        let options = DicomExportOptions::default();

        assert_eq!(options.tile_size, 512);
        assert_eq!(options.transfer_syntax.uid(), "1.2.840.10008.1.2.4.90");
        assert_eq!(options.encode_backend, EncodeBackendPreference::Auto);
    }

    #[test]
    fn transfer_syntax_uids_include_htj2k_lossless_profiles() {
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
                encode_backend: EncodeBackendPreference::PreferDevice,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
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
            },
            metadata: MetadataSource::ResearchPlaceholder,
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
                )
                .unwrap();
                assert_transfer_syntax_codestream(transfer_syntax, &codestream);
                assert_j2k_facade_roundtrip(samples, &codestream);
            }
        }
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

        let mut metal_input = MetalInputTileReader::new(EncodeBackendPreference::RequireDevice);
        let mut encoder = DicomJ2kEncoder::new(
            EncodeBackendPreference::RequireDevice,
            TransferSyntax::Jpeg2000Lossless,
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
                &frame.0.codestream,
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
            TransferSyntax::JpegBaseline8Bit | TransferSyntax::ExplicitVrLittleEndian => {
                panic!("non-JPEG 2000 transfer syntax in lossless J2K fixture test");
            }
        }
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
        let mut metal_input = MetalInputTileReader::new(EncodeBackendPreference::RequireDevice);
        let mut j2k_encoder = DicomJ2kEncoder::new(
            EncodeBackendPreference::RequireDevice,
            TransferSyntax::Htj2kLossless,
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
            &encoded.codestream,
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
}
