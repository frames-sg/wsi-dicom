//! Parametric Map constraints that the generated IOD tables cannot express
//! (PS3.3 A.75, C.8.32 and the Macros of A.75.5): Image Type and Frame Type
//! (C.8.16.1.3 and C.8.16.1.4 by Table C.8.32-2), the window and VOI LUT of
//! the Frame VOI LUT With LUT Macro (C.11.2.1), the Float and Double Float
//! Pixel Data encoding (PS3.5 8.2), Color Space, the Palette Color Lookup
//! Table, Frame Laterality and the patient image plane. The Synchronization
//! Module is `image_semantics::run_synchronization_check`'s.

use std::path::Path;

use dicom_core::value::Value;
use dicom_core::{Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::image_semantics::{
    color_space, encapsulated, frame_laterality, integer, integers, label, palette, patient_planes,
    string, strings, voi_window, Findings, FunctionalGroups, Iod, ItemPath, CITATION,
    PARAMETRIC_MAP_RULE, PS3_5,
};
use super::iod::{dicom_edition, items};
use super::ValidationCheck;

/// The Parametric Map check of one object; `held` is `None` when the source
/// holds stand-ins for the Pixel Data Values, whose Value Representation it
/// then does not show.
pub(super) fn run_parametric_map_check(
    path: &Path,
    object: &DefaultDicomObject,
    held: Option<usize>,
) -> Option<ValidationCheck> {
    if Iod::of(object) != Some(Iod::ParametricMap) {
        return None;
    }
    let root: &InMemDicomObject = object;
    let mut findings = Findings::default();
    let groups = FunctionalGroups::of(root);
    image_type(root, &groups, &mut findings);
    let photometric = string(root, tags::PHOTOMETRIC_INTERPRETATION);
    let signed = signed_input(root);
    let explicit = object
        .meta()
        .transfer_syntax()
        .trim_end_matches(['\0', ' '])
        != uids::IMPLICIT_VR_LITTLE_ENDIAN;
    for (path, voi) in groups.macro_items(tags::FRAME_VOILUT_SEQUENCE) {
        voi_window(voi, path, photometric.as_deref(), &mut findings);
        for (index, lut) in items(voi, tags::VOILUT_SEQUENCE).iter().enumerate() {
            let at = LutPath { voi: path, index };
            lut_descriptor(lut, at, signed, explicit, &mut findings);
        }
    }
    float_pixels(object, held.is_some(), &mut findings);
    color_space(root, "Table C.8.32-2", &mut findings);
    palette(root, false, &mut findings);
    frame_laterality(&groups, &mut findings);
    patient_planes(root, &groups, &mut findings);
    Some(findings.check(
        path,
        PARAMETRIC_MAP_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " A.75 and C.8.32"),
    ))
}

/// Table C.8.32-2 with C.8.16.1.3 and C.8.16.1.4: Value 3 of Image Type is
/// the Image Flavor, not zero length and not MIXED, and Value 4 the Derived
/// Pixel Contrast, not zero length; Value 4 is NONE when Value 1 is
/// ORIGINAL, in Image Type and in each Frame Type; and no Frame Type Value
/// is MIXED (Table C.8.32-3).
fn image_type(root: &InMemDicomObject, groups: &FunctionalGroups<'_>, findings: &mut Findings) {
    if let Some(values) = strings(root, tags::IMAGE_TYPE) {
        for (index, what) in [
            (2, "Value 3 (Image Flavor)"),
            (3, "Value 4 (Derived Pixel Contrast)"),
        ] {
            if values.get(index).is_none_or(String::is_empty) {
                findings.push(|| format!(
                    "Image Type (0008,0008) {what} is absent or zero length; it shall be present and not zero length ({CITATION} Table C.8.32-2, {})",
                    if index == 2 { "C.8.16.1.3" } else { "C.8.16.1.4" }
                ));
            }
        }
        if values.get(2).is_some_and(|flavor| flavor == "MIXED") {
            findings.push(|| format!(
                "Image Type (0008,0008) Value 3 (Image Flavor) is MIXED; it may not be MIXED, as it is a summary of the primary purpose of the images whether the Frames have the same Value or not ({CITATION} Table C.8.32-2 and C.8.16.1.3)"
            ));
        }
        original_contrast("", "Image Type (0008,0008)", &values, findings);
    }
    for (path, frame_type) in groups.macro_items(tags::PARAMETRIC_MAP_FRAME_TYPE_SEQUENCE) {
        let Some(values) = strings(frame_type, tags::FRAME_TYPE) else {
            continue;
        };
        if values.get(3).is_none_or(String::is_empty) {
            findings.push(|| format!(
                "{path}Frame Type (0008,9007) Value 4 is absent or zero length; it shall not be zero length ({CITATION} C.8.16.1.4)"
            ));
        }
        // Table C.8.32-3: Frame Type takes the Values of Image Type, except
        // that the Value MIXED is not allowed.
        for (index, _) in values
            .iter()
            .enumerate()
            .filter(|(_, value)| *value == "MIXED")
        {
            findings.push(|| format!(
                "{path}Frame Type (0008,9007) Value {} is MIXED; the Value MIXED is not allowed in a Parametric Map Frame Type ({CITATION} Table C.8.32-3)",
                index + 1
            ));
        }
        original_contrast(path, "Frame Type (0008,9007)", &values, findings);
    }
}

