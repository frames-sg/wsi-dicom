//! Requirements of the Segmentation, Parametric Map and Microscopy Bulk
//! Simple Annotations IODs that the generated tables cannot express, as far as
//! more than one family check calls them or they read Modules that other IODs
//! also include: the VOI LUT window (PS3.3 C.11.2.1.2 and C.11.2.1.3), the NTP
//! source address of the Synchronization Module (Table C.7-7), the Frame
//! Laterality of the Frame Anatomy Macro (Table C.7.6.16-9), Patient
//! Orientation and the patient image plane (C.7.6.1.1.1 and C.7.6.2.1.1), the
//! Color Space of an ICC Profile (Tables C.11.15-1 and C.8.32-2) and the
//! Palette Color Lookup Table Module (C.7.9 and C.7.6.3.1.5). `FamilyCorpus`
//! holds the facts of the set-level checks.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;

use dicom_core::value::Value;
use dicom_core::{PrimitiveValue, Tag};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::disclosure::shown;
use super::iod::{
    dicom_edition, element_label, items, raw_tag, string_value, tag_text, Coded, Family,
};
use super::ValidationCheck;

mod corpus;

pub(in crate::validation) use corpus::{run_family_set_checks, FamilyCorpus};

pub(super) const CITATION: &str = concat!("PS3.3 ", dicom_edition!());
pub(super) const PS3_5: &str = concat!("PS3.5 ", dicom_edition!());

pub(super) const BULK_ANNOTATIONS_RULE: &str = "intrinsic-wsi-dicom-bulk-annotations";
pub(super) const SEGMENTATION_RULE: &str = "intrinsic-wsi-dicom-segmentation";
pub(super) const PARAMETRIC_MAP_RULE: &str = "intrinsic-wsi-dicom-parametric-map";
pub(super) const SYNCHRONIZATION_RULE: &str = "intrinsic-wsi-dicom-synchronization";

/// The IODs that include the Synchronization Module (C.7.4.2): Parametric
/// Map and the Enhanced, Comprehensive and Comprehensive 3D SR documents.
const SYNCHRONIZED_IODS: [&str; 4] = ["A.75", "A.35.2", "A.35.3", "A.35.13"];

/// Whether the object's IOD includes the Synchronization Module.
pub(super) fn includes_synchronization(object: &DefaultDicomObject) -> bool {
    super::wsi_conformance::iod_of(object).is_some_and(|iod| SYNCHRONIZED_IODS.contains(&iod))
}

/// The Synchronization Module check of one object of an IOD that includes
/// it: the NTP Source Address format (Table C.7-7). That every Instance of a
/// synchronized Series shares one Frame of Reference (C.7.4.2.1.1) is the
/// set-level part, in `run_family_set_checks`.
pub(super) fn run_synchronization_check(
    path: &Path,
    object: &DefaultDicomObject,
) -> Option<ValidationCheck> {
    if !includes_synchronization(object) {
        return None;
    }
    let mut findings = Findings::default();
    ntp_source_address(object, &mut findings);
    Some(findings.check(
        path,
        SYNCHRONIZATION_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " C.7.4.2"),
    ))
}

/// The pathology-slide IODs with a family check, by the SOP Classes of PS3.4
/// Table B.5-1 that name them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Iod {
    BulkAnnotations,
    Segmentation,
    ParametricMap,
}

impl Iod {
    /// The IOD of the Data Set's SOP Class UID, or else of the File Meta
    /// Information's, as the IOD checks select it.
    pub(super) fn of(object: &DefaultDicomObject) -> Option<Self> {
        let declared = string_value(object, tags::SOP_CLASS_UID, 1);
        let meta = Some(
            object
                .meta()
                .media_storage_sop_class_uid()
                .trim_end_matches(['\0', ' '])
                .to_string(),
        );
        [declared, meta]
            .into_iter()
            .flatten()
            .find_map(|uid| Self::of_sop_class(&uid))
    }

