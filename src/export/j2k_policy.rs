use j2k::{J2kView, ReversibleTransform};
use j2k_core::{Colorspace, CompressedPayloadKind, PassthroughRequirements};
use wsi_rs::{Compression, EncodedTilePhotometricInterpretation, RawCompressedTile};

use super::{J2kPassthroughFrame, LosslessJ2kPlannedFrame};
use crate::error::Error;
use crate::options::TransferSyntax;
use crate::passthrough::j2k_codestream_is_rpcl;
use crate::routing::{j2k_encoded_lossless_profile, required_passthrough_syntax};
use crate::tile::PixelProfile;

fn j2k_edge_fallback_allowed(
    planned: &LosslessJ2kPlannedFrame,
    transfer_syntax: TransferSyntax,
    tile_size: u32,
) -> bool {
    transfer_syntax == TransferSyntax::Jpeg2000
        && planned.source_j2k_syntax.is_some()
        && planned.source_j2k_dimensions == Some((planned.width, planned.height))
        && (planned.width < tile_size || planned.height < tile_size)
}

pub(super) fn j2k_non_passthrough_encode_allowed(
    planned: &LosslessJ2kPlannedFrame,
    transfer_syntax: TransferSyntax,
    tile_size: u32,
) -> bool {
    if transfer_syntax == TransferSyntax::Htj2k {
        return false;
    }
    planned.passthrough.is_none()
        && (!transfer_syntax.is_jpeg2000_passthrough_only()
            || j2k_edge_fallback_allowed(planned, transfer_syntax, tile_size))
}

pub(super) fn lossless_j2k_cpu_fallback_indices(
    planned: &[LosslessJ2kPlannedFrame],
    transfer_syntax: TransferSyntax,
    tile_size: u32,
    mut frame_already_encoded: impl FnMut(usize) -> bool,
) -> Vec<usize> {
    planned
        .iter()
        .enumerate()
        .filter_map(|(idx, planned_frame)| {
            (j2k_non_passthrough_encode_allowed(planned_frame, transfer_syntax, tile_size)
                && !frame_already_encoded(idx))
            .then_some(idx)
        })
        .collect()
}

pub(super) fn j2k_fallback_profile(
    planned: &LosslessJ2kPlannedFrame,
    encoded_profile: PixelProfile,
    transfer_syntax: TransferSyntax,
) -> PixelProfile {
    if transfer_syntax == TransferSyntax::Jpeg2000 {
        if let Some(source_profile) = j2k_lossless_fallback_source_profile(planned, encoded_profile)
        {
            return source_profile;
        }
    }
    let profile = encoded_profile;
    j2k_encoded_lossless_profile(profile, transfer_syntax)
}

pub(super) fn j2k_fallback_reversible_transform(
    planned: &LosslessJ2kPlannedFrame,
    transfer_syntax: TransferSyntax,
) -> ReversibleTransform {
    if transfer_syntax == TransferSyntax::Jpeg2000
        && planned.source_j2k_profile.is_some_and(|profile| {
            profile.components == 3 && profile.photometric_interpretation == "RGB"
        })
    {
        ReversibleTransform::None53
    } else {
        ReversibleTransform::Rct53
    }
}

fn j2k_lossless_fallback_source_profile(
    planned: &LosslessJ2kPlannedFrame,
    encoded_profile: PixelProfile,
) -> Option<PixelProfile> {
    planned.source_j2k_profile.filter(|source_profile| {
        source_profile.components == encoded_profile.components
            && source_profile.bits_allocated == encoded_profile.bits_allocated
            && matches!(source_profile.photometric_interpretation, "RGB" | "YBR_RCT")
    })
}

pub(super) fn reject_lossy_j2k_lossless_fallback(
    planned: &LosslessJ2kPlannedFrame,
    transfer_syntax: TransferSyntax,
    row: u64,
) -> Result<(), Error> {
    if transfer_syntax == TransferSyntax::Jpeg2000Lossless
        && planned
            .source_j2k_syntax
            .is_some_and(|syntax| !syntax.is_lossless())
    {
        return Err(Error::Unsupported {
            reason: format!(
                "JPEG 2000 Lossless export cannot losslessly fall back from lossy source frame row={} col={}",
                row, planned.col
            ),
        });
    }
    Ok(())
}

