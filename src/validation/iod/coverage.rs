//! The disposition of a statement in each IOD it applies to, where it differs
//! from `disposition()`. For a statement the VL Whole Slide Microscopy IOD
//! shares, `disposition()` is how VL WSI objects are held to it: in the other
//! IODs a check that runs on VL WSI objects alone does not hold their objects
//! to it, an unchecked statement awaits each IOD's check family, and some
//! statements outside the VL WSI scope are in theirs.

use std::collections::BTreeMap;

use super::model::Statement;
use super::requirements::Requirement::{self, *};
use crate::validation::RequirementDisposition::{
    self, Behavior, Checked, OutsideScope, Tables, Unchecked, Undecidable,
};

/// The IOD whose objects `disposition()` describes when it is among a
/// statement's IODs.
const VL_WSI: &str = "A.32.8";
/// The Microscopy Bulk Simple Annotations IOD, which defines no Pixel Data.
const BULK_ANNOTATIONS: &str = "A.87";
const NO_PIXEL_DATA: RequirementDisposition = OutsideScope(
    "The Microscopy Bulk Simple Annotations IOD defines no Pixel, Overlay or Waveform Data",
);
/// The single-frame and multi-frame microscopy image IODs that permit
/// YBR_FULL_422 and YBR_PARTIAL_420 (Tables C.8-77 and C.8.35.1-1).
const CHROMA_IODS: [&str; 4] = ["A.32.2", "A.32.3", "A.90.1", "A.90.2"];
/// The image IODs whose Pixel Data a motion Transfer Syntax may encode.
const IMAGE_IODS: [&str; 6] = ["A.32.2", "A.32.3", "A.90.1", "A.90.2", "A.51", "A.75"];
/// The IODs with Plane Orientation (Patient) Functional Groups.
const PATIENT_PLANE_IODS: [&str; 2] = ["A.51", "A.75"];

/// Checks that run on the objects of every pathology-slide IOD; every other
/// intrinsic check runs on VL WSI objects alone.
const EVERY_IOD_RULES: [&str; 11] = [
    "intrinsic-pixel-structure",
    "intrinsic-wsi-dicom-encoding",
    "intrinsic-wsi-dicom-digital-signatures",
    "intrinsic-wsi-dicom-iod-modules",
    "intrinsic-wsi-dicom-iod-attribute-presence",
    "intrinsic-wsi-dicom-iod-enumerated-values",
    "intrinsic-wsi-dicom-iod-vr-vm",
    "intrinsic-wsi-dicom-iod-value-format",
    "intrinsic-wsi-dicom-iod-sequence-items",
    "intrinsic-wsi-dicom-iod-value-sets",
    "intrinsic-wsi-dicom-file-meta",
];

