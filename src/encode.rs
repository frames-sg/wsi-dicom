use signinum_j2k::{
    encode_j2k_lossless, encode_j2k_lossless_with_accelerator, BackendKind,
    J2kEncodeStageAccelerator, J2kLosslessEncodeOptions, J2kLosslessSamples, J2kProgressionOrder,
    ReversibleTransform,
};

use crate::{EncodeBackendPreference, WsiDicomError};

pub(crate) struct DicomJ2kEncoder {
    preference: EncodeBackendPreference,
    #[cfg(all(feature = "metal", target_os = "macos"))]
    metal: Option<signinum_j2k_metal::MetalEncodeStageAccelerator>,
    #[cfg(feature = "cuda")]
    cuda: Option<signinum_j2k_cuda::CudaEncodeStageAccelerator>,
}

impl DicomJ2kEncoder {
    pub(crate) fn new(preference: EncodeBackendPreference) -> Self {
        Self {
            preference,
            #[cfg(all(feature = "metal", target_os = "macos"))]
            metal: None,
            #[cfg(feature = "cuda")]
            cuda: None,
        }
    }

    pub(crate) fn encode(
        &mut self,
        samples: J2kLosslessSamples<'_>,
    ) -> Result<Vec<u8>, WsiDicomError> {
        if self.preference == EncodeBackendPreference::CpuOnly {
            return encode_j2k_lossless_cpu(samples);
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
            None => encode_j2k_lossless_cpu(samples),
        }
    }

    fn try_device(
        &mut self,
        samples: J2kLosslessSamples<'_>,
    ) -> Result<Option<Vec<u8>>, WsiDicomError> {
        if let Some(codestream) = self.try_metal(samples)? {
            return Ok(Some(codestream));
        }
        if let Some(codestream) = self.try_cuda(samples)? {
            return Ok(Some(codestream));
        }
        Ok(None)
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    fn try_metal(
        &mut self,
        samples: J2kLosslessSamples<'_>,
    ) -> Result<Option<Vec<u8>>, WsiDicomError> {
        let accelerator = self.metal.get_or_insert_with(Default::default);
        encode_j2k_lossless_with_device_accelerator(samples, BackendKind::Metal, accelerator)
    }

    #[cfg(not(all(feature = "metal", target_os = "macos")))]
    fn try_metal(
        &mut self,
        _samples: J2kLosslessSamples<'_>,
    ) -> Result<Option<Vec<u8>>, WsiDicomError> {
        Ok(None)
    }

    #[cfg(feature = "cuda")]
    fn try_cuda(
        &mut self,
        samples: J2kLosslessSamples<'_>,
    ) -> Result<Option<Vec<u8>>, WsiDicomError> {
        let accelerator = self.cuda.get_or_insert_with(Default::default);
        encode_j2k_lossless_with_device_accelerator(samples, BackendKind::Cuda, accelerator)
    }

    #[cfg(not(feature = "cuda"))]
    fn try_cuda(
        &mut self,
        _samples: J2kLosslessSamples<'_>,
    ) -> Result<Option<Vec<u8>>, WsiDicomError> {
        Ok(None)
    }
}

#[cfg(test)]
pub(crate) fn encode_dicom_j2k_lossless(
    samples: J2kLosslessSamples<'_>,
    preference: EncodeBackendPreference,
) -> Result<Vec<u8>, WsiDicomError> {
    DicomJ2kEncoder::new(preference).encode(samples)
}

fn encode_j2k_lossless_cpu(samples: J2kLosslessSamples<'_>) -> Result<Vec<u8>, WsiDicomError> {
    encode_j2k_lossless(
        samples,
        &J2kLosslessEncodeOptions {
            backend: EncodeBackendPreference::CpuOnly.to_signinum(),
            progression: J2kProgressionOrder::Lrcp,
            reversible_transform: ReversibleTransform::Rct53,
        },
    )
    .map(|encoded| encoded.codestream)
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
    backend: BackendKind,
    accelerator: &mut impl J2kEncodeStageAccelerator,
) -> Result<Option<Vec<u8>>, WsiDicomError> {
    let encoded = encode_j2k_lossless_with_accelerator(
        samples,
        &J2kLosslessEncodeOptions {
            backend: EncodeBackendPreference::PreferDevice.to_signinum(),
            progression: J2kProgressionOrder::Lrcp,
            reversible_transform: ReversibleTransform::Rct53,
        },
        backend,
        accelerator,
    )
    .map_err(|err| WsiDicomError::Encode {
        message: format!("JPEG 2000 device encode failed: {err}"),
    })?;
    Ok((encoded.backend == backend).then_some(encoded.codestream))
}

#[cfg(test)]
pub(crate) fn dicom_j2k_decomposition_levels(samples: J2kLosslessSamples<'_>) -> u8 {
    signinum_j2k::j2k_lossless_decomposition_levels(samples)
}
