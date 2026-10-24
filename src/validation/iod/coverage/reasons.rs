//! Reasons in the other IODs for the statements whose VL WSI reason speaks
//! of VL WSI objects: what the IOD's own Modules and Pixel Data make of the
//! statement. A statement without Pixel Data to bind in the IOD is outside
//! its scope there.

// The match names Requirement Variants through `use Requirement::*`: a name no
// Variant has any more would bind as a catch-all pattern instead of failing to
// compile.
#![deny(unreachable_patterns, non_snake_case)]

use super::{BULK_ANNOTATIONS, DOCUMENT_IODS, FILE_SET, NO_DOCUMENT_PIXEL_DATA, NO_PIXEL_DATA};
use crate::validation::iod::requirements::Requirement::{self, *};
use crate::validation::RequirementDisposition::{self, Behavior, OutsideScope, Undecidable};

const SEGMENTATION: &str = "A.51";
const PARAMETRIC_MAP: &str = "A.75";

/// The reason in `iod` for `requirement`, where its VL WSI reason does not
/// hold there.
pub(super) fn reworded(requirement: Requirement, iod: &str) -> Option<RequirementDisposition> {
    let pixel_less = if iod == BULK_ANNOTATIONS {
        Some(NO_PIXEL_DATA)
    } else if DOCUMENT_IODS.contains(&iod) {
        Some(NO_DOCUMENT_PIXEL_DATA)
    } else {
        None
    };
    if let (Some(pixel_less), true) = (pixel_less, binds_pixel_data(requirement)) {
        return Some(pixel_less);
    }
    let disposition = match requirement {
        Ps3_C_2_1_122fa1 => OutsideScope(
            "C.2.1 Patient Relationship Module belongs to the normalized Patient IOD of PS3.4 Annex F; no composite object of the IOD carries it",
        ),
        Ps3_C_2_3_1a990f => OutsideScope(
            "C.2.3 Patient Demographic Module belongs to the normalized Patient IOD of PS3.4 Annex F; the IOD's patient data come from the C.7.1.1 Patient Module rows the IOD tables check",
        ),
        Ps3_C_2_3_99887b => OutsideScope(
            "C.2.3 Patient Demographic Module belongs to the normalized Patient IOD of PS3.4 Annex F; no composite object of the IOD carries it",
        ),
        Ps3_C_2_4_9fde48 => OutsideScope(
            "C.2.4 Patient Medical Module belongs to the normalized Patient IOD of PS3.4 Annex F; no composite object of the IOD carries it",
        ),
        Ps3_C_7_4_1_1_2_c02f4d => OutsideScope(
            "CORNEAL_VERTEX_R and _L identify an ophthalmic Frame of Reference; annotations of a slide share the slide-related Frame of Reference of the images they annotate",
        ),
        Ps3_C_8_30_3_1_4_6f526b
        | Ps3_C_8_30_3_1_4_fc2365
        | Ps3_C_8_30_3_1_4_b39027
        | Ps3_C_8_30_3_1_4_df3f8b => OutsideScope(
            "Binds Corneal Topography Map data (C.8.30.3); no Module of the IOD carries corneal coordinates",
        ),
        Ps3_C_7_5_1_1_2_efdd0a | Ps3_C_7_9_PresentationLUTShape_579186 => monochrome_one(iod)?,
        Ps3_C_7_6_3_1_2_5fa00b
        | Ps3_C_7_6_3_1_2_8fee6e
        | Ps3_C_7_6_3_1_2_fc3f71
        | Ps3_C_7_6_3_1_2_2da8f9
        | Ps3_C_7_6_3_1_2_7be829
        | Ps3_C_7_6_3_1_3_f9dea3
        | Ps3_C_7_11c_Rows_456628
        | Ps3_C_7_11c_Columns_0168ee => single_sample(iod)?,
        Ps3_C_7_6_16_2_2_6_d4a473 | Ps3_C_7_6_16_2_2_6_f8bd55 if iod == SEGMENTATION => {
            OutsideScope(
                "Binds PET Dynamic images, whose Image Type Value 3 is DYNAMIC; a Segmentation's Frames are segments of its source images, not PET acquisitions",
            )
        }
        Ps3_C_7_6_16_2_2_6_d4a473 | Ps3_C_7_6_16_2_2_6_f8bd55 if iod == PARAMETRIC_MAP => {
            OutsideScope(
                "Binds PET Dynamic images, whose Image Type Value 3 is DYNAMIC; a Parametric Map's Frames are values derived from its source images, not PET acquisitions",
            )
        }
        Ps3_C_7_6_3_1_5_b1076b => OutsideScope(
            "Alpha Palette Color Lookup Table Descriptor (0028,1104) belongs to the Enhanced Palette Color Lookup Table Module of presentation and blending IODs; no Module or Macro of the IOD includes it",
        ),
        Ps3_C_7_6_3_1_5_7659ed => OutsideScope(
            "Binds the Palette Color Lookup Tables of a Color Palette IOD (A.58) object; the IOD is not A.58",
        ),
        Ps3_C_7_9_1_a8a229 => OutsideScope(
            "Binds Palette Color Lookup Table UID in a Color Palette IOD (A.58) object, where it equals the SOP Instance UID; in the IOD it identifies the palette",
        ),
        Ps3_C_7_6_6_1_3_c71343 => Undecidable(
            "Whether Frames are left and right stereoscopic pairs is image content that the IOD records only in Stereo Pairs Present itself",
        ),
        Ps5_8_2_5_fc541c | Ps5_8_2_6_9618ea => Behavior(
            "Binds decoders to take the image characteristics from the MPEG2 bit stream; no file records which characteristics a decoder used",
        ),
        Ps5_8_2_7_c4778c | Ps5_8_2_8_f5ebd6 => Behavior(
            "Binds decoders to take the image characteristics from the MPEG-4 AVC/H.264 bit stream; no file records which characteristics a decoder used",
        ),
        Ps5_8_2_10_fd07b0 | Ps5_8_2_11_f8bc4f => Behavior(
            "Binds decoders to take the image characteristics from the HEVC/H.265 bit stream; no file records which characteristics a decoder used",
        ),
        Ps5_A_10_c6cb25 | Ps5_A_10_adf45d | Ps5_A_10_3cc0b0 | Ps5_A_10_3289c0
        | Ps5_A_10_2fa6d0 => OutsideScope(
            "Constrains a SMPTE ST 2110 Transfer Syntax, which describes a DICOM-RTV Flow (PS3.22) sent outside any Part 10 file; no Part 10 file of the IOD uses it, and the Waveform audio attributes it constrains belong to no Module of the IOD",
        ),
        // The frame packing a 2D AVC bit stream may not use (PS3.5 10.11).
        Ps5_10_11_194813 if iod == SEGMENTATION || iod == PARAMETRIC_MAP => OutsideScope(
            "video bit streams are not validated: a pathology Segmentation or Parametric Map is not video; the syntax itself is reported as an advisory",
        ),
        _ => return None,
    };
    Some(disposition)
}