fn original_contrast(
    path: impl std::fmt::Display,
    name: &str,
    values: &[String],
    findings: &mut Findings,
) {
    if values.first().map(String::as_str) != Some("ORIGINAL") {
        return;
    }
    if let Some(contrast) = values.get(3).filter(|value| value.as_str() != "NONE") {
        findings.push(|| format!(
            "{path}{name} Value 1 is ORIGINAL and Value 4 is {contrast}; Value 4 shall be NONE when Value 1 is ORIGINAL ({CITATION} C.8.16.1.4)"
        ));
    }
}

/// Whether the VOI LUT input may be signed: the Identity Pixel Value
/// Transformation leaves the stored values, which are signed for Pixel
/// Representation 1 and for floating point pixels.
fn signed_input(root: &InMemDicomObject) -> bool {
    root.get(tags::FLOAT_PIXEL_DATA).is_some()
        || root.get(tags::DOUBLE_FLOAT_PIXEL_DATA).is_some()
        || integer(root, tags::PIXEL_REPRESENTATION) == Some(1)
}

/// Where a VOI LUT Sequence Item lies, formatted only when a message names it.
#[derive(Clone, Copy)]
struct LutPath {
    voi: ItemPath,
    index: usize,
}

impl std::fmt::Display for LutPath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}VOI LUT Sequence[{}] > ",
            self.voi,
            self.index + 1
        )
    }
}

/// C.11.2.1.1: the LUT Descriptor and LUT Data of a VOI LUT in an Image IOD.
fn lut_descriptor(
    lut: &InMemDicomObject,
    path: LutPath,
    signed: bool,
    explicit: bool,
    findings: &mut Findings,
) {
    let Some(descriptor) = lut.get(tags::LUT_DESCRIPTOR) else {
        return;
    };
    let expected = if signed { VR::SS } else { VR::US };
    let vr = descriptor.header().vr();
    if explicit && matches!(vr, VR::US | VR::SS) && vr != expected {
        findings.push(|| format!(
            "{path}LUT Descriptor (0028,3002) is encoded as {vr:?}, but the input to the VOI LUT {} signed; the Value Representation of its second Value shall be {expected:?} ({CITATION} C.11.2.1.1)",
            if signed { "may be" } else { "is not" }
        ));
    }
    // The first and third Values are unsigned whatever the VR.
    let values: Vec<i64> = integers(lut, tags::LUT_DESCRIPTOR)
        .unwrap_or_default()
        .into_iter()
        .map(|value| if value < 0 { value + 65536 } else { value })
        .collect();
    let [first, _, bits] = values[..] else {
        return;
    };
    if !matches!(bits, 8 | 16) {
        findings.push(|| format!(
            "{path}LUT Descriptor (0028,3002) third Value is {bits}; in an Image IOD it shall be 8 or 16 ({CITATION} C.11.2.1.1)"
        ));
        return;
    }
    let Some(data) = lut.get(tags::LUT_DATA) else {
        return;
    };
    let Value::Primitive(primitive) = data.value() else {
        return;
    };
    let length = primitive.calculate_byte_len();
    let entry_bytes = if bits == 8 { 1 } else { 2 };
    let entries = if first == 0 {
        65536
    } else {
        usize::try_from(first).unwrap_or(usize::MAX)
    };
    let stored = length / entry_bytes;
    let fits = if bits == 8 {
        length == entries + entries % 2
    } else {
        length == entries * 2
    };
    if fits {
        return;
    }
    if stored == 65536 && first != 0 {
        findings.push(|| format!(
            "{path}LUT Data (0028,3006) has 65536 entries, but the LUT Descriptor (0028,3002) first Value is {first}; when the number of table entries is 2^16 it shall be 0 ({CITATION} C.11.2.1.1)"
        ));
        return;
    }
    findings.push(|| format!(
        "{path}LUT Data (0028,3006) has {length} bytes for {entries} entries of {bits} bits; it shall be stored as if with {bits} bits allocated, {} bytes ({CITATION} C.11.2.1.1)",
        if bits == 8 { entries + entries % 2 } else { entries * 2 }
    ));
}