    pub(super) fn of_sop_class(uid: &str) -> Option<Self> {
        Some(match uid {
            uids::MICROSCOPY_BULK_SIMPLE_ANNOTATIONS_STORAGE => Self::BulkAnnotations,
            uids::SEGMENTATION_STORAGE | uids::LABEL_MAP_SEGMENTATION_STORAGE => Self::Segmentation,
            uids::PARAMETRIC_MAP_STORAGE => Self::ParametricMap,
            _ => return None,
        })
    }
}

/// What a family check found in one object: the violations, bounded and
/// formatted only while shown, and the requirements the object gave it no
/// means to evaluate, with why.
#[derive(Debug, Default)]
pub(super) struct Findings {
    violations: Family,
    unexamined: Vec<String>,
    /// Neither counts nor formats anything: for a pass that wants only the
    /// facts a check reads, not its findings.
    discard: bool,
}

impl Findings {
    pub(super) fn discarding() -> Self {
        Self {
            discard: true,
            ..Self::default()
        }
    }

    /// Counts a violation; `violation` is formatted only while it is shown.
    pub(super) fn push(&mut self, violation: impl FnOnce() -> String) {
        if !self.discard {
            self.violations.push(violation);
        }
    }

    pub(super) fn unexamined(&mut self, reason: String) {
        if !self.discard {
            self.unexamined.push(reason);
        }
    }

    /// The check: failed with the violations, else passed, naming what was
    /// not examined either way.
    pub(super) fn check(self, path: &Path, rule: &str, citation: &str) -> ValidationCheck {
        let mut check = self.violations.check(path, rule, citation);
        if !self.unexamined.is_empty() {
            check.message.push_str("; not examined: ");
            check.message.push_str(&self.unexamined.join("; "));
        }
        check
    }
}

/// A string Attribute's Values, empty ones kept, whether the Data Set holds
/// them split or as one backslash-delimited string; `None` when absent.
pub(super) fn strings(object: &InMemDicomObject, tag: Tag) -> Option<Vec<String>> {
    let element = object.get(tag)?;
    let values = element.to_multi_str().ok()?;
    Some(
        values
            .iter()
            .flat_map(|value| value.split('\\'))
            .map(|value| value.trim_matches([' ', '\0']).to_string())
            .collect(),
    )
}

pub(super) fn string(object: &InMemDicomObject, tag: Tag) -> Option<String> {
    string_value(object, tag, 1)
}

fn is_text(object: &InMemDicomObject, tag: Tag) -> Option<bool> {
    Some(matches!(
        object.get(tag)?.value(),
        Value::Primitive(PrimitiveValue::Str(_) | PrimitiveValue::Strs(_))
    ))
}

/// A numeric Attribute's Values, of a text (DS, IS) or binary VR; `None`
/// when absent or any Value is not a number.
pub(super) fn numbers(object: &InMemDicomObject, tag: Tag) -> Option<Vec<f64>> {
    if is_text(object, tag)? {
        return strings(object, tag)?
            .iter()
            .map(|value| value.trim().parse::<f64>().ok())
            .collect();
    }
    object.get(tag)?.to_multi_float64().ok()
}

pub(super) fn integers(object: &InMemDicomObject, tag: Tag) -> Option<Vec<i64>> {
    if is_text(object, tag)? {
        return strings(object, tag)?
            .iter()
            .map(|value| value.trim().parse::<i64>().ok())
            .collect();
    }
    object.get(tag)?.to_multi_int::<i64>().ok()
}

pub(super) fn integer(object: &InMemDicomObject, tag: Tag) -> Option<i64> {
    integers(object, tag)?.first().copied()
}

/// The number of Values of a primitive Attribute.
pub(super) fn multiplicity(object: &InMemDicomObject, tag: Tag) -> Option<u32> {
    if is_text(object, tag)? {
        return strings(object, tag).map(|values| u32::try_from(values.len()).unwrap_or(u32::MAX));
    }
    match object.get(tag)?.value() {
        Value::Primitive(primitive) => Some(primitive.multiplicity()),
        _ => None,
    }
}

