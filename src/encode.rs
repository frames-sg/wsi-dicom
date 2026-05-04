use std::time::{Duration, Instant};

use signinum_j2k::{
    encode_j2k_lossless, encode_j2k_lossless_with_accelerator, BackendKind, J2kBlockCodingMode,
    J2kEncodeStageAccelerator, J2kEncodeValidation, J2kLosslessEncodeOptions, J2kLosslessSamples,
    J2kProgressionOrder, ReversibleTransform,
};

use crate::{EncodeBackendPreference, TransferSyntax, WsiDicomError};

pub(crate) struct DicomJ2kEncoder {
    preference: EncodeBackendPreference,
    transfer_syntax: TransferSyntax,
    #[cfg(all(feature = "metal", target_os = "macos"))]
    metal: Option<signinum_j2k_metal::MetalEncodeStageAccelerator>,
    #[cfg(all(feature = "metal", target_os = "macos"))]
    metal_session: Option<signinum_j2k_metal::MetalBackendSession>,
    #[cfg(feature = "cuda")]
    cuda: Option<signinum_j2k_cuda::CudaEncodeStageAccelerator>,
}

pub(crate) struct EncodedDicomJ2kFrame {
    pub(crate) codestream: Vec<u8>,
    pub(crate) used_device_encode: bool,
    pub(crate) used_device_validation: bool,
    pub(crate) encode_duration: Duration,
    pub(crate) validation_duration: Duration,
}

impl DicomJ2kEncoder {
    pub(crate) fn new(
        preference: EncodeBackendPreference,
        transfer_syntax: TransferSyntax,
    ) -> Self {
        Self {
            preference,
            transfer_syntax,
            #[cfg(all(feature = "metal", target_os = "macos"))]
            metal: None,
            #[cfg(all(feature = "metal", target_os = "macos"))]
            metal_session: None,
            #[cfg(feature = "cuda")]
            cuda: None,
        }
    }

    pub(crate) fn encode(
        &mut self,
        samples: J2kLosslessSamples<'_>,
    ) -> Result<EncodedDicomJ2kFrame, WsiDicomError> {
        if self.preference == EncodeBackendPreference::CpuOnly {
            return encode_lossless_cpu(samples, self.transfer_syntax);
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
            None => encode_lossless_cpu(samples, self.transfer_syntax),
        }
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
        )?;
        let encode_duration = encode_started.elapsed();
        let mut validation_duration = Duration::ZERO;
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
        Ok(encoded.map(|codestream| EncodedDicomJ2kFrame {
            codestream,
            used_device_encode: true,
            used_device_validation: true,
            encode_duration,
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
            J2kEncodeValidation::CpuRoundTrip,
        )?;
        let encode_duration = started.elapsed();
        Ok(encoded.map(|codestream| EncodedDicomJ2kFrame {
            codestream,
            used_device_encode: true,
            used_device_validation: false,
            encode_duration,
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
    ) -> Result<Vec<Option<EncodedDicomJ2kFrame>>, WsiDicomError> {
        if self.preference == EncodeBackendPreference::CpuOnly {
            return Ok((0..tiles.len()).map(|_| None).collect());
        }

        let session = self.ensure_metal_session()?.clone();
        let options =
            lossless_encode_options(self.transfer_syntax, EncodeBackendPreference::PreferDevice)?;
        let mut encoded = Vec::with_capacity(tiles.len());
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
            let outcomes = if padded {
                signinum_j2k_metal::encode_lossless_from_padded_metal_buffers_with_report(
                    &requests, &options, &session,
                )
            } else {
                signinum_j2k_metal::encode_lossless_from_metal_buffers_with_report(
                    &requests, &options, &session,
                )
            }
            .map_err(|err| WsiDicomError::Encode {
                message: format!("JPEG 2000 Metal tile batch encode failed: {err}"),
            })?;

            for outcome in outcomes {
                encoded.push(
                    (outcome.encoded.backend == BackendKind::Metal).then_some(
                        EncodedDicomJ2kFrame {
                            codestream: outcome.encoded.codestream,
                            used_device_encode: true,
                            used_device_validation: true,
                            encode_duration: outcome
                                .encode_duration
                                .saturating_add(outcome.input_copy_duration),
                            validation_duration: outcome.validation_duration,
                        },
                    ),
                );
            }
            start = end;
        }
        Ok(encoded)
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

#[cfg(test)]
pub(crate) fn encode_dicom_j2k_lossless(
    samples: J2kLosslessSamples<'_>,
    preference: EncodeBackendPreference,
) -> Result<Vec<u8>, WsiDicomError> {
    encode_dicom_lossless(samples, TransferSyntax::Jpeg2000Lossless, preference)
}

#[cfg(test)]
pub(crate) fn encode_dicom_lossless(
    samples: J2kLosslessSamples<'_>,
    transfer_syntax: TransferSyntax,
    preference: EncodeBackendPreference,
) -> Result<Vec<u8>, WsiDicomError> {
    DicomJ2kEncoder::new(preference, transfer_syntax)
        .encode(samples)
        .map(|encoded| encoded.codestream)
}

fn encode_lossless_cpu(
    samples: J2kLosslessSamples<'_>,
    transfer_syntax: TransferSyntax,
) -> Result<EncodedDicomJ2kFrame, WsiDicomError> {
    let started = Instant::now();
    encode_j2k_lossless(
        samples,
        &lossless_encode_options(transfer_syntax, EncodeBackendPreference::CpuOnly)?,
    )
    .map(|encoded| {
        let encode_duration = started.elapsed();
        EncodedDicomJ2kFrame {
            codestream: encoded.codestream,
            used_device_encode: false,
            used_device_validation: false,
            encode_duration,
            validation_duration: Duration::ZERO,
        }
    })
    .map_err(|source| WsiDicomError::Encode {
        message: source.to_string(),
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn metal_tile_is_padded_contiguous(
    tile: &statumen::output::metal::MetalDeviceTile,
    output_width: u32,
    output_height: u32,
) -> bool {
    tile.width == output_width
        && tile.height == output_height
        && tile.pitch_bytes == (output_width as usize).saturating_mul(tile.format.bytes_per_pixel())
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
) -> Result<Option<Vec<u8>>, WsiDicomError> {
    let encoded = encode_j2k_lossless_with_accelerator(
        samples,
        &J2kLosslessEncodeOptions {
            validation,
            ..lossless_encode_options(transfer_syntax, EncodeBackendPreference::PreferDevice)?
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
        TransferSyntax::JpegBaseline8Bit | TransferSyntax::ExplicitVrLittleEndian => {
            return Err(WsiDicomError::Unsupported {
                reason: "transfer syntax is not implemented for lossless JPEG 2000 export".into(),
            });
        }
    };

    Ok(J2kLosslessEncodeOptions {
        backend: backend.to_signinum(),
        block_coding_mode,
        progression,
        reversible_transform: ReversibleTransform::Rct53,
        validation: J2kEncodeValidation::CpuRoundTrip,
    })
}

#[cfg(test)]
pub(crate) fn dicom_j2k_decomposition_levels(samples: J2kLosslessSamples<'_>) -> u8 {
    signinum_j2k::j2k_lossless_decomposition_levels(samples)
}
