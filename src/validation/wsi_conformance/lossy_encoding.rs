//! Evidence in the encoded Frames that they underwent lossy compression
//! (PS3.3 C.7.6.1.1.5): a Transfer Syntax that admits only lossy coding, or
//! a codestream header that selects lossy coding in a syntax that also admits
//! lossless coding. A reversible JPEG 2000 codestream may still have been
//! truncated, so only its irreversible transform is evidence.

use dicom_core::value::Value;
use dicom_dictionary_std::{tags, uids};
use dicom_object::DefaultDicomObject;

use crate::lossy::method_for_lossy_transfer_syntax;

/// How the encoded Frames bear on the lossy compression history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lossiness {
    /// No evidence of loss in the encoding.
    Unknown,
    /// The Frames are lossy compressed, as `evidence` shows; `method` is the
    /// Lossy Image Compression Method of that step when one alone names it.
    Lossy {
        method: Option<&'static str>,
        evidence: &'static str,
    },
}

/// The JPEG-LS Near-lossless Lossy Image Compression Method (C.7.6.1.1.5.1);
/// the export's history names the others.
const JPEG_LS: &str = "ISO_14495_1";

pub(super) fn lossiness(object: &DefaultDicomObject) -> Lossiness {
    let syntax = object.meta().transfer_syntax.trim_end_matches(['\0', ' ']);
    match syntax {
        uids::JPEG_BASELINE8_BIT | uids::JPEG_EXTENDED12_BIT => Lossiness::Lossy {
            method: method_for_lossy_transfer_syntax(uids::JPEG_BASELINE8_BIT),
            evidence: "encoded in a JPEG lossy (DCT) Transfer Syntax",
        },
        // The recompression is reversible, but the JPEG data it carries was
        // lossy; which step a producer names last is not fixed.
        uids::JPEGXLJPEG_RECOMPRESSION => Lossiness::Lossy {
            method: None,
            evidence: "a JPEG XL recompression of lossy JPEG data",
        },
        uids::JPEG2000 | uids::JPEG2000MC if any_frame(object, irreversible_wavelet) => {
            Lossiness::Lossy {
                method: method_for_lossy_transfer_syntax(uids::JPEG2000),
                evidence: "JPEG 2000 codestreams whose COD selects the irreversible 9-7 wavelet",
            }
        }
        uids::HTJ2K if any_frame(object, irreversible_wavelet) => Lossiness::Lossy {
            method: method_for_lossy_transfer_syntax(uids::HTJ2K),
            evidence: "HTJ2K codestreams whose COD selects the irreversible 9-7 wavelet",
        },
        uids::JPEGLS_NEAR_LOSSLESS if any_frame(object, near_lossless_scan) => Lossiness::Lossy {
            method: Some(JPEG_LS),
            evidence: "JPEG-LS scans with a NEAR parameter greater than 0",
        },
        // MPEG2, MPEG-4 AVC/H.264 and HEVC/H.265 compression is inherently
        // lossy (PS3.5 8.2.5 to 8.2.11).
        uids::MPEG2MPML | uids::MPEG2MPMLF | uids::MPEG2MPHL | uids::MPEG2MPHLF => {
            Lossiness::Lossy {
                method: Some("ISO_13818_2"),
                evidence: "encoded in an MPEG2 Transfer Syntax, which is inherently lossy",
            }
        }
        uids::MPEG4HP41
        | uids::MPEG4HP41F
        | uids::MPEG4HP41BD
        | uids::MPEG4HP41BDF
        | uids::MPEG4HP422D
        | uids::MPEG4HP422DF
        | uids::MPEG4HP423D
        | uids::MPEG4HP423DF
        | uids::MPEG4HP42STEREO
        | uids::MPEG4HP42STEREOF => Lossiness::Lossy {
            method: Some("ISO_14496_10"),
            evidence: "encoded in an MPEG-4 AVC/H.264 Transfer Syntax, which is inherently lossy",
        },
        uids::HEVCMP51 | uids::HEVCM10P51 => Lossiness::Lossy {
            method: Some("ISO_23008_2"),
            evidence: "encoded in an HEVC/H.265 Transfer Syntax, which is inherently lossy",
        },
        _ => Lossiness::Unknown,
    }
}