fn is_code(item: &InMemDicomObject, (scheme, value): (&str, &str)) -> bool {
    Coded::of(item).is_some_and(|coded| coded.is(scheme, value))
}

/// The Attribute names messages give, by tag.
const NAMES: [(Tag, &str); 21] = [
    (
        tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
        "Red Palette Color Lookup Table Descriptor",
    ),
    (
        tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
        "Green Palette Color Lookup Table Descriptor",
    ),
    (
        tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
        "Blue Palette Color Lookup Table Descriptor",
    ),
    (
        tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        "Red Palette Color Lookup Table Data",
    ),
    (
        tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        "Green Palette Color Lookup Table Data",
    ),
    (
        tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        "Blue Palette Color Lookup Table Data",
    ),
    (
        tags::SEGMENTED_RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        "Segmented Red Palette Color Lookup Table Data",
    ),
    (
        tags::SEGMENTED_GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        "Segmented Green Palette Color Lookup Table Data",
    ),
    (
        tags::SEGMENTED_BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        "Segmented Blue Palette Color Lookup Table Data",
    ),
    (tags::FLOAT_PIXEL_DATA, "Float Pixel Data"),
    (tags::DOUBLE_FLOAT_PIXEL_DATA, "Double Float Pixel Data"),
    (tags::BITS_STORED, "Bits Stored"),
    (tags::HIGH_BIT, "High Bit"),
    (tags::PIXEL_REPRESENTATION, "Pixel Representation"),
    (tags::POINT_COORDINATES_DATA, "Point Coordinates Data"),
    (
        tags::DOUBLE_POINT_COORDINATES_DATA,
        "Double Point Coordinates Data",
    ),
    (tags::FRAME_VOILUT_SEQUENCE, "Frame VOI LUT Sequence"),
    (
        tags::SEGMENT_IDENTIFICATION_SEQUENCE,
        "Segment Identification Sequence",
    ),
    (tags::FRAME_ANATOMY_SEQUENCE, "Frame Anatomy Sequence"),
    (tags::DERIVATION_IMAGE_SEQUENCE, "Derivation Image Sequence"),
    (
        tags::PARAMETRIC_MAP_FRAME_TYPE_SEQUENCE,
        "Parametric Map Frame Type Sequence",
    ),
];

/// `Name (gggg,eeee)`, or the keyword for a tag `NAMES` does not hold.
pub(super) fn label(tag: Tag) -> String {
    NAMES.iter().find(|(named, _)| *named == tag).map_or_else(
        || element_label("", tag),
        |(_, name)| format!("{name} {}", tag_text(raw_tag(tag))),
    )
}

/// The Functional Group Items of a multi-frame image.
#[derive(Clone, Copy)]
pub(super) struct FunctionalGroups<'a> {
    shared: Option<&'a InMemDicomObject>,
    per_frame: &'a [InMemDicomObject],
    frames: usize,
}

impl<'a> FunctionalGroups<'a> {
    pub(super) fn of(object: &'a InMemDicomObject) -> Self {
        let per_frame = items(object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE);
        let declared = integer(object, tags::NUMBER_OF_FRAMES)
            .and_then(|frames| usize::try_from(frames).ok())
            .unwrap_or(1);
        Self {
            shared: items(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE).first(),
            per_frame,
            // Frames past the Per-Frame Items are the modules check's to
            // report; they have no Items of their own to read.
            frames: if per_frame.is_empty() {
                declared.min(1)
            } else {
                per_frame.len()
            },
        }
    }

    /// The Frames whose Items can be read: one per Per-Frame Item, or one
    /// standing for every Frame when only the Shared Item exists.
    pub(super) fn frames(&self) -> usize {
        self.frames
    }

