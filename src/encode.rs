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
        let encoded = encode_j2k_lossless_with_device_accelerator(
            samples,
            self.transfer_syntax,
            BackendKind::Metal,
            accelerator,
            J2kEncodeValidation::External,
        )?;
        if let Some(codestream) = &encoded {
            signinum_j2k_metal::validate_lossless_roundtrip_on_metal_with_session(
                samples, codestream, &session,
            )
            .map_err(|err| WsiDicomError::Encode {
                message: format!("JPEG 2000 Metal validation failed: {err}"),
            })?;
        }
        Ok(encoded.map(|codestream| EncodedDicomJ2kFrame {
            codestream,
            used_device_encode: true,
            used_device_validation: true,
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
        let encoded = encode_j2k_lossless_with_device_accelerator(
            samples,
            self.transfer_syntax,
            BackendKind::Cuda,
            accelerator,
            J2kEncodeValidation::CpuRoundTrip,
        )?;
        Ok(encoded.map(|codestream| EncodedDicomJ2kFrame {
            codestream,
            used_device_encode: true,
            used_device_validation: false,
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
    pub(crate) fn encode_metal_tile(
        &mut self,
        tile: &statumen::output::metal::MetalDeviceTile,
        output_width: u32,
        output_height: u32,
    ) -> Result<Option<EncodedDicomJ2kFrame>, WsiDicomError> {
        if self.preference == EncodeBackendPreference::CpuOnly {
            return Ok(None);
        }

        let session = self.ensure_metal_session()?.clone();
        let statumen::output::metal::MetalDeviceStorage::Buffer {
            buffer,
            byte_offset,
        } = &tile.storage;
        let encoded = signinum_j2k_metal::encode_lossless_from_metal_buffer(
            signinum_j2k_metal::MetalLosslessEncodeTile {
                buffer,
                byte_offset: *byte_offset,
                width: tile.width,
                height: tile.height,
                pitch_bytes: tile.pitch_bytes,
                output_width,
                output_height,
                format: tile.format,
            },
            &lossless_encode_options(self.transfer_syntax, EncodeBackendPreference::PreferDevice)?,
            &session,
        )
        .map_err(|err| WsiDicomError::Encode {
            message: format!("JPEG 2000 Metal tile encode failed: {err}"),
        })?;

        Ok(
            (encoded.backend == BackendKind::Metal).then_some(EncodedDicomJ2kFrame {
                codestream: encoded.codestream,
                used_device_encode: true,
                used_device_validation: true,
            }),
        )
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
    encode_j2k_lossless(
        samples,
        &lossless_encode_options(transfer_syntax, EncodeBackendPreference::CpuOnly)?,
    )
    .map(|encoded| EncodedDicomJ2kFrame {
        codestream: encoded.codestream,
        used_device_encode: false,
        used_device_validation: false,
    })
    .map_err(|source| WsiDicomError::Encode {
        message: source.to_string(),
    })
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