impl Requirement {
    /// The statement's disposition in each of its IODs other than VL WSI,
    /// where it differs from `disposition()`.
    pub(super) fn iod_dispositions(self) -> BTreeMap<&'static str, RequirementDisposition> {
        let base = self.disposition();
        let statement = self.statement();
        statement
            .iods
            .iter()
            .copied()
            .filter(|iod| *iod != VL_WSI)
            .filter_map(|iod| {
                let disposition = match self
                    .listed(iod)
                    .unwrap_or_else(|| derived(base, statement, iod))
                {
                    OutsideScope(reason) => OutsideScope(self.outside(iod).unwrap_or(reason)),
                    other => other,
                };
                (disposition != base).then_some((iod, disposition))
            })
            .collect()
    }

    /// Dispositions decided statement by statement: statements outside the VL
    /// WSI scope that another IOD's Modules or encodings bring into its scope.
    fn listed(self, iod: &str) -> Option<RequirementDisposition> {
        let family = family(iod);
        let disposition = match self {
            Ps3_C_7_10_1_1_1_053155 | Ps3_C_7_10_1_1_1_b59c2b if PATIENT_PLANE_IODS.contains(&iod) => {
                Undecidable("How an X-Ray acquisition delimited its irradiation events is not encoded")
            }
            Ps3_C_7_4_1_1_2_c02f4d if PATIENT_PLANE_IODS.contains(&iod) => Undecidable(
                "Whether the Frame of Reference is a corneal coordinate system is not encoded apart from this Value",
            ),
            Ps3_C_7_6_1_1_1_ab0c02
            | Ps3_C_7_6_2_1_1_859096
            | Ps3_C_7_6_2_1_1_34cf77
            | Ps3_C_7_6_2_1_1_29ac83
            | Ps3_C_7_11c_ColorSpace_763a29
                if PATIENT_PLANE_IODS.contains(&iod) =>
            {
                Unchecked(family)
            }
            Ps3_C_7_11c_ColorSpace_763a29 if iod == "A.32.2" || iod == "A.32.3" => {
                Unchecked(family)
            }
            Ps3_C_7_6_16_2_25_2f8353 | Ps3_C_7_6_16_2_25_b8dc4e if iod == "A.75" => Undecidable(
                "Whether an Attribute had one Value in every converted source image is not encoded",
            ),
            Ps3_C_7_6_16_2_25_8fe231 if iod == "A.75" => {
                Behavior("Binds how a conversion matches Private Attribute Values")
            }
            Ps3_C_7_6_3_1_2_5fa00b if CHROMA_IODS.contains(&iod) => Undecidable(
                "The order of Y, CB and CR samples within native pixel bytes is not observable",
            ),
            Ps3_C_7_6_3_1_2_7be829 if CHROMA_IODS.contains(&iod) => {
                Undecidable("Chroma siting is not signalled in native bytes or JPEG headers")
            }
            Ps3_C_7_11c_Rows_456628 | Ps3_C_7_11c_Columns_0168ee if CHROMA_IODS.contains(&iod) => {
                Unchecked(family)
            }
            Ps3_C_7_6_3_1_5_1d78b7 if iod == "A.51" => Unchecked(family),
            // The generated Slide Coordinates Module of Table A.32.1-3 holds its Attributes.
            Ps3_C_8_12_2_1_1_4fc3ea if iod == "A.32.3" => Tables,
            Ps3_C_8_12_5_1_1_6f325a | Ps5_8_1_2_05616b | Ps5_8_1_2_70a5ff | Ps5_8_1_2_3b62f6
                if iod == "A.32.2" || iod == "A.32.3" =>
            {
                Unchecked(family)
            }
            Ps5_8_2_0fadc3 | Ps5_8_2_18602a | Ps5_8_2_4bcda5 if iod == "A.75" => Unchecked(family),
            // Table C.8.32-2 takes Image Type Values 3 and 4 from C.8.16.1.3 and C.8.16.1.4.
            Ps3_C_8_16_1_3_66c2ea | Ps3_C_8_16_1_4_0067a1 | Ps3_C_8_16_1_4_e1a716
                if iod == "A.75" =>
            {
                Unchecked(family)
            }
            Ps3_C_8_16_1_4_2f3775 if iod == "A.75" => Undecidable(
                "Whether a pixel contrast was derived, and how, is not encoded apart from Value 4 itself",
            ),
            _ if is_motion(self) && IMAGE_IODS.contains(&iod) => Unchecked(family),
            _ => return None,
        };
        Some(disposition)
    }
}

impl Requirement {
    /// Why a statement is outside the scope of `iod`, where the reason of
    /// `disposition()` speaks of VL WSI objects.
    fn outside(self, iod: &str) -> Option<&'static str> {
        let reason = match self {
            Ps3_C_7_6_1_1_1_ab0c02
            | Ps3_C_7_6_2_1_1_859096
            | Ps3_C_7_6_2_1_1_34cf77
            | Ps3_C_7_6_2_1_1_29ac83 => {
                "Image Position and Image Orientation (Patient) belong to no Module or Functional Group of the IOD"
            }
            Ps3_C_7_6_16_2_25_2f8353 | Ps3_C_7_6_16_2_25_b8dc4e | Ps3_C_7_6_16_2_25_8fe231 => {
                "The Unassigned Converted Attributes Macros belong to no Functional Group of the IOD"
            }
            Ps3_C_7_6_16_2_3_1_209538
            | Ps3_C_7_6_16_2_3_1_95795d
            | Ps3_C_7_6_16_2_3_1_94e7cc
            | Ps3_C_7_6_16_2_3_1_39c820 => {
                "Binds Frames whose Volumetric Properties (0008,9206) is SAMPLED; the IOD includes no Volumetric Properties or enumerates only VOLUME (Table C.8.35.2-1)"
            }
            Ps3_C_7_6_3_1_5_1d78b7 => "Binds Palette Color Lookup Tables in the Segmentation IOD",
            Ps3_C_8_12_2_1_1_4fc3ea => {
                "Image Center Point Coordinates Sequence (0040,071A) belongs to the Slide Coordinates Module, which the IOD does not include"
            }
            Ps3_C_8_12_5_1_1_6f325a => {
                "Binds single-frame images without the Multi-frame Functional Groups Module, which the Confocal Microscopy IODs include (Tables A.90.1.3-1 and A.90.2.3-1)"
            }
            Ps3_C_8_16_1_3e29fe
            | Ps3_C_8_16_1_95e046
            | Ps3_C_8_16_1_a44052
            | Ps3_C_8_16_1_4d8cea
            | Ps3_C_8_16_1_1_3d5347
            | Ps3_C_8_16_1_1_d275fb
            | Ps3_C_8_16_1_2_9cda19
            | Ps3_C_8_16_1_2_d4b014
            | Ps3_C_8_16_1_3_66c2ea
            | Ps3_C_8_16_1_4_2f3775
            | Ps3_C_8_16_1_4_0067a1
            | Ps3_C_8_16_1_4_e1a716
            | Ps3_C_8_16_2_1_3_2c806a
            | Ps3_C_8_16_2_1_3_49f9d6 => {
                "C.8.16 binds the Image Type, Frame Type and Image Description Attributes of Enhanced CT, MR, US and Photoacoustic images; the Confocal Microscopy Tiled Pyramidal Image Module specifies its own (Table C.8.35.2-1)"
            }
            Ps5_8_1_2_05616b | Ps5_8_1_2_3b62f6 | Ps5_8_1_2_70a5ff
                if PATIENT_PLANE_IODS.contains(&iod) =>
            {
                "Binds Overlay Data, and A.51.4 and A.75.4 say the Overlay Plane Module shall not be present"
            }
            Ps5_8_1_2_05616b | Ps5_8_1_2_3b62f6 | Ps5_8_1_2_70a5ff => {
                "Binds Overlay Data, and the IOD includes no Overlay Plane Module"
            }
            Ps5_8_2_0fadc3 | Ps5_8_2_18602a | Ps5_8_2_4bcda5 => {
                "Float Pixel Data and Double Float Pixel Data belong to no Module of the IOD"
            }
            _ => return None,
        };
        Some(reason)
    }
}