pub(super) fn j2k_passthrough_frame(
    raw: RawCompressedTile,
    frame_columns: u32,
    frame_rows: u32,
    transfer_syntax: TransferSyntax,
) -> Result<Option<J2kPassthroughFrame>, Error> {
    if raw.width() != frame_columns || raw.height() != frame_rows {
        return Ok(None);
    }
    if !matches!(
        raw.compression(),
        Compression::Jp2kRgb | Compression::Jp2kYcbcr
    ) {
        return Ok(None);
    }
    if raw.bits_allocated() > u8::MAX as u16 || raw.samples_per_pixel() > u8::MAX as u16 {
        return Ok(None);
    }
    let (passthrough_syntax, photometric_interpretation) = {
        let view = match J2kView::parse(raw.data()) {
            Ok(view) => view,
            Err(_) => return Ok(None),
        };
        if transfer_syntax == TransferSyntax::Htj2kLosslessRpcl
            && !j2k_codestream_is_rpcl(raw.data())
        {
            return Ok(None);
        }
        let Some(candidate) = view.passthrough_candidate() else {
            return Ok(None);
        };
        let candidate_syntax = candidate.transfer_syntax();
        let Some(source_syntax) = required_passthrough_syntax(transfer_syntax, candidate_syntax)
        else {
            return Ok(None);
        };
        let Some(photometric_interpretation) = j2k_passthrough_photometric_interpretation(
            raw.photometric_interpretation(),
            view.info(),
        ) else {
            return Ok(None);
        };
        let requirements =
            PassthroughRequirements::new(source_syntax, CompressedPayloadKind::Jpeg2000Codestream)
                .with_dimensions((frame_columns, frame_rows))
                .with_components(raw.samples_per_pixel() as u8)
                .with_bit_depth(raw.bits_allocated() as u8);
        if candidate.copy_bytes_if_eligible(&requirements).is_err() {
            return Ok(None);
        }
        (candidate_syntax, photometric_interpretation)
    };

    let components = raw.samples_per_pixel() as u8;
    let bits_allocated = raw.bits_allocated();
    Ok(Some(J2kPassthroughFrame {
        codestream: raw.into_data(),
        profile: PixelProfile {
            components,
            bits_allocated,
            photometric_interpretation,
        },
        transfer_syntax: passthrough_syntax,
    }))
}

pub(super) fn j2k_raw_frame_syntax_and_profile(
    raw: &RawCompressedTile,
) -> (
    Option<j2k_core::CompressedTransferSyntax>,
    Option<PixelProfile>,
) {
    if !matches!(
        raw.compression(),
        Compression::Jp2kRgb | Compression::Jp2kYcbcr
    ) {
        return (None, None);
    }
    let Ok(view) = J2kView::parse(raw.data()) else {
        return (None, None);
    };
    let Some(candidate) = view.passthrough_candidate() else {
        return (None, None);
    };
    let syntax = candidate.transfer_syntax();
    if raw.bits_allocated() > u8::MAX as u16 || raw.samples_per_pixel() > u8::MAX as u16 {
        return (Some(syntax), None);
    }
    let Some(photometric_interpretation) =
        j2k_passthrough_photometric_interpretation(raw.photometric_interpretation(), view.info())
    else {
        return (Some(syntax), None);
    };
    (
        Some(syntax),
        Some(PixelProfile {
            components: raw.samples_per_pixel() as u8,
            bits_allocated: raw.bits_allocated(),
            photometric_interpretation,
        }),
    )
}

fn j2k_passthrough_photometric_interpretation(
    raw_photometric: EncodedTilePhotometricInterpretation,
    info: &j2k_core::Info,
) -> Option<&'static str> {
    match (info.components, raw_photometric) {
        (1, EncodedTilePhotometricInterpretation::Monochrome2) => Some("MONOCHROME2"),
        (3, EncodedTilePhotometricInterpretation::Rgb) => Some("RGB"),
        (3, EncodedTilePhotometricInterpretation::YbrFull422) => match info.colorspace {
            Colorspace::Rct => Some("YBR_RCT"),
            Colorspace::Ict => Some("YBR_ICT"),
            Colorspace::YCbCr => Some("YBR_FULL_422"),
            Colorspace::Rgb | Colorspace::SRgb => Some("RGB"),
            _ => None,
        },
        _ => None,
    }
}
