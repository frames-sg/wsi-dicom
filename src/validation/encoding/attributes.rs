//! Attribute Values whose encoding PS3.3 C.12.1, PS3.5 6.2, 8.3, 9 and Annex A
//! constrain beyond their VR, read from the parsed Data Set.

use dicom_core::value::{PrimitiveValue, Value};
use dicom_core::{Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::mem::InMemElement;
use dicom_object::InMemDicomObject;

use super::EXPLICIT_VR_BIG_ENDIAN;
use crate::validation::disclosure::{shown, Nesting, Shown};
use crate::validation::iod::{element_label, Family, MAX_NESTING};

/// PS3.5 9.1: the root of the UIDs that DICOM itself defines.
const DICOM_ROOT: &str = "1.2.840.10008.";
/// PS3.6 Annex A: the arc of the well-known Frames of Reference.
const WELL_KNOWN_FRAMES_OF_REFERENCE: &str = "1.2.840.10008.1.4.";

/// UIDs that identify privately defined items: instances, studies, series,
/// Frames of Reference, specimens and the like (PS3.5 9).
const INSTANCE_UIDS: &[Tag] = &[
    tags::SOP_INSTANCE_UID,
    tags::STUDY_INSTANCE_UID,
    tags::SERIES_INSTANCE_UID,
    tags::FRAME_OF_REFERENCE_UID,
    tags::DIMENSION_ORGANIZATION_UID,
    tags::CONCATENATION_UID,
    tags::SPECIMEN_UID,
    tags::INSTANCE_CREATOR_UID,
    tags::DEVICE_UID,
    tags::IRRADIATION_EVENT_UID,
    tags::PYRAMID_UID,
    tags::ACQUISITION_UID,
];

const DESCRIPTORS: &[Tag] = &[
    tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
    tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
    tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
    tags::LUT_DESCRIPTOR,
];

const SEGMENTED_PALETTES: &[Tag] = &[
    tags::SEGMENTED_RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
    tags::SEGMENTED_GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
    tags::SEGMENTED_BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA,
];

/// SNOMED CT and SNOMED-RT codes of Homo sapiens (PS3.16 CID 7454).
const HUMAN_SPECIES: &[(&str, &str)] = &[("SCT", "337915000"), ("SRT", "L-85B00")];

pub(super) struct Attributes<'f> {
    pub(super) findings: &'f mut Family,
    /// The Annex A section of a Little Endian Transfer Syntax; `None` for
    /// Big Endian, whose byte order rules differ.
    pub(super) little_endian: Option<&'static str>,
    /// Whether the Transfer Syntax conveys the VR, so the parsed VR is the
    /// encoded one.
    pub(super) explicit_vr: bool,
}