    /// The first Item of `macro_sequence` that describes Frame `index`
    /// (0-based), from its Per-Frame Item or else the Shared Item.
    pub(super) fn macro_item(
        &self,
        index: usize,
        macro_sequence: Tag,
    ) -> Option<&'a InMemDicomObject> {
        self.per_frame
            .get(index)
            .and_then(|item| items(item, macro_sequence).first())
            .or_else(|| {
                self.shared
                    .and_then(|item| items(item, macro_sequence).first())
            })
    }

    /// Every Functional Group Item, with where it lies.
    pub(super) fn all(&self) -> impl Iterator<Item = (ItemPath, &'a InMemDicomObject)> + 'a {
        let shared = self.shared.map(|item| (ItemPath::default(), item));
        let per_frame = self.per_frame.iter().enumerate().map(|(index, item)| {
            let path = ItemPath {
                frame: Some(index),
                item: None,
            };
            (path, item)
        });
        shared.into_iter().chain(per_frame)
    }

    /// Every Item of `macro_sequence` in the Functional Group Items, with
    /// where it lies.
    pub(super) fn macro_items(
        &self,
        macro_sequence: Tag,
    ) -> impl Iterator<Item = (ItemPath, &'a InMemDicomObject)> + 'a {
        self.all().flat_map(move |(path, group)| {
            items(group, macro_sequence)
                .iter()
                .enumerate()
                .map(move |(index, item)| {
                    let path = ItemPath {
                        item: Some((macro_sequence, index)),
                        ..path
                    };
                    (path, item)
                })
        })
    }
}

/// Where a Functional Group Item, or an Item of a Macro in it, lies; it is
/// formatted only when a message names it.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ItemPath {
    /// The 0-based Per-Frame Functional Groups Item, or `None` for the
    /// Shared one.
    frame: Option<usize>,
    /// The Macro Sequence and the 0-based Item of it.
    item: Option<(Tag, usize)>,
}

impl std::fmt::Display for ItemPath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.frame {
            None => formatter.write_str("Shared Functional Groups Sequence[1] > ")?,
            Some(index) => write!(
                formatter,
                "Per-Frame Functional Groups Sequence[{}] > ",
                index + 1
            )?,
        }
        if let Some((sequence, index)) = self.item {
            write!(formatter, "{}[{}] > ", label(sequence), index + 1)?;
        }
        Ok(())
    }
}

/// C.11.2.1.2 and C.11.2.1.3: the Window Center, Window Width and VOI LUT
/// Function of one Data Set or Frame VOI LUT Item.
pub(super) fn voi_window(
    item: &InMemDicomObject,
    path: impl std::fmt::Display,
    photometric: Option<&str>,
    findings: &mut Findings,
) {
    let center = numbers(item, tags::WINDOW_CENTER);
    let width = numbers(item, tags::WINDOW_WIDTH);
    let function = string(item, tags::VOILUT_FUNCTION);
    if center.is_none() && width.is_none() && function.is_none() {
        return;
    }
    if !matches!(photometric, Some("MONOCHROME1" | "MONOCHROME2")) {
        findings.push(|| format!(
            "{path}Window Center, Window Width or VOI LUT Function is present, but Photometric Interpretation is {}; they are used only for MONOCHROME1 and MONOCHROME2 images ({CITATION} C.11.2.1.2.2)",
            photometric.unwrap_or("absent")
        ));
    }
    if let (Some(center), Some(width)) = (&center, &width) {
        if center.len() != width.len() {
            findings.push(|| format!(
                "{path}Window Center (0028,1050) has {} Values and Window Width (0028,1051) {}; they are pairs and have the same number of Values ({CITATION} C.11.2.1.2.2)",
                center.len(),
                width.len()
            ));
        }
    }
    let (minimum, section) = match function.as_deref() {
        None | Some("LINEAR") => (Bound::AtLeastOne, "C.11.2.1.2.1"),
        Some("SIGMOID") => (Bound::Positive, "C.11.2.1.3.1"),
        Some("LINEAR_EXACT") => (Bound::Positive, "C.11.2.1.3.2"),
        // Another VOI LUT Function names its own bivariate function.
        Some(_) => return,
    };
    for value in width.iter().flatten() {
        let fails = match minimum {
            Bound::AtLeastOne => *value < 1.0,
            Bound::Positive => *value <= 0.0,
        };
        if fails {
            findings.push(|| format!(
                "{path}Window Width (0028,1051) is {value} with VOI LUT Function {}; it shall be {} ({CITATION} {section})",
                function.as_deref().unwrap_or("absent (LINEAR)"),
                match minimum {
                    Bound::AtLeastOne => "greater than or equal to 1",
                    Bound::Positive => "greater than 0",
                }
            ));
        }
    }
}

