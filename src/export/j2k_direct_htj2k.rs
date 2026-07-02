use std::time::Instant;

use j2k::{J2kProgressionOrder, J2kToHtj2kOptions};
use j2k_core::CompressedPayloadKind;
use rayon::prelude::*;

use super::{
    ensure_consistent_pixel_profile, CodecValidation, Compression, Error, ExportMetrics,
    PixelProfile, RawCompressedTile, TransferSyntax,
};

pub(super) struct BatchOutcome {
    pub(super) codestream: Vec<u8>,
    pub(super) profile: PixelProfile,
    transcode_micros: u128,
}

#[derive(Clone)]
pub(super) struct Frame {
    data: Vec<u8>,
    profile: PixelProfile,
}

pub(super) fn transfer_syntax(transfer_syntax: TransferSyntax) -> bool {
    matches!(
        transfer_syntax,
        TransferSyntax::Htj2kLossless | TransferSyntax::Htj2kLosslessRpcl
    )
}

pub(super) fn frame(
    raw: &RawCompressedTile,
    frame_columns: u32,
    frame_rows: u32,
    transfer_syntax: TransferSyntax,
    profile: Option<PixelProfile>,
) -> Option<Frame> {
    if !self::transfer_syntax(transfer_syntax)
        || raw.width() != frame_columns
        || raw.height() != frame_rows
        || !matches!(
            raw.compression(),
            Compression::Jp2kRgb | Compression::Jp2kYcbcr
        )
    {
        return None;
    }
    Some(Frame {
        data: raw.data().to_vec(),
        profile: profile?,
    })
}

