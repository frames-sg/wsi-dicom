use j2k::{J2kView, ReversibleTransform};
use j2k_core::{Colorspace, CompressedPayloadKind, PassthroughRequirements};
use wsi_rs::{Compression, EncodedTilePhotometricInterpretation, RawCompressedTile};

#[cfg(all(feature = "metal", target_os = "macos"))]
use std::path::Path;

#[cfg(all(feature = "metal", target_os = "macos"))]
use super::jpeg_baseline::JpegBaselineFrameLocation;
use super::jpeg_direct_htj2k;
#[cfg(all(feature = "metal", target_os = "macos"))]
use super::route_cache::AutoMetalInputRouteCacheKey;
use super::{J2kPassthroughFrame, LosslessJ2kPlannedFrame};
use crate::error::Error;
use crate::options::{EncodeBackendPreference, ExportOptions, TransferSyntax};
use crate::passthrough::j2k_codestream_is_rpcl;
use crate::routing::{
    j2k_encode_backend, j2k_encoded_lossless_profile, required_passthrough_syntax,
};
use crate::tile::PixelProfile;

fn j2k_source_fallback_allowed(
    planned: &LosslessJ2kPlannedFrame,
    transfer_syntax: TransferSyntax,
) -> bool {
    transfer_syntax == TransferSyntax::Jpeg2000
        && planned.source_j2k_syntax.is_some()
        && planned.source_j2k_dimensions == Some((planned.width, planned.height))
}

pub(super) fn j2k_non_passthrough_encode_allowed(
    planned: &LosslessJ2kPlannedFrame,
    transfer_syntax: TransferSyntax,
) -> bool {
    if transfer_syntax == TransferSyntax::Htj2k {
        return false;
    }
    planned.passthrough.is_none()
        && (!transfer_syntax.is_jpeg2000_passthrough_only()
            || j2k_source_fallback_allowed(planned, transfer_syntax))
}

