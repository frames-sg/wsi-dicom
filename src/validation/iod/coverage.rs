//! The disposition of a statement in each IOD it applies to, where it differs
//! from `disposition()`. For a statement the VL Whole Slide Microscopy IOD
//! shares, `disposition()` is how VL WSI objects are held to it: in another
//! IOD the check holds its objects to it where the dispatch runs that check
//! on them (`rule_iods`), save the statements `not_held` names; elsewhere the
//! statement awaits each IOD's check family, and some statements outside the
//! VL WSI scope are in theirs.

// The matches name Requirement Variants through `use Requirement::*`: a name
// no Variant has any more (a statement gone from the edition or the IODs)
// would bind as a catch-all pattern instead of failing to compile.
#![deny(unreachable_patterns, non_snake_case)]

use std::collections::BTreeMap;

mod reasons;

use super::model::Statement;
use super::requirements::Requirement::{self, *};
use crate::validation::wsi_conformance::{family_rule, is_family_rule, rule_iods};
use crate::validation::RequirementDisposition::{
    self, Behavior, Checked, OutsideScope, Unchecked, Undecidable,
};

/// The IOD whose objects `disposition()` describes when it is among a
/// statement's IODs.
const VL_WSI: &str = "A.32.8";
/// The File-set of a DICOMDIR: PS3.3 Annex F and PS3.10 Chapter 8.
const FILE_SET: &str = "file-set";
/// The Microscopy Bulk Simple Annotations IOD, which defines no Pixel Data.
const BULK_ANNOTATIONS: &str = "A.87";
const NO_PIXEL_DATA: RequirementDisposition = OutsideScope(
    "The Microscopy Bulk Simple Annotations IOD defines no Pixel, Overlay or Waveform Data",
);
/// The Key Object Selection Document and SR document IODs (A.35), which
/// define no Pixel Data either.
const DOCUMENT_IODS: [&str; 5] = ["A.35.4", "A.35.1", "A.35.2", "A.35.3", "A.35.13"];
const NO_DOCUMENT_PIXEL_DATA: RequirementDisposition =
    OutsideScope("An SR or KOS Document has no Pixel Data, Overlay or Waveform");