impl Attributes<'_> {
    pub(super) fn check(&mut self, root: &InMemDicomObject) {
        self.data_set(root, "", (0, Nesting::default()));
        self.veterinary_name(root);
    }

    /// `object` at `depth`, whose Values `nesting` withholds.
    fn data_set(
        &mut self,
        object: &InMemDicomObject,
        path: &str,
        (depth, nesting): (usize, Nesting),
    ) {
        self.private_definition(object, path);
        if self.explicit_vr {
            self.waveform(object, path);
        }
        for element in object.iter() {
            self.element(element, path, nesting);
            let Some(items) = element.items() else {
                continue;
            };
            // The value-format check reports Sequences nested deeper.
            if depth >= MAX_NESTING {
                continue;
            }
            let tag = element.header().tag;
            let label = element_label(path, tag);
            for (index, item) in items.iter().enumerate() {
                let nested = (depth + 1, nesting.item(tag, item));
                self.data_set(item, &format!("{label}[{}] > ", index + 1), nested);
            }
        }
    }

    fn element(&mut self, element: &InMemElement, path: &str, nesting: Nesting) {
        let tag = element.header().tag;
        let label = || element_label(path, tag);
        // Only the text Attributes below are read as text.
        let first = || texts(element).into_iter().next();
        match tag {
            tags::TIMEZONE_OFFSET_FROM_UTC => {
                if let Some(value) = first() {
                    self.timezone_offset(&label(), &value, nesting.shown(tag, &value));
                }
            }
            tags::DIGITAL_SIGNATURE_DATE_TIME => {
                let value = first().unwrap_or_default();
                let value = value.trim_end_matches(' ');
                let offset = value.get(4..).is_some_and(|rest| rest.contains(['+', '-']));
                if !value.is_empty() && !offset {
                    self.findings.push(|| format!(
                        "{} {}: no &ZZXX offset from UTC; the time shall include one (PS3.3 Table C.12-6)",
                        label(),
                        nesting.shown(tag, value)
                    ));
                }
            }
            tags::ENCRYPTED_CONTENT_TRANSFER_SYNTAX_UID
            | tags::MAC_CALCULATION_TRANSFER_SYNTAX_UID => {
                let uid = first().unwrap_or_default();
                let uid = uid.trim_end_matches(['\0', ' ']);
                let table = if tag == tags::MAC_CALCULATION_TRANSFER_SYNTAX_UID {
                    "C.12-6"
                } else {
                    "C.12-1"
                };
                let encoding = match uid {
                    uids::IMPLICIT_VR_LITTLE_ENDIAN => Some("an Implicit VR"),
                    EXPLICIT_VR_BIG_ENDIAN => Some("a Big Endian"),
                    _ => None,
                };
                if let Some(encoding) = encoding {
                    self.findings.push(|| format!(
                        "{} {uid} is {encoding} Transfer Syntax; only Explicit VR Little Endian Transfer Syntaxes shall be used (PS3.3 Table {table})",
                        label()
                    ));
                }
            }
            tags::IDENTIFYING_PRIVATE_ELEMENTS | tags::NONIDENTIFYING_PRIVATE_ELEMENTS => {
                let values = integers(element);
                if let Some(pair) = values.windows(2).find(|pair| pair[0] >= pair[1]) {
                    let (before, after) = (pair[0], pair[1]);
                    self.findings.push(|| format!(
                        "{}: Value {after:04X}H follows {before:04X}H; multiple Values shall be in increasing order and each listed at most once (PS3.3 Table C.12-1)",
                        label()
                    ));
                }
            }
            _ if INSTANCE_UIDS.contains(&tag) => {
                for uid in texts(element) {
                    let uid = uid.trim_end_matches(['\0', ' ']);
                    let well_known = tag == tags::FRAME_OF_REFERENCE_UID
                        && uid.starts_with(WELL_KNOWN_FRAMES_OF_REFERENCE);
                    if uid.starts_with(DICOM_ROOT) && !well_known {
                        self.findings.push(|| format!(
                            "{} {}: the root 1.2.840.10008 is reserved for DICOM defined items and shall not be used for privately defined items (PS3.5 9)",
                            label(),
                            nesting.shown(tag, uid)
                        ));
                    }
                }
            }
            _ if DESCRIPTORS.contains(&tag) => self.descriptor(element, &label()),
            _ if SEGMENTED_PALETTES.contains(&tag) => self.segmented_palette(element, &label()),
            _ => {}
        }
    }

    /// PS3.3 C.12.1.1.8: "&ZZXX", no leading spaces, and +0000 for UTC.
    /// `shown` is `value` as a message shows it.
    fn timezone_offset(&mut self, label: &str, value: &str, shown: Shown<'_>) {
        let offset = value.trim_end_matches(' ');
        let reason = if offset.starts_with(' ') {
            "leading space characters shall not be present"
        } else if offset == "-0000" {
            "the offset for UTC shall be +0000; -0000 shall not be used"
        } else if !(offset.len() == 5
            && offset.starts_with(['+', '-'])
            && offset[1..].bytes().all(|byte| byte.is_ascii_digit()))
        {
            "it shall be encoded as \"&ZZXX\""
        } else {
            return;
        };
        self.findings
            .push(|| format!("{label} {shown}: {reason} (PS3.3 C.12.1.1.8)"));
    }

    /// PS3.3 C.12.1.1.7: Private Data Element Value Multiplicity and Number
    /// of Items of one Private Data Element Definition Sequence Item.
    fn private_definition(&mut self, item: &InMemDicomObject, path: &str) {
        let element = |tag| item.get(tag);
        if let Some(multiplicity) = element(tags::PRIVATE_DATA_ELEMENT_VALUE_MULTIPLICITY) {
            let values = integers(multiplicity);
            let label = element_label(path, tags::PRIVATE_DATA_ELEMENT_VALUE_MULTIPLICITY);
            let sequence = element(tags::PRIVATE_DATA_ELEMENT_VALUE_REPRESENTATION)
                .and_then(|vr| texts(vr).into_iter().next())
                .is_some_and(|vr| vr.trim() == "SQ");
            if sequence && values != [1] {
                self.findings.push(|| format!(
                    "{label} {values:?}: for a Private Data Element Value Representation of SQ the multiplicity shall be 1 (PS3.3 C.12.1.1.7.1)"
                ));
            } else if fixed_as_range(&values) {
                self.findings.push(|| format!(
                    "{label} {values:?}: a fixed multiplicity shall be a single integer Value (PS3.3 C.12.1.1.7.1)"
                ));
            }
        }
        if let Some(count) = element(tags::PRIVATE_DATA_ELEMENT_NUMBER_OF_ITEMS) {
            let values = integers(count);
            if values.len() == 2 && fixed_as_range(&values) {
                let label = element_label(path, tags::PRIVATE_DATA_ELEMENT_NUMBER_OF_ITEMS);
                self.findings.push(|| format!(
                    "{label} {values:?}: a fixed number of Items shall be a single integer Value (PS3.3 C.12.1.1.7.2)"
                ));
            }
        }
    }

    /// PS3.5 8.3: in an Explicit VR Transfer Syntax Waveform Data is OB when
    /// Waveform Bits Allocated is 8 and OW otherwise, and Channel Minimum
    /// Value, Channel Maximum Value and Waveform Padding Value have its VR.
    /// A zero-length element means the same as an absent one (PS3.5 7.4.5).
    fn waveform(&mut self, item: &InMemDicomObject, path: &str) {
        let Some(data) = valued(item, tags::WAVEFORM_DATA) else {
            return;
        };
        let vr = data.vr();
        let label = |tag| element_label(path, tag);
        let bits = item
            .get(tags::WAVEFORM_BITS_ALLOCATED)
            .and_then(|element| element.to_int::<u16>().ok());
        // Without Waveform Bits Allocated, a presence violation the IOD
        // checks report, the required VR is unknown.
        if let Some(bits) = bits {
            let expected = if bits == 8 { VR::OB } else { VR::OW };
            if vr != expected {
                self.findings.push(|| format!(
                    "{}: VR {vr:?} with Waveform Bits Allocated (5400,1004) {bits}; Waveform Data shall be OW, and OB where Waveform Bits Allocated is 8 in an Explicit VR Transfer Syntax (PS3.5 8.3)",
                    label(tags::WAVEFORM_DATA)
                ));
            }
        }
        let channels = item
            .get(tags::CHANNEL_DEFINITION_SEQUENCE)
            .and_then(|element| element.items())
            .unwrap_or_default();
        let related = std::iter::once((path.to_string(), item, tags::WAVEFORM_PADDING_VALUE))
            .chain(channels.iter().enumerate().flat_map(|(index, channel)| {
                let channel_path = format!(
                    "{}[{}] > ",
                    element_label(path, tags::CHANNEL_DEFINITION_SEQUENCE),
                    index + 1
                );
                [tags::CHANNEL_MINIMUM_VALUE, tags::CHANNEL_MAXIMUM_VALUE]
                    .map(|tag| (channel_path.clone(), channel, tag))
            }));
        for (holder_path, holder, tag) in related {
            let Some(element) = valued(holder, tag) else {
                continue;
            };
            if element.vr() != vr {
                self.findings.push(|| format!(
                    "{}: VR {:?} where Waveform Data (5400,1010) is {vr:?}; it shall be encoded with the same VR as Waveform Data (PS3.5 8.3)",
                    element_label(&holder_path, tag),
                    element.vr()
                ));
            }
        }
    }

    /// PS3.5 A.1, A.2 and A.4: a Descriptor's third Value (bits per entry,
    /// 8 to 16) read byte-swapped shows a Big Endian encoding.
    fn descriptor(&mut self, element: &InMemElement, label: &str) {
        let Some(annex) = self.little_endian else {
            return;
        };
        let Some(&bits) = integers(element).get(2) else {
            return;
        };
        let bits = bits as u16;
        if !(1..=16).contains(&bits) && (8..=16).contains(&bits.swap_bytes()) {
            self.findings.push(|| format!(
                "{label}: third Value {bits} is {} byte-swapped, so the Descriptor is encoded Big Endian; it shall be encoded in Little Endian (PS3.5 {annex})",
                bits.swap_bytes()
            ));
        }
    }

    /// PS3.5 A.1, A.2 and A.4: segments (PS3.3 C.7.9.2) that parse only
    /// byte-swapped show a Big Endian encoding.
    fn segmented_palette(&mut self, element: &InMemElement, label: &str) {
        let Some(annex) = self.little_endian else {
            return;
        };
        let words = words(element);
        let swapped: Vec<u16> = words.iter().map(|word| word.swap_bytes()).collect();
        if !segments_parse(&words) && segments_parse(&swapped) {
            self.findings.push(|| format!(
                "{label}: the segments parse only byte-swapped, so the Value is encoded Big Endian; it shall be encoded in Little Endian (PS3.5 {annex})"
            ));
        }
    }

    /// PS3.5 6.2: a veterinary Patient's Name has only the responsible party
    /// and patient name components. A Patient Species Code other than Homo
    /// sapiens identifies a veterinary patient.
    fn veterinary_name(&mut self, root: &InMemDicomObject) {
        let animal = root
            .get(tags::PATIENT_SPECIES_CODE_SEQUENCE)
            .and_then(|element| element.items())
            .and_then(|items| items.first())
            .and_then(|item| {
                let scheme = item.get(tags::CODING_SCHEME_DESIGNATOR)?;
                let code = item.get(tags::CODE_VALUE)?;
                let scheme = texts(scheme).into_iter().next()?;
                let code = texts(code).into_iter().next()?;
                let (scheme, code) = (scheme.trim(), code.trim());
                HUMAN_SPECIES
                    .iter()
                    .any(|(human_scheme, _)| *human_scheme == scheme)
                    .then(|| !HUMAN_SPECIES.contains(&(scheme, code)))
            })
            .unwrap_or(false);
        let Some(name) = root.get(tags::PATIENT_NAME) else {
            return;
        };
        if !animal {
            return;
        }
        for value in texts(name) {
            let value = value.trim_end_matches(' ');
            if let Some((index, group)) = value
                .split('=')
                .enumerate()
                .find(|(_, group)| group.matches('^').count() > 1)
            {
                let components = group.matches('^').count() + 1;
                self.findings.push(|| format!(
                    "{} {}: component group {} of a veterinary Patient's Name has {components} components; only the responsible party and patient name are used and the remaining components shall not be present (PS3.5 6.2)",
                    element_label("", tags::PATIENT_NAME),
                    shown(tags::PATIENT_NAME, value),
                    index + 1
                ));
            }
        }
    }
}