/// The statements that bind Pixel Data, Waveform Data or their decoding,
/// which an IOD without them does not carry.
fn binds_pixel_data(requirement: Requirement) -> bool {
    matches!(
        requirement,
        Ps3_C_7_5_1_1_2_efdd0a
            | Ps3_C_7_9_PresentationLUTShape_579186
            | Ps5_8_2_5_fc541c
            | Ps5_8_2_6_9618ea
            | Ps5_8_2_7_c4778c
            | Ps5_8_2_8_f5ebd6
            | Ps5_8_2_10_fd07b0
            | Ps5_8_2_11_f8bc4f
            | Ps5_A_10_c6cb25
            | Ps5_A_10_adf45d
            | Ps5_A_10_3cc0b0
            | Ps5_A_10_3289c0
            | Ps5_A_10_2fa6d0
            | Ps5_10_11_194813
    )
}

/// A MONOCHROME1 statement in an image IOD whose images are not MONOCHROME1.
fn monochrome_one(iod: &str) -> Option<RequirementDisposition> {
    match iod {
        SEGMENTATION => Some(OutsideScope(
            "Binds MONOCHROME1 images; Segmentation images are MONOCHROME2 or PALETTE COLOR (Table C.8.20-2), which the IOD tables check",
        )),
        PARAMETRIC_MAP => Some(OutsideScope(
            "Binds MONOCHROME1 images; Parametric Map images are MONOCHROME2 (Table C.8.32-2), which the IOD tables check",
        )),
        _ => None,
    }
}

/// A statement of multi-sample or chroma-subsampled Pixel Data in an IOD
/// whose images, or Icon Images, are single-sample.
fn single_sample(iod: &str) -> Option<RequirementDisposition> {
    match iod {
        SEGMENTATION => Some(OutsideScope(
            "Binds multi-sample or chroma-subsampled Pixel Data; Segmentation images are single-sample MONOCHROME2 or PALETTE COLOR (Table C.8.20-2), which the IOD tables check",
        )),
        PARAMETRIC_MAP => Some(OutsideScope(
            "Binds multi-sample or chroma-subsampled Pixel Data; Parametric Map images are single-sample MONOCHROME2 (Table C.8.32-2), which the IOD tables check",
        )),
        FILE_SET => Some(OutsideScope(
            "Binds multi-sample or chroma-subsampled Pixel Data; an Icon Image of a Directory Record is a single MONOCHROME1, MONOCHROME2 or PALETTE COLOR sample (F.7), which intrinsic-wsi-dicom-file-set checks",
        )),
        _ => None,
    }
}