#[derive(Clone, Copy)]
enum Bound {
    AtLeastOne,
    Positive,
}

/// Table C.7-7: NTP Source Address is an IPv4 address in dotted decimal or
/// an IPv6 address in colon separated hexadecimal.
fn ntp_source_address(object: &InMemDicomObject, findings: &mut Findings) {
    let Some(address) = string(object, tags::NTP_SOURCE_ADDRESS) else {
        return;
    };
    let well_formed = if address.contains(':') {
        address.parse::<Ipv6Addr>().is_ok()
    } else {
        address.parse::<Ipv4Addr>().is_ok()
    };
    if !well_formed {
        findings.push(|| format!(
            "NTP Source Address (0018,1803) {} is neither an IPv4 address in dotted decimal (e.g., 192.168.1.1) nor an IPv6 address in colon separated hexadecimal ({CITATION} Table C.7-7)",
            shown(tags::NTP_SOURCE_ADDRESS, &address)
        ));
    }
}

/// Laterality modifier codes (CID 244) and the laterality each means.
const LATERALITY_CODES: [(&str, &str, &str); 6] = [
    ("SCT", "24028007", "R"),
    ("SCT", "7771000", "L"),
    ("SCT", "51440002", "B"),
    ("SRT", "G-A100", "R"),
    ("SRT", "G-A101", "L"),
    ("SRT", "G-A102", "B"),
];

/// The lateralities that the modifier codes of `modifiers` mean.
fn modifier_lateralities(
    modifiers: &[InMemDicomObject],
) -> impl Iterator<Item = &'static str> + '_ {
    modifiers.iter().filter_map(|code| {
        LATERALITY_CODES
            .iter()
            .find(|(scheme, value, _)| is_code(code, (scheme, value)))
            .map(|(_, _, laterality)| *laterality)
    })
}

/// Table C.7.6.16-9: Frame Laterality agrees with the laterality modifiers of
/// the Primary Anatomic Structures in each Frame Anatomy Item.
pub(super) fn frame_laterality(groups: &FunctionalGroups<'_>, findings: &mut Findings) {
    for (path, anatomy) in groups.macro_items(tags::FRAME_ANATOMY_SEQUENCE) {
        let Some(frame) = string(anatomy, tags::FRAME_LATERALITY) else {
            continue;
        };
        let structures = items(anatomy, tags::PRIMARY_ANATOMIC_STRUCTURE_SEQUENCE);
        for structure in structures {
            let modifiers = items(
                structure,
                tags::PRIMARY_ANATOMIC_STRUCTURE_MODIFIER_SEQUENCE,
            );
            for laterality in modifier_lateralities(modifiers) {
                if laterality != frame {
                    findings.push(|| format!(
                        "{path}Frame Laterality (0020,9072) is {frame} but a Primary Anatomic Structure Modifier Sequence (0008,2230) code means {laterality}; it shall be consistent ({CITATION} Table C.7.6.16-9)"
                    ));
                }
            }
        }
    }
}

