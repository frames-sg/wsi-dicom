use std::time::Duration;

use j2k_transcode::accelerator::{DctGridToDwt97Job, DctToWaveletStageAccelerator};
use j2k_transcode::dct97_2d::{
    dct8x8_blocks_then_dwt97_float_with_scratch, Dct97GridScratch, Dwt97TwoDimensional,
};
use j2k_transcode::{
    EncodeProgressionOrder, JpegTileBatchInput, JpegToHtj2kOptions, JpegToHtj2kTranscoder,
};
use rayon::prelude::*;

use super::{
    ensure_consistent_pixel_profile, pixel_profile_from_raw_jpeg_tile,
    raw_jpeg_matches_frame_geometry, Error, ExportMetrics, PixelProfile, RawCompressedTile,
    TransferSyntax,
};
use crate::{EncodeBackendPreference, JpegDirectHtj2kProfile};

pub(super) struct BatchOutcome {
    pub(super) codestream: Vec<u8>,
    pub(super) profile: PixelProfile,
    timings: Option<j2k_transcode::TranscodeTimingReport>,
    transcode_micros: u128,
}

#[derive(Debug, Default)]
struct RayonDwt97BatchAccelerator;

impl DctToWaveletStageAccelerator for RayonDwt97BatchAccelerator {
    fn supports_dwt97_batch(&self) -> bool {
        true
    }

