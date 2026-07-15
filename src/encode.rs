use std::{
    borrow::Cow,
    time::{Duration, Instant},
};

use crate::{CodecValidation, EncodeBackendPreference, Error, TransferSyntax};
use j2k::{
    encode_j2k_lossless, encode_j2k_lossless_with_accelerator, BackendKind, J2kBlockCodingMode,
    J2kEncodeStageAccelerator, J2kEncodeValidation, J2kLosslessEncodeOptions, J2kLosslessSamples,
    J2kProgressionOrder, ReversibleTransform,
};

#[cfg(all(feature = "metal", target_os = "macos"))]
mod metal;
#[cfg(all(feature = "metal", target_os = "macos"))]
pub(crate) use metal::{DicomJ2kGpuEncodeBatchStats, SubmittedDicomJ2kMetalTileBatch};

pub(crate) struct DicomJ2kEncoder {
    preference: EncodeBackendPreference,
    transfer_syntax: TransferSyntax,
    codec_validation: CodecValidation,
    j2k_decomposition_levels: Option<u8>,
    reversible_transform: ReversibleTransform,
    gpu_encode_inflight_tiles: Option<usize>,
    gpu_encode_memory_mib: Option<u64>,
    #[cfg(all(feature = "metal", target_os = "macos"))]
    metal: Option<j2k_metal::MetalEncodeStageAccelerator>,
    #[cfg(all(feature = "metal", target_os = "macos"))]
    metal_session: Option<j2k_metal::MetalBackendSession>,
    #[cfg(feature = "cuda")]
    cuda: Option<j2k_cuda::CudaEncodeStageAccelerator>,
}

pub(crate) struct EncodedDicomJ2kFrame {
    codestream: EncodedDicomJ2kCodestream,
    pub(crate) used_device_encode: bool,
    pub(crate) used_device_validation: bool,
    pub(crate) encode_duration: Duration,
    pub(crate) gpu_encode_wall_duration: Option<Duration>,
    pub(crate) device_gpu_duration: Option<Duration>,
    pub(crate) validation_duration: Duration,
}

pub(crate) enum EncodedDicomJ2kCodestream {
    Host(Vec<u8>),
    #[cfg(all(feature = "metal", target_os = "macos"))]
    Metal(j2k_metal::MetalEncodedJ2k),
}

#[cfg_attr(
    not(any(test, feature = "cuda", all(feature = "metal", target_os = "macos"))),
    allow(dead_code)
)]
struct DeviceEncodedCodestream {
    codestream: Vec<u8>,
    used_device_encode: bool,
}