/// Tables C.11.15-1 and C.8.32-2: Color Space names the space the ICC
/// Profile of the same Data Set defines.
pub(super) fn color_space(object: &InMemDicomObject, table: &str, findings: &mut Findings) {
    let (Some(term), Some(profile)) = (
        string(object, tags::COLOR_SPACE),
        object.get(tags::ICC_PROFILE),
    ) else {
        return;
    };
    let Ok(bytes) = profile.to_bytes() else {
        findings.unexamined(format!(
            "Color Space {term} against an ICC Profile (0028,2000) whose Value is not bytes ({CITATION} {table})"
        ));
        return;
    };
    match crate::icc::defines_color_space(&bytes, &term) {
        Some(true) => {}
        Some(false) => findings.push(|| format!(
            "Color Space (0028,2002) is {term}, but the primaries of ICC Profile (0028,2000) are not those of {term}; it shall be consistent with the ICC Profile ({CITATION} {table} and C.11.15.1.2)"
        )),
        None => findings.unexamined(format!(
            "whether Color Space {term} is consistent with ICC Profile (0028,2000): the term is not a well-known space with defined primaries, or the profile is table-based or unreadable ({CITATION} {table})"
        )),
    }
}

/// The Palette Color Lookup Table Data and Segmented Data of red, green and
/// blue, and their Descriptors.
const PALETTE: [(&str, Tag, Tag, Tag); 3] = [
    (
        "Red",
        tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
        tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        tags::SEGMENTED_RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
    ),
    (
        "Green",
        tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
        tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        tags::SEGMENTED_GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
    ),
    (
        "Blue",
        tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
        tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        tags::SEGMENTED_BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA,
    ),
];

/// C.7.9 and Table C.7-22: in an Image IOD that includes the Palette Color
/// Lookup Table Module, its conditions decide between Palette Color Lookup
/// Table Data and Segmented Data. A Segmentation uses the Data and never the
/// Segmented Data, and its Descriptors' third Value is 8 or 16 (C.7.6.3.1.5);
/// another Image IOD uses exactly one of the two.
pub(super) fn palette(object: &InMemDicomObject, segmentation: bool, findings: &mut Findings) {
    let present = PALETTE.iter().any(|(_, descriptor, data, segmented)| {
        [descriptor, data, segmented]
            .iter()
            .any(|tag| object.get(**tag).is_some())
    });
    if !present {
        return;
    }
    for (color, descriptor, data, segmented) in PALETTE {
        let (has_data, has_segmented) =
            (object.get(data).is_some(), object.get(segmented).is_some());
        if segmentation {
            if !has_data || has_segmented {
                findings.push(|| format!(
                    "the {color} palette is encoded in {}; a Segmentation encodes it in {} alone, the Palette Color Lookup Table Module conditions taking precedence over the Image Pixel Module's ({CITATION} C.7.9 and Table C.7-22)",
                    describe_palette(has_data, has_segmented, data, segmented),
                    label(data)
                ));
            }
            if let Some(bits) =
                integers(object, descriptor).and_then(|values| values.get(2).copied())
            {
                if !matches!(bits, 8 | 16) {
                    findings.push(|| format!(
                        "{} third Value is {bits}; in the Segmentation IOD the number of bits for each entry shall be 8 or 16 ({CITATION} C.7.6.3.1.5)",
                        label(descriptor)
                    ));
                }
            }
        } else if has_data == has_segmented {
            findings.push(|| format!(
                "the {color} palette is encoded in {}; the Palette Color Lookup Table Module requires exactly one of {} and {} ({CITATION} C.7.9 and Table C.7-22)",
                describe_palette(has_data, has_segmented, data, segmented),
                label(data),
                label(segmented)
            ));
        }
    }
}

fn describe_palette(has_data: bool, has_segmented: bool, data: Tag, segmented: Tag) -> String {
    match (has_data, has_segmented) {
        (true, true) => format!("both {} and {}", label(data), label(segmented)),
        (true, false) => label(data),
        (false, true) => label(segmented),
        (false, false) => "neither Lookup Table Data Attribute".to_string(),
    }
}

/// An anatomical direction abbreviation and the axes and signs it names.
type Direction<'a> = (&'a str, &'a [(usize, f64)]);

