use super::*;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) struct MetalEncodedTileRun {
    pub(super) tiles: Vec<Option<(EncodedDicomJ2kFrame, PixelProfile)>>,
    pub(super) input_decode_duration: Duration,
    pub(super) compose_duration: Duration,
    pub(super) input_decode_batches: u64,
    pub(super) compose_batches: u64,
    pub(super) encode_batches: u64,
    pub(super) gpu_encode_stats: encode::DicomJ2kGpuEncodeBatchStats,
    pub(super) row_batch_rows: usize,
    pub(super) row_batch_target_tiles: Option<usize>,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) struct PendingMetalEncodedTileRun {
    pub(super) tile_profiles: Vec<Option<PixelProfile>>,
    pub(super) submission: encode::SubmittedDicomJ2kMetalTileBatch,
    pub(super) input_decode_duration: Duration,
    pub(super) compose_duration: Duration,
    pub(super) input_decode_batches: u64,
    pub(super) compose_batches: u64,
    pub(super) encode_batches: u64,
    pub(super) row_batch_rows: usize,
    pub(super) row_batch_target_tiles: Option<usize>,
    pub(super) preference: EncodeBackendPreference,
    pub(super) missing_encode_message: &'static str,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
impl PendingMetalEncodedTileRun {
    pub(super) fn wait(self) -> Result<MetalEncodedTileRun, Error> {
        let batch_encoded = self.submission.wait()?;
        let gpu_encode_stats = batch_encoded.gpu_encode_stats;
        let mut batch_encoded = batch_encoded.frames.into_iter();
        let mut encoded = Vec::with_capacity(self.tile_profiles.len());
        for profile in self.tile_profiles {
            let Some(profile) = profile else {
                encoded.push(None);
                continue;
            };
            let Some(encoded_frame) = batch_encoded.next() else {
                return Err(Error::Encode {
                    message: "Metal batch encode result count did not match input tile count"
                        .into(),
                });
            };
            match encoded_frame {
                Some(codestream) => encoded.push(Some((codestream, profile))),
                None if self.preference == EncodeBackendPreference::RequireDevice => {
                    return Err(Error::Unsupported {
                        reason: self.missing_encode_message.into(),
                    });
                }
                None => encoded.push(None),
            }
        }

        Ok(MetalEncodedTileRun {
            tiles: encoded,
            input_decode_duration: self.input_decode_duration,
            compose_duration: self.compose_duration,
            input_decode_batches: self.input_decode_batches,
            compose_batches: self.compose_batches,
            encode_batches: self.encode_batches,
            gpu_encode_stats,
            row_batch_rows: self.row_batch_rows,
            row_batch_target_tiles: self.row_batch_target_tiles,
        })
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) struct PendingMetalEncodedGridRun {
    pub(super) run: PendingMetalEncodedTileRun,
    pub(super) first_row_key: MetalEncodedRowRunKey,
    pub(super) tiles_per_row: usize,
    pub(super) row_count: usize,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) struct RoutedLosslessJ2kTile {
    pub(super) encoded: Result<EncodedDicomJ2kFrame, Error>,
    pub(super) profile: PixelProfile,
    pub(super) used_gpu_input: bool,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) struct CpuEncodedTileRun {
    pub(super) tiles: Vec<(Result<EncodedDicomJ2kFrame, Error>, PixelProfile)>,
    pub(super) input_decode_duration: Duration,
    pub(super) compose_duration: Duration,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) struct AutoMetalInputProbeRun {
    pub(super) tiles: Vec<Option<RoutedLosslessJ2kTile>>,
    pub(super) input_decode_duration: Duration,
    pub(super) compose_duration: Duration,
    pub(super) gpu_input_decode_batches: u64,
    pub(super) gpu_compose_batches: u64,
    pub(super) gpu_encode_batches: u64,
    pub(super) gpu_encode_stats: encode::DicomJ2kGpuEncodeBatchStats,
    pub(super) probe_cpu_duration: Duration,
    pub(super) probe_gpu_duration: Duration,
    pub(super) probe_gpu_batches: u64,
    pub(super) route: AutoLosslessJ2kRouteDecision,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) struct MetalInputTileReader {
    pub(super) preference: EncodeBackendPreference,
    pub(super) source_device_decode: bool,
    pub(super) auto_device_decode_allowed: bool,
    pub(super) auto_decision: AutoLosslessJ2kRouteDecision,
    pub(super) auto_cache_key: Option<AutoMetalInputRouteCacheKey>,
    pub(super) device: Option<metal::Device>,
    pub(super) sessions: Option<wsi_rs::output::metal::MetalBackendSessions>,
    pub(super) jpeg_encode_session: Option<j2k_jpeg_metal::MetalBackendSession>,
    pub(super) strip_composer: Option<MetalStripComposer>,
    pub(super) whole_level_cache: MetalSourceTileCache,
    pub(super) encoded_row_runs: HashMap<MetalEncodedRowRunKey, MetalEncodedTileRun>,
    pub(super) pending_encoded_grid_runs:
        HashMap<MetalEncodedRowRunKey, PendingMetalEncodedGridRun>,
    pub(super) next_grid_pipeline_row: Option<u64>,
    pub(super) private_jpeg_decode: bool,
    pub(super) row_batch_rows: Option<usize>,
    pub(super) row_batch_target_tiles: Option<usize>,
    pub(super) pipeline_depth: usize,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
impl MetalInputTileReader {
    pub(super) fn new(preference: EncodeBackendPreference, source_device_decode: bool) -> Self {
        Self::new_with_auto_device_decode(preference, false, source_device_decode)
    }

    pub(super) fn new_with_auto_device_decode(
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

    pub(super) fn new_for_lossless_j2k(
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
        if source_device_decode || auto_device_decode_allowed {
            reader.enable_private_jpeg_decode();
        }
        reader
    }

    pub(super) fn new_with_auto_device_decode_and_cache_key(
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
            encoded_row_runs: HashMap::new(),
            pending_encoded_grid_runs: HashMap::new(),
            next_grid_pipeline_row: None,
            private_jpeg_decode: false,
            row_batch_rows: None,
            row_batch_target_tiles: None,
            pipeline_depth: DEFAULT_GPU_PIPELINE_DEPTH,
        }
    }

    pub(super) fn enable_private_jpeg_decode(&mut self) {
        self.private_jpeg_decode = true;
    }

    pub(super) fn with_row_batch_tuning(
        mut self,
        row_batch_rows: Option<usize>,
        row_batch_target_tiles: Option<usize>,
    ) -> Self {
        self.row_batch_rows = row_batch_rows;
        self.row_batch_target_tiles = row_batch_target_tiles;
        self
    }

    pub(super) fn with_pipeline_depth(mut self, pipeline_depth: usize) -> Self {
        self.pipeline_depth = pipeline_depth.max(1);
        self
    }

    pub(super) fn enabled(&self) -> bool {
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

    pub(super) fn auto_input_probe_pending(&self) -> bool {
        self.preference == EncodeBackendPreference::Auto
            && self.auto_device_decode_allowed
            && self.auto_decision == AutoLosslessJ2kRouteDecision::Undecided
    }

    pub(super) fn auto_route_decision(&self) -> AutoLosslessJ2kRouteDecision {
        self.auto_decision
    }

    pub(super) fn record_auto_route_probe_decision(&mut self, route: AutoLosslessJ2kRouteDecision) {
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

    fn sessions(&mut self) -> Result<wsi_rs::output::metal::MetalBackendSessions, Error> {
        if self.sessions.is_none() {
            let device = metal::Device::system_default().ok_or_else(|| Error::Unsupported {
                reason: "Metal is unavailable for WSI input tile decode".into(),
            })?;
            self.device = Some(device.clone());
            self.sessions = Some(wsi_rs::output::metal::MetalBackendSessions::new(device));
        }
        self.sessions
            .as_ref()
            .cloned()
            .ok_or_else(|| Error::Unsupported {
                reason: "Metal input sessions were not initialized".into(),
            })
    }

    pub(super) fn source_tile_output_preference(&mut self) -> Result<TileOutputPreference, Error> {
        let sessions = self.sessions()?;
        let compressed_device_decode = self.source_device_decode || self.auto_device_decode_allowed;
        Ok(match (self.preference, compressed_device_decode) {
            (EncodeBackendPreference::RequireDevice, true) => {
                TileOutputPreference::require_device_auto_with_metal_and_compressed_decode(sessions)
            }
            (_, true) => {
                TileOutputPreference::prefer_device_auto_with_metal_and_compressed_decode(sessions)
            }
            _ => TileOutputPreference::prefer_device_auto_with_metal(sessions),
        })
    }

    pub(super) fn strip_composer(&mut self) -> Result<&MetalStripComposer, Error> {
        if self.strip_composer.is_none() {
            let _ = self.sessions()?;
            let device = self
                .device
                .as_ref()
                .cloned()
                .ok_or_else(|| Error::Unsupported {
                    reason: "Metal input device was not initialized".into(),
                })?;
            self.strip_composer = Some(MetalStripComposer::new(device)?);
        }
        self.strip_composer.as_ref().ok_or_else(|| Error::Encode {
            message: "Metal strip composer was not initialized".into(),
        })
    }

    pub(super) fn jpeg_encode_session(
        &mut self,
    ) -> Result<&j2k_jpeg_metal::MetalBackendSession, Error> {
        if self.jpeg_encode_session.is_none() {
            let _ = self.sessions()?;
            let device = self
                .device
                .as_ref()
                .cloned()
                .ok_or_else(|| Error::Unsupported {
                    reason: "Metal input device was not initialized".into(),
                })?;
            self.jpeg_encode_session = Some(j2k_jpeg_metal::MetalBackendSession::new(device));
        }
        self.jpeg_encode_session
            .as_ref()
            .ok_or_else(|| Error::Encode {
                message: "JPEG Baseline Metal encode session was not initialized".into(),
            })
    }
}

#[cfg(all(test, feature = "metal", target_os = "macos"))]
pub(super) fn wsi_rs_device_decode_opted_in() -> bool {
    env_flag_enabled(WSI_RS_JPEG_DEVICE_DECODE_ENV)
        || env_flag_enabled(WSI_RS_JP2K_DEVICE_DECODE_ENV)
}

#[cfg(all(test, feature = "metal", target_os = "macos"))]
pub(super) fn env_flag_enabled(name: &str) -> bool {
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
pub(super) struct MetalSourceTileKey {
    pub(super) scene: usize,
    pub(super) series: usize,
    pub(super) level: u32,
    pub(super) z: u32,
    pub(super) c: u32,
    pub(super) t: u32,
    pub(super) col: i64,
    pub(super) row: i64,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct MetalEncodedRowRunKey {
    pub(super) scene: usize,
    pub(super) series: usize,
    pub(super) level: u32,
    pub(super) z: u32,
    pub(super) c: u32,
    pub(super) t: u32,
    pub(super) row: u64,
    pub(super) start_col: u64,
    pub(super) tile_count: usize,
    pub(super) matrix_columns: u64,
    pub(super) matrix_rows: u64,
    pub(super) tile_size: u32,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) struct MetalSourceTileCache {
    pub(super) capacity: usize,
    pub(super) entries: HashMap<MetalSourceTileKey, MetalSourceTileCacheEntry>,
    pub(super) order: VecDeque<(MetalSourceTileKey, u64)>,
    pub(super) next_generation: u64,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
impl Default for MetalSourceTileCache {
    fn default() -> Self {
        Self {
            capacity: METAL_WHOLE_LEVEL_SOURCE_TILE_CACHE_CAPACITY,
            entries: HashMap::new(),
            order: VecDeque::new(),
            next_generation: 0,
        }
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) struct MetalSourceTileCacheEntry {
    pub(super) tile: wsi_rs::output::metal::MetalDeviceTile,
    pub(super) generation: u64,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
impl MetalSourceTileCache {
    pub(super) fn get(
        &mut self,
        key: MetalSourceTileKey,
    ) -> Option<wsi_rs::output::metal::MetalDeviceTile> {
        let tile = self.entries.get(&key)?.tile.clone();
        self.touch(key);
        Some(tile)
    }

    pub(super) fn insert(
        &mut self,
        key: MetalSourceTileKey,
        tile: wsi_rs::output::metal::MetalDeviceTile,
    ) {
        if self.capacity == 0 {
            return;
        }
        let generation = self.next_generation();
        self.entries
            .insert(key, MetalSourceTileCacheEntry { tile, generation });
        self.order.push_back((key, generation));
        while self.entries.len() > self.capacity {
            let Some((oldest, generation)) = self.order.pop_front() else {
                break;
            };
            if self
                .entries
                .get(&oldest)
                .is_some_and(|entry| entry.generation == generation)
            {
                self.entries.remove(&oldest);
            }
        }
        self.compact_stale_order_entries_if_needed();
    }

    fn touch(&mut self, key: MetalSourceTileKey) {
        let generation = self.next_generation();
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.generation = generation;
            self.order.push_back((key, generation));
            self.compact_stale_order_entries_if_needed();
        }
    }

    fn next_generation(&mut self) -> u64 {
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1);
        generation
    }

    fn compact_stale_order_entries_if_needed(&mut self) {
        let max_order_len = self.capacity.saturating_mul(4).max(1);
        if self.order.len() <= max_order_len {
            return;
        }
        self.order.retain(|(key, generation)| {
            self.entries
                .get(key)
                .is_some_and(|entry| entry.generation == *generation)
        });
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn try_encode_metal_input_tile_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    level: &wsi_rs::Level,
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
) -> Result<MetalEncodedTileRun, Error> {
    // Long NDPI exports create thousands of autoreleased Metal/ObjC temporaries.
    // Drain them per run so later rows do not encode zero-filled composed buffers.
    objc::rc::autoreleasepool(|| {
        let tile_count = usize::try_from(tile_count).map_err(|_| Error::Unsupported {
            reason: "tile batch size exceeds platform addressable memory".into(),
        })?;
        let row_run_key = MetalEncodedRowRunKey {
            scene: scene_idx,
            series: series_idx,
            level: level_idx,
            z,
            c,
            t,
            row,
            start_col,
            tile_count,
            matrix_columns,
            matrix_rows,
            tile_size,
        };

        if !metal_input.enabled() {
            return Ok(empty_metal_tile_run(tile_count));
        }
        if let Some(cached) = metal_input.encoded_row_runs.remove(&row_run_key) {
            return Ok(cached);
        }
        if level_is_synthetic_downsample(slide, scene_idx, series_idx, level_idx)? {
            return Ok(empty_metal_tile_run(tile_count));
        }

        if let Some(run) = metal_row_batch::try_encode_metal_input_tile_grid_pipeline_run(
            slide,
            metal_input,
            j2k_encoder,
            metal_row_batch::MetalTileGridRunRequest {
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
                first_row_key: row_run_key,
            },
        )? {
            return Ok(run);
        }

        if output_tile_maps_to_wsi_rs_tile(level, tile_size) {
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
            return Err(Error::Unsupported {
                reason:
                    "requested Metal input tile decode requires a DICOM tile grid that can be sourced from aligned wsi-rs tiles, regular tiled composition, or WholeLevel strip tiles"
                        .into(),
            });
        }
        Ok(empty_metal_tile_run(tile_count))
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn probe_auto_metal_input_tile_run(
    slide: &Slide,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    level: &wsi_rs::Level,
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
) -> Result<AutoMetalInputProbeRun, Error> {
    let first = planned.first().ok_or_else(|| Error::Unsupported {
        reason: "auto Metal input route probe requires at least one tile".into(),
    })?;
    let tile_count = u64::try_from(planned.len()).map_err(|_| Error::Unsupported {
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
            let partial_gpu_run = partial_gpu_run.ok_or_else(|| Error::Unsupported {
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
pub(super) fn encode_cpu_input_planned_tile_run(
    slide: &Slide,
    j2k_encoder: &mut DicomJ2kEncoder,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    planned: &[LosslessJ2kPlannedFrame],
    tile_size: u32,
) -> Result<CpuEncodedTileRun, Error> {
    let location = JpegBaselineFrameLocation {
        scene_idx,
        series_idx,
        level_idx,
        z,
        c,
        t,
    };
    let mut tiles = Vec::with_capacity(planned.len());
    let mut input_decode_duration = Duration::ZERO;
    let mut compose_duration = Duration::ZERO;
    for planned_frame in planned {
        let (encoded, profile, frame_input_decode_duration, frame_compose_duration) =
            encode_cpu_input_tile(
                slide,
                j2k_encoder,
                location,
                planned_frame.rect(),
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
pub(super) fn cpu_encoded_tile_run_total_duration(run: &CpuEncodedTileRun) -> Duration {
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
pub(super) fn cpu_input_device_encode_auto_allowed(run: &CpuEncodedTileRun) -> bool {
    run.tiles.iter().all(|(_, profile)| {
        matches!(profile.components, 1 | 3) && matches!(profile.bits_allocated, 8 | 16)
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn cpu_input_device_encode_auto_probe_allowed(
    run: &CpuEncodedTileRun,
    frame_count: usize,
) -> bool {
    frame_count >= LOSSLESS_J2K_AUTO_PARTIAL_GPU_MIN_FRAMES
        && cpu_input_device_encode_auto_allowed(run)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn metal_encoded_tile_run_total_duration(run: &MetalEncodedTileRun) -> Duration {
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
pub(super) struct AutoLosslessJ2kRouteCandidate {
    pub(super) complete: bool,
    pub(super) duration: Duration,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn select_auto_lossless_j2k_probe_route(
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
pub(super) fn route_beats_cpu_baseline(route_duration: Duration, cpu_duration: Duration) -> bool {
    route_duration
        .as_nanos()
        .saturating_mul(LOSSLESS_J2K_AUTO_ROUTE_SPEEDUP_DENOMINATOR)
        < cpu_duration
            .as_nanos()
            .saturating_mul(LOSSLESS_J2K_AUTO_ROUTE_SPEEDUP_NUMERATOR)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn empty_metal_tile_run(tile_count: usize) -> MetalEncodedTileRun {
    MetalEncodedTileRun {
        tiles: (0..tile_count).map(|_| None).collect(),
        input_decode_duration: Duration::ZERO,
        compose_duration: Duration::ZERO,
        input_decode_batches: 0,
        compose_batches: 0,
        encode_batches: 0,
        gpu_encode_stats: encode::DicomJ2kGpuEncodeBatchStats::default(),
        row_batch_rows: 0,
        row_batch_target_tiles: None,
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn metal_j2k_encode_batch_count(
    tiles: &[wsi_rs::output::metal::MetalDeviceTile],
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
