use super::*;

mod auto_route;
mod cache;
mod dispatch;

#[cfg(test)]
pub(super) use auto_route::{
    cpu_input_device_encode_auto_allowed, cpu_input_device_encode_auto_probe_allowed,
    select_auto_lossless_j2k_probe_route, AutoLosslessJ2kRouteCandidate, CpuEncodedTileRun,
};
pub(super) use auto_route::{
    probe_auto_metal_input_tile_run, AutoMetalInputProbeRequest, RoutedLosslessJ2kTile,
};
pub(super) use cache::{MetalEncodedRowRunKey, MetalSourceTileCache, MetalSourceTileKey};
pub(super) use dispatch::{
    empty_metal_tile_run, metal_j2k_encode_batch_count, try_encode_metal_input_tile_run,
};

#[derive(Clone, Copy)]
pub(super) struct MetalInputTileRunRequest<'a> {
    pub(super) level: &'a wsi_rs::Level,
    pub(super) location: JpegBaselineFrameLocation,
    pub(super) row: u64,
    pub(super) start_col: u64,
    pub(super) tile_count: usize,
    pub(super) matrix_columns: u64,
    pub(super) matrix_rows: u64,
    pub(super) tile_size: u32,
}

impl MetalInputTileRunRequest<'_> {
    pub(super) fn row_run_key(self) -> MetalEncodedRowRunKey {
        MetalEncodedRowRunKey {
            scene: self.location.scene_idx,
            series: self.location.series_idx,
            level: self.location.level_idx,
            z: self.location.z,
            c: self.location.c,
            t: self.location.t,
            row: self.row,
            start_col: self.start_col,
            tile_count: self.tile_count,
            matrix_columns: self.matrix_columns,
            matrix_rows: self.matrix_rows,
            tile_size: self.tile_size,
        }
    }
}

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
        let mut encoded = Vec::new();
        encoded
            .try_reserve_exact(self.tile_profiles.len())
            .map_err(|_| Error::Unsupported {
                reason: "Metal encoded tile batch exceeds available memory".into(),
            })?;
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
pub(super) struct MetalInputTileReader {
    pub(super) preference: EncodeBackendPreference,
    pub(super) source_device_decode: bool,
    pub(super) auto_device_decode_allowed: bool,
    pub(super) auto_decision: AutoLosslessJ2kRouteDecision,
    pub(super) auto_cache_key: Option<AutoMetalInputRouteCacheKey>,
    pub(super) device: Option<crate::metal_interop::MetalDevice>,
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
            let device = j2k_metal_support::system_default_device().map_err(|source| {
                crate::metal_interop::support_error("Metal WSI input device", source)
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