impl EncodedDicomJ2kFrame {
    pub(crate) fn codestream_bytes(&self) -> Result<Cow<'_, [u8]>, Error> {
        match &self.codestream {
            EncodedDicomJ2kCodestream::Host(bytes) => Ok(Cow::Borrowed(bytes)),
            #[cfg(all(feature = "metal", target_os = "macos"))]
            EncodedDicomJ2kCodestream::Metal(encoded) => encoded
                .codestream_bytes()
                .map(Cow::Owned)
                .map_err(|err| Error::Encode {
                    message: format!("JPEG 2000 Metal encoded buffer read failed: {err}"),
                }),
        }
    }

    pub(crate) fn into_codestream(self) -> Result<Vec<u8>, Error> {
        match self.codestream {
            EncodedDicomJ2kCodestream::Host(bytes) => Ok(bytes),
            #[cfg(all(feature = "metal", target_os = "macos"))]
            EncodedDicomJ2kCodestream::Metal(encoded) => {
                encoded.codestream_bytes().map_err(|err| Error::Encode {
                    message: format!("JPEG 2000 Metal encoded buffer read failed: {err}"),
                })
            }
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
    ) -> Result<EncodedDicomJ2kFrame, Error> {
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
            None if self.preference.requires_device() => Err(Error::Unsupported {
                reason: "requested JPEG 2000 device encode backend is unavailable or unsupported"
                    .into(),
            }),
            None => encode_lossless_cpu(
                samples,
                self.transfer_syntax,
                self.codec_validation,
                self.j2k_decomposition_levels,
                self.reversible_transform,
            ),
        }
    }

    pub(crate) fn cpu_batch_settings(
        &self,
    ) -> Option<(
        TransferSyntax,
        CodecValidation,
        Option<u8>,
        ReversibleTransform,
    )> {
        self.preference.cpu_batch_safe().then_some((
            self.transfer_syntax,
            self.codec_validation,
            self.j2k_decomposition_levels,
            self.reversible_transform,
        ))
    }

    fn try_device(
        &mut self,
        samples: J2kLosslessSamples<'_>,
    ) -> Result<Option<EncodedDicomJ2kFrame>, Error> {
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
    ) -> Result<Option<EncodedDicomJ2kFrame>, Error> {
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
        let codec_validation_enabled = self.codec_validation == CodecValidation::RoundTrip;
        let mut validation_duration = Duration::ZERO;
        if codec_validation_enabled {
            if let Some(encoded) = &encoded {
                let validation_started = Instant::now();
                j2k_metal::validate_lossless_roundtrip_on_metal_with_session(
                    samples,
                    &encoded.codestream,
                    &session,
                )
                .map_err(|err| Error::Encode {
                    message: format!("JPEG 2000 Metal validation failed: {err}"),
                })?;
                validation_duration = validation_started.elapsed();
            }
        }
        Ok(encoded.map(|codestream| EncodedDicomJ2kFrame {
            codestream: EncodedDicomJ2kCodestream::Host(codestream.codestream),
            used_device_encode: codestream.used_device_encode,
            used_device_validation: codec_validation_enabled,
            encode_duration,
            gpu_encode_wall_duration: codestream.used_device_encode.then_some(encode_duration),
            device_gpu_duration: None,
            validation_duration,
        }))
    }

    #[cfg(not(all(feature = "metal", target_os = "macos")))]
    fn try_metal(
        &mut self,
        _samples: J2kLosslessSamples<'_>,
    ) -> Result<Option<EncodedDicomJ2kFrame>, Error> {
        Ok(None)
    }

    #[cfg(feature = "cuda")]
    fn try_cuda(
        &mut self,
        samples: J2kLosslessSamples<'_>,
    ) -> Result<Option<EncodedDicomJ2kFrame>, Error> {
        let accelerator = self.cuda.get_or_insert_with(Default::default);
        let started = Instant::now();
        let encoded = encode_j2k_lossless_with_device_accelerator(
            samples,
            self.transfer_syntax,
            BackendKind::Cuda,
            accelerator,
            self.codec_validation.to_j2k_validation(),
            self.j2k_decomposition_levels,
            self.reversible_transform,
        )?;
        let encode_duration = started.elapsed();
        Ok(encoded.map(|codestream| EncodedDicomJ2kFrame {
            codestream: EncodedDicomJ2kCodestream::Host(codestream.codestream),
            used_device_encode: codestream.used_device_encode,
            used_device_validation: false,
            encode_duration,
            gpu_encode_wall_duration: codestream.used_device_encode.then_some(encode_duration),
            device_gpu_duration: None,
            validation_duration: Duration::ZERO,
        }))
    }

    #[cfg(not(feature = "cuda"))]
    fn try_cuda(
        &mut self,
        _samples: J2kLosslessSamples<'_>,
    ) -> Result<Option<EncodedDicomJ2kFrame>, Error> {
        Ok(None)
    }
}