/// Element `tag` of `holder`, unless absent or of zero length.
fn valued(holder: &InMemDicomObject, tag: Tag) -> Option<&InMemElement> {
    holder.get(tag).filter(|element| {
        !matches!(element.value(), Value::Primitive(primitive) if primitive.calculate_byte_len() == 0)
    })
}

/// Two or three Values whose minimum and (non-zero) maximum are equal.
fn fixed_as_range(values: &[i64]) -> bool {
    matches!(values, [minimum, maximum, ..] if minimum == maximum && *maximum != 0)
        && values.len() <= 3
}

fn texts(element: &InMemElement) -> Vec<String> {
    element
        .to_multi_str()
        .map(|values| values.to_vec())
        .unwrap_or_default()
}

fn integers(element: &InMemElement) -> Vec<i64> {
    element.to_multi_int::<i64>().unwrap_or_default()
}

fn words(element: &InMemElement) -> Vec<u16> {
    match element.value() {
        Value::Primitive(PrimitiveValue::U16(words)) => words.to_vec(),
        Value::Primitive(PrimitiveValue::U8(bytes)) => bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect(),
        _ => Vec::new(),
    }
}

/// PS3.3 C.7.9.2: discrete (0), linear (1) and indirect (2) segments that
/// end exactly at the end of the data, the first not linear.
fn segments_parse(words: &[u16]) -> bool {
    let mut index = 0;
    while index < words.len() {
        let Some(&length) = words.get(index + 1) else {
            return false;
        };
        let size = match words[index] {
            0 => 2 + usize::from(length),
            1 if index > 0 => 3,
            2 => 4,
            _ => return false,
        };
        index += size;
    }
    index == words.len() && !words.is_empty()
}