/// Whether any Fragment that begins a codestream passes `test`. Only main
/// headers are read; later Fragments of a Frame do not begin with its start
/// marker and fail every test.
fn any_frame(object: &DefaultDicomObject, test: fn(&[u8]) -> bool) -> bool {
    let Ok(element) = object.element(tags::PIXEL_DATA) else {
        return false;
    };
    let Value::PixelSequence(sequence) = element.value() else {
        return false;
    };
    sequence.fragments().iter().any(|fragment| test(fragment))
}

/// A JPEG 2000 main header whose COD transformation is 0, the 9-7
/// irreversible filter (ISO/IEC 15444-1 A.6.1 Table A.20).
fn irreversible_wavelet(bytes: &[u8]) -> bool {
    if !bytes.starts_with(&[0xFF, 0x4F]) {
        return false;
    }
    let mut at = 2usize;
    loop {
        let Some(segment) = bytes.get(at..at + 4) else {
            return false;
        };
        match u16::from_be_bytes([segment[0], segment[1]]) {
            // SOT ends the main header.
            0xFF90 => return false,
            // COD: Lcod, Scod, SGcod (4 bytes), then SPcod's levels, code-block
            // width, height and style before the transformation.
            0xFF52 => return bytes.get(at + 13) == Some(&0),
            _ => {}
        }
        let length = usize::from(u16::from_be_bytes([segment[2], segment[3]]));
        if length < 2 {
            return false;
        }
        at += 2 + length;
    }
}

/// A JPEG-LS stream whose first scan has NEAR greater than 0 (ISO/IEC
/// 14495-1 C.2.3).
fn near_lossless_scan(bytes: &[u8]) -> bool {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return false;
    }
    let mut at = 2usize;
    loop {
        let Some(segment) = bytes.get(at..at + 4) else {
            return false;
        };
        if segment[0] != 0xFF {
            return false;
        }
        if segment[1] == 0xDA {
            // SOS: Ls, Ns, Ns component selector and mapping table pairs, NEAR.
            let Some(components) = bytes.get(at + 4).copied() else {
                return false;
            };
            return bytes
                .get(at + 5 + 2 * usize::from(components))
                .is_some_and(|near| *near > 0);
        }
        let length = usize::from(u16::from_be_bytes([segment[2], segment[3]]));
        if length < 2 {
            return false;
        }
        at += 2 + length;
    }
}

#[cfg(test)]
mod tests {
    use super::{irreversible_wavelet, near_lossless_scan};

    /// SOC, a minimal SIZ-less main header with a COD whose transformation
    /// is `transform`, then SOT.
    fn j2k(transform: u8) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0x4F];
        bytes.extend_from_slice(&[0xFF, 0x52, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x01, 0x00]);
        bytes.extend_from_slice(&[0x05, 0x04, 0x04, 0x00, transform]);
        bytes.extend_from_slice(&[0xFF, 0x90, 0x00, 0x0A]);
        bytes
    }

    #[test]
    fn the_cod_transformation_tells_irreversible_from_reversible() {
        assert!(irreversible_wavelet(&j2k(0)));
        assert!(!irreversible_wavelet(&j2k(1)));
        assert!(!irreversible_wavelet(&j2k(0)[2..]));
        assert!(!irreversible_wavelet(&[0xFF, 0x4F, 0xFF, 0x90]));
    }

    #[test]
    fn the_first_scan_near_parameter_tells_near_lossless_from_lossless() {
        // SOI, SOS with three components and NEAR 3 or 0.
        let scan = |near: u8| {
            vec![
                0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x00, 0x03, 0x00, near,
                0x01, 0x00,
            ]
        };
        assert!(near_lossless_scan(&scan(3)));
        assert!(!near_lossless_scan(&scan(0)));
        assert!(!near_lossless_scan(&scan(3)[..10]));
    }
}
