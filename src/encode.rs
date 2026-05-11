use std::time::{Duration, Instant};

use signinum_j2k::{
    encode_j2k_lossless, encode_j2k_lossless_with_accelerator, BackendKind, J2kBlockCodingMode,
    J2kEncodeStageAccelerator, J2kEncodeValidation, J2kLosslessEncodeOptions, J2kLosslessSamples,
    J2kProgressionOrder, ReversibleTransform,
};

use crate::{CodecValidation, EncodeBackendPreference, TransferSyntax, WsiDicomError};

pub(crate) struct DicomJ2kEncoder {
    preference: EncodeBackendPreference,
    transfer_syntax: TransferSyntax,
    codec_validation: CodecValidation,
    j2k_decomposition_levels: Option<u8>,
    reversible_transform: ReversibleTransform,
    gpu_encode_inflight_tiles: Option<usize>,
    gpu_encode_memory_mib: Option<u64>,
    #[cfg(all(feature = "metal", target_os = "macos"))]
    metal: Option<signinum_j2k_metal::MetalEncodeStageAccelerator>,
    #[cfg(all(feature = "metal", target_os = "macos"))]
    metal_session: Option<signinum_j2k_metal::MetalBackendSession>,
    #[cfg(feature = "cuda")]
    cuda: Option<signinum_j2k_cuda::CudaEncodeStageAccelerator>,
}

pub(crate) struct EncodedDicomJ2kFrame {
    codestream: EncodedDicomJ2kCodestream,
    pub(crate) used_device_encode: bool,
    pub(crate) used_device_validation: bool,
    pub(crate) encode_duration: Duration,
    pub(crate) device_gpu_duration: Option<Duration>,
    pub(crate) validation_duration: Duration,
}