/// Anatomical direction abbreviations: biped ones by axis and sign
/// (C.7.6.2.1.1: x to the left, y to the posterior, z to the head).
const BIPED_DIRECTIONS: [Direction<'static>; 6] = [
    ("L", &[(0, 1.0)]),
    ("R", &[(0, -1.0)]),
    ("P", &[(1, 1.0)]),
    ("A", &[(1, -1.0)]),
    ("H", &[(2, 1.0)]),
    ("F", &[(2, -1.0)]),
];

/// Quadruped abbreviations by the axes and signs they can name, which
/// depend on the body part (C.7.6.2.1.1).
const QUADRUPED_DIRECTIONS: [Direction<'static>; 13] = [
    ("LE", &[(0, 1.0)]),
    ("RT", &[(0, -1.0)]),
    ("D", &[(1, 1.0)]),
    ("V", &[(1, -1.0)]),
    ("PA", &[(1, -1.0)]),
    ("PL", &[(1, -1.0)]),
    ("CR", &[(1, 1.0), (2, 1.0)]),
    ("CD", &[(1, -1.0), (2, -1.0)]),
    ("R", &[(2, 1.0)]),
    ("PR", &[(2, 1.0)]),
    ("DI", &[(2, -1.0)]),
    ("M", &[]),
    ("L", &[]),
];

/// Direction cosines within this of unit length or orthogonality pass;
/// a 16-character DS holds them far more precisely.
const COSINE_TOLERANCE: f64 = 1e-4;

/// C.7.6.1.1.1 and C.7.6.2.1.1: Image Position and Image Orientation
/// (Patient) of each Frame are a pair, the direction cosines are orthogonal
/// unit vectors, and Patient Orientation agrees with them.
pub(super) fn patient_planes(
    object: &InMemDicomObject,
    groups: &FunctionalGroups<'_>,
    findings: &mut Findings,
) {
    let top = (
        object.get(tags::IMAGE_POSITION_PATIENT).is_some(),
        object.get(tags::IMAGE_ORIENTATION_PATIENT).is_some(),
    );
    if top.0 != top.1 {
        findings.push(|| unpaired("", top.0));
    }
    let orientation = strings(object, tags::PATIENT_ORIENTATION)
        .filter(|values| values.iter().any(|value| !value.is_empty()));
    let quadruped =
        string(object, tags::ANATOMICAL_ORIENTATION_TYPE).as_deref() == Some("QUADRUPED");
    for frame in 0..groups.frames() {
        let path = FrameLabel(frame);
        let position = groups.macro_item(frame, tags::PLANE_POSITION_SEQUENCE);
        let plane = groups.macro_item(frame, tags::PLANE_ORIENTATION_SEQUENCE);
        let cosines = plane.and_then(|plane| numbers(plane, tags::IMAGE_ORIENTATION_PATIENT));
        let has_position =
            position.is_some_and(|item| item.get(tags::IMAGE_POSITION_PATIENT).is_some());
        if has_position != cosines.is_some() {
            findings.push(|| unpaired(path, has_position));
        }
        let Some(cosines) = cosines.filter(|values| values.len() == 6) else {
            continue;
        };
        let (row, column) = (
            [cosines[0], cosines[1], cosines[2]],
            [cosines[3], cosines[4], cosines[5]],
        );
        cosine_constraints(path, row, column, findings);
        if let Some(orientation) = &orientation {
            for (index, (value, cosine)) in orientation.iter().zip([row, column]).enumerate() {
                if let Some(reason) = orientation_mismatch(value, cosine, quadruped) {
                    findings.push(|| format!(
                        "{path}Patient Orientation (0020,0020) Value {} {} {reason} of Image Orientation (Patient) {cosine:?}; it shall be consistent with Image Orientation (Patient) ({CITATION} C.7.6.1.1.1)",
                        index + 1,
                        shown(tags::PATIENT_ORIENTATION, value)
                    ));
                }
            }
        }
    }
}

