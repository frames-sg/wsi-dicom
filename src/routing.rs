//! Encode route selection and fallback policy.

use std::path::Path;

use j2k_core::CompressedTransferSyntax;
use wsi_rs::{LevelSourceKind, Slide, TileLayout};

use crate::error::Error;
use crate::options::{EncodeBackendPreference, TransferSyntax};
use crate::tile::PixelProfile;

#[cfg(test)]
std::thread_local! {
    static SLIDE_OPEN_COUNT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

pub(crate) fn open_slide(path: &Path) -> Result<Slide, Error> {
    #[cfg(test)]
    SLIDE_OPEN_COUNT.with(|count| count.set(count.get().saturating_add(1)));
    Slide::open(path).map_err(|source| Error::SourceOpen {
        path: path.to_path_buf(),
        message: source.to_string(),
    })
}

#[cfg(test)]
pub(crate) fn reset_slide_open_count_for_current_thread() {
    SLIDE_OPEN_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn slide_open_count_for_current_thread() -> u32 {
    SLIDE_OPEN_COUNT.with(std::cell::Cell::get)
}

pub(crate) fn j2k_route_tile_size(
    tile_size: u32,
    transfer_syntax: TransferSyntax,
    level: &wsi_rs::Level,
) -> Result<u32, Error> {
    if tile_size == 0 {
        return Err(Error::InvalidOptions {
            reason: "tile_size must be greater than zero".into(),
        });
    }
    if transfer_syntax.is_jpeg2000_passthrough_only() {
        let native_square = match level.tile_layout {
            TileLayout::Regular {
                tile_width,
                tile_height,
                ..
            }
            | TileLayout::WholeLevel {
                virtual_tile_width: tile_width,
                virtual_tile_height: tile_height,
                ..
            } if tile_width == tile_height && tile_width > 0 => Some(tile_width),
            TileLayout::Regular { .. }
            | TileLayout::WholeLevel { .. }
            | TileLayout::Irregular { .. }
            | _ => None,
        };
        if let Some(native_tile_size) = native_square {
            return Ok(native_tile_size.min(tile_size));
        }
    }
    Ok(tile_size)
}

pub(crate) fn j2k_encode_transfer_syntax(transfer_syntax: TransferSyntax) -> TransferSyntax {
    if transfer_syntax == TransferSyntax::Jpeg2000 {
        TransferSyntax::Jpeg2000Lossless
    } else {
        transfer_syntax
    }
}

pub(crate) fn j2k_encode_backend(
    transfer_syntax: TransferSyntax,
    requested_backend: EncodeBackendPreference,
) -> EncodeBackendPreference {
    if transfer_syntax == TransferSyntax::Jpeg2000 {
        EncodeBackendPreference::CpuOnly
    } else {
        requested_backend
    }
}

pub(crate) fn j2k_encoded_lossless_profile(
    profile: PixelProfile,
    transfer_syntax: TransferSyntax,
) -> PixelProfile {
    if matches!(
        transfer_syntax,
        TransferSyntax::Jpeg2000
            | TransferSyntax::Jpeg2000Lossless
            | TransferSyntax::Htj2kLossless
            | TransferSyntax::Htj2kLosslessRpcl
    ) && profile.components == 3
    {
        PixelProfile {
            components: profile.components,
            bits_allocated: profile.bits_allocated,
            photometric_interpretation: "YBR_RCT",
        }
    } else {
        profile
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(crate) fn transfer_syntax_from_uid(uid: &str) -> Option<TransferSyntax> {
    TransferSyntax::ALL
        .into_iter()
        .find(|transfer_syntax| transfer_syntax.uid() == uid)
}

pub(crate) fn source_path_has_extension(path: &Path, ext: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(ext))
}

pub(crate) fn j2k_family_passthrough_probe_allowed(
    source_path: &Path,
    transfer_syntax: TransferSyntax,
) -> bool {
    match transfer_syntax {
        TransferSyntax::Jpeg2000 | TransferSyntax::Jpeg2000Lossless => true,
        TransferSyntax::Htj2k
        | TransferSyntax::Htj2kLossless
        | TransferSyntax::Htj2kLosslessRpcl => source_path_has_extension(source_path, "dcm"),
        _ => false,
    }
}

pub(crate) fn level_is_synthetic_downsample(
    slide: &Slide,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
) -> Result<bool, Error> {
    slide
        .level_source_kind(scene_idx, series_idx, level_idx)
        .map(|kind| kind == LevelSourceKind::SyntheticDownsample)
        .map_err(|err| Error::SlideRead {
            message: format!("failed to inspect level source kind: {err}"),
        })
}

pub(crate) fn required_passthrough_syntax(
    transfer_syntax: TransferSyntax,
    candidate_syntax: CompressedTransferSyntax,
) -> Option<CompressedTransferSyntax> {
    match transfer_syntax {
        TransferSyntax::Jpeg2000 => match candidate_syntax {
            CompressedTransferSyntax::Jpeg2000Lossless
            | CompressedTransferSyntax::Jpeg2000Lossy => Some(candidate_syntax),
            _ => None,
        },
        TransferSyntax::Jpeg2000Lossless => Some(CompressedTransferSyntax::Jpeg2000Lossless),
        TransferSyntax::Htj2k => match candidate_syntax {
            CompressedTransferSyntax::HtJpeg2000Lossless
            | CompressedTransferSyntax::HtJpeg2000Lossy => Some(candidate_syntax),
            _ => None,
        },
        TransferSyntax::Htj2kLossless => Some(CompressedTransferSyntax::HtJpeg2000Lossless),
        TransferSyntax::Htj2kLosslessRpcl => Some(CompressedTransferSyntax::HtJpeg2000Lossless),
        TransferSyntax::JpegBaseline8Bit | TransferSyntax::ExplicitVrLittleEndian => None,
    }
}

pub(crate) fn unsupported_j2k_route_error(
    transfer_syntax: TransferSyntax,
    row: u64,
    col: u64,
) -> Error {
    let reason = if transfer_syntax == TransferSyntax::Htj2k {
        format!(
            "HTJ2K 9/7 export requires direct source JPEG-to-HTJ2K transcoding; frame row={row} col={col} was not eligible"
        )
    } else {
        format!(
            "JPEG 2000 transfer syntax export is passthrough-only; frame row={row} col={col} was not eligible for compressed-frame passthrough"
        )
    };
    Error::Unsupported { reason }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::options::ExportOptions;

    fn level_with_layout(tile_layout: TileLayout) -> wsi_rs::Level {
        wsi_rs::Level::new((2048, 2048), 1.0, tile_layout)
    }

    #[test]
    fn j2k_passthrough_tile_size_caps_oversized_native_geometry() {
        let options = ExportOptions {
            tile_size: 512,
            transfer_syntax: TransferSyntax::Jpeg2000,
            ..ExportOptions::default()
        };
        let level = level_with_layout(TileLayout::Regular {
            tile_width: 2048,
            tile_height: 2048,
            tiles_across: 1,
            tiles_down: 1,
        });

        assert_eq!(
            j2k_route_tile_size(options.tile_size, options.transfer_syntax, &level).unwrap(),
            512
        );
    }

    #[test]
    fn j2k_passthrough_tile_size_preserves_smaller_native_geometry() {
        let options = ExportOptions {
            tile_size: 512,
            transfer_syntax: TransferSyntax::Jpeg2000,
            ..ExportOptions::default()
        };
        let level = level_with_layout(TileLayout::WholeLevel {
            width: 256,
            height: 256,
            virtual_tile_width: 256,
            virtual_tile_height: 256,
        });

        assert_eq!(
            j2k_route_tile_size(options.tile_size, options.transfer_syntax, &level).unwrap(),
            256
        );
    }

    #[test]
    fn j2k_route_tile_size_rejects_zero_tile_size_before_native_geometry() {
        let options = ExportOptions {
            tile_size: 0,
            transfer_syntax: TransferSyntax::Jpeg2000,
            ..ExportOptions::default()
        };
        let level = level_with_layout(TileLayout::Regular {
            tile_width: 256,
            tile_height: 256,
            tiles_across: 1,
            tiles_down: 1,
        });

        let err =
            j2k_route_tile_size(options.tile_size, options.transfer_syntax, &level).unwrap_err();
        assert!(err.to_string().contains("tile_size"));
    }

    #[test]
    fn htj2k_passthrough_probe_is_limited_to_dicom_sources() {
        assert!(j2k_family_passthrough_probe_allowed(
            Path::new("source.svs"),
            TransferSyntax::Jpeg2000
        ));
        assert!(j2k_family_passthrough_probe_allowed(
            Path::new("source.svs"),
            TransferSyntax::Jpeg2000Lossless
        ));
        assert!(j2k_family_passthrough_probe_allowed(
            Path::new("source.dcm"),
            TransferSyntax::Htj2kLosslessRpcl
        ));
        assert!(j2k_family_passthrough_probe_allowed(
            Path::new("source.DCM"),
            TransferSyntax::Htj2kLossless
        ));
        assert!(!j2k_family_passthrough_probe_allowed(
            Path::new("source.svs"),
            TransferSyntax::Htj2kLosslessRpcl
        ));
        assert!(!j2k_family_passthrough_probe_allowed(
            Path::new("source.ndpi"),
            TransferSyntax::Htj2kLosslessRpcl
        ));
    }
}
