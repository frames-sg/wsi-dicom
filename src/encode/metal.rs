use super::*;
use j2k_core::DeviceSubmission;
use rayon::prelude::*;

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DicomJ2kGpuEncodeBatchStats {
    pub(crate) configured_inflight_tiles: Option<usize>,
    pub(crate) effective_inflight_tiles: usize,
    pub(crate) max_observed_inflight_tiles: usize,
    pub(crate) configured_memory_mib: Option<u64>,
    pub(crate) effective_memory_mib: u64,
    pub(crate) encode_wall_duration: Duration,
    pub(crate) stage_stats: j2k_metal::MetalLosslessEncodeStageStats,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
impl DicomJ2kGpuEncodeBatchStats {
    fn add_assign(&mut self, other: Self) {
        self.configured_inflight_tiles = self
            .configured_inflight_tiles
            .max(other.configured_inflight_tiles);
        self.effective_inflight_tiles = self
            .effective_inflight_tiles
            .max(other.effective_inflight_tiles);
        self.max_observed_inflight_tiles = self
            .max_observed_inflight_tiles
            .max(other.max_observed_inflight_tiles);
        self.configured_memory_mib = self.configured_memory_mib.max(other.configured_memory_mib);
        self.effective_memory_mib = self.effective_memory_mib.max(other.effective_memory_mib);
        self.encode_wall_duration = self
            .encode_wall_duration
            .saturating_add(other.encode_wall_duration);
        self.stage_stats.plan_duration = self
            .stage_stats
            .plan_duration
            .saturating_add(other.stage_stats.plan_duration);
        self.stage_stats.prepare_submit_duration = self
            .stage_stats
            .prepare_submit_duration
            .saturating_add(other.stage_stats.prepare_submit_duration);
        self.stage_stats.ht_table_build_duration = self
            .stage_stats
            .ht_table_build_duration
            .saturating_add(other.stage_stats.ht_table_build_duration);
        self.stage_stats.ht_buffer_allocation_duration = self
            .stage_stats
            .ht_buffer_allocation_duration
            .saturating_add(other.stage_stats.ht_buffer_allocation_duration);
        self.stage_stats.ht_command_encode_duration = self
            .stage_stats
            .ht_command_encode_duration
            .saturating_add(other.stage_stats.ht_command_encode_duration);
        self.stage_stats.codestream_wait_duration = self
            .stage_stats
            .codestream_wait_duration
            .saturating_add(other.stage_stats.codestream_wait_duration);
        self.stage_stats.chunk_count = self
            .stage_stats
            .chunk_count
            .saturating_add(other.stage_stats.chunk_count);
        self.stage_stats.tile_count = self
            .stage_stats
            .tile_count
            .saturating_add(other.stage_stats.tile_count);
        self.stage_stats.code_block_count = self
            .stage_stats
            .code_block_count
            .saturating_add(other.stage_stats.code_block_count);
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(crate) struct EncodedDicomJ2kMetalTileBatch {
    pub(crate) frames: Vec<Option<EncodedDicomJ2kFrame>>,
    pub(crate) gpu_encode_stats: DicomJ2kGpuEncodeBatchStats,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(crate) struct SubmittedDicomJ2kMetalTileBatch {
    tiles: Vec<wsi_rs::output::metal::MetalDeviceTile>,
    output_width: u32,
    output_height: u32,
    options: J2kLosslessEncodeOptions,
    session: Option<j2k_metal::MetalBackendSession>,
    preference: EncodeBackendPreference,
    used_device_validation: bool,
    configured_inflight_tiles: Option<usize>,
    configured_memory_mib: Option<u64>,
    groups: Vec<SubmittedDicomJ2kMetalTileGroup>,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
enum SubmittedDicomJ2kMetalTileGroup {
    Submitted {
        start: usize,
        end: usize,
        submission: Box<j2k_metal::SubmittedJ2kLosslessMetalBufferEncodeBatch>,
    },
    HostFallback {
        start: usize,
        end: usize,
    },
}

#[cfg(all(feature = "metal", target_os = "macos"))]
impl SubmittedDicomJ2kMetalTileBatch {
    pub(crate) fn wait(self) -> Result<EncodedDicomJ2kMetalTileBatch, Error> {
        let Self {
            tiles,
            output_width,
            output_height,
            options,
            session,
            preference,
            used_device_validation,
            configured_inflight_tiles,
            configured_memory_mib,
            groups,
        } = self;

        if preference == EncodeBackendPreference::CpuOnly {
            return Ok(EncodedDicomJ2kMetalTileBatch {
                frames: (0..tiles.len()).map(|_| None).collect(),
                gpu_encode_stats: DicomJ2kGpuEncodeBatchStats::default(),
            });
        }

        let session = session.ok_or_else(|| Error::Encode {
            message: "submitted JPEG 2000 Metal tile batch is missing its session".into(),
        })?;
        let mut encoded = Vec::with_capacity(tiles.len());
        let mut gpu_encode_stats = DicomJ2kGpuEncodeBatchStats::default();
        for group in groups {
            match group {
                SubmittedDicomJ2kMetalTileGroup::Submitted {
                    start,
                    end,
                    submission,
                } => match submission.wait() {
                    Ok(batch) => {
                        gpu_encode_stats.add_assign(dicom_gpu_encode_stats_from_metal(
                            batch.stats,
                            configured_memory_mib,
                        ));
                        for outcome in batch.outcomes {
                            encoded.push(Some(EncodedDicomJ2kFrame {
                                codestream: EncodedDicomJ2kCodestream::Metal(outcome.encoded),
                                used_device_encode: true,
                                used_device_validation,
                                encode_duration: outcome
                                    .encode_duration
                                    .saturating_add(outcome.input_copy_duration),
                                gpu_encode_wall_duration: None,
                                device_gpu_duration: outcome.gpu_duration,
                                validation_duration: outcome.validation_duration,
                            }));
                        }
                    }
                    Err(_) => {
                        let requests = metal_encode_requests_from_device_tiles(
                            &tiles[start..end],
                            output_width,
                            output_height,
                        )?;
                        encoded.extend(encode_metal_tiles_to_host_with_settings(
                            &requests,
                            &options,
                            &session,
                            preference,
                            used_device_validation,
                            configured_inflight_tiles,
                        )?);
                    }
                },
                SubmittedDicomJ2kMetalTileGroup::HostFallback { start, end } => {
                    let requests = metal_encode_requests_from_device_tiles(
                        &tiles[start..end],
                        output_width,
                        output_height,
                    )?;
                    encoded.extend(encode_metal_tiles_to_host_with_settings(
                        &requests,
                        &options,
                        &session,
                        preference,
                        used_device_validation,
                        configured_inflight_tiles,
                    )?);
                }
            }
        }
        Ok(EncodedDicomJ2kMetalTileBatch {
            frames: encoded,
            gpu_encode_stats,
        })
    }
}

impl DicomJ2kEncoder {
    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub(crate) fn encode_metal_tiles(
        &mut self,
        tiles: &[wsi_rs::output::metal::MetalDeviceTile],
        output_width: u32,
        output_height: u32,
    ) -> Result<EncodedDicomJ2kMetalTileBatch, Error> {
        self.submit_metal_tiles_owned(tiles.to_vec(), output_width, output_height)?
            .wait()
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub(crate) fn submit_metal_tiles_owned(
        &mut self,
        tiles: Vec<wsi_rs::output::metal::MetalDeviceTile>,
        output_width: u32,
        output_height: u32,
    ) -> Result<SubmittedDicomJ2kMetalTileBatch, Error> {
        if self.preference == EncodeBackendPreference::CpuOnly {
            return Ok(SubmittedDicomJ2kMetalTileBatch {
                tiles,
                output_width,
                output_height,
                options: lossless_encode_options(
                    self.transfer_syntax,
                    EncodeBackendPreference::PreferDevice,
                    self.codec_validation,
                    self.j2k_decomposition_levels,
                    self.reversible_transform,
                )?,
                session: None,
                preference: self.preference,
                used_device_validation: self.codec_validation == CodecValidation::RoundTrip,
                configured_inflight_tiles: self.gpu_encode_inflight_tiles,
                configured_memory_mib: self.gpu_encode_memory_mib,
                groups: Vec::new(),
            });
        }

        let session = self.ensure_metal_session()?.clone();
        let options = lossless_encode_options(
            self.transfer_syntax,
            EncodeBackendPreference::PreferDevice,
            self.codec_validation,
            self.j2k_decomposition_levels,
            self.reversible_transform,
        )?;

        let mut groups = Vec::new();
        let mut start = 0usize;
        while start < tiles.len() {
            let padded =
                metal_tile_is_padded_contiguous(&tiles[start], output_width, output_height);
            let mut end = start + 1;
            while end < tiles.len()
                && metal_tile_is_padded_contiguous(&tiles[end], output_width, output_height)
                    == padded
            {
                end += 1;
            }
            let requests = metal_encode_requests_from_device_tiles(
                &tiles[start..end],
                output_width,
                output_height,
            )?;
            let config = j2k_metal::MetalLosslessEncodeConfig {
                gpu_encode_inflight_tiles: self.gpu_encode_inflight_tiles,
                gpu_encode_memory_budget_bytes: self
                    .gpu_encode_memory_mib
                    .and_then(|mib| usize::try_from(mib).ok())
                    .and_then(|mib| mib.checked_mul(1024 * 1024)),
            };
            let staging = if padded {
                j2k_metal::MetalEncodeInputStaging::AlreadyPaddedContiguous
            } else {
                j2k_metal::MetalEncodeInputStaging::CopyAndPad
            };
            let request = j2k_metal::MetalLosslessEncodeBatchRequest {
                tiles: &requests,
                staging,
                config,
            };
            match j2k_metal::submit_lossless_batch_to_metal(request, &options, &session) {
                Ok(submission) => groups.push(SubmittedDicomJ2kMetalTileGroup::Submitted {
                    start,
                    end,
                    submission: Box::new(submission),
                }),
                Err(err) if self.preference == EncodeBackendPreference::RequireDevice => {
                    return Err(Error::Encode {
                        message: format!("JPEG 2000 Metal tile batch submit failed: {err}"),
                    });
                }
                Err(_) => groups.push(SubmittedDicomJ2kMetalTileGroup::HostFallback { start, end }),
            }
            start = end;
        }

        Ok(SubmittedDicomJ2kMetalTileBatch {
            tiles,
            output_width,
            output_height,
            options,
            session: Some(session),
            preference: self.preference,
            used_device_validation: self.codec_validation == CodecValidation::RoundTrip,
            configured_inflight_tiles: self.gpu_encode_inflight_tiles,
            configured_memory_mib: self.gpu_encode_memory_mib,
            groups,
        })
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub(super) fn ensure_metal_session(
        &mut self,
    ) -> Result<&j2k_metal::MetalBackendSession, Error> {
        if self.metal_session.is_none() {
            self.metal_session = Some(j2k_metal::MetalBackendSession::system_default().map_err(
                |err| Error::Encode {
                    message: format!("JPEG 2000 Metal session unavailable: {err}"),
                },
            )?);
        }
        self.metal_session.as_ref().ok_or_else(|| Error::Encode {
            message: "JPEG 2000 Metal session was not initialized".into(),
        })
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn metal_encode_requests_from_device_tiles(
    tiles: &[wsi_rs::output::metal::MetalDeviceTile],
    output_width: u32,
    output_height: u32,
) -> Result<Vec<j2k_metal::MetalLosslessEncodeTile<'_>>, Error> {
    let mut requests = Vec::with_capacity(tiles.len());
    for tile in tiles {
        let image = crate::metal_interop::device_tile_image(tile)?;
        requests.push(j2k_metal::MetalLosslessEncodeTile::from_resident(
            image,
            (output_width, output_height),
        ));
    }
    Ok(requests)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn encode_metal_tiles_to_host_with_settings(
    requests: &[j2k_metal::MetalLosslessEncodeTile<'_>],
    options: &J2kLosslessEncodeOptions,
    session: &j2k_metal::MetalBackendSession,
    preference: EncodeBackendPreference,
    used_device_validation: bool,
    configured_inflight_tiles: Option<usize>,
) -> Result<Vec<Option<EncodedDicomJ2kFrame>>, Error> {
    let chunk_size = metal_host_fallback_parallel_chunk_size(
        requests.len(),
        configured_inflight_tiles,
        rayon::current_num_threads(),
    );

    let chunks = requests
        .par_chunks(chunk_size)
        .map(|chunk| {
            encode_metal_tile_chunk_to_host(
                chunk,
                options,
                session,
                preference,
                used_device_validation,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(chunks.into_iter().flatten().collect())
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn dicom_gpu_encode_stats_from_metal(
    stats: j2k_metal::MetalLosslessEncodeBatchStats,
    configured_memory_mib: Option<u64>,
) -> DicomJ2kGpuEncodeBatchStats {
    DicomJ2kGpuEncodeBatchStats {
        configured_inflight_tiles: stats.configured_inflight_tiles,
        effective_inflight_tiles: stats.effective_inflight_tiles,
        max_observed_inflight_tiles: stats.max_observed_inflight_tiles,
        configured_memory_mib,
        effective_memory_mib: bytes_to_mib_ceil(stats.effective_memory_budget_bytes),
        encode_wall_duration: stats.encode_wall_duration,
        stage_stats: stats.stage_stats,
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn metal_host_fallback_parallel_chunk_size(
    request_count: usize,
    configured_inflight_tiles: Option<usize>,
    worker_threads: usize,
) -> usize {
    if request_count == 0 {
        return 1;
    }

    let parallel_chunks = configured_inflight_tiles
        .unwrap_or(worker_threads)
        .max(1)
        .min(request_count);
    request_count.div_ceil(parallel_chunks)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn encode_metal_tile_chunk_to_host(
    requests: &[j2k_metal::MetalLosslessEncodeTile<'_>],
    options: &J2kLosslessEncodeOptions,
    session: &j2k_metal::MetalBackendSession,
    preference: EncodeBackendPreference,
    used_device_validation: bool,
) -> Result<Vec<Option<EncodedDicomJ2kFrame>>, Error> {
    let request = j2k_metal::MetalLosslessEncodeBatchRequest {
        tiles: requests,
        staging: j2k_metal::MetalEncodeInputStaging::CopyAndPad,
        config: j2k_metal::MetalLosslessEncodeConfig::default(),
    };
    let outcomes = match j2k_metal::encode_lossless_batch_with_report(request, options, session) {
        Ok(outcomes) => outcomes,
        Err(_) if preference != EncodeBackendPreference::RequireDevice => {
            return Ok((0..requests.len()).map(|_| None).collect());
        }
        Err(err) => {
            return Err(Error::Encode {
                message: format!("JPEG 2000 Metal tile batch encode failed: {err}"),
            });
        }
    };

    outcomes
        .into_iter()
        .map(|outcome| {
            let used_device_encode = outcome.encoded.backend == BackendKind::Metal;
            if !used_device_encode {
                if preference == EncodeBackendPreference::RequireDevice {
                    return Err(Error::Unsupported {
                        reason:
                            "requested JPEG 2000 device encode backend did not preserve the requested profile"
                                .into(),
                    });
                }
                return Ok(None);
            }

            Ok(Some(EncodedDicomJ2kFrame {
                codestream: EncodedDicomJ2kCodestream::Host(outcome.encoded.codestream),
                used_device_encode,
                used_device_validation,
                encode_duration: outcome
                    .encode_duration
                    .saturating_add(outcome.input_copy_duration),
                gpu_encode_wall_duration: Some(
                    outcome
                        .encode_duration
                        .saturating_add(outcome.input_copy_duration),
                ),
                device_gpu_duration: outcome.gpu_duration,
                validation_duration: outcome.validation_duration,
            }))
        })
        .collect()
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn bytes_to_mib_ceil(bytes: usize) -> u64 {
    let mib = 1024usize * 1024;
    bytes.div_ceil(mib) as u64
}

#[cfg(test)]
mod tests;