/// `Frame <n>: `, for a message about a 0-based Frame.
#[derive(Clone, Copy)]
struct FrameLabel(usize);

impl std::fmt::Display for FrameLabel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Frame {}: ", self.0 + 1)
    }
}

fn unpaired(path: impl std::fmt::Display, has_position: bool) -> String {
    let (present, absent) = if has_position {
        (
            "Image Position (Patient) (0020,0032)",
            "Image Orientation (Patient) (0020,0037)",
        )
    } else {
        (
            "Image Orientation (Patient) (0020,0037)",
            "Image Position (Patient) (0020,0032)",
        )
    };
    format!(
        "{path}{present} is present without {absent}; they shall be provided as a pair ({CITATION} C.7.6.2.1.1)"
    )
}

fn cosine_constraints(path: FrameLabel, row: [f64; 3], column: [f64; 3], findings: &mut Findings) {
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let product = dot(row, column);
    if product.abs() > COSINE_TOLERANCE {
        findings.push(|| format!(
            "{path}Image Orientation (Patient) row {row:?} and column {column:?} have a dot product of {product}; they shall be orthogonal ({CITATION} C.7.6.2.1.1)"
        ));
    }
    for (name, vector) in [("row", row), ("column", column)] {
        let length = dot(vector, vector);
        if (length - 1.0).abs() > COSINE_TOLERANCE {
            findings.push(|| format!(
                "{path}Image Orientation (Patient) {name} direction cosine {vector:?} has a squared length of {length}; it shall be normal ({CITATION} C.7.6.2.1.1)"
            ));
        }
    }
}

/// Why a Patient Orientation Value disagrees with a direction cosine, or
/// `None` when it agrees: its first abbreviation names the principal
/// component and each one names a component of its sign.
fn orientation_mismatch(value: &str, cosine: [f64; 3], quadruped: bool) -> Option<String> {
    let directions: &[Direction<'_>] = if quadruped {
        &QUADRUPED_DIRECTIONS
    } else {
        &BIPED_DIRECTIONS
    };
    let abbreviations = split_abbreviations(value, directions)?;
    let largest = cosine
        .iter()
        .fold(0.0_f64, |largest, component| largest.max(component.abs()));
    for (index, (abbreviation, axes)) in abbreviations.iter().enumerate() {
        // Medial and lateral name no fixed axis.
        if axes.is_empty() {
            continue;
        }
        let agrees = axes.iter().any(|(axis, sign)| {
            let component = cosine[*axis] * sign;
            let threshold = if index == 0 {
                largest - COSINE_TOLERANCE
            } else {
                COSINE_TOLERANCE
            };
            component > 0.0 && component >= threshold
        });
        if !agrees {
            let role = if index == 0 {
                "names as principal a direction other than that"
            } else {
                "names a direction that is not one"
            };
            return Some(format!("abbreviation {abbreviation} {role}"));
        }
    }
    None
}

/// The abbreviations of a Value, read left to right; `None` when it is not
/// made of them (the format is the IOD relations check's to judge).
fn split_abbreviations<'a>(
    value: &str,
    directions: &'a [Direction<'a>],
) -> Option<Vec<Direction<'a>>> {
    let mut rest = value;
    let mut found = Vec::new();
    while !rest.is_empty() {
        // The longest abbreviation first: QUADRUPED's are one or two letters.
        let (abbreviation, axes) = directions
            .iter()
            .filter(|(abbreviation, _)| rest.starts_with(abbreviation))
            .max_by_key(|(abbreviation, _)| abbreviation.len())?;
        found.push((*abbreviation, *axes));
        rest = &rest[abbreviation.len()..];
    }
    Some(found)
}

/// Whether Pixel Data is encapsulated in a Standard Transfer Syntax.
pub(super) fn encapsulated(object: &DefaultDicomObject) -> bool {
    matches!(
        super::transfer_syntaxes::pixel_encoding(object.meta().transfer_syntax()),
        super::transfer_syntaxes::PixelEncoding::Encapsulated(_)
    )
}