    fn dct_grid_to_dwt97_batch(
        &mut self,
        jobs: &[DctGridToDwt97Job<'_>],
    ) -> Result<Option<Vec<Dwt97TwoDimensional<f64>>>, &'static str> {
        jobs.par_iter()
            .map(|job| {
                let mut scratch = Dct97GridScratch::default();
                dct8x8_blocks_then_dwt97_float_with_scratch(
                    job.blocks,
                    job.block_cols,
                    job.block_rows,
                    job.width,
                    job.height,
                    &mut scratch,
                )
                .map_err(|_| "CPU 9/7 batch transform failed")
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
    }
}

#[derive(Clone)]
pub(super) struct Frame {
    pub(super) data: Vec<u8>,
    pub(super) profile: PixelProfile,
}

pub(super) fn transfer_syntax(transfer_syntax: TransferSyntax) -> bool {
    matches!(
        transfer_syntax,
        TransferSyntax::Htj2k | TransferSyntax::Htj2kLossless | TransferSyntax::Htj2kLosslessRpcl
    )
}

pub(super) fn generated_candidate(
    _transfer_syntax: TransferSyntax,
    _row_has_jpeg_source: bool,
    _direct_jpeg_ok: bool,
    _source_jpeg_direct_rejected: bool,
    _source_raw_probe_failed: bool,
    _has_passthrough: bool,
) -> bool {
    false
}

pub(super) fn frame(
    raw: &RawCompressedTile,
    frame_columns: u32,
    frame_rows: u32,
    transfer_syntax: TransferSyntax,
) -> Option<Frame> {
    if !self::transfer_syntax(transfer_syntax)
        || !raw_jpeg_matches_frame_geometry(raw, frame_columns, frame_rows)
    {
        return None;
    }
    let profile = htj2k_direct_pixel_profile(pixel_profile_from_raw_jpeg_tile(raw).ok()?);
    if profile.photometric_interpretation == "YBR_FULL" {
        // VL Whole Slide Microscopy does not admit YBR_FULL for HTJ2K
        // transfer syntaxes. Fall back through decoded RGB so the writer emits
        // a conformant YBR_RCT profile instead of preserving JPEG chroma
        // subsampling in the HTJ2K codestream.
        return None;
    }
    Some(Frame {
        data: raw.data.clone(),
        profile,
    })
}

fn htj2k_direct_pixel_profile(profile: PixelProfile) -> PixelProfile {
    if profile.components == 3 && profile.photometric_interpretation == "YBR_FULL_422" {
        PixelProfile {
            photometric_interpretation: "YBR_FULL",
            ..profile
        }
    } else {
        profile
    }
}

pub(super) fn record_success(
    metrics: &mut ExportMetrics,
    pixel_profile: &mut Option<PixelProfile>,
    direct: &BatchOutcome,
    profile: JpegDirectHtj2kProfile,
    mismatch_reason: &'static str,
) -> Result<(), Error> {
    ensure_consistent_pixel_profile(pixel_profile, direct.profile, mismatch_reason)?;
    metrics.record_pixel_profile(direct.profile);
    if profile == JpegDirectHtj2kProfile::Lossless53 {
        metrics.record_jpeg_direct_htj2k_53_frame(direct.transcode_micros);
    } else {
        metrics.record_jpeg_direct_htj2k_97_frame(direct.transcode_micros);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn record_route_success(
    metrics: &mut ExportMetrics,
    pixel_profile: &mut Option<PixelProfile>,
    direct: &BatchOutcome,
    profile: JpegDirectHtj2kProfile,
    source_jpeg_retiled: bool,
    source_jpeg_retile_duration: Duration,
    mismatch_reason: &'static str,
) -> Result<(), Error> {
    record_success(metrics, pixel_profile, direct, profile, mismatch_reason)?;
    if let Some(timings) = direct.timings {
        metrics.record_jpeg_direct_htj2k_timings(timings);
    }
    if source_jpeg_retiled && profile == JpegDirectHtj2kProfile::Lossless53 {
        metrics.record_jpeg_retile_to_htj2k_53_frame(source_jpeg_retile_duration);
    }
    Ok(())
}

pub(super) fn encode_planned_batch_with_encoder(
    planned: &[super::LosslessJ2kPlannedFrame],
    encoder: &mut BatchEncoder,
) -> Result<Vec<Option<Result<BatchOutcome, Error>>>, Error> {
    let indices = planned
        .iter()
        .enumerate()
        .filter_map(|(idx, planned_frame)| planned_frame.source_jpeg.is_some().then_some(idx))
        .collect::<Vec<_>>();
    let mut outcomes = (0..planned.len()).map(|_| None).collect::<Vec<_>>();
    if indices.is_empty() {
        return Ok(outcomes);
    }

    let frames = indices
        .iter()
        .map(|&idx| {
            planned[idx]
                .source_jpeg
                .as_ref()
                .ok_or_else(|| Error::Encode {
                    message: "direct JPEG route missing source JPEG frame".into(),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let batch = encoder.encode_frame_refs_batch(&frames)?;

    for (input_idx, encoded) in batch.into_iter().enumerate() {
        let planned_idx = indices[input_idx];
        outcomes[planned_idx] = Some(encoded);
    }

    Ok(outcomes)
}

#[cfg_attr(
    not(all(test, feature = "metal", target_os = "macos")),
    allow(dead_code)
)]
pub(super) fn encode_frames_batch(
    frames: &[Frame],
    transfer_syntax: TransferSyntax,
    profile: JpegDirectHtj2kProfile,
    backend: EncodeBackendPreference,
) -> Result<Vec<Result<BatchOutcome, Error>>, Error> {
    let mut encoder = BatchEncoder::new(transfer_syntax, profile, backend)?;
    encode_frames_batch_with_encoder(frames, &mut encoder)
}

pub(super) fn encode_frames_batch_with_encoder(
    frames: &[Frame],
    encoder: &mut BatchEncoder,
) -> Result<Vec<Result<BatchOutcome, Error>>, Error> {
    let frame_refs = frames.iter().collect::<Vec<_>>();
    encoder.encode_frame_refs_batch(&frame_refs)
}

pub(super) struct BatchEncoder {
    backend: EncodeBackendPreference,
    options: JpegToHtj2kOptions,
    transcoder: JpegToHtj2kTranscoder,
    #[cfg(all(feature = "metal", target_os = "macos"))]
    metal_accelerator: Option<j2k_transcode_metal::MetalDctToWaveletStageAccelerator>,
    #[cfg(all(feature = "metal", target_os = "macos"))]
    metal_encode_accelerator: Option<j2k_metal::MetalEncodeStageAccelerator>,
}

impl BatchEncoder {
    pub(super) fn new(
        transfer_syntax: TransferSyntax,
        profile: JpegDirectHtj2kProfile,
        backend: EncodeBackendPreference,
    ) -> Result<Self, Error> {
        Ok(Self {
            backend,
            options: options_for_profile(transfer_syntax, profile)?,
            transcoder: JpegToHtj2kTranscoder::default(),
            #[cfg(all(feature = "metal", target_os = "macos"))]
            metal_accelerator: None,
            #[cfg(all(feature = "metal", target_os = "macos"))]
            metal_encode_accelerator: None,
        })
    }

    fn encode_frame_refs_batch(
        &mut self,
        frames: &[&Frame],
    ) -> Result<Vec<Result<BatchOutcome, Error>>, Error> {
        if frames.is_empty() {
            return Ok(Vec::new());
        }
        let inputs = frames
            .iter()
            .map(|frame| JpegTileBatchInput {
                bytes: frame.data.as_slice(),
            })
            .collect::<Vec<_>>();
        let batch = self.transcode_batch(&inputs)?;
        let batch_timings = batch.report.timings;
        let mut timing_recorded = false;
        let transform_share = if batch.report.successful_tiles > 0 {
            batch.report.transform_us / batch.report.successful_tiles as u128
        } else {
            0
        };

        Ok(batch
            .tiles
            .into_iter()
            .zip(frames.iter())
            .map(|(encoded, frame)| {
                encoded.map_or_else(
                    |err| Err(to_wsi_error(err)),
                    |encoded| {
                        let timings = (!timing_recorded).then_some(batch_timings);
                        timing_recorded = true;
                        let transcode_micros = encoded
                            .report
                            .extract_us
                            .saturating_add(encoded.report.transform_us)
                            .saturating_add(transform_share)
                            .saturating_add(encoded.report.encode_us);
                        Ok(BatchOutcome {
                            codestream: encoded.codestream,
                            profile: htj2k_direct_pixel_profile(frame.profile),
                            timings,
                            transcode_micros,
                        })
                    },
                )
            })
            .collect())
    }

    fn transcode_batch(
        &mut self,
        inputs: &[JpegTileBatchInput<'_>],
    ) -> Result<j2k_transcode::EncodedTranscodeBatch, Error> {
        transcode_batch_with_encoder(self, inputs)
    }
}

fn transcode_batch_with_encoder(
    encoder: &mut BatchEncoder,
    inputs: &[JpegTileBatchInput<'_>],
) -> Result<j2k_transcode::EncodedTranscodeBatch, Error> {
    let is_float_97 = encoder.options.coefficient_path
        == j2k_transcode::JpegToHtj2kCoefficientPath::FloatDirectLinear97;
    if encoder.backend == EncodeBackendPreference::CpuOnly {
        if is_float_97 {
            let mut accelerator = RayonDwt97BatchAccelerator;
            return encoder
                .transcoder
                .transcode_batch_with_accelerator(inputs, &encoder.options, &mut accelerator)
                .map_err(to_wsi_error);
        }
        return encoder
            .transcoder
            .transcode_batch(inputs, &encoder.options)
            .map_err(to_wsi_error);
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    {
        let accelerator = encoder
            .metal_accelerator
            .get_or_insert_with(|| match encoder.backend {
                EncodeBackendPreference::RequireDevice => {
                    j2k_transcode_metal::MetalDctToWaveletStageAccelerator::new_explicit()
                }
                EncodeBackendPreference::Auto | EncodeBackendPreference::PreferDevice => {
                    j2k_transcode_metal::MetalDctToWaveletStageAccelerator::for_auto()
                }
                EncodeBackendPreference::CpuOnly => {
                    unreachable!("CpuOnly backend returned before Metal accelerator setup")
                }
            });
        let encode_accelerator = encoder
            .metal_encode_accelerator
            .get_or_insert_with(j2k_metal::MetalEncodeStageAccelerator::for_ht_code_block_encode);
        encoder
            .transcoder
            .transcode_batch_with_accelerators(
                inputs,
                &encoder.options,
                accelerator,
                encode_accelerator,
            )
            .map_err(to_wsi_error)
    }

    #[cfg(not(all(feature = "metal", target_os = "macos")))]
    {
        if encoder.backend == EncodeBackendPreference::RequireDevice {
            return Err(Error::Unsupported {
                reason:
                    "direct JPEG to HTJ2K device acceleration requires the metal feature on macOS"
                        .into(),
            });
        }
        encoder
            .transcoder
            .transcode_batch(inputs, &encoder.options)
            .map_err(to_wsi_error)
    }
}

fn options_for_profile(
    transfer_syntax: TransferSyntax,
    profile: JpegDirectHtj2kProfile,
) -> Result<JpegToHtj2kOptions, Error> {
    match profile {
        JpegDirectHtj2kProfile::Lossless53 => options_53(transfer_syntax),
        JpegDirectHtj2kProfile::Lossy97
        | JpegDirectHtj2kProfile::Lossy97Near
        | JpegDirectHtj2kProfile::Lossy97Balanced
        | JpegDirectHtj2kProfile::Lossy97Aggressive
        | JpegDirectHtj2kProfile::Lossy97Preview
        | JpegDirectHtj2kProfile::Lossy97Thumbnail => options_97(transfer_syntax, profile),
    }
}

fn options_53(transfer_syntax: TransferSyntax) -> Result<JpegToHtj2kOptions, Error> {
    if !self::transfer_syntax(transfer_syntax) {
        return Err(Error::Unsupported {
            reason: "direct JPEG to HTJ2K requires an HTJ2K lossless transfer syntax".into(),
        });
    }
    if !matches!(
        transfer_syntax,
        TransferSyntax::Htj2kLossless | TransferSyntax::Htj2kLosslessRpcl
    ) {
        return Err(Error::Unsupported {
            reason: "direct JPEG to HTJ2K 5/3 requires an HTJ2K lossless transfer syntax".into(),
        });
    }
    let mut options = JpegToHtj2kOptions::lossless_53();
    if transfer_syntax == TransferSyntax::Htj2kLosslessRpcl {
        options.encode_options.progression_order = EncodeProgressionOrder::Rpcl;
        options.encode_options.write_tlm = true;
    }
    Ok(options)
}

fn options_97(
    transfer_syntax: TransferSyntax,
    profile: JpegDirectHtj2kProfile,
) -> Result<JpegToHtj2kOptions, Error> {
    if transfer_syntax != TransferSyntax::Htj2k {
        return Err(Error::Unsupported {
            reason: "direct JPEG to HTJ2K 9/7 requires the general HTJ2K transfer syntax".into(),
        });
    }
    let mut options = JpegToHtj2kOptions::lossy_97();
    options.encode_options.irreversible_quantization_scale = profile
        .irreversible_quantization_scale()
        .ok_or_else(|| Error::Unsupported {
            reason: "direct JPEG to HTJ2K 9/7 requires an irreversible 9/7 profile".into(),
        })?;
    Ok(options)
}

fn to_wsi_error(source: j2k_transcode::JpegToHtj2kError) -> Error {
    Error::Encode {
        message: format!("direct JPEG to HTJ2K transcode failed: {source}"),
    }
}

#[cfg(test)]
mod tests {
    use dicom_dictionary_std::tags;

    use crate::api::Export;
    use crate::metadata::MetadataSource;
    use crate::options::{
        CodecValidation, EncodeBackendPreference, ExportOptions, JpegDirectHtj2kProfile,
    };
    use crate::request::{ExportRequest, RouteProfileRequest};
    use crate::test_support::{
        assert_htj2k_rpcl_codestream, dicom_fragment_payload_without_padding, encode_test_jpeg,
        write_tiled_jpeg_tiff,
    };

    use super::super::{export_dicom, profile_dicom_routes};
    use super::*;

    #[test]
    fn options_for_97_profiles_set_expected_quantization_scales() {
        for (profile, scale) in [
            (JpegDirectHtj2kProfile::Lossy97Near, 2.0_f32),
            (JpegDirectHtj2kProfile::Lossy97, 5.0_f32),
            (JpegDirectHtj2kProfile::Lossy97Balanced, 5.0_f32),
            (JpegDirectHtj2kProfile::Lossy97Aggressive, 10.0_f32),
            (JpegDirectHtj2kProfile::Lossy97Preview, 20.0_f32),
            (JpegDirectHtj2kProfile::Lossy97Thumbnail, 50.0_f32),
        ] {
            let options = options_for_profile(TransferSyntax::Htj2k, profile).unwrap();
            assert_eq!(
                options
                    .encode_options
                    .irreversible_quantization_scale
                    .to_bits(),
                scale.to_bits()
            );
        }
    }

    #[test]
    fn export_htj2k_from_native_jpeg_tiles_falls_back_to_conformant_ybr_rct() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.svs");
        let out = tmp.path().join("out");
        let jpeg_a = encode_test_jpeg(8, 8, [160, 20, 40]);
        let jpeg_b = encode_test_jpeg(8, 8, [20, 160, 40]);
        write_tiled_jpeg_tiff(&source, 16, 8, 8, 8, &[jpeg_a, jpeg_b]);

        let report = export_dicom(ExportRequest {
            source_path: source,
            output_dir: out,
            options: ExportOptions {
                tile_size: 8,
                transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..ExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();

        assert_eq!(report.instances.len(), 1);
        assert_eq!(report.instances[0].frame_count, 2);
        assert_eq!(report.metrics.routes.total_frames, 2);
        assert_eq!(
            report.metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_53_frames,
            0
        );
        assert_eq!(
            report.metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_97_frames,
            0
        );
        assert_eq!(
            report
                .metrics
                .jpeg_direct_htj2k
                .jpeg_direct_htj2k_rejected_frames,
            2
        );
        assert_eq!(report.metrics.routes.cpu_input_frames, 2);
        assert_eq!(report.metrics.routes.jpeg_decode_fallback_frames, 0);
        assert_eq!(report.metrics.routes.cpu_fallback_frames, 2);
        assert_eq!(report.metrics.route_unclassified_frames(), 0);
        assert!(report.metrics.timings.encode_micros > 0);

        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        assert_eq!(
            object.meta().transfer_syntax.trim_end_matches('\0'),
            TransferSyntax::Htj2kLosslessRpcl.uid()
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
        assert_eq!(fragments.len(), 2);
        assert_htj2k_rpcl_codestream(dicom_fragment_payload_without_padding(&fragments[0]));
    }

    #[test]
    fn export_general_htj2k_from_native_jpeg_tiles_rejects_nonconformant_direct_97() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.svs");
        let out = tmp.path().join("out");
        let jpeg_a = encode_test_jpeg(8, 8, [160, 20, 40]);
        let jpeg_b = encode_test_jpeg(8, 8, [20, 160, 40]);
        write_tiled_jpeg_tiff(&source, 16, 8, 8, 8, &[jpeg_a, jpeg_b]);

        let err = export_dicom(ExportRequest {
            source_path: source,
            output_dir: out,
            options: ExportOptions {
                tile_size: 8,
                transfer_syntax: TransferSyntax::Htj2k,
                jpeg_direct_htj2k_profile: JpegDirectHtj2kProfile::Lossy97,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..ExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .expect_err("YBR_FULL direct HTJ2K should not be emitted for VL WSI");

        assert!(err.to_string().contains("HTJ2K 9/7 export"));
    }

    #[test]
    fn source_aware_jpeg_backed_export_defaults_to_jpeg_passthrough() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.svs");
        let out = tmp.path().join("out");
        let jpeg_a = encode_test_jpeg(8, 8, [160, 20, 40]);
        let jpeg_b = encode_test_jpeg(8, 8, [20, 160, 40]);
        write_tiled_jpeg_tiff(&source, 16, 8, 8, 8, &[jpeg_a, jpeg_b]);

        let report = Export::from_slide(&source)
            .to_directory(&out)
            .with_research_placeholder_metadata()
            .with_options(ExportOptions {
                tile_size: 8,
                transfer_syntax: TransferSyntax::JpegBaseline8Bit,
                encode_backend: EncodeBackendPreference::CpuOnly,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..ExportOptions::default()
            })
            .source_aware_transfer_syntax()
            .run()
            .unwrap();

        assert_eq!(report.metrics.routes.total_frames, 2);
        assert_eq!(
            report.metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_53_frames,
            0
        );
        assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 2);
        assert_eq!(report.metrics.routes.jpeg_decode_fallback_frames, 0);
        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        assert_eq!(
            object.meta().transfer_syntax.trim_end_matches('\0'),
            TransferSyntax::JpegBaseline8Bit.uid()
        );
    }

    #[test]
    fn profile_dicom_routes_reports_conformant_fallback_for_jpeg_backed_htj2k() {
        let tmp = tempfile::tempdir().unwrap();
        let jpeg_a = encode_test_jpeg(8, 8, [160, 20, 40]);
        let jpeg_b = encode_test_jpeg(8, 8, [20, 160, 40]);
        let source = tmp.path().join("source.svs");
        write_tiled_jpeg_tiff(&source, 16, 8, 8, 8, &[jpeg_a, jpeg_b]);
        let encode_backend = if cfg!(all(feature = "metal", target_os = "macos")) {
            EncodeBackendPreference::RequireDevice
        } else {
            EncodeBackendPreference::CpuOnly
        };

        let report = profile_dicom_routes(RouteProfileRequest {
            source_path: source,
            options: ExportOptions {
                tile_size: 8,
                transfer_syntax: TransferSyntax::Htj2kLossless,
                encode_backend,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: false,
                ..ExportOptions::default()
            },
            source_aware_transfer_syntax: false,
            level: 0,
            max_frames: 2,
        })
        .unwrap();

        assert_eq!(report.level, 0);
        assert_eq!(report.requested_frames, 2);
        assert_eq!(report.metrics.routes.total_frames, 2);
        assert_eq!(
            report.metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_53_frames,
            0
        );
        assert_eq!(
            report.metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_97_frames,
            0
        );
        assert_eq!(
            report
                .metrics
                .jpeg_direct_htj2k
                .jpeg_direct_htj2k_rejected_frames,
            2
        );
        assert_eq!(report.metrics.routes.jpeg_decode_fallback_frames, 0);
        if cfg!(all(feature = "metal", target_os = "macos")) {
            assert_eq!(report.metrics.routes.cpu_input_frames, 2);
            assert_eq!(report.metrics.routes.gpu_input_decode_frames, 0);
            assert_eq!(report.metrics.routes.gpu_encode_frames, 2);
            assert_eq!(report.metrics.routes.gpu_transcode_frames, 2);
            assert_eq!(report.metrics.routes.resident_gpu_transcode_frames, 0);
            assert_eq!(report.metrics.routes.partial_gpu_transcode_frames, 2);
            assert_eq!(report.metrics.routes.cpu_fallback_frames, 0);
            assert!(report.metrics.gpu_encode.gpu_encode_wall_micros > 0);
        } else {
            assert_eq!(report.metrics.routes.cpu_input_frames, 2);
            assert_eq!(report.metrics.routes.gpu_input_decode_frames, 0);
            assert_eq!(report.metrics.routes.gpu_encode_frames, 0);
            assert_eq!(report.metrics.routes.gpu_transcode_frames, 0);
            assert_eq!(report.metrics.routes.cpu_fallback_frames, 2);
        }
        assert_eq!(report.metrics.route_unclassified_frames(), 0);
        assert!(report.metrics.timings.encode_micros > 0);
        assert!(report.elapsed_micros > 0);
    }

    #[test]
    #[cfg(all(feature = "metal", target_os = "macos"))]
    fn direct_97_require_device_uses_metal_ht_blocks_but_cpu_packetization() {
        let jpeg_a = encode_test_jpeg(8, 8, [160, 20, 40]);
        let jpeg_b = encode_test_jpeg(8, 8, [20, 160, 40]);
        let frames = vec![
            Frame {
                data: jpeg_a,
                profile: PixelProfile {
                    components: 3,
                    bits_allocated: 8,
                    photometric_interpretation: "YBR_FULL_422",
                },
            },
            Frame {
                data: jpeg_b,
                profile: PixelProfile {
                    components: 3,
                    bits_allocated: 8,
                    photometric_interpretation: "YBR_FULL_422",
                },
            },
        ];

        let batch = encode_frames_batch(
            &frames,
            TransferSyntax::Htj2k,
            JpegDirectHtj2kProfile::Lossy97,
            EncodeBackendPreference::RequireDevice,
        )
        .unwrap();

        assert_eq!(batch.len(), 2);
        let timings = batch[0]
            .as_ref()
            .expect("9/7 direct transcode should succeed")
            .timings
            .expect("first successful frame records batch timings");
        assert!(timings.accelerator_dispatches > 0);
        assert!(timings.htj2k_encode_ht_code_block_dispatches > 0);
        assert_eq!(timings.htj2k_encode_packetization_dispatches, 0);
    }
}