#[cfg(test)]
pub(crate) fn encode_dicom_j2k_lossless(
    samples: J2kLosslessSamples<'_>,
    preference: EncodeBackendPreference,
) -> Result<Vec<u8>, Error> {
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
) -> Result<Vec<u8>, Error> {
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
) -> Result<EncodedDicomJ2kFrame, Error> {
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
            gpu_encode_wall_duration: None,
            device_gpu_duration: None,
            validation_duration: Duration::ZERO,
        }
    })
    .map_err(|source| Error::Encode {
        message: source.to_string(),
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(crate) fn metal_tile_is_padded_contiguous(
    tile: &wsi_rs::output::metal::MetalDeviceTile,
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
    j2k_decomposition_levels: Option<u8>,
    reversible_transform: ReversibleTransform,
) -> Result<Option<DeviceEncodedCodestream>, Error> {
    let before_dispatch = accelerator.dispatch_report();
    let options = lossless_encode_options(
        transfer_syntax,
        EncodeBackendPreference::PreferDevice,
        CodecValidation::RoundTrip,
        j2k_decomposition_levels,
        reversible_transform,
    )?;
    let options = J2kLosslessEncodeOptions::new(
        options.backend,
        options.block_coding_mode,
        options.progression,
        options.max_decomposition_levels,
        options.reversible_transform,
        validation,
    );
    let encoded = encode_j2k_lossless_with_accelerator(samples, &options, backend, accelerator)
        .map_err(|err| Error::Encode {
            message: format!("JPEG 2000 device encode failed: {err}"),
        })?;
    let dispatch = accelerator
        .dispatch_report()
        .saturating_delta(before_dispatch);
    let used_device_encode = encoded.backend == backend || dispatch.any();
    Ok(used_device_encode.then_some(DeviceEncodedCodestream {
        codestream: encoded.codestream,
        used_device_encode,
    }))
}

fn lossless_encode_options(
    transfer_syntax: TransferSyntax,
    backend: EncodeBackendPreference,
    codec_validation: CodecValidation,
    j2k_decomposition_levels: Option<u8>,
    reversible_transform: ReversibleTransform,
) -> Result<J2kLosslessEncodeOptions, Error> {
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
        | TransferSyntax::Htj2k
        | TransferSyntax::ExplicitVrLittleEndian => {
            return Err(Error::Unsupported {
                reason: "transfer syntax is not implemented for lossless JPEG 2000 export".into(),
            });
        }
    };

    Ok(J2kLosslessEncodeOptions::new(
        backend.to_j2k(),
        block_coding_mode,
        progression,
        j2k_decomposition_levels,
        reversible_transform,
        codec_validation.to_j2k_validation(),
    ))
}

#[cfg(test)]
pub(crate) fn dicom_j2k_decomposition_levels(samples: J2kLosslessSamples<'_>) -> u8 {
    j2k::j2k_lossless_decomposition_levels(samples)
}

#[cfg(test)]
mod cpu_tests {
    use super::{encode_j2k_lossless_with_device_accelerator, DicomJ2kEncoder};
    use crate::{CodecValidation, EncodeBackendPreference, TransferSyntax};
    use j2k::{
        J2kEncodeDispatchReport, J2kEncodeStageAccelerator, J2kEncodeStageResult,
        J2kForwardDwt53Job, J2kForwardDwt53Output,
    };
    use j2k::{J2kEncodeValidation, J2kLosslessSamples, ReversibleTransform};
    use j2k_core::BackendKind;

    #[derive(Default)]
    struct DwtOnlyAccelerator {
        dispatch: J2kEncodeDispatchReport,
    }

    impl J2kEncodeStageAccelerator for DwtOnlyAccelerator {
        fn dispatch_report(&self) -> J2kEncodeDispatchReport {
            self.dispatch
        }

        fn encode_forward_dwt53(
            &mut self,
            _job: J2kForwardDwt53Job<'_>,
        ) -> J2kEncodeStageResult<Option<J2kForwardDwt53Output>> {
            self.dispatch.forward_dwt53 = self.dispatch.forward_dwt53.saturating_add(1);
            Ok(None)
        }
    }

    #[test]
    fn auto_j2k_encoder_exposes_cpu_batch_settings_for_cpu_fallback() {
        let encoder = DicomJ2kEncoder::new(
            EncodeBackendPreference::Auto,
            TransferSyntax::Htj2kLosslessRpcl,
            CodecValidation::Disabled,
        );

        if cfg!(any(feature = "metal", feature = "cuda")) {
            assert!(encoder.cpu_batch_settings().is_none());
        } else {
            assert!(encoder.cpu_batch_settings().is_some());
        }

        let cpu = DicomJ2kEncoder::new(
            EncodeBackendPreference::CpuOnly,
            TransferSyntax::Htj2kLosslessRpcl,
            CodecValidation::Disabled,
        );
        assert!(cpu.cpu_batch_settings().is_some());

        let prefer_device = DicomJ2kEncoder::new(
            EncodeBackendPreference::PreferDevice,
            TransferSyntax::Htj2kLosslessRpcl,
            CodecValidation::Disabled,
        );
        assert!(prefer_device.cpu_batch_settings().is_none());

        let require_device = DicomJ2kEncoder::new(
            EncodeBackendPreference::RequireDevice,
            TransferSyntax::Htj2kLosslessRpcl,
            CodecValidation::Disabled,
        );
        assert!(require_device.cpu_batch_settings().is_none());
    }

    #[test]
    fn partial_device_dispatch_counts_as_device_used_lossless_encode() {
        let pixels = vec![7u8; 128 * 128];
        let samples =
            J2kLosslessSamples::new(&pixels, 128, 128, 1, 8, false).expect("valid samples");
        let mut accelerator = DwtOnlyAccelerator::default();

        let encoded = encode_j2k_lossless_with_device_accelerator(
            samples,
            TransferSyntax::Htj2kLosslessRpcl,
            BackendKind::Cuda,
            &mut accelerator,
            J2kEncodeValidation::CpuRoundTrip,
            None,
            ReversibleTransform::Rct53,
        )
        .expect("partial device encode should still produce a codestream")
        .expect("DWT dispatch should count as device-used encode");

        assert!(encoded.used_device_encode);
        assert!(!encoded.codestream.is_empty());
        assert!(accelerator.dispatch_report().forward_dwt53 > 0);
    }
}
