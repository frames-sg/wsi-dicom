//! How each Transfer Syntax of PS3.5 Annex A conveys Pixel Data, for the
//! checks that depend on the syntax a file declares.

use dicom_dictionary_std::uids;

/// How a Transfer Syntax conveys the top-level Pixel Data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::validation) enum PixelEncoding {
    /// Native Format (PS3.5 A.1, A.2, A.3, A.5).
    Native,
    /// Encapsulated Format (PS3.5 A.4), with the syntax's Fragment rule.
    Encapsulated(Fragments),
    /// No Pixel Data; Pixel Data Provider URL references it (PS3.5 A.6, A.7,
    /// A.11, A.12).
    Referenced,
}

/// How the Frames of an Encapsulated Transfer Syntax map to Fragments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::validation) enum Fragments {
    /// A Fragment holds data of one Frame, and a Frame may span Fragments
    /// (PS3.5 A.4.1, A.4.3 and A.4.4 for JPEG 2000).
    SpanFrames,
    /// Each Frame is encoded in exactly one Fragment.
    OnePerFrame {
        /// PS3.5 section stating the rule.
        section: &'static str,
        /// The Fragment holds the Frame as it would be encoded in Native
        /// Format (PS3.5 A.4.11).
        native_payload: bool,
    },
    /// One video bit stream carries every Frame (PS3.5 8.2.5 to 8.2.11).
    Video {
        /// PS3.5 section stating the rule.
        section: &'static str,
        /// Non-Fragmentable: one Fragment holds the whole stream.
        single_fragment: bool,
        /// The Basic Offset Table shall be empty (MPEG2).
        empty_offset_table: bool,
    },
}

/// The Pixel Data encoding of `uid`. A Transfer Syntax PS3.5 does not define
/// is classified by its UID root: `1.2.840.10008.1.2.4.` syntaxes (including
/// retired JPEG processes) are encapsulated, others native.
pub(in crate::validation) fn pixel_encoding(uid: &str) -> PixelEncoding {
    let uid = uid.trim_end_matches(['\0', ' ']);
    if is_jpip_referenced(uid) {
        return PixelEncoding::Referenced;
    }
    if let Some(fragments) = one_per_frame(uid).or_else(|| video(uid)) {
        return PixelEncoding::Encapsulated(fragments);
    }
    if uid.starts_with("1.2.840.10008.1.2.4.") {
        PixelEncoding::Encapsulated(Fragments::SpanFrames)
    } else {
        PixelEncoding::Native
    }
}

/// JPIP Referenced syntaxes. PS3.3 Table C.7-11a conditions Pixel Data
/// Provider URL on JPIP Referenced and JPIP Referenced Deflate only, but
/// PS3.5 A.11 and A.12 give the JPIP HTJ2K Referenced syntaxes the same
/// encoding (no Pixel Data, URL instead); PS3.5 defines the encoding, so all
/// four are treated alike.
pub(in crate::validation) fn is_jpip_referenced(uid: &str) -> bool {
    [
        uids::JPIP_REFERENCED,
        uids::JPIP_REFERENCED_DEFLATE,
        uids::JPIPHTJ2K_REFERENCED,
        uids::JPIPHTJ2K_REFERENCED_DEFLATE,
    ]
    .contains(&uid.trim_end_matches(['\0', ' ']))
}

/// The SMPTE ST 2110 syntaxes and what each describes. Each is "used for"
/// pixels or audio "carried in a DICOM-RTV Flow (separated from DICOM-RTV
/// Metadata Flow)" (PS3.5 10.15, 10.16 and 10.17), in a DICOM-RTV Metadata
/// Flow "to describe the accompanying" Video or Audio Flow (A.8, A.9 and
/// A.10): none defines how a Data Set is encoded, which is what the Transfer
/// Syntax UID of a file identifies (PS3.10 7.1 and 7.2).
pub(in crate::validation) fn smpte_st2110(uid: &str) -> Option<(&'static str, &'static str)> {
    Some(match uid.trim_end_matches(['\0', ' ']) {
        "1.2.840.10008.1.2.7.1" => (
            "SMPTE ST 2110-20 Uncompressed Progressive Active Video",
            "10.15 and A.8",
        ),
        "1.2.840.10008.1.2.7.2" => (
            "SMPTE ST 2110-20 Uncompressed Interlaced Active Video",
            "10.16 and A.9",
        ),
        "1.2.840.10008.1.2.7.3" => ("SMPTE ST 2110-30 PCM Digital Audio", "10.17 and A.10"),
        _ => return None,
    })
}

fn one_per_frame(uid: &str) -> Option<Fragments> {
    let (section, native_payload) = match uid {
        uids::RLE_LOSSLESS => ("A.4.2", false),
        uids::HTJ2K_LOSSLESS | uids::HTJ2K_LOSSLESS_RPCL | uids::HTJ2K => ("A.4.4", false),
        uids::ENCAPSULATED_UNCOMPRESSED_EXPLICIT_VR_LITTLE_ENDIAN => ("A.4.11", true),
        uids::JPEGXL_LOSSLESS | uids::JPEGXLJPEG_RECOMPRESSION | uids::JPEGXL => ("A.4.12", false),
        uids::DEFLATED_IMAGE_FRAME_COMPRESSION => ("A.4.13", false),
        _ => return None,
    };
    Some(Fragments::OnePerFrame {
        section,
        native_payload,
    })
}

fn video(uid: &str) -> Option<Fragments> {
    let (section, single_fragment) = match uid {
        uids::MPEG2MPML => ("8.2.5", true),
        uids::MPEG2MPMLF => ("8.2.5", false),
        uids::MPEG2MPHL => ("8.2.6", true),
        uids::MPEG2MPHLF => ("8.2.6", false),
        uids::MPEG4HP41 | uids::MPEG4HP41BD => ("8.2.7", true),
        uids::MPEG4HP41F | uids::MPEG4HP41BDF => ("8.2.7", false),
        uids::MPEG4HP422D | uids::MPEG4HP423D => ("8.2.8", true),
        uids::MPEG4HP422DF | uids::MPEG4HP423DF => ("8.2.8", false),
        // 8.2.9 takes its properties, including the Fragment rule, from 8.2.8.
        uids::MPEG4HP42STEREO => ("8.2.9", true),
        uids::MPEG4HP42STEREOF => ("8.2.9", false),
        // HEVC syntaxes are Fragmentable only (8.2.10, 8.2.11).
        uids::HEVCMP51 => ("8.2.10", false),
        uids::HEVCM10P51 => ("8.2.11", false),
        _ => return None,
    };
    Some(Fragments::Video {
        section,
        single_fragment,
        empty_offset_table: section == "8.2.5" || section == "8.2.6",
    })
}