/// The floating point Pixel Data Attributes and the Bits Allocated each
/// requires (PS3.5 8.2).
const FLOAT_PIXELS: [(Tag, VR, i64); 2] = [
    (tags::FLOAT_PIXEL_DATA, VR::OF, 32),
    (tags::DOUBLE_FLOAT_PIXEL_DATA, VR::OD, 64),
];

/// PS3.5 8.2: Float and Double Float Pixel Data are Native in a Standard
/// Transfer Syntax, of VR OF or OD, with Bits Allocated 32 or 64 and no Bits
/// Stored, High Bit or Pixel Representation.
fn float_pixels(object: &DefaultDicomObject, held: bool, findings: &mut Findings) {
    let syntax = object
        .meta()
        .transfer_syntax()
        .trim_end_matches(['\0', ' ']);
    let standard = syntax.starts_with("1.2.840.10008.1.2");
    let explicit = syntax != uids::IMPLICIT_VR_LITTLE_ENDIAN;
    if !held
        && FLOAT_PIXELS
            .iter()
            .any(|(tag, ..)| object.get(*tag).is_some())
    {
        findings.unexamined(format!(
            "the encoding of Float or Double Float Pixel Data, which the source does not hold ({PS3_5} 8.2)"
        ));
    }
    for (tag, vr, bits) in FLOAT_PIXELS {
        let Some(element) = object.get(tag) else {
            continue;
        };
        let name = label(tag);
        let encapsulated_value = matches!(element.value(), Value::PixelSequence(_));
        if held && standard && (encapsulated_value || encapsulated(object)) {
            findings.push(|| format!(
                "{name} is encapsulated by Transfer Syntax {syntax}; in a Standard Transfer Syntax it shall be in a Native (uncompressed) Format ({PS3_5} 8.2)"
            ));
        }
        if held && explicit && !encapsulated_value && element.header().vr() != vr {
            findings.push(|| {
                format!(
                    "{name} has Value Representation {:?}; it shall be {vr:?} ({PS3_5} 8.2)",
                    element.header().vr()
                )
            });
        }
        if let Some(allocated) =
            integer(object, tags::BITS_ALLOCATED).filter(|value| *value != bits)
        {
            findings.push(|| format!(
                "Bits Allocated (0028,0100) is {allocated} with {name}; it shall be {bits} ({PS3_5} 8.2)"
            ));
        }
        for absent in [
            tags::BITS_STORED,
            tags::HIGH_BIT,
            tags::PIXEL_REPRESENTATION,
        ] {
            if object.get(absent).is_some() {
                findings.push(|| {
                    format!(
                        "{} is present with {name}; it shall not be present ({PS3_5} 8.2)",
                        label(absent)
                    )
                });
            }
        }
    }
}