pub(super) fn encode_planned_batch(
    planned: &[super::LosslessJ2kPlannedFrame],
    transfer_syntax: TransferSyntax,
    codec_validation: CodecValidation,
) -> Result<Vec<Option<Result<BatchOutcome, Error>>>, Error> {
    let indices = planned
        .iter()
        .enumerate()
        .filter_map(|(idx, planned_frame)| planned_frame.source_j2k.is_some().then_some(idx))
        .collect::<Vec<_>>();
    let mut outcomes = (0..planned.len()).map(|_| None).collect::<Vec<_>>();
    if indices.is_empty() {
        return Ok(outcomes);
    }

    let frames = indices
        .iter()
        .map(|&idx| {
            planned[idx]
                .source_j2k
                .as_ref()
                .ok_or_else(|| Error::Encode {
                    message: "direct J2K route missing source J2K frame".into(),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let batch = encode_frame_refs_batch(&frames, transfer_syntax, codec_validation)?;

    for (input_idx, encoded) in batch.into_iter().enumerate() {
        let planned_idx = indices[input_idx];
        outcomes[planned_idx] = Some(encoded);
    }

    Ok(outcomes)
}

pub(super) fn record_success(
    metrics: &mut ExportMetrics,
    pixel_profile: &mut Option<PixelProfile>,
    direct: &BatchOutcome,
    mismatch_reason: &'static str,
) -> Result<(), Error> {
    ensure_consistent_pixel_profile(pixel_profile, direct.profile, mismatch_reason)?;
    metrics.record_pixel_profile(direct.profile);
    metrics.record_j2k_direct_htj2k_frame(direct.transcode_micros);
    Ok(())
}

fn encode_frame_refs_batch(
    frames: &[&Frame],
    transfer_syntax: TransferSyntax,
    codec_validation: CodecValidation,
) -> Result<Vec<Result<BatchOutcome, Error>>, Error> {
    let options = options(transfer_syntax, codec_validation)?;
    Ok(frames
        .par_iter()
        .map(|frame| {
            let started = Instant::now();
            j2k::recode_j2k_to_htj2k_lossless(&frame.data, options).map_or_else(
                |err| Err(to_wsi_error(err)),
                |recoded| {
                    Ok(BatchOutcome {
                        codestream: recoded.bytes,
                        profile: frame.profile,
                        transcode_micros: started.elapsed().as_micros(),
                    })
                },
            )
        })
        .collect())
}

fn options(
    transfer_syntax: TransferSyntax,
    codec_validation: CodecValidation,
) -> Result<J2kToHtj2kOptions, Error> {
    if !self::transfer_syntax(transfer_syntax) {
        return Err(Error::Unsupported {
            reason: "direct J2K to HTJ2K recode requires an HTJ2K lossless transfer syntax".into(),
        });
    }
    let progression = if transfer_syntax == TransferSyntax::Htj2kLosslessRpcl {
        J2kProgressionOrder::Rpcl
    } else {
        J2kProgressionOrder::Lrcp
    };
    Ok(J2kToHtj2kOptions::new(
        CompressedPayloadKind::Jpeg2000Codestream,
        progression,
        codec_validation.to_j2k_validation(),
    ))
}

fn to_wsi_error(source: j2k::J2kError) -> Error {
    Error::Encode {
        message: format!("direct J2K to HTJ2K recode failed: {source}"),
    }
}

#[cfg(test)]
mod tests {
    use dicom_dictionary_std::tags;
    use j2k::J2kLosslessSamples;

    use crate::encode::encode_dicom_lossless;
    use crate::metadata::MetadataSource;
    use crate::options::{EncodeBackendPreference, ExportOptions};
    use crate::request::ExportRequest;
    use crate::test_support::{
        assert_htj2k_rpcl_codestream, dicom_fragment_payload_without_padding,
        write_tiled_jp2k_rgb_tiff,
    };

    use super::super::export_dicom;
    use super::*;

    #[test]
    fn export_htj2k_from_native_j2k_tiles_uses_direct_coefficient_recode() {
        let tmp = tempfile::tempdir().unwrap();
        let width = 64;
        let height = 64;
        let bytes: Vec<u8> = (0..width * height * 3)
            .map(|value| ((value * 37 + 11) & 0xFF) as u8)
            .collect();
        let samples =
            J2kLosslessSamples::new(&bytes, width, height, 3, 8, false).expect("valid samples");
        let codestream = encode_dicom_lossless(
            samples,
            TransferSyntax::Jpeg2000Lossless,
            EncodeBackendPreference::CpuOnly,
            CodecValidation::Disabled,
        )
        .unwrap();
        let source = tmp.path().join("source.svs");
        write_tiled_jp2k_rgb_tiff(
            &source,
            width,
            height,
            width,
            height,
            std::slice::from_ref(&codestream),
        );

        let report = export_dicom(ExportRequest {
            source_path: source,
            output_dir: tmp.path().join("out"),
            options: ExportOptions {
                tile_size: width,
                transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
                encode_backend: EncodeBackendPreference::RequireDevice,
                codec_validation: CodecValidation::Disabled,
                source_device_decode: true,
                ..ExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: None,
        })
        .unwrap();

        assert_eq!(report.metrics.routes.total_frames, 1);
        assert_eq!(report.metrics.routes.j2k_direct_htj2k_frames, 1);
        assert_eq!(report.metrics.routes.j2k_passthrough_frames, 0);
        assert_eq!(report.metrics.routes.cpu_input_frames, 0);
        assert_eq!(report.metrics.routes.gpu_input_decode_frames, 0);
        assert_eq!(report.metrics.routes.gpu_encode_frames, 0);
        assert_eq!(report.metrics.routes.cpu_fallback_frames, 0);

        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        assert_eq!(
            object.meta().transfer_syntax.trim_end_matches('\0'),
            TransferSyntax::Htj2kLosslessRpcl.uid()
        );
        let fragments = object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .fragments()
            .unwrap();
        assert_eq!(fragments.len(), 1);
        let payload = dicom_fragment_payload_without_padding(&fragments[0]);
        assert_ne!(payload, codestream);
        assert_htj2k_rpcl_codestream(payload);
        assert_eq!(
            decode_j2k_frame_for_test(payload, width, height, 3, 8),
            bytes
        );
    }

    fn decode_j2k_frame_for_test(
        codestream: &[u8],
        width: u32,
        height: u32,
        components: u8,
        bits_allocated: u16,
    ) -> Vec<u8> {
        let fmt = match (components, bits_allocated) {
            (1, 8) => j2k::PixelFormat::Gray8,
            (3, 8) => j2k::PixelFormat::Rgb8,
            (1, 16) => j2k::PixelFormat::Gray16,
            (3, 16) => j2k::PixelFormat::Rgb16,
            other => panic!("unsupported frame profile: {other:?}"),
        };
        let bytes_per_sample = if bits_allocated <= 8 { 1usize } else { 2usize };
        let stride = width as usize * components as usize * bytes_per_sample;
        let mut decoder = j2k::J2kDecoder::new(codestream).unwrap();
        let mut decoded = vec![0; stride * height as usize];
        decoder.decode_into(&mut decoded, stride, fmt).unwrap();
        decoded
    }
}