/// The Clean Pixel Data and Clean Recognizable Visual Features statements
/// (E.3.1, E.3.2) in an IOD without pixels: no Pixel Data, Float Pixel Data
/// or Double Float Pixel Data.
const NO_PIXELS_TO_CLEAN: RequirementDisposition = OutsideScope(
    "E.3.1: \"This Option is intended to apply to the Pixel Data (7FE0,0010) Attribute that occurs in the top level Data Set of an Image Storage SOP Instance\", and E.3.2 acts on the Pixel Data; the IOD defines no Pixel Data, Float Pixel Data or Double Float Pixel Data, and the de-identification check says the Options do not apply",
);
/// The check family of the recommendations ("should") no check holds yet.
const RECOMMENDATIONS: &str = "recommendations";
/// The other image IODs whose pixel restrictions constrain motion encodings.
const IMAGE_IODS: [&str; 2] = ["A.51", "A.75"];
/// The IODs with Plane Orientation (Patient) Functional Groups.
const PATIENT_PLANE_IODS: [&str; 2] = ["A.51", "A.75"];

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
                    .unwrap_or_else(|| self.derived(base, statement, iod))
                {
                    OutsideScope(reason) => OutsideScope(self.outside(iod).unwrap_or(reason)),
                    other => other,
                };
                let disposition = match disposition {
                    OutsideScope(_) | Behavior(_) | Undecidable(_) => {
                        reasons::reworded(self, iod).unwrap_or(disposition)
                    }
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
        // PS3.15 Annex E binds a de-identified object of every IOD alike;
        // an IOD whose objects the de-identification check does not hold
        // awaits it.
        if self.statement().part == "PS3.15" {
            // E.3.1 and E.3.2 bind the Pixel Data (7FE0,0010) and what it shows.
            let pixel_option = matches!(
                self,
                Ps15_E_3_1_40d8f8 | Ps15_E_3_1_3a4bfe | Ps15_E_3_2_8c7718 | Ps15_E_3_2_4931da
            );
            let without_pixels = iod == BULK_ANNOTATIONS || DOCUMENT_IODS.contains(&iod);
            return Some(match self.disposition() {
                _ if pixel_option && without_pixels => NO_PIXELS_TO_CLEAN,
                Checked(rule) if !rule_iods(rule).contains(&iod) => Unchecked(family),
                other => other,
            });
        }
        let disposition = match self {
            // Table C.8.32-2 applies C.8.16.1.3 to Parametric Map Image Type
            // Value 3, which intrinsic-wsi-dicom-parametric-map fails as MIXED.
            Ps3_C_8_16_1_3_6f1c70 if iod == "A.75" => {
                family_rule(iod).map_or(Unchecked(family), Checked)
            }
            // A.35.4.3.1.3 fixes TID 2010 at the root, and the KOS check fails
            // a root without its Content Template Sequence (DCMR, 2010).
            Ps3_C_18_8_1_2_96463f if iod == "A.35.4" => {
                Checked("intrinsic-wsi-dicom-key-object-selection")
            }
            // The File-set check holds the Content Items of SR DOCUMENT and
            // KEY OBJECT DOC Directory Records to the Content Item rules of
            // the structured-report check (structured_report::content_item_violations).
            Ps3_C_18_6_0d9ea4
            | Ps3_C_18_6_c2d2c9
            | Ps3_C_18_7_c08e13
            | Ps3_C_18_9_1_2_e8d12e
            | Ps3_C_17_5_TextValue_cbe4b1
            | Ps3_C_18_1_1_MeasuredValueSequence_fcf6c2
            | Ps3_C_18_1_1_NumericValue_04da1c
            | Ps3_C_18_1_1_FloatingPointValue_929170
            | Ps3_C_18_1_1_RationalNumeratorValue_93b0c9
            | Ps3_C_18_1_1_RationalDenominatorValue_7a7dc3
            | Ps3_C_18_4_1_ReferencedFrameNumber_cb2c38
            | Ps3_C_18_4_1_IconImageSequence_9594fc
            | Ps3_C_18_6_1_GraphicData_fb751a
            | Ps3_C_18_7_1_ReferencedSamplePositions_581602
                if iod == FILE_SET =>
            {
                Checked("intrinsic-wsi-dicom-file-set")
            }
            Ps3_C_17_3_DisplayURI_29ba6d | Ps3_C_17_3a_MACCalculationTransferSyntaxUID_1ead61
                if iod == FILE_SET =>
            {
                OutsideScope(
                    "In a File-set, the Hierarchical SOP Instance Reference Macro is reached only through the F.5-27 keys of SPECTROSCOPY and RAW DATA Directory Records, which reference Spectroscopy and Raw Data objects; no pathology-slide File-set holds them",
                )
            }
            // A File-set's DICOMDIR and the File IDs it references are read in
            // the File-set scope (PS3.10 Chapter 8).
            Ps10_8_1_1b75db | Ps10_8_6_cda158 if iod == FILE_SET => Undecidable(
                "A File-set is recognized only by its DICOMDIR: a directory without one is indistinguishable from DICOM files that form no File-set, and a directory holds at most one file of a name",
            ),
            Ps10_8_1_efcf92 if iod == FILE_SET => Undecidable(
                "Whether the File-set UID is unique and its root registered is recorded in no file; its UID syntax is the file-meta check's (PS3.5 9.1)",
            ),
            // The DICOMDIR is in the validated set, and the File Meta check
            // holds it to PS3.10 Section 7.
            Ps10_8_6_6f3918 if iod == FILE_SET => Checked("intrinsic-wsi-dicom-file-meta"),
            Ps10_8_5_e938f0 | Ps10_8_6_cab9e6 | Ps10_8_6_0ebc18 | Ps10_8_6_572268
            | Ps10_8_6_f6c0f3
                if iod == FILE_SET =>
            {
                Checked("intrinsic-wsi-dicom-file-set")
            }
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
                if PATIENT_PLANE_IODS.contains(&iod) =>
            {
                family_rule(iod).map_or(Unchecked(family), Checked)
            }
            Ps3_C_7_6_16_2_25_2f8353 | Ps3_C_7_6_16_2_25_b8dc4e if iod == "A.75" => Undecidable(
                "Whether an Attribute had one Value in every converted source image is not encoded",
            ),
            Ps3_C_7_6_16_2_25_8fe231 if iod == "A.75" => {
                Behavior("Binds how a conversion matches Private Attribute Values")
            }
            Ps3_C_7_6_3_1_5_1d78b7 if iod == "A.51" => family_rule(iod).map_or(Unchecked(family), Checked),
            // An encapsulated Float or Double Float Pixel Data Value is an
            // invalid file encoding before the Parametric Map family can
            // inspect its pixels.
            Ps5_8_2_0fadc3 if iod == "A.75" => Checked("intrinsic-wsi-dicom-encoding"),
            Ps5_8_2_18602a | Ps5_8_2_4bcda5 if iod == "A.75" => {
                family_rule(iod).map_or(Unchecked(family), Checked)
            }
            // Table C.8.32-2 takes Image Type Values 3 and 4 from C.8.16.1.3 and C.8.16.1.4.
            Ps3_C_8_16_1_3_66c2ea | Ps3_C_8_16_1_4_0067a1 | Ps3_C_8_16_1_4_e1a716
                if iod == "A.75" =>
            {
                family_rule(iod).map_or(Unchecked(family), Checked)
            }
            Ps3_C_8_16_1_4_2f3775 if iod == "A.75" => Undecidable(
                "Whether a pixel contrast was derived, and how, is not encoded apart from Value 4 itself",
            ),
            _ if is_motion(self) && IMAGE_IODS.contains(&iod) => motion(self, iod),
            _ if is_motion(self) && iod == FILE_SET => OutsideScope(
                "A DICOMDIR has no top-level Pixel Data; its Icon Image Sequence uses native Pixel Data and no motion encoding (PS3.3 F.7)",
            ),
            Ps5_A_6_871dd9 | Ps5_A_11_a86261 => OutsideScope(
                "Binds Overlay Data; no Module of the supported pathology IOD or a DICOMDIR Icon Image carries Overlay Data",
            ),
            Ps5_A_6_e0a5fd | Ps5_A_11_ce7500 if IMAGE_IODS.contains(&iod) => {
                Unchecked("jpip-dataset-encoding")
            }
            Ps5_A_6_e0a5fd | Ps5_A_11_ce7500 => OutsideScope(
                "JPIP Referenced encodings require top-level integer Pixel Data (PS3.5 A.6 and A.11); this IOD has none, and DICOMDIR Icon Images use native Pixel Data",
            ),
            // A Bulk Annotations object has no Pixel Data whose stream the
            // codestream check compares with the syntax; parsing the Data Set
            // in the syntax the File Meta Information names holds it.
            Ps10_7_2_770df5 if iod == BULK_ANNOTATIONS => Checked("intrinsic-pixel-structure"),
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

/// Motion encoding restrictions follow the intersection of the Transfer
/// Syntax and IOD tables. Single-component MPEG2 fits 8-bit Segmentation;
/// no video syntax fits the Parametric Map's 16-bit MONOCHROME2 samples.
fn motion(requirement: Requirement, iod: &str) -> RequirementDisposition {
    let section = requirement.statement().section;
    if iod == "A.75" {
        return OutsideScope("Table C.8.32-2 requires one MONOCHROME2 sample with 16 stored bits (or native floating-point pixels); MPEG2 requires 8 bits, while AVC/H.264 and HEVC/H.265 require three YBR_PARTIAL_420 samples, so no Parametric Map uses a motion encoding");
    }
    if matches!(section, "8.2.5" | "8.2.6" | "A.4.5") {
        return match requirement {
            Ps5_8_2_5_0943da => OutsideScope("Table C.8.20-2 requires one sample per pixel; Table 8.2.5-1 requires Planar Configuration to be absent for single-component MPEG2, so the value-0 rule for multi-component data does not apply"),
            other => other.disposition(),
        };
    }
    OutsideScope("Table C.8.20-2 requires one sample and MONOCHROME2 or PALETTE COLOR; AVC/H.264 and HEVC/H.265 require three samples and YBR_PARTIAL_420 (PS3.5 8.2.7-8.2.11), so no Segmentation uses these encodings")
}

fn is_motion(requirement: Requirement) -> bool {
    let statement = requirement.statement();
    statement.part == "PS3.5"
        && matches!(
            statement.section,
            "8.2.5"
                | "8.2.6"
                | "8.2.7"
                | "8.2.8"
                | "8.2.9"
                | "8.2.10"
                | "8.2.11"
                | "8.2.12"
                | "A.4.5"
                | "A.4.6"
                | "A.4.7"
                | "A.4.8"
                | "A.4.9"
                | "A.4.10"
        )
}

impl Requirement {
    /// The disposition in `iod` that follows from `base`: the check holds the
    /// IOD's objects where the dispatch runs it on them, unless `not_held`
    /// says why it does not hold this statement there; an IOD family's check
    /// stands for the family check of `iod`; another check that does not run
    /// on them leaves the statement unchecked, an unchecked statement
    /// awaits the IOD's own family, and the Pixel Data encoding statements
    /// and those only an image check holds do not bind an IOD without Pixel
    /// Data.
    fn derived(
        self,
        base: RequirementDisposition,
        statement: Statement,
        iod: &str,
    ) -> RequirementDisposition {
        if let Some(outside) = outside_wsi_image_module(statement, iod) {
            return outside;
        }
        let runs = matches!(base, Checked(rule) if rule_iods(rule).contains(&iod));
        if runs {
            return not_held(self, iod).unwrap_or(base);
        }
        match base {
            Checked(_) | OutsideScope(_) | Unchecked(_)
                if iod == BULK_ANNOTATIONS && (encodes_pixels(statement) || images_only(base)) =>
            {
                NO_PIXEL_DATA
            }
            Checked(_) | OutsideScope(_) | Unchecked(_)
                if DOCUMENT_IODS.contains(&iod)
                    && (encodes_pixels(statement) || images_only(base)) =>
            {
                NO_DOCUMENT_PIXEL_DATA
            }
            Checked(_) if iod == FILE_SET && icon_pixel_statement(self) => {
                Checked("intrinsic-wsi-dicom-file-set")
            }
            // A statement several IOD families carry is held by each one's
            // own family check.
            Checked(rule) if is_family_rule(rule) => {
                family_rule(iod).map_or(Unchecked(family(iod)), Checked)
            }
            Checked(_) => Unchecked(family(iod)),
            // A recommendation awaits its check in every scope alike.
            Unchecked(RECOMMENDATIONS) => base,
            Unchecked(_) => Unchecked(family(iod)),
            other => other,
        }
    }
}

/// The Image Pixel statements a DICOMDIR binds through its Icon Images
/// alone (PS3.3 F.5): F.7 gives the Icon Image of a Directory Record a
/// single MONOCHROME1, MONOCHROME2 or PALETTE COLOR sample without Planar
/// Configuration, which intrinsic-wsi-dicom-file-set checks, so an Icon
/// Image of another Photometric Interpretation, Planar Configuration or
/// plane count, the only way to break these, fails it.
fn icon_pixel_statement(requirement: Requirement) -> bool {
    matches!(
        requirement,
        Ps3_C_7_6_3_1_2_93fa52
            | Ps3_C_7_6_3_1_2_dbe575
            | Ps3_C_7_6_3_1_1_c17c68
            | Ps3_C_7_11c_BitsAllocated_69f390
            | Ps3_C_7_11c_BitsStored_d52069
            | Ps3_C_7_11c_HighBit_a62636
            | Ps3_C_7_11c_PixelRepresentation_178735
    )
}

/// Whether `base` is a check that runs on the objects of every image IOD
/// but not on those of an IOD without Pixel Data: a statement only it holds
/// binds Pixel Data or the attributes describing it.
fn images_only(base: RequirementDisposition) -> bool {
    let Checked(rule) = base else {
        return false;
    };
    let iods = rule_iods(rule);
    !iods.contains(&BULK_ANNOTATIONS) && IMAGE_IODS.iter().all(|iod| iods.contains(iod))
}

/// The Image Type statements of C.8.12.4.1.1 in an IOD without the Whole
/// Slide Microscopy Image Module (Table C.8.12.4-1). Pixel Measures (Table
/// C.7.6.16-2) cites C.8.12.4 for extended depth of field, Plane Position
/// (Slide) (Table C.8.12.6.1-1) C.8.12.4.1.1 for the Total Pixel Matrix and
/// the Bulk Annotations Module (Table C.37.1-2) C.8.12.4.1.4 for its origin;
/// none takes up the Image Type Values that C.8.12.4.1.1 specializes.
fn outside_wsi_image_module(statement: Statement, iod: &str) -> Option<RequirementDisposition> {
    (iod != VL_WSI && statement.part == "PS3.3" && statement.section == "C.8.12.4.1.1").then_some(
        OutsideScope(
            "C.8.12.4.1.1 specializes Image Type (0008,0008) of the Whole Slide Microscopy Image Module, which the IOD does not include; its Pixel Measures and Plane Position (Slide) Functional Groups cite C.8.12.4 for depth of field and the Total Pixel Matrix, not for these Values",
        ),
    )
}

/// Why a check that runs on the objects of `iod` does not hold them to
/// `requirement`, which binds them on content the IOD does not encode,
/// binds images of a Module the IOD does not include, or yields there to
/// what the IOD specifies instead.
fn not_held(requirement: Requirement, iod: &str) -> Option<RequirementDisposition> {
    if iod == VL_WSI {
        return None;
    }
    let disposition = match requirement {
        Ps3_C_7_6_22_2_SpecimenDescriptionSequence_184620 if iod == BULK_ANNOTATIONS => {
            NO_PIXEL_DATA
        }
        Ps3_C_7_6_22_2_SpecimenDescriptionSequence_184620 => Undecidable(
            "Which specimens the Pixel Data images is encoded by Specimen Reference Functional Groups (C.8.12.6.3), which the IOD does not include",
        ),
        Ps3_C_7_6_1_1_6_8478b8
        | Ps3_C_7_6_1_1_6_b7c7c0
        | Ps3_C_7_6_1_1_6_c2da50
        | Ps3_C_7_6_1_1_6_c5156c
        | Ps3_C_7_6_1_1_6_cc071c
        | Ps3_C_7_6_1_1_6_6776f0
        | Ps3_C_7_6_1_1_6_aeb525
        | Ps3_C_7_6_1_1_6_502a69
            if iod == FILE_SET =>
        {
            OutsideScope(
                "The Icon Image Sequence of a Directory Record is specified by PS3.3 F.7, which C.7.6.1.1.6 defers to (\"unless otherwise specified\") and intrinsic-wsi-dicom-file-set checks; a DICOMDIR holds no other Icon Image",
            )
        }
        Ps3_C_7_6_1_1_2_ef6680 => Undecidable(
            "Value 3 may be zero length unless the IOD specializes it (C.7.6.1.1.2); whether a Value present identifies a specialization is its meaning, and the IOD's own Image Type statements hold its Values",
        ),
        _ => return None,
    };
    Some(disposition)
}

/// PS3.5 Section 8 (Encoding of Pixel, Overlay and Waveform Data), Annex A.4
/// (Transfer Syntaxes for Encapsulation of Encoded Pixel Data), A.6 and A.11
/// (JPIP Referenced Pixel Data), Annex G (RLE Compression) and 10.18
/// (HTJ2K): each binds Pixel Data or its codestream.
fn encodes_pixels(statement: Statement) -> bool {
    let section = statement.section;
    let within = |prefix: &str| section == prefix || section.starts_with(&format!("{prefix}."));
    statement.part == "PS3.5"
        && ["8", "A.4", "A.6", "A.11", "G", "10.18"]
            .into_iter()
            .any(within)
}

/// The check family a statement awaits in an IOD's objects.
pub(super) fn family(iod: &str) -> &'static str {
    match iod {
        "A.87" => "bulk-annotations",
        "A.51" => "segmentation",
        "A.75" => "parametric-map",
        "A.35.4" => "key-object-selection",
        "A.35.1" | "A.35.2" | "A.35.3" | "A.35.13" => "structured-report",
        FILE_SET => "file-set",
        "dicomweb" => "dicomweb",
        _ => "vl-whole-slide",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The IODs beside VL WSI whose pairs the lists below pin.
    const PINNED_IODS: [&str; 4] = ["A.87", "A.51", "A.75", FILE_SET];

    /// The (statement, IOD) pairs of `Requirement::ALL` that `select` picks
    /// in the pinned IODs, as "id IOD".
    fn pairs(select: impl Fn(Requirement, Statement, &str) -> bool) -> Vec<String> {
        let mut found = Vec::new();
        for requirement in Requirement::ALL {
            let statement = requirement.statement();
            for iod in statement.iods {
                if PINNED_IODS.contains(iod) && select(*requirement, statement, iod) {
                    found.push(format!("{} {iod}", requirement.id()));
                }
            }
        }
        found
    }

    /// Every pair a running check does not hold, so that a later edition's
    /// statements cannot widen the classifier unseen.
    #[test]
    fn the_statements_a_running_check_does_not_hold_are_pinned() {
        let reclassified = pairs(|requirement, _, iod| {
            requirement.listed(iod).is_none()
                && matches!(requirement.disposition(), Checked(rule) if rule_iods(rule).contains(&iod))
                && not_held(requirement, iod).is_some()
        });
        assert_eq!(
            reclassified,
            [
                "Ps3_C_7_6_1_1_2_ef6680 A.51",
                "Ps3_C_7_6_1_1_2_ef6680 A.75",
                "Ps3_C_7_6_1_1_6_8478b8 file-set",
                "Ps3_C_7_6_1_1_6_b7c7c0 file-set",
                "Ps3_C_7_6_1_1_6_c2da50 file-set",
                "Ps3_C_7_6_1_1_6_c5156c file-set",
                "Ps3_C_7_6_1_1_6_cc071c file-set",
                "Ps3_C_7_6_1_1_6_6776f0 file-set",
                "Ps3_C_7_6_1_1_6_aeb525 file-set",
                "Ps3_C_7_6_1_1_6_502a69 file-set",
                "Ps3_C_7_6_22_2_SpecimenDescriptionSequence_184620 A.87",
                "Ps3_C_7_6_22_2_SpecimenDescriptionSequence_184620 A.51",
                "Ps3_C_7_6_22_2_SpecimenDescriptionSequence_184620 A.75",
            ]
        );
        let outside = pairs(|requirement, statement, iod| {
            requirement.listed(iod).is_none() && outside_wsi_image_module(statement, iod).is_some()
        });
        assert_eq!(
            outside,
            [
                "Ps3_C_8_12_4_1_1_296245 A.51",
                "Ps3_C_8_12_4_1_1_296245 A.75",
                "Ps3_C_8_12_4_1_1_1befe3 A.51",
                "Ps3_C_8_12_4_1_1_1befe3 A.75",
                "Ps3_C_8_12_4_1_1_f4d4e2 A.51",
                "Ps3_C_8_12_4_1_1_f4d4e2 A.75",
                "Ps3_C_8_12_4_1_1_db2a55 A.51",
                "Ps3_C_8_12_4_1_1_db2a55 A.75",
                "Ps3_C_8_12_4_1_1_fa1cda A.51",
                "Ps3_C_8_12_4_1_1_fa1cda A.75",
                "Ps3_C_8_12_4_1_1_8b67da A.51",
                "Ps3_C_8_12_4_1_1_8b67da A.75",
                "Ps3_C_8_12_4_1_1_cd67be A.51",
                "Ps3_C_8_12_4_1_1_cd67be A.75",
                "Ps3_C_8_12_4_1_1_b18b99 A.51",
                "Ps3_C_8_12_4_1_1_b18b99 A.75",
                "Ps3_C_8_12_4_1_1_75b5c5 A.51",
                "Ps3_C_8_12_4_1_1_75b5c5 A.75",
            ]
        );
    }

    /// A reason in another IOD never speaks of VL WSI objects (`reasons`).
    #[test]
    fn no_reason_outside_vl_wsi_speaks_of_vl_wsi_objects() {
        let mut stray = Vec::new();
        for requirement in Requirement::ALL {
            for (iod, disposition) in requirement.iod_dispositions() {
                if let OutsideScope(reason) | Behavior(reason) | Undecidable(reason) = disposition {
                    if reason.contains("VL WSI") {
                        stray.push(format!("{} {iod}: {reason}", requirement.id()));
                    }
                }
            }
            let statement = requirement.statement();
            if !statement.iods.contains(&VL_WSI) {
                if let OutsideScope(reason) | Behavior(reason) | Undecidable(reason) =
                    requirement.disposition()
                {
                    if reason.contains("VL WSI") && requirement.iod_dispositions().is_empty() {
                        stray.push(format!("{}: {reason}", requirement.id()));
                    }
                }
            }
        }
        assert_eq!(stray, Vec::<String>::new());
    }

    /// A statement is Checked in an IOD only by a check the dispatch runs on
    /// that IOD's objects.
    #[test]
    fn every_checked_pair_names_a_check_that_runs_on_its_iod() {
        let mut stray = Vec::new();
        for requirement in Requirement::ALL {
            let dispositions = requirement.iod_dispositions();
            for iod in requirement.statement().iods {
                let disposition = dispositions
                    .get(iod)
                    .copied()
                    .unwrap_or_else(|| requirement.disposition());
                if let Checked(rule) = disposition {
                    if !rule_iods(rule).contains(iod) {
                        stray.push(format!("{} {iod} {rule}", requirement.id()));
                    }
                }
            }
        }
        assert_eq!(stray, Vec::<String>::new());
    }

    /// Every A.87 pair of an image-only check, which binds Pixel Data or
    /// its description: none of them is a PS3.5 encoding statement.
    #[test]
    fn the_image_only_statements_of_bulk_annotations_are_pinned() {
        let reclassified = pairs(|requirement, statement, iod| {
            iod == BULK_ANNOTATIONS
                && requirement.listed(iod).is_none()
                && images_only(requirement.disposition())
                && !encodes_pixels(statement)
        });
        assert_eq!(
            reclassified,
            [
                "Ps3_C_7_5_1_1_2_8d8819 A.87",
                "Ps3_C_7_5_1_1_2_6790e6 A.87",
                "Ps3_C_7_5_1_1_2_62cd75 A.87",
            ]
        );
    }

    #[test]
    fn proven_pathology_statement_bindings_are_pinned() {
        assert_eq!(
            Ps3__C_37_1_2_GraphicType_707986.disposition(),
            Checked("intrinsic-wsi-dicom-iod-enumerated-values")
        );
        assert_eq!(
            Ps5_8_2_0fadc3.iod_dispositions().get("A.75").copied(),
            Some(Checked("intrinsic-wsi-dicom-encoding"))
        );

        for requirement in [
            Ps3_C_18_10_1_TabulatedValuesSequence_1093d3,
            Ps3_C_18_10_1_TableRowDefinitionSequence_da170e,
            Ps3_C_18_10_1_TableColumnDefinitionSequence_7fba82,
            Ps3_C_18_10_1_CellValuesSequence_9090da,
        ] {
            let base = requirement.disposition();
            let dispositions = requirement.iod_dispositions();
            for iod in requirement.statement().iods {
                assert!(matches!(
                    dispositions.get(iod).copied().unwrap_or(base),
                    OutsideScope(_)
                ));
            }
        }
    }
}