/// The motion Transfer Syntax statements of PS3.5 8.2.5 to 8.2.12 and A.4.5
/// to A.4.10, outside the VL WSI scope (C.8.12.4.1.5).
fn is_motion(requirement: Requirement) -> bool {
    matches!(
        requirement,
        Ps5_8_2_5_5c5c79
            | Ps5_8_2_5_c5b5a7
            | Ps5_8_2_5_6cde8d
            | Ps5_8_2_6_405812
            | Ps5_8_2_6_4d8fa8
            | Ps5_8_2_6_571188
            | Ps5_8_2_7_7886b4
            | Ps5_8_2_7_7773fd
            | Ps5_8_2_7_ce01ab
            | Ps5_8_2_7_3618e9
            | Ps5_8_2_7_d08204
            | Ps5_8_2_8_d6c85e
            | Ps5_8_2_8_9d4541
            | Ps5_8_2_8_68acb5
            | Ps5_8_2_8_7fe9c4
            | Ps5_8_2_8_34a4e3
            | Ps5_8_2_9_c7bb6c
            | Ps5_8_2_9_b1098e
            | Ps5_8_2_9_a52046
            | Ps5_8_2_10_c1679c
            | Ps5_8_2_10_7e5161
            | Ps5_8_2_10_17fcd6
            | Ps5_8_2_10_64b5e7
            | Ps5_8_2_10_af1171
            | Ps5_8_2_11_455596
            | Ps5_8_2_11_84bfbd
            | Ps5_8_2_11_a9912e
            | Ps5_8_2_11_ce01f7
            | Ps5_8_2_11_7456b1
            | Ps5_8_2_12_b38f9a
            | Ps5_A_4_5_54be61
            | Ps5_A_4_6_eaaff4
            | Ps5_A_4_7_7502f5
            | Ps5_A_4_7_313c85
            | Ps5_A_4_7_d07e3a
            | Ps5_A_4_7_9b3674
            | Ps5_A_4_8_c22295
            | Ps5_A_4_9_85a504
            | Ps5_A_4_10_7a2eb4
    )
}

/// The disposition in `iod` that follows from `base`: a check of VL WSI
/// objects alone leaves the statement unchecked there, an unchecked statement
/// awaits the IOD's own family, and the Pixel Data encoding of PS3.5 Section 8
/// and Annex A.4 does not bind an IOD without Pixel Data.
fn derived(
    base: RequirementDisposition,
    statement: Statement,
    iod: &str,
) -> RequirementDisposition {
    let every_iod = matches!(base, Checked(rule) if EVERY_IOD_RULES.contains(&rule));
    match base {
        Checked(_) | OutsideScope(_) | Unchecked(_)
            if !every_iod && iod == BULK_ANNOTATIONS && encodes_pixels(statement) =>
        {
            NO_PIXEL_DATA
        }
        Checked(_) if !every_iod => Unchecked(family(iod)),
        Unchecked(_) => Unchecked(family(iod)),
        other => other,
    }
}

/// PS3.5 Section 8 (Encoding of Pixel, Overlay and Waveform Data) and Annex A.4
/// (Transfer Syntaxes for Encapsulation of Encoded Pixel Data).
fn encodes_pixels(statement: Statement) -> bool {
    let section = statement.section;
    statement.part == "PS3.5"
        && (section == "8"
            || section.starts_with("8.")
            || section == "A.4"
            || section.starts_with("A.4."))
}

/// The check family a statement awaits in an IOD's objects.
pub(super) fn family(iod: &str) -> &'static str {
    match iod {
        "A.32.2" | "A.32.3" => "vl-microscopic",
        "A.90.1" | "A.90.2" => "confocal-microscopy",
        "A.87" => "bulk-annotations",
        "A.51" => "segmentation",
        "A.75" => "parametric-map",
        _ => "vl-whole-slide",
    }
}