pub(super) fn lossless_j2k_cpu_fallback_indices(
    planned: &[LosslessJ2kPlannedFrame],
    transfer_syntax: TransferSyntax,
    mut frame_already_encoded: impl FnMut(usize) -> bool,
) -> Vec<usize> {
    planned
        .iter()
        .enumerate()
        .filter_map(|(idx, planned_frame)| {
            (j2k_non_passthrough_encode_allowed(planned_frame, transfer_syntax)
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
    let (_passthrough_syntax, profile) = {
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
        let Some(profile) = j2k_passthrough_pixel_profile(
            raw.photometric_interpretation(),
            raw.samples_per_pixel() as u8,
            raw.bits_allocated(),
            &view,
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
        (candidate_syntax, profile)
    };

    Ok(Some(J2kPassthroughFrame {
        codestream: raw.into_data(),
        profile,
        #[cfg(test)]
        transfer_syntax: _passthrough_syntax,
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
    let Some(profile) = j2k_passthrough_pixel_profile(
        raw.photometric_interpretation(),
        raw.samples_per_pixel() as u8,
        raw.bits_allocated(),
        &view,
    ) else {
        return (Some(syntax), None);
    };
    (Some(syntax), Some(profile))
}

fn j2k_passthrough_pixel_profile(
    raw_photometric: EncodedTilePhotometricInterpretation,
    components: u8,
    bits_allocated: u16,
    view: &J2kView<'_>,
) -> Option<PixelProfile> {
    let profile = PixelProfile {
        components,
        bits_allocated,
        photometric_interpretation: j2k_passthrough_photometric_interpretation(
            raw_photometric,
            view.info(),
        )?,
    };
    j2k_view_matches_dicom_profile(view, profile).then_some(profile)
}

fn j2k_passthrough_photometric_interpretation(
    raw_photometric: EncodedTilePhotometricInterpretation,
    info: &j2k_core::Info,
) -> Option<&'static str> {
    // Passthrough is limited to component semantics that the decoded fallback can
    // reproduce. MCT=0 YCbCr cannot be declared RGB, while the fallback encoder
    // emits reversible RCT, so ICT also requires a whole-instance transcode.
    match (info.components, raw_photometric) {
        (1, EncodedTilePhotometricInterpretation::Monochrome2)
            if matches!(info.colorspace, Colorspace::Grayscale | Colorspace::SGray) =>
        {
            Some("MONOCHROME2")
        }
        (3, EncodedTilePhotometricInterpretation::Rgb) => match info.colorspace {
            Colorspace::Rgb | Colorspace::SRgb => Some("RGB"),
            Colorspace::Rct => Some("YBR_RCT"),
            _ => None,
        },
        (3, EncodedTilePhotometricInterpretation::YbrFull422) => match info.colorspace {
            Colorspace::Rct => Some("YBR_RCT"),
            _ => None,
        },
        _ => None,
    }
}

fn j2k_view_matches_dicom_profile(view: &J2kView<'_>, profile: PixelProfile) -> bool {
    let Some(support) = view.support_info() else {
        return false;
    };
    let Ok(bits_allocated) = u8::try_from(profile.bits_allocated) else {
        return false;
    };
    if support.payload_kind != CompressedPayloadKind::Jpeg2000Codestream
        || support.components.len() != usize::from(profile.components)
        || support.components.iter().any(|component| {
            component.signed
                || component.bit_depth != bits_allocated
                || component.x_rsiz != 1
                || component.y_rsiz != 1
        })
    {
        return false;
    }
    matches!(
        (profile.photometric_interpretation, view.info().colorspace),
        ("MONOCHROME2", Colorspace::Grayscale | Colorspace::SGray)
            | ("RGB", Colorspace::Rgb | Colorspace::SRgb)
            | ("YBR_RCT", Colorspace::Rct)
            | ("YBR_ICT", Colorspace::Ict)
    )
}

pub(super) fn validate_dicom_j2k_frame(
    codestream: &[u8],
    profile: PixelProfile,
    transfer_syntax: TransferSyntax,
) -> Result<(), Error> {
    let view = J2kView::parse(codestream).map_err(|err| Error::Encode {
        message: format!("published JPEG 2000 frame is invalid: {err}"),
    })?;
    let candidate = view.passthrough_candidate().ok_or_else(|| Error::Encode {
        message: "published JPEG 2000 frame has no validated codestream profile".into(),
    })?;
    let required_syntax = required_passthrough_syntax(transfer_syntax, candidate.transfer_syntax())
        .ok_or_else(|| Error::Encode {
            message: format!(
                "published JPEG 2000 frame syntax {:?} is incompatible with DICOM transfer syntax {}",
                candidate.transfer_syntax(),
                transfer_syntax.uid()
            ),
        })?;
    if candidate.transfer_syntax() != required_syntax
        || (transfer_syntax == TransferSyntax::Htj2kLosslessRpcl
            && !j2k_codestream_is_rpcl(codestream))
        || !j2k_view_matches_dicom_profile(&view, profile)
    {
        let components = view
            .support_info()
            .map(|support| {
                support
                    .components
                    .iter()
                    .map(|component| {
                        (
                            component.bit_depth,
                            component.signed,
                            component.x_rsiz,
                            component.y_rsiz,
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        return Err(Error::Encode {
            message: format!(
                "published JPEG 2000 frame is incompatible with DICOM profile {profile:?}: syntax={:?}, colorspace={:?}, components={components:?}",
                candidate.transfer_syntax(),
                view.info().colorspace
            ),
        });
    }
    Ok(())
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) const WSI_DICOM_METAL_ROW_BATCH_ROWS_ENV: &str = "WSI_DICOM_METAL_ROW_BATCH_ROWS";

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) const DEFAULT_METAL_ROW_BATCH_TARGET_TILES: usize = 384;
pub(super) const PREFER_DEVICE_TINY_HTJ2K_RPCL_CPU_MAX_FRAMES: u64 = 128;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) const DEFAULT_GPU_PIPELINE_DEPTH: usize = 2;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn effective_gpu_pipeline_depth(options: &ExportOptions) -> usize {
    options
        .gpu_pipeline_depth
        .unwrap_or(DEFAULT_GPU_PIPELINE_DEPTH)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn effective_gpu_row_batch_target_tiles(options: &ExportOptions) -> Option<usize> {
    Some(
        options
            .gpu_row_batch_target_tiles
            .unwrap_or(DEFAULT_METAL_ROW_BATCH_TARGET_TILES),
    )
}

pub(super) fn effective_lossless_j2k_encode_backend(
    options: &ExportOptions,
    frame_count: u64,
) -> EncodeBackendPreference {
    if options.encode_backend == EncodeBackendPreference::PreferDevice {
        if options.transfer_syntax == TransferSyntax::Jpeg2000Lossless {
            // Keep classic J2K lossless on CPU until Metal beats CPU in route-level benchmarks.
            return EncodeBackendPreference::CpuOnly;
        }
        if options.transfer_syntax == TransferSyntax::Htj2kLosslessRpcl
            && frame_count <= PREFER_DEVICE_TINY_HTJ2K_RPCL_CPU_MAX_FRAMES
        {
            return EncodeBackendPreference::CpuOnly;
        }
    }
    j2k_encode_backend(options.transfer_syntax, options.encode_backend)
}

pub(super) fn jpeg_direct_htj2k_supported_for_backend(
    transfer_syntax: TransferSyntax,
    backend: EncodeBackendPreference,
) -> bool {
    if !jpeg_direct_htj2k::transfer_syntax(transfer_syntax) {
        return false;
    }
    backend != EncodeBackendPreference::RequireDevice
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) const LOSSLESS_J2K_AUTO_ROUTE_PROBE_MAX_FRAMES: usize = 16;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) const LOSSLESS_J2K_AUTO_ROUTE_MIN_FRAMES: u64 = 16;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) const LOSSLESS_J2K_AUTO_PARTIAL_GPU_MIN_FRAMES: usize = 32;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) const LOSSLESS_J2K_AUTO_ROUTE_SPEEDUP_NUMERATOR: u128 = 92;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) const LOSSLESS_J2K_AUTO_ROUTE_SPEEDUP_DENOMINATOR: u128 = 100;

#[cfg(any(test, not(all(feature = "metal", target_os = "macos"))))]
pub(super) const LOSSLESS_J2K_CPU_ROW_BATCH_TARGET_TILES: u64 = 256;
pub(super) const LOSSLESS_J2K_DIRECT_PIXELDATA_MAX_MEMORY_BYTES: u64 = 256 * 1024 * 1024;
pub(super) const LOSSLESS_J2K_DIRECT_PIXELDATA_BYTES_PER_PIXEL: u64 = 6;

#[cfg(any(test, not(all(feature = "metal", target_os = "macos"))))]
pub(super) fn lossless_j2k_cpu_row_batch_count(tiles_across: u64, remaining_rows: u64) -> u64 {
    if tiles_across == 0 {
        return 1;
    }
    let rows = LOSSLESS_J2K_CPU_ROW_BATCH_TARGET_TILES
        .div_ceil(tiles_across)
        .max(1);
    rows.min(remaining_rows.max(1))
}

pub(super) fn lossless_j2k_direct_pixel_data_memory_bytes(rayon_threads: usize) -> u64 {
    let scaled = u64::try_from(rayon_threads)
        .unwrap_or(u64::MAX)
        .saturating_mul(32 * 1024 * 1024);
    scaled.clamp(
        64 * 1024 * 1024,
        LOSSLESS_J2K_DIRECT_PIXELDATA_MAX_MEMORY_BYTES,
    )
}

pub(super) fn lossless_j2k_use_direct_pixel_data(
    frame_count: u32,
    tile_size: u32,
    rayon_threads: usize,
) -> bool {
    if frame_count == 0 || rayon_threads <= 1 {
        return false;
    }
    let estimated_bytes = u64::from(frame_count)
        .saturating_mul(u64::from(tile_size))
        .saturating_mul(u64::from(tile_size))
        .saturating_mul(LOSSLESS_J2K_DIRECT_PIXELDATA_BYTES_PER_PIXEL);
    estimated_bytes <= lossless_j2k_direct_pixel_data_memory_bytes(rayon_threads)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn lossless_j2k_auto_allows_metal_input(
    preference: EncodeBackendPreference,
    transfer_syntax: TransferSyntax,
    frame_count: u64,
    _source_device_decode: bool,
) -> bool {
    if !transfer_syntax.is_lossless_j2k_family() {
        return false;
    }
    match preference {
        EncodeBackendPreference::CpuOnly => false,
        EncodeBackendPreference::PreferDevice | EncodeBackendPreference::RequireDevice => true,
        EncodeBackendPreference::Auto => {
            if frame_count < LOSSLESS_J2K_AUTO_ROUTE_MIN_FRAMES {
                return false;
            }
            matches!(
                transfer_syntax,
                TransferSyntax::Htj2kLossless | TransferSyntax::Htj2kLosslessRpcl
            )
        }
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn lossless_j2k_metal_input_preference(
    preference: EncodeBackendPreference,
    source_device_decode: bool,
) -> EncodeBackendPreference {
    if preference == EncodeBackendPreference::RequireDevice && !source_device_decode {
        EncodeBackendPreference::CpuOnly
    } else {
        preference
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn lossless_j2k_auto_should_start_cpu_only(
    preference: EncodeBackendPreference,
    transfer_syntax: TransferSyntax,
    frame_count: u64,
    source_device_decode: bool,
) -> bool {
    preference == EncodeBackendPreference::Auto
        && transfer_syntax.is_lossless_j2k_family()
        && !lossless_j2k_auto_allows_metal_input(
            preference,
            transfer_syntax,
            frame_count,
            source_device_decode,
        )
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn auto_metal_input_route_cache_key(
    source_path: &Path,
    options: ExportOptions,
    location: JpegBaselineFrameLocation,
    route_scope_frames: u64,
) -> Option<AutoMetalInputRouteCacheKey> {
    (options.encode_backend == EncodeBackendPreference::Auto
        && matches!(
            options.transfer_syntax,
            TransferSyntax::Htj2kLossless | TransferSyntax::Htj2kLosslessRpcl
        ))
    .then(|| AutoMetalInputRouteCacheKey {
        source_path: source_path.to_path_buf(),
        scene_idx: location.scene_idx,
        series_idx: location.series_idx,
        level: location.level_idx,
        z: location.z,
        c: location.c,
        t: location.t,
        tile_size: options.tile_size,
        transfer_syntax: options.transfer_syntax,
        route_scope_frames,
    })
}