pub(crate) enum EncodedDicomJ2kCodestream {
    Host(Vec<u8>),
    #[cfg(all(feature = "metal", target_os = "macos"))]
    Metal(signinum_j2k_metal::MetalEncodedJ2k),
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DicomJ2kGpuEncodeBatchStats {
    pub(crate) configured_inflight_tiles: Option<usize>,
    pub(crate) effective_inflight_tiles: usize,
    pub(crate) max_observed_inflight_tiles: usize,
    pub(crate) configured_memory_mib: Option<u64>,
    pub(crate) effective_memory_mib: u64,
    pub(crate) encode_wall_duration: Duration,
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
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(crate) struct EncodedDicomJ2kMetalTileBatch {
    pub(crate) frames: Vec<Option<EncodedDicomJ2kFrame>>,
    pub(crate) gpu_encode_stats: DicomJ2kGpuEncodeBatchStats,
}

impl EncodedDicomJ2kFrame {
    pub(crate) fn codestream_bytes(&self) -> Result<&[u8], WsiDicomError> {
        match &self.codestream {
            EncodedDicomJ2kCodestream::Host(bytes) => Ok(bytes),
            #[cfg(all(feature = "metal", target_os = "macos"))]
            EncodedDicomJ2kCodestream::Metal(encoded) => {
                encoded
                    .codestream_bytes()
                    .map_err(|err| WsiDicomError::Encode {
                        message: format!("JPEG 2000 Metal encoded buffer read failed: {err}"),
                    })
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn into_codestream(self) -> Result<Vec<u8>, WsiDicomError> {
        match self.codestream {
            EncodedDicomJ2kCodestream::Host(bytes) => Ok(bytes),
            #[cfg(all(feature = "metal", target_os = "macos"))]
            EncodedDicomJ2kCodestream::Metal(encoded) => Ok(encoded
                .codestream_bytes()
                .map_err(|err| WsiDicomError::Encode {
                    message: format!("JPEG 2000 Metal encoded buffer read failed: {err}"),
                })?
                .to_vec()),
        }
    }

    #[cfg(all(test, feature = "metal", target_os = "macos"))]
    pub(crate) fn codestream_is_metal_buffer_backed(&self) -> bool {
        matches!(self.codestream, EncodedDicomJ2kCodestream::Metal(_))
    }
}

impl DicomJ2kEncoder {
    pub(crate) fn new(
        preference: EncodeBackendPreference,
        transfer_syntax: TransferSyntax,
        codec_validation: CodecValidation,
    ) -> Self {
        Self {
            preference,
            transfer_syntax,
            codec_validation,
            j2k_decomposition_levels: None,
            reversible_transform: ReversibleTransform::Rct53,
            gpu_encode_inflight_tiles: None,
            gpu_encode_memory_mib: None,
            #[cfg(all(feature = "metal", target_os = "macos"))]
            metal: None,
            #[cfg(all(feature = "metal", target_os = "macos"))]
            metal_session: None,
            #[cfg(feature = "cuda")]
            cuda: None,
        }
    }

    pub(crate) fn with_gpu_encode_tuning(
        mut self,
        gpu_encode_inflight_tiles: Option<usize>,
        gpu_encode_memory_mib: Option<u64>,
    ) -> Self {
        self.gpu_encode_inflight_tiles = gpu_encode_inflight_tiles;
        self.gpu_encode_memory_mib = gpu_encode_memory_mib;
        self
    }

    pub(crate) fn with_j2k_decomposition_levels(
        mut self,
        j2k_decomposition_levels: Option<u8>,
    ) -> Self {
        self.j2k_decomposition_levels = j2k_decomposition_levels;
        self
    }

    pub(crate) fn set_reversible_transform(&mut self, reversible_transform: ReversibleTransform) {
        self.reversible_transform = reversible_transform;
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub(crate) fn cpu_only_peer(&self) -> Self {
        self.peer_with_preference(EncodeBackendPreference::CpuOnly)
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub(crate) fn require_device_peer(&self) -> Self {
        self.peer_with_preference(EncodeBackendPreference::RequireDevice)
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    fn peer_with_preference(&self, preference: EncodeBackendPreference) -> Self {
        Self::new(preference, self.transfer_syntax, self.codec_validation)
            .with_j2k_decomposition_levels(self.j2k_decomposition_levels)
            .with_reversible_transform(self.reversible_transform)
            .with_gpu_encode_tuning(self.gpu_encode_inflight_tiles, self.gpu_encode_memory_mib)
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    fn with_reversible_transform(mut self, reversible_transform: ReversibleTransform) -> Self {
        self.reversible_transform = reversible_transform;
        self
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub(crate) fn force_cpu_only_for_auto(&mut self) {
        if self.preference == EncodeBackendPreference::Auto {
            self.preference = EncodeBackendPreference::CpuOnly;
        }
    }

    #[cfg(all(test, feature = "metal", target_os = "macos"))]
    pub(crate) fn preference(&self) -> EncodeBackendPreference {
        self.preference
    }

    pub(crate) fn encode(
        &mut self,
        samples: J2kLosslessSamples<'_>,
    ) -> Result<EncodedDicomJ2kFrame, WsiDicomError> {
        if self.preference == EncodeBackendPreference::CpuOnly {
            return encode_lossless_cpu(
                samples,
                self.transfer_syntax,
                self.codec_validation,
                self.j2k_decomposition_levels,
                self.reversible_transform,
            );
        }

        match self.try_device(samples)? {
            Some(codestream) => Ok(codestream),
            None if self.preference == EncodeBackendPreference::RequireDevice => {
                Err(WsiDicomError::Unsupported {
                    reason:
                        "requested JPEG 2000 device encode backend is unavailable or unsupported"
                            .into(),
                })
            }
            None => encode_lossless_cpu(
                samples,
                self.transfer_syntax,
                self.codec_validation,
                self.j2k_decomposition_levels,
                self.reversible_transform,
            ),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn cpu_batch_settings(
        &self,
    ) -> Option<(
        TransferSyntax,
        CodecValidation,
        Option<u8>,
        ReversibleTransform,
    )> {
        (self.preference == EncodeBackendPreference::CpuOnly).then_some((
            self.transfer_syntax,
            self.codec_validation,
            self.j2k_decomposition_levels,
            self.reversible_transform,
        ))
    }

    fn try_device(
        &mut self,
        samples: J2kLosslessSamples<'_>,
    ) -> Result<Option<EncodedDicomJ2kFrame>, WsiDicomError> {
        if let Some(encoded) = self.try_metal(samples)? {
            return Ok(Some(encoded));
        }
        if let Some(encoded) = self.try_cuda(samples)? {
            return Ok(Some(encoded));
        }
        Ok(None)
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    fn try_metal(
        &mut self,
        samples: J2kLosslessSamples<'_>,
    ) -> Result<Option<EncodedDicomJ2kFrame>, WsiDicomError> {
        let session = self.ensure_metal_session()?.clone();
        let accelerator = self.metal.get_or_insert_with(Default::default);
        let encode_started = Instant::now();
        let encoded = encode_j2k_lossless_with_device_accelerator(
            samples,
            self.transfer_syntax,
            BackendKind::Metal,
            accelerator,
            J2kEncodeValidation::External,
            self.j2k_decomposition_levels,
            self.reversible_transform,
        )?;
        let encode_duration = encode_started.elapsed();
        let mut validation_duration = Duration::ZERO;
        if self.codec_validation.enabled() {
            if let Some(codestream) = &encoded {
                let validation_started = Instant::now();
                signinum_j2k_metal::validate_lossless_roundtrip_on_metal_with_session(
                    samples, codestream, &session,
                )
                .map_err(|err| WsiDicomError::Encode {
                    message: format!("JPEG 2000 Metal validation failed: {err}"),
                })?;
                validation_duration = validation_started.elapsed();
            }
        }
        Ok(encoded.map(|codestream| EncodedDicomJ2kFrame {
            codestream: EncodedDicomJ2kCodestream::Host(codestream),
            used_device_encode: true,
            used_device_validation: self.codec_validation.enabled(),
            encode_duration,
            device_gpu_duration: None,
            validation_duration,
        }))
    }

    #[cfg(not(all(feature = "metal", target_os = "macos")))]
    fn try_metal(
        &mut self,
        _samples: J2kLosslessSamples<'_>,
    ) -> Result<Option<EncodedDicomJ2kFrame>, WsiDicomError> {
        Ok(None)
    }

    #[cfg(feature = "cuda")]
    fn try_cuda(
        &mut self,
        samples: J2kLosslessSamples<'_>,
    ) -> Result<Option<EncodedDicomJ2kFrame>, WsiDicomError> {
        let accelerator = self.cuda.get_or_insert_with(Default::default);
        let started = Instant::now();
        let encoded = encode_j2k_lossless_with_device_accelerator(
            samples,
            self.transfer_syntax,
            BackendKind::Cuda,
            accelerator,
            self.codec_validation.to_j2k_validation(),
            self.j2k_decomposition_levels,
        )?;
        let encode_duration = started.elapsed();
        Ok(encoded.map(|codestream| EncodedDicomJ2kFrame {
            codestream: EncodedDicomJ2kCodestream::Host(codestream),
            used_device_encode: true,
            used_device_validation: false,
            encode_duration,
            device_gpu_duration: None,
            validation_duration: Duration::ZERO,
        }))
    }

    #[cfg(not(feature = "cuda"))]
    fn try_cuda(
        &mut self,
        _samples: J2kLosslessSamples<'_>,
    ) -> Result<Option<EncodedDicomJ2kFrame>, WsiDicomError> {
        Ok(None)
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub(crate) fn encode_metal_tiles(
        &mut self,
        tiles: &[statumen::output::metal::MetalDeviceTile],
        output_width: u32,
        output_height: u32,
    ) -> Result<EncodedDicomJ2kMetalTileBatch, WsiDicomError> {
        if self.preference == EncodeBackendPreference::CpuOnly {
            return Ok(EncodedDicomJ2kMetalTileBatch {
                frames: (0..tiles.len()).map(|_| None).collect(),
                gpu_encode_stats: DicomJ2kGpuEncodeBatchStats::default(),
            });
        }

        let session = self.ensure_metal_session()?.clone();
        let options = lossless_encode_options(
            self.transfer_syntax,
            EncodeBackendPreference::PreferDevice,
            self.codec_validation,
            metal_resident_j2k_decomposition_levels(self.j2k_decomposition_levels),
            self.reversible_transform,
        )?;
        let mut encoded = Vec::with_capacity(tiles.len());
        let mut gpu_encode_stats = DicomJ2kGpuEncodeBatchStats::default();
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
            let mut requests = Vec::with_capacity(end - start);
            for tile in &tiles[start..end] {
                let statumen::output::metal::MetalDeviceStorage::Buffer {
                    buffer,
                    byte_offset,
                } = &tile.storage;
                requests.push(signinum_j2k_metal::MetalLosslessEncodeTile {
                    buffer,
                    byte_offset: *byte_offset,
                    width: tile.width,
                    height: tile.height,
                    pitch_bytes: tile.pitch_bytes,
                    output_width,
                    output_height,
                    format: tile.format,
                });
            }
            let config = signinum_j2k_metal::MetalLosslessEncodeConfig {
                gpu_encode_inflight_tiles: self.gpu_encode_inflight_tiles,
                gpu_encode_memory_budget_bytes: self
                    .gpu_encode_memory_mib
                    .and_then(|mib| usize::try_from(mib).ok())
                    .and_then(|mib| mib.checked_mul(1024 * 1024)),
            };
            if padded {
                let batch = match signinum_j2k_metal::encode_lossless_from_padded_metal_buffers_to_metal_batch(
                    &requests,
                    &options,
                    &session,
                    config,
                ) {
                    Ok(batch) => batch,
                    Err(_) if self.preference != EncodeBackendPreference::RequireDevice => {
                        encoded.extend((start..end).map(|_| None));
                        start = end;
                        continue;
                    }
                    Err(err) => {
                        return Err(WsiDicomError::Encode {
                            message: format!("JPEG 2000 Metal tile batch encode failed: {err}"),
                        });
                    }
                };
                gpu_encode_stats.add_assign(dicom_gpu_encode_stats_from_metal(
                    batch.stats,
                    self.gpu_encode_memory_mib,
                ));

                for outcome in batch.outcomes {
                    encoded.push(Some(EncodedDicomJ2kFrame {
                        codestream: EncodedDicomJ2kCodestream::Metal(outcome.encoded),
                        used_device_encode: true,
                        used_device_validation: self.codec_validation.enabled(),
                        encode_duration: outcome
                            .encode_duration
                            .saturating_add(outcome.input_copy_duration),
                        device_gpu_duration: outcome.gpu_duration,
                        validation_duration: outcome.validation_duration,
                    }));
                }
            } else {
                let batch =
                    match signinum_j2k_metal::encode_lossless_from_metal_buffers_to_metal_batch(
                        &requests, &options, &session, config,
                    ) {
                        Ok(batch) => batch,
                        Err(_) if self.preference != EncodeBackendPreference::RequireDevice => {
                            encoded.extend((start..end).map(|_| None));
                            start = end;
                            continue;
                        }
                        Err(err) => {
                            return Err(WsiDicomError::Encode {
                                message: format!("JPEG 2000 Metal tile batch encode failed: {err}"),
                            });
                        }
                    };
                gpu_encode_stats.add_assign(dicom_gpu_encode_stats_from_metal(
                    batch.stats,
                    self.gpu_encode_memory_mib,
                ));

                for outcome in batch.outcomes {
                    encoded.push(Some(EncodedDicomJ2kFrame {
                        codestream: EncodedDicomJ2kCodestream::Metal(outcome.encoded),
                        used_device_encode: true,
                        used_device_validation: self.codec_validation.enabled(),
                        encode_duration: outcome
                            .encode_duration
                            .saturating_add(outcome.input_copy_duration),
                        device_gpu_duration: outcome.gpu_duration,
                        validation_duration: outcome.validation_duration,
                    }));
                }
            }
            start = end;
        }
        Ok(EncodedDicomJ2kMetalTileBatch {
            frames: encoded,
            gpu_encode_stats,
        })
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    fn ensure_metal_session(
        &mut self,
    ) -> Result<&signinum_j2k_metal::MetalBackendSession, WsiDicomError> {
        if self.metal_session.is_none() {
            self.metal_session = Some(
                signinum_j2k_metal::MetalBackendSession::system_default().map_err(|err| {
                    WsiDicomError::Encode {
                        message: format!("JPEG 2000 Metal session unavailable: {err}"),
                    }
                })?,
            );
        }
        Ok(self
            .metal_session
            .as_ref()
            .expect("Metal session is initialized"))
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn dicom_gpu_encode_stats_from_metal(
    stats: signinum_j2k_metal::MetalLosslessEncodeBatchStats,
    configured_memory_mib: Option<u64>,
) -> DicomJ2kGpuEncodeBatchStats {
    DicomJ2kGpuEncodeBatchStats {
        configured_inflight_tiles: stats.configured_inflight_tiles,
        effective_inflight_tiles: stats.effective_inflight_tiles,
        max_observed_inflight_tiles: stats.max_observed_inflight_tiles,
        configured_memory_mib,
        effective_memory_mib: bytes_to_mib_ceil(stats.effective_memory_budget_bytes),
        encode_wall_duration: stats.encode_wall_duration,
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn bytes_to_mib_ceil(bytes: usize) -> u64 {
    let mib = 1024usize * 1024;
    bytes.div_ceil(mib) as u64
}

#[cfg(test)]
pub(crate) fn encode_dicom_j2k_lossless(
    samples: J2kLosslessSamples<'_>,
    preference: EncodeBackendPreference,
) -> Result<Vec<u8>, WsiDicomError> {
    encode_dicom_lossless(
        samples,
        TransferSyntax::Jpeg2000Lossless,
        preference,
        CodecValidation::RoundTrip,
    )
}

#[cfg(test)]
pub(crate) fn encode_dicom_lossless(
    samples: J2kLosslessSamples<'_>,
    transfer_syntax: TransferSyntax,
    preference: EncodeBackendPreference,
    codec_validation: CodecValidation,
) -> Result<Vec<u8>, WsiDicomError> {
    DicomJ2kEncoder::new(preference, transfer_syntax, codec_validation)
        .encode(samples)
        .and_then(EncodedDicomJ2kFrame::into_codestream)
}

pub(crate) fn encode_lossless_cpu(
    samples: J2kLosslessSamples<'_>,
    transfer_syntax: TransferSyntax,
    codec_validation: CodecValidation,
    j2k_decomposition_levels: Option<u8>,
    reversible_transform: ReversibleTransform,
) -> Result<EncodedDicomJ2kFrame, WsiDicomError> {
    let started = Instant::now();
    encode_j2k_lossless(
        samples,
        &lossless_encode_options(
            transfer_syntax,
            EncodeBackendPreference::CpuOnly,
            codec_validation,
            j2k_decomposition_levels,
            reversible_transform,
        )?,
    )
    .map(|encoded| {
        let encode_duration = started.elapsed();
        EncodedDicomJ2kFrame {
            codestream: EncodedDicomJ2kCodestream::Host(encoded.codestream),
            used_device_encode: false,
            used_device_validation: false,
            encode_duration,
            device_gpu_duration: None,
            validation_duration: Duration::ZERO,
        }
    })
    .map_err(|source| WsiDicomError::Encode {
        message: source.to_string(),
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(crate) fn metal_tile_is_padded_contiguous(
    tile: &statumen::output::metal::MetalDeviceTile,
    output_width: u32,
    output_height: u32,
) -> bool {
    tile.width == output_width
        && tile.height == output_height
        && tile.pitch_bytes == (output_width as usize).saturating_mul(tile.format.bytes_per_pixel())
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn metal_resident_j2k_decomposition_levels(j2k_decomposition_levels: Option<u8>) -> Option<u8> {
    Some(j2k_decomposition_levels.unwrap_or(1).min(1))
}

#[cfg_attr(
    not(any(feature = "cuda", all(feature = "metal", target_os = "macos"))),
    allow(dead_code)
)]
fn encode_j2k_lossless_with_device_accelerator(
    samples: J2kLosslessSamples<'_>,
    transfer_syntax: TransferSyntax,
    backend: BackendKind,
    accelerator: &mut impl J2kEncodeStageAccelerator,
    validation: J2kEncodeValidation,
    j2k_decomposition_levels: Option<u8>,
    reversible_transform: ReversibleTransform,
) -> Result<Option<Vec<u8>>, WsiDicomError> {
    let encoded = encode_j2k_lossless_with_accelerator(
        samples,
        &J2kLosslessEncodeOptions {
            validation,
            ..lossless_encode_options(
                transfer_syntax,
                EncodeBackendPreference::PreferDevice,
                CodecValidation::RoundTrip,
                j2k_decomposition_levels,
                reversible_transform,
            )?
        },
        backend,
        accelerator,
    )
    .map_err(|err| WsiDicomError::Encode {
        message: format!("JPEG 2000 device encode failed: {err}"),
    })?;
    Ok((encoded.backend == backend).then_some(encoded.codestream))
}

fn lossless_encode_options(
    transfer_syntax: TransferSyntax,
    backend: EncodeBackendPreference,
    codec_validation: CodecValidation,
    j2k_decomposition_levels: Option<u8>,
    reversible_transform: ReversibleTransform,
) -> Result<J2kLosslessEncodeOptions, WsiDicomError> {
    let (block_coding_mode, progression) = match transfer_syntax {
        TransferSyntax::Jpeg2000Lossless => {
            (J2kBlockCodingMode::Classic, J2kProgressionOrder::Lrcp)
        }
        TransferSyntax::Htj2kLossless => (
            J2kBlockCodingMode::HighThroughput,
            J2kProgressionOrder::Lrcp,
        ),
        TransferSyntax::Htj2kLosslessRpcl => (
            J2kBlockCodingMode::HighThroughput,
            J2kProgressionOrder::Rpcl,
        ),
        TransferSyntax::JpegBaseline8Bit
        | TransferSyntax::Jpeg2000
        | TransferSyntax::ExplicitVrLittleEndian => {
            return Err(WsiDicomError::Unsupported {
                reason: "transfer syntax is not implemented for lossless JPEG 2000 export".into(),
            });
        }
    };

    Ok(J2kLosslessEncodeOptions {
        backend: backend.to_signinum(),
        block_coding_mode,
        progression,
        max_decomposition_levels: j2k_decomposition_levels,
        reversible_transform,
        validation: codec_validation.to_j2k_validation(),
    })
}

#[cfg(test)]
pub(crate) fn dicom_j2k_decomposition_levels(samples: J2kLosslessSamples<'_>) -> u8 {
    signinum_j2k::j2k_lossless_decomposition_levels(samples)
}

#[cfg(all(test, feature = "metal", target_os = "macos"))]
mod tests {
    use super::DicomJ2kEncoder;
    use crate::{CodecValidation, EncodeBackendPreference, TransferSyntax};
    use signinum_core::PixelFormat;
    use statumen::output::metal::{MetalDeviceStorage, MetalDeviceTile};

    #[test]
    fn auto_j2k_encoder_can_be_demoted_after_cpu_input_probe_wins() {
        let mut encoder = DicomJ2kEncoder::new(
            EncodeBackendPreference::Auto,
            TransferSyntax::Htj2kLosslessRpcl,
            CodecValidation::Disabled,
        );

        let cpu_peer = encoder.cpu_only_peer();
        assert_eq!(cpu_peer.preference(), EncodeBackendPreference::CpuOnly);

        encoder.force_cpu_only_for_auto();
        assert_eq!(encoder.preference(), EncodeBackendPreference::CpuOnly);

        let mut preferred = DicomJ2kEncoder::new(
            EncodeBackendPreference::PreferDevice,
            TransferSyntax::Htj2kLosslessRpcl,
            CodecValidation::Disabled,
        );
        preferred.force_cpu_only_for_auto();
        assert_eq!(
            preferred.preference(),
            EncodeBackendPreference::PreferDevice
        );
    }

    #[test]
    fn metal_tile_encode_returns_buffer_backed_codestream_for_padded_tiles() {
        let pixels: Vec<u8> = (0..8 * 8 * 3)
            .map(|idx| ((idx * 29) & 0xFF) as u8)
            .collect();
        let session =
            signinum_j2k_metal::MetalBackendSession::system_default().expect("Metal session");
        let buffer = session.device().new_buffer_with_data(
            pixels.as_ptr().cast(),
            pixels.len() as u64,
            metal::MTLResourceOptions::StorageModeShared,
        );
        let tile = MetalDeviceTile {
            width: 8,
            height: 8,
            pitch_bytes: 8 * 3,
            format: PixelFormat::Rgb8,
            storage: MetalDeviceStorage::Buffer {
                buffer,
                byte_offset: 0,
            },
        };
        let mut encoder = DicomJ2kEncoder::new(
            EncodeBackendPreference::RequireDevice,
            TransferSyntax::Jpeg2000Lossless,
            CodecValidation::RoundTrip,
        );

        let encoded = encoder
            .encode_metal_tiles(&[tile], 8, 8)
            .expect("Metal DICOM tile encode")
            .frames;
        let frame = encoded
            .into_iter()
            .next()
            .expect("one frame")
            .expect("Metal frame");

        assert!(frame.codestream_is_metal_buffer_backed());
        let codestream = frame.codestream_bytes().expect("codestream bytes");
        assert!(codestream.starts_with(&[0xFF, 0x4F]));
        let mut decoded = vec![0u8; pixels.len()];
        signinum_j2k::J2kDecoder::new(codestream)
            .expect("parse J2K")
            .decode_into(&mut decoded, 8 * 3, PixelFormat::Rgb8)
            .expect("decode J2K");
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn metal_tile_encode_returns_buffer_backed_codestream_for_edge_tiles() {
        let pixels: Vec<u8> = (0..7 * 5 * 3)
            .map(|idx| ((idx * 31) & 0xFF) as u8)
            .collect();
        let session =
            signinum_j2k_metal::MetalBackendSession::system_default().expect("Metal session");
        let buffer = session.device().new_buffer_with_data(
            pixels.as_ptr().cast(),
            pixels.len() as u64,
            metal::MTLResourceOptions::StorageModeShared,
        );
        let tile = MetalDeviceTile {
            width: 7,
            height: 5,
            pitch_bytes: 7 * 3,
            format: PixelFormat::Rgb8,
            storage: MetalDeviceStorage::Buffer {
                buffer,
                byte_offset: 0,
            },
        };
        let mut encoder = DicomJ2kEncoder::new(
            EncodeBackendPreference::RequireDevice,
            TransferSyntax::Jpeg2000Lossless,
            CodecValidation::RoundTrip,
        );

        let encoded = encoder
            .encode_metal_tiles(&[tile], 8, 8)
            .expect("Metal DICOM edge tile encode")
            .frames;
        let frame = encoded
            .into_iter()
            .next()
            .expect("one frame")
            .expect("Metal frame");

        assert!(frame.codestream_is_metal_buffer_backed());
        let codestream = frame.codestream_bytes().expect("codestream bytes");
        assert!(codestream.starts_with(&[0xFF, 0x4F]));
        let mut decoded = vec![0u8; 8 * 8 * 3];
        signinum_j2k::J2kDecoder::new(codestream)
            .expect("parse J2K")
            .decode_into(&mut decoded, 8 * 3, PixelFormat::Rgb8)
            .expect("decode J2K");
        for y in 0..8usize {
            for x in 0..8usize {
                let dst = (y * 8 + x) * 3;
                if x < 7 && y < 5 {
                    let src = (y * 7 + x) * 3;
                    assert_eq!(&decoded[dst..dst + 3], &pixels[src..src + 3]);
                } else {
                    assert_eq!(&decoded[dst..dst + 3], &[0, 0, 0]);
                }
            }
        }
    }

    #[test]
    fn metal_tile_encode_returns_buffer_backed_codestream_for_htj2k_tiles() {
        let pixels: Vec<u8> = (0..8 * 8).map(|idx| ((idx * 37) & 0xFF) as u8).collect();
        let session =
            signinum_j2k_metal::MetalBackendSession::system_default().expect("Metal session");
        let buffer = session.device().new_buffer_with_data(
            pixels.as_ptr().cast(),
            pixels.len() as u64,
            metal::MTLResourceOptions::StorageModeShared,
        );
        let tile = MetalDeviceTile {
            width: 8,
            height: 8,
            pitch_bytes: 8,
            format: PixelFormat::Gray8,
            storage: MetalDeviceStorage::Buffer {
                buffer,
                byte_offset: 0,
            },
        };
        let mut encoder = DicomJ2kEncoder::new(
            EncodeBackendPreference::RequireDevice,
            TransferSyntax::Htj2kLossless,
            CodecValidation::RoundTrip,
        );

        let encoded = encoder
            .encode_metal_tiles(&[tile], 8, 8)
            .expect("Metal DICOM HTJ2K tile encode")
            .frames;
        let frame = encoded
            .into_iter()
            .next()
            .expect("one frame")
            .expect("Metal frame");

        assert!(frame.codestream_is_metal_buffer_backed());
        let codestream = frame.codestream_bytes().expect("codestream bytes");
        assert!(codestream.windows(2).any(|window| window == [0xFF, 0x50]));
        let cod_marker = codestream
            .windows(2)
            .position(|window| window == [0xFF, 0x52])
            .expect("COD marker");
        assert_eq!(codestream[cod_marker + 12], 0x40);
        let mut decoded = vec![0u8; pixels.len()];
        signinum_j2k::J2kDecoder::new(codestream)
            .expect("parse HTJ2K")
            .decode_into(&mut decoded, 8, PixelFormat::Gray8)
            .expect("decode HTJ2K");
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn metal_tile_encode_returns_buffer_backed_codestream_for_wsi_sized_htj2k_rpcl_tiles() {
        let pixels: Vec<u8> = (0..256 * 256 * 3)
            .map(|idx| ((idx * 41) & 0xFF) as u8)
            .collect();
        let session =
            signinum_j2k_metal::MetalBackendSession::system_default().expect("Metal session");
        let buffer = session.device().new_buffer_with_data(
            pixels.as_ptr().cast(),
            pixels.len() as u64,
            metal::MTLResourceOptions::StorageModeShared,
        );
        let tile = MetalDeviceTile {
            width: 256,
            height: 256,
            pitch_bytes: 256 * 3,
            format: PixelFormat::Rgb8,
            storage: MetalDeviceStorage::Buffer {
                buffer,
                byte_offset: 0,
            },
        };
        let mut encoder = DicomJ2kEncoder::new(
            EncodeBackendPreference::RequireDevice,
            TransferSyntax::Htj2kLosslessRpcl,
            CodecValidation::RoundTrip,
        );

        let encoded = encoder
            .encode_metal_tiles(&[tile], 256, 256)
            .expect("Metal DICOM HTJ2K RPCL tile encode")
            .frames;
        let frame = encoded
            .into_iter()
            .next()
            .expect("one frame")
            .expect("Metal frame");

        assert!(frame.codestream_is_metal_buffer_backed());
        let codestream = frame.codestream_bytes().expect("codestream bytes");
        assert!(codestream.windows(2).any(|window| window == [0xFF, 0x50]));
        let cod_marker = codestream
            .windows(2)
            .position(|window| window == [0xFF, 0x52])
            .expect("COD marker");
        assert_eq!(codestream[cod_marker + 5], 0x02);
        assert_eq!(codestream[cod_marker + 12], 0x40);
        let mut decoded = vec![0u8; pixels.len()];
        signinum_j2k::J2kDecoder::new(codestream)
            .expect("parse HTJ2K")
            .decode_into(&mut decoded, 256 * 3, PixelFormat::Rgb8)
            .expect("decode HTJ2K");
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn metal_edge_rgb8_htj2k_rpcl_codestream_decodes_with_reference_codec_when_available() {
        let Some(grk_decompress) = find_command_for_test("grk_decompress") else {
            eprintln!("skipping resident Metal HTJ2K edge parity smoke: grk_decompress not found");
            return;
        };
        let pixels: Vec<u8> = (0..7 * 5 * 3)
            .map(|idx| ((idx * 43 + 17) & 0xFF) as u8)
            .collect();
        let session =
            signinum_j2k_metal::MetalBackendSession::system_default().expect("Metal session");
        let buffer = session.device().new_buffer_with_data(
            pixels.as_ptr().cast(),
            pixels.len() as u64,
            metal::MTLResourceOptions::StorageModeShared,
        );
        let tile = MetalDeviceTile {
            width: 7,
            height: 5,
            pitch_bytes: 7 * 3,
            format: PixelFormat::Rgb8,
            storage: MetalDeviceStorage::Buffer {
                buffer,
                byte_offset: 0,
            },
        };
        let mut encoder = DicomJ2kEncoder::new(
            EncodeBackendPreference::RequireDevice,
            TransferSyntax::Htj2kLosslessRpcl,
            CodecValidation::RoundTrip,
        );

        let encoded = encoder
            .encode_metal_tiles(&[tile], 8, 8)
            .expect("resident Metal DICOM HTJ2K RPCL edge tile encode")
            .frames;
        let frame = encoded
            .into_iter()
            .next()
            .expect("one frame")
            .expect("Metal frame");

        assert!(frame.codestream_is_metal_buffer_backed());
        let codestream = frame.codestream_bytes().expect("codestream bytes");
        assert!(codestream.windows(2).any(|window| window == [0xFF, 0x50]));
        let cod_marker = codestream
            .windows(2)
            .position(|window| window == [0xFF, 0x52])
            .expect("COD marker");
        assert_eq!(codestream[cod_marker + 5], 0x02);
        assert_eq!(codestream[cod_marker + 12], 0x40);

        let tmp = tempfile::tempdir().expect("tempdir");
        let codestream_path = tmp.path().join("edge-rgb8.j2k");
        let ppm_path = tmp.path().join("edge-rgb8.ppm");
        std::fs::write(&codestream_path, codestream).expect("write codestream");
        let status = std::process::Command::new(grk_decompress)
            .args(["-i"])
            .arg(&codestream_path)
            .args(["-o"])
            .arg(&ppm_path)
            .status()
            .expect("run grk_decompress");
        assert!(status.success(), "grk_decompress failed with {status}");

        let (width, height, decoded) = read_binary_ppm_for_test(&ppm_path);
        assert_eq!((width, height), (8, 8));
        for y in 0..8usize {
            for x in 0..8usize {
                let dst = (y * 8 + x) * 3;
                if x < 7 && y < 5 {
                    let src = (y * 7 + x) * 3;
                    assert_eq!(&decoded[dst..dst + 3], &pixels[src..src + 3]);
                } else {
                    assert_eq!(&decoded[dst..dst + 3], &[0, 0, 0]);
                }
            }
        }
    }

    #[test]
    fn prefer_device_metal_tile_encode_returns_buffer_backed_codestream_for_wsi_sized_htj2k_rpcl_tiles(
    ) {
        let pixels: Vec<u8> = (0..256 * 256 * 3)
            .map(|idx| ((idx * 41) & 0xFF) as u8)
            .collect();
        let session =
            signinum_j2k_metal::MetalBackendSession::system_default().expect("Metal session");
        let buffer = session.device().new_buffer_with_data(
            pixels.as_ptr().cast(),
            pixels.len() as u64,
            metal::MTLResourceOptions::StorageModeShared,
        );
        let tile = MetalDeviceTile {
            width: 256,
            height: 256,
            pitch_bytes: 256 * 3,
            format: PixelFormat::Rgb8,
            storage: MetalDeviceStorage::Buffer {
                buffer,
                byte_offset: 0,
            },
        };
        let mut encoder = DicomJ2kEncoder::new(
            EncodeBackendPreference::PreferDevice,
            TransferSyntax::Htj2kLosslessRpcl,
            CodecValidation::RoundTrip,
        );

        let encoded = encoder
            .encode_metal_tiles(&[tile], 256, 256)
            .expect("PreferDevice Metal DICOM HTJ2K RPCL tile encode")
            .frames;

        assert_eq!(encoded.len(), 1);
        assert!(encoded[0]
            .as_ref()
            .expect("Metal frame")
            .codestream_is_metal_buffer_backed());
    }

    fn find_command_for_test(name: &str) -> Option<String> {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|path| path.join(name))
                .find(|path| path.is_file())
                .map(|path| path.to_string_lossy().into_owned())
        })
    }

    fn read_binary_ppm_for_test(path: &std::path::Path) -> (usize, usize, Vec<u8>) {
        let bytes = std::fs::read(path).expect("read PPM");
        let mut cursor = 0usize;
        let magic = next_ppm_token_for_test(&bytes, &mut cursor);
        assert_eq!(magic, "P6");
        let width: usize = next_ppm_token_for_test(&bytes, &mut cursor)
            .parse()
            .expect("PPM width");
        let height: usize = next_ppm_token_for_test(&bytes, &mut cursor)
            .parse()
            .expect("PPM height");
        let maxval = next_ppm_token_for_test(&bytes, &mut cursor);
        assert_eq!(maxval, "255");
        if cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let pixels = bytes[cursor..].to_vec();
        assert_eq!(pixels.len(), width * height * 3);
        (width, height, pixels)
    }

    fn next_ppm_token_for_test(bytes: &[u8], cursor: &mut usize) -> String {
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
            .expect("PPM token is UTF-8")
            .to_string()
    }
}
