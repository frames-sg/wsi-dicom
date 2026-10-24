//! The DICOM JSON Model (PS3.18 Annex F): each Attribute object checked
//! against F.2.2 to F.2.7 and read into an in-memory Data Set.
//!
//! Values are read as their DICOM Value Representation holds them, so the
//! PS3.5 6.2 Definition, Character Repertoire and Length of Value checks that
//! F.2.3 requires are the IOD value checks' to apply to the converted object;
//! this module checks what only the JSON shows: member names and order, JSON
//! types, null values, InlineBinary and BulkDataURI.

use std::collections::{BTreeMap, BTreeSet};

use base64::Engine;
use dicom_core::smallvec::SmallVec;
use dicom_core::value::{DataSetSequence, PrimitiveValue, Value};
use dicom_core::{DataElement, Tag, VR};
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::json::Json;
use crate::validation::iod::{check_value_format, Family, EDITION, MAX_NESTING};

/// Where the document comes from, which decides whether PS3.18 10.4.1.1.2
/// (no File Meta Information in Metadata) applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// A saved DICOM JSON document.
    File,
    /// The response of a Retrieve Transaction on a Metadata resource.
    Metadata,
}

/// One Attribute the document gives as a BulkDataURI rather than a Value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BulkReference {
    /// JSON Pointer (RFC 6901) to the Attribute object.
    pub(crate) pointer: String,
    pub(crate) tag: Tag,
    pub(crate) vr: VR,
    pub(crate) uri: String,
    /// Pixel Data, Float Pixel Data or Double Float Pixel Data of the
    /// top-level Data Set, which only Frames retrieval checks.
    pub(crate) pixel_data: bool,
}

/// What a BulkDataURI Attribute holds while it is read: the fetched Value,
/// or one of two stand-ins whose difference shows which checks depend on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stand {
    /// Short Values of zeros.
    Sparse,
    /// Values a check would accept where it can: an sRGB ICC Profile, Pixel
    /// Data of the Icon Image's declared size, ones elsewhere.
    Plausible,
}

/// Reads Data Sets out of DICOM JSON Model objects.
pub(super) struct Reader<'r> {
    pub(super) source: Source,
    pub(super) stand: Stand,
    /// Bulk Data Values fetched by URI.
    pub(super) fetched: &'r BTreeMap<String, Vec<u8>>,
    /// Model violations; `None` while re-reading for a second stand-in.
    pub(super) violations: Option<&'r mut Family>,
    pub(super) bulk: Vec<BulkReference>,
    /// Group 0002 Attributes the document holds, by tag.
    pub(super) file_meta: Vec<(Tag, String)>,
}

/// The value tokens of a VR the model writes as JSON Strings.
const STRING_VRS: &[VR] = &[
    VR::AE,
    VR::AS,
    VR::CS,
    VR::DA,
    VR::DT,
    VR::LO,
    VR::LT,
    VR::SH,
    VR::ST,
    VR::TM,
    VR::UC,
    VR::UI,
    VR::UR,
    VR::UT,
];
const BINARY_VRS: &[VR] = &[VR::OB, VR::OD, VR::OF, VR::OL, VR::OV, VR::OW, VR::UN];
/// F.2.2 BulkDataURI, which PS3.18 10.4.3.3.2 repeats for Metadata.
const BULK_VRS: &[VR] = &[
    VR::DS,
    VR::FL,
    VR::FD,
    VR::IS,
    VR::LT,
    VR::OB,
    VR::OD,
    VR::OF,
    VR::OL,
    VR::OV,
    VR::OW,
    VR::SL,
    VR::SS,
    VR::ST,
    VR::SV,
    VR::UC,
    VR::UL,
    VR::UN,
    VR::US,
    VR::UT,
    VR::UV,
];
const PIXEL_DATA_TAGS: [Tag; 3] = [
    tags::PIXEL_DATA,
    tags::FLOAT_PIXEL_DATA,
    tags::DOUBLE_FLOAT_PIXEL_DATA,
];

fn cite(section: &str) -> String {
    format!("PS3.18 {EDITION} {section}")
}

/// RFC 6901 escaping of one reference token.
pub(super) fn pointer_token(name: &str) -> String {
    name.replace('~', "~0").replace('/', "~1")
}

impl Reader<'_> {
    fn violation(&mut self, pointer: &str, message: impl FnOnce() -> String) {
        if let Some(family) = self.violations.as_deref_mut() {
            family.push(|| format!("{pointer}: {}", message()));
        }
    }

    /// Reads one DICOM JSON Model object (F.2.2) at `pointer` as a Data Set.
    pub(super) fn data_set(
        &mut self,
        members: &[(String, Json<'_>)],
        pointer: &str,
        depth: usize,
    ) -> InMemDicomObject {
        let mut object = InMemDicomObject::new_empty();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut previous: Option<&str> = None;
        for (name, value) in members {
            let here = format!("{pointer}/{}", pointer_token(name));
            if !seen.insert(name.as_str()) {
                self.violation(&here, || format!(
                    "Attribute {name} appears more than once in one object; the DICOM JSON Model allows each Tag once ({})",
                    cite("F.3.1")
                ));
                continue;
            }
            if let Some(previous) = previous.filter(|previous| name.as_str() < *previous) {
                self.violation(&here, || format!(
                    "Attribute {name} follows {previous}; Attribute objects shall be ordered by property name in ascending order ({})",
                    cite("F.2.2")
                ));
            }
            previous = Some(previous.map_or(name.as_str(), |previous| previous.max(name.as_str())));
            let Some(tag) = self.tag_name(name, &here) else {
                continue;
            };
            let Json::Object(attribute) = value else {
                self.violation(&here, || {
                    format!(
                        "the Attribute is {}, not an Attribute object ({})",
                        value.kind(),
                        cite("F.2.2")
                    )
                });
                continue;
            };
            if tag.element() == 0x0000 {
                self.violation(&here, || {
                    format!(
                        "Group Length ({:04X},0000) shall not be included ({})",
                        tag.group(),
                        cite("F.2.2")
                    )
                });
                continue;
            }
            self.check_private(tag, attribute, &object, &here);
            let Some(element) = self.attribute(tag, attribute, &object, &here, depth) else {
                continue;
            };
            if tag.group() == 0x0002 {
                self.file_meta_attribute(&element, &here);
                continue;
            }
            object.put(element);
        }
        object
    }

    /// F.2.2: "The eight character uppercase hexadecimal representation of a
    /// DICOM Tag".
    fn tag_name(&mut self, name: &str, here: &str) -> Option<Tag> {
        let hex = name.len() == 8 && name.bytes().all(|byte| byte.is_ascii_hexdigit());
        if !hex {
            self.violation(here, || format!(
                "property name {name:?} is not the eight character uppercase hexadecimal representation of a Tag ({})",
                cite("F.2.2")
            ));
            return None;
        }
        if name.bytes().any(|byte| byte.is_ascii_lowercase()) {
            self.violation(here, || {
                format!(
                    "property name {name} uses lowercase hexadecimal; Tags are uppercase ({})",
                    cite("F.2.2")
                )
            });
        }
        let value = u32::from_str_radix(name, 16).ok()?;
        Some(Tag((value >> 16) as u16, (value & 0xFFFF) as u16))
    }

    /// F.2.2 "For Private Data Elements, the group and element numbers will
    /// follow the rules specified in PS3.5 7.8.1": a Private Creator is LO,
    /// and a Private Data Element follows its Private Creator.
    fn check_private(
        &mut self,
        tag: Tag,
        attribute: &[(String, Json<'_>)],
        object: &InMemDicomObject,
        here: &str,
    ) {
        if tag.group().is_multiple_of(2) {
            return;
        }
        let element = tag.element();
        if (0x0010..=0x00FF).contains(&element) {
            let vr = attribute.iter().find_map(|(name, value)| match value {
                Json::String(vr) if name == "vr" => Some(vr.as_str()),
                _ => None,
            });
            if vr.is_some_and(|vr| vr != "LO") {
                self.violation(here, || format!(
                    "Private Creator ({:04X},{element:04X}) has VR {}; a Private Creator Data Element is LO (PS3.5 {EDITION} 7.8.1, {})",
                    tag.group(),
                    vr.unwrap_or_default(),
                    cite("F.2.2")
                ));
            }
        } else if element >= 0x1000 {
            let creator = Tag(tag.group(), element >> 8);
            if object.get(creator).is_none() {
                self.violation(here, || format!(
                    "Private Data Element ({:04X},{element:04X}) has no Private Creator ({:04X},{:04X}) in its object (PS3.5 {EDITION} 7.8.1, {})",
                    tag.group(),
                    creator.group(),
                    creator.element(),
                    cite("F.2.2")
                ));
            }
        }
    }

    /// Group 0002 in Metadata violates PS3.18 10.4.1.1.2; in a saved document
    /// it is kept as the File Meta Information the document declares.
    fn file_meta_attribute(&mut self, element: &DataElement<InMemDicomObject>, here: &str) {
        let tag = element.header().tag;
        if self.source == Source::Metadata {
            self.violation(here, || {
                format!(
                "({:04X},{:04X}) is File Meta Information, which Metadata does not include ({})",
                tag.group(),
                tag.element(),
                cite("10.4.1.1.2")
            )
            });
        }
        if let Ok(value) = element.to_str() {
            self.file_meta
                .push((tag, value.trim_end_matches(['\0', ' ']).to_string()));
        }
    }

    /// One Attribute object (F.2.2): its "vr" and at most one of "Value",
    /// "BulkDataURI" and "InlineBinary".
    fn attribute(
        &mut self,
        tag: Tag,
        members: &[(String, Json<'_>)],
        parent: &InMemDicomObject,
        here: &str,
        depth: usize,
    ) -> Option<DataElement<InMemDicomObject>> {
        let mut seen: Vec<&str> = Vec::new();
        let mut vr_member = None;
        let mut payload: Vec<(&str, &Json<'_>)> = Vec::new();
        for (name, value) in members {
            if seen.contains(&name.as_str()) {
                self.violation(here, || {
                    format!(
                        "member {name:?} appears more than once in the Attribute object ({})",
                        cite("F.2.2")
                    )
                });
                continue;
            }
            seen.push(name.as_str());
            match name.as_str() {
                "vr" => vr_member = Some(value),
                "Value" | "BulkDataURI" | "InlineBinary" => payload.push((name.as_str(), value)),
                _ => self.violation(here, || format!(
                    "member {name:?} is not part of an Attribute object, which holds \"vr\" and at most one of \"Value\", \"BulkDataURI\" and \"InlineBinary\" ({})",
                    cite("F.2.2")
                )),
            }
        }
        let vr = self.vr(vr_member, here)?;
        if payload.len() > 1 {
            let names: Vec<&str> = payload.iter().map(|(name, _)| *name).collect();
            self.violation(here, || {
                format!(
                    "the Attribute object holds {}; it holds at most one of them ({})",
                    names.join(" and "),
                    cite("F.2.2")
                )
            });
        }
        let value = match payload.first() {
            None => empty(vr),
            Some(("Value", value)) => self.value(tag, vr, value, here, depth)?,
            Some(("InlineBinary", value)) => self.inline_binary(vr, value, here)?,
            Some((_, value)) => self.bulk_data(tag, vr, value, parent, here, depth)?,
        };
        Some(DataElement::new(tag, vr, value))
    }

    fn vr(&mut self, member: Option<&Json<'_>>, here: &str) -> Option<VR> {
        let Some(member) = member else {
            self.violation(here, || {
                format!(
                    "the Attribute object has no \"vr\"; each Attribute object includes its VR ({})",
                    cite("F.2.3")
                )
            });
            return None;
        };
        let Json::String(text) = member else {
            self.violation(here, || {
                format!(
                    "\"vr\" is {}, not a String ({})",
                    member.kind(),
                    cite("F.2.2")
                )
            });
            return None;
        };
        let vr = text.parse::<VR>().ok();
        if vr.is_none() {
            self.violation(here, || {
                format!(
                    "\"vr\" {text:?} is not a Value Representation of Table F.2.3-1 ({})",
                    cite("F.2.3")
                )
            });
        }
        vr
    }

    /// F.2.4 "Value": a non-empty Array, element types per Table F.2.3-1.
    fn value(
        &mut self,
        tag: Tag,
        vr: VR,
        value: &Json<'_>,
        here: &str,
        depth: usize,
    ) -> Option<Value<InMemDicomObject>> {
        let here = format!("{here}/Value");
        let Json::Array(elements) = value else {
            self.violation(&here, || {
                format!(
                    "\"Value\" is {}, not an Array ({})",
                    value.kind(),
                    cite("F.2.4")
                )
            });
            return None;
        };
        if BINARY_VRS.contains(&vr) {
            self.violation(&here, || format!(
                "a {vr} Attribute has a \"Value\"; OB, OD, OF, OL, OV, OW and UN are encoded as InlineBinary or BulkDataURI ({})",
                cite("F.2.2")
            ));
            return None;
        }
        let empty_value = elements.is_empty()
            || (vr != VR::SQ && elements.len() == 1 && is_empty_value(&elements[0]));
        if empty_value {
            self.violation(&here, || format!(
                "an empty Attribute (Value Length 0) shall be encoded without \"Value\", \"BulkDataURI\" or \"InlineBinary\", not as {} ({})",
                if elements.is_empty() { "an empty Array".to_string() } else { format!("[{}]", render(&elements[0])) },
                cite("F.2.5")
            ));
        }
        Some(match vr {
            VR::SQ => Value::from(DataSetSequence::from(self.items(elements, &here, depth))),
            VR::PN => Value::Primitive(self.person_names(elements, &here)),
            _ => Value::Primitive(self.primitive(tag, vr, elements, &here)),
        })
    }

    fn items(&mut self, elements: &[Json<'_>], here: &str, depth: usize) -> Vec<InMemDicomObject> {
        if depth >= MAX_NESTING {
            self.violation(here, || {
                format!("Sequence Items nest deeper than {MAX_NESTING} levels; deeper Items are not read")
            });
            return Vec::new();
        }
        let mut items = Vec::with_capacity(elements.len());
        for (index, element) in elements.iter().enumerate() {
            let item_pointer = format!("{here}/{index}");
            match element {
                Json::Object(members) => {
                    items.push(self.data_set(members, &item_pointer, depth + 1))
                }
                other => {
                    self.violation(&item_pointer, || format!(
                        "a Sequence Item is {}; Items are DICOM JSON Model objects, an empty Item an empty object ({})",
                        other.kind(),
                        cite("F.2.5")
                    ));
                    items.push(InMemDicomObject::new_empty());
                }
            }
        }
        items
    }

    /// F.2.2: each PN Value is an object of the non-empty component groups
    /// "Alphabetic", "Ideographic" and "Phonetic"; an empty Value is null.
    fn person_names(&mut self, elements: &[Json<'_>], here: &str) -> PrimitiveValue {
        let mut names = SmallVec::<[String; 2]>::new();
        for (index, element) in elements.iter().enumerate() {
            let at = format!("{here}/{index}");
            match element {
                Json::Null => names.push(String::new()),
                Json::Object(members) => {
                    let name = self.person_name(members, &at);
                    if !name.is_empty() {
                        self.encoding(VR::PN, &name, &at);
                    }
                    names.push(name);
                }
                other => {
                    self.violation(&at, || format!(
                        "a PN Value is {}; Table F.2.3-1 requires an object of Person Name component groups ({})",
                        other.kind(),
                        cite("F.2.3")
                    ));
                    if let Json::String(text) = other {
                        names.push(text.clone());
                    }
                }
            }
        }
        PrimitiveValue::Strs(names)
    }

    fn person_name(&mut self, members: &[(String, Json<'_>)], at: &str) -> String {
        let mut groups: [Option<String>; 3] = [None, None, None];
        for (name, value) in members {
            let slot = match name.as_str() {
                "Alphabetic" => 0,
                "Ideographic" => 1,
                "Phonetic" => 2,
                _ => {
                    self.violation(at, || format!(
                        "PN member {name:?} is not \"Alphabetic\", \"Ideographic\" or \"Phonetic\" ({})",
                        cite("F.2.2")
                    ));
                    continue;
                }
            };
            if groups[slot].is_some() {
                self.violation(at, || {
                    format!(
                        "PN member {name:?} appears more than once ({})",
                        cite("F.2.2")
                    )
                });
                continue;
            }
            match value {
                Json::String(text) if text.is_empty() => {
                    self.violation(at, || format!(
                        "PN component group {name:?} is empty; only non-empty component groups are encoded ({})",
                        cite("F.2.2")
                    ));
                    groups[slot] = Some(String::new());
                }
                Json::String(text) => groups[slot] = Some(text.clone()),
                other => self.violation(at, || {
                    format!(
                        "PN component group {name:?} is {}, not a String ({})",
                        other.kind(),
                        cite("F.2.2")
                    )
                }),
            }
        }
        if groups.iter().all(Option::is_none) {
            self.violation(at, || {
                format!(
                    "a PN Value object has no component group; an empty Value is null ({})",
                    cite("F.2.5")
                )
            });
        }
        let mut joined = groups
            .iter()
            .map(|group| group.as_deref().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("=");
        while joined.ends_with('=') {
            joined.pop();
        }
        joined
    }

    fn primitive(&mut self, tag: Tag, vr: VR, elements: &[Json<'_>], here: &str) -> PrimitiveValue {
        match vr {
            VR::AT => self.tags(elements, here),
            VR::DS | VR::IS => self.numeric_strings(vr, elements, here),
            VR::US | VR::SS | VR::UL | VR::SL | VR::SV | VR::UV => {
                self.integers(vr, elements, here)
            }
            VR::FL | VR::FD => self.floats(vr, elements, here),
            _ => {
                debug_assert!(STRING_VRS.contains(&vr), "{tag} {vr}");
                self.strings(vr, elements, here)
            }
        }
    }

    fn strings(&mut self, vr: VR, elements: &[Json<'_>], here: &str) -> PrimitiveValue {
        let mut values = SmallVec::<[String; 2]>::new();
        for (index, element) in elements.iter().enumerate() {
            let at = format!("{here}/{index}");
            match element {
                Json::Null => values.push(String::new()),
                Json::String(text) => {
                    if text.is_empty() && elements.len() > 1 {
                        self.empty_string(&at);
                    }
                    if !text.is_empty() {
                        self.encoding(vr, text, &at);
                    }
                    values.push(text.clone());
                }
                Json::Number(text) => {
                    self.wrong_type(&at, vr, element, "a String");
                    values.push((*text).to_string());
                }
                other => self.wrong_type(&at, vr, other, "a String"),
            }
        }
        PrimitiveValue::Strs(values)
    }

    /// F.1: "a data source that is creating a new instance of a DICOM JSON
    /// Model shall follow the DICOM encoding rules in creating Values", and
    /// F.2.3: the encodings "shall conform to the Definition, Character
    /// Repertoire (if applicable) and Length of Value" of the VR (PS3.5
    /// 6.2), padding to even length aside.
    fn encoding(&mut self, vr: VR, value: &str, at: &str) {
        let unpadded = match vr {
            VR::UI => value.strip_suffix('\0'),
            _ => value.strip_suffix(' '),
        }
        .unwrap_or(value);
        if let Err(reason) = check_value_format(vr, unpadded) {
            self.violation(at, || format!(
                "{vr} Value {value:?}: {reason}; a Value follows the Definition, Character Repertoire and Length of Value of its VR (PS3.5 {EDITION} 6.2, {})",
                cite("F.1 and F.2.3")
            ));
        }
    }

    fn empty_string(&mut self, at: &str) {
        self.violation(at, || {
            format!(
                "an empty Value is encoded as null, not as an empty String ({})",
                cite("F.2.5")
            )
        });
    }

    fn wrong_type(&mut self, at: &str, vr: VR, found: &Json<'_>, required: &str) {
        let found = found.kind();
        self.violation(at, || {
            format!(
                "a {vr} Value is {found}; Table F.2.3-1 requires {required} ({})",
                cite("F.2.3")
            )
        });
    }

    /// F.2.3: "Attributes with a Value Representation of AT shall be
    /// restricted to eight character uppercase hexadecimal representation".
    fn tags(&mut self, elements: &[Json<'_>], here: &str) -> PrimitiveValue {
        let mut values = SmallVec::<[Tag; 2]>::new();
        for (index, element) in elements.iter().enumerate() {
            let at = format!("{here}/{index}");
            let Json::String(text) = element else {
                self.violation(&at, || {
                    format!(
                    "an AT Value is {}; it is a String of eight uppercase hexadecimal digits ({})",
                    element.kind(),
                    cite("F.2.3")
                )
                });
                continue;
            };
            let valid = text.len() == 8
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte));
            match u32::from_str_radix(text, 16) {
                Ok(value) if valid => values.push(Tag((value >> 16) as u16, value as u16)),
                _ => self.violation(&at, || format!(
                    "AT Value {text:?} is not the eight character uppercase hexadecimal representation of a Tag ({})",
                    cite("F.2.3")
                )),
            }
        }
        PrimitiveValue::Tags(values)
    }

    /// DS and IS: a Number or a String (Table F.2.3-1); the String's format
    /// is the IOD value checks' to judge. An IS Number is an integer.
    fn numeric_strings(&mut self, vr: VR, elements: &[Json<'_>], here: &str) -> PrimitiveValue {
        let mut values = SmallVec::<[String; 2]>::new();
        for (index, element) in elements.iter().enumerate() {
            let at = format!("{here}/{index}");
            match element {
                Json::Null => values.push(String::new()),
                Json::String(text) => {
                    if text.is_empty() && elements.len() > 1 {
                        self.empty_string(&at);
                    }
                    if !text.is_empty() {
                        self.encoding(vr, text, &at);
                    }
                    values.push(text.clone());
                }
                Json::Number(text) => {
                    if vr == VR::IS && !is_integer_literal(text) {
                        self.violation(&at, || format!(
                            "IS Number {text} is not an integer; an Integer String holds an integer (PS3.5 {EDITION} 6.2, {})",
                            cite("F.2.3")
                        ));
                    }
                    values.push((*text).to_string());
                }
                other => self.wrong_type(&at, vr, other, "a Number or String"),
            }
        }
        PrimitiveValue::Strs(values)
    }

    /// US, SS, UL, SL: Numbers. SV, UV: Numbers or decimal Strings.
    fn integers(&mut self, vr: VR, elements: &[Json<'_>], here: &str) -> PrimitiveValue {
        let (min, max): (i128, i128) = match vr {
            VR::US => (0, u16::MAX.into()),
            VR::SS => (i16::MIN.into(), i16::MAX.into()),
            VR::UL => (0, u32::MAX.into()),
            VR::SL => (i32::MIN.into(), i32::MAX.into()),
            VR::SV => (i64::MIN.into(), i64::MAX.into()),
            _ => (0, u64::MAX.into()),
        };
        let strings_allowed = matches!(vr, VR::SV | VR::UV);
        let mut values: Vec<i128> = Vec::with_capacity(elements.len());
        for (index, element) in elements.iter().enumerate() {
            let at = format!("{here}/{index}");
            let text = match element {
                Json::Number(text) => *text,
                Json::String(text) if strings_allowed => text.as_str(),
                Json::Null => {
                    self.binary_null(&at, vr);
                    continue;
                }
                other => {
                    let required = if strings_allowed {
                        "a Number or String"
                    } else {
                        "a Number"
                    };
                    self.wrong_type(&at, vr, other, required);
                    continue;
                }
            };
            match integer_value(text) {
                Some(value) if (min..=max).contains(&value) => values.push(value),
                Some(_) => self.violation(&at, || {
                    format!(
                        "{vr} Value {text} is outside {min} to {max} (PS3.5 {EDITION} 6.2, {})",
                        cite("F.2.3")
                    )
                }),
                None => self.violation(&at, || {
                    format!(
                        "{vr} Value {text} is not an integer (PS3.5 {EDITION} 6.2, {})",
                        cite("F.2.3")
                    )
                }),
            }
        }
        integer_primitive(vr, &values)
    }

    fn binary_null(&mut self, at: &str, vr: VR) {
        self.violation(at, || format!(
            "a {vr} Value is null; a binary VR has no empty Value within a multi-valued Attribute (PS3.5 {EDITION} 6.4, {})",
            cite("F.2.5")
        ));
    }

    fn floats(&mut self, vr: VR, elements: &[Json<'_>], here: &str) -> PrimitiveValue {
        let mut values = Vec::with_capacity(elements.len());
        for (index, element) in elements.iter().enumerate() {
            let at = format!("{here}/{index}");
            match element {
                Json::Number(text) => {
                    let value = text.parse::<f64>().unwrap_or(f64::INFINITY);
                    let finite =
                        value.is_finite() && (vr == VR::FD || value.abs() <= f64::from(f32::MAX));
                    if finite {
                        values.push(value);
                    } else {
                        self.violation(&at, || format!(
                            "{vr} Value {text} is outside the range of an IEEE 754 {} (PS3.5 {EDITION} 6.2, {})",
                            if vr == VR::FL { "binary32" } else { "binary64" },
                            cite("F.2.3")
                        ));
                    }
                }
                Json::Null => self.binary_null(&at, vr),
                other => self.wrong_type(&at, vr, other, "a Number"),
            }
        }
        if vr == VR::FL {
            PrimitiveValue::F32(values.iter().map(|value| *value as f32).collect())
        } else {
            PrimitiveValue::F64(values.into_iter().collect())
        }
    }

    /// F.2.7 InlineBinary: the base64 encoding of an OB, OD, OF, OL, OV, OW
    /// or UN Value Field.
    fn inline_binary(
        &mut self,
        vr: VR,
        value: &Json<'_>,
        here: &str,
    ) -> Option<Value<InMemDicomObject>> {
        let here = format!("{here}/InlineBinary");
        if !BINARY_VRS.contains(&vr) {
            self.violation(&here, || format!(
                "a {vr} Attribute has InlineBinary, which encodes only OB, OD, OF, OL, OV, OW and UN ({})",
                cite("F.2.2")
            ));
            return None;
        }
        let Json::String(text) = value else {
            self.violation(&here, || {
                format!(
                    "InlineBinary is {}, not a base64 String ({})",
                    value.kind(),
                    cite("F.2.7")
                )
            });
            return None;
        };
        let bytes = match base64::engine::general_purpose::STANDARD.decode(text) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.violation(&here, || {
                    format!(
                        "InlineBinary is not base64 (RFC 4648 4, with padding): {err} ({})",
                        cite("F.2.7")
                    )
                });
                return None;
            }
        };
        self.binary_length(vr, bytes.len(), &here);
        Some(Value::Primitive(binary_value(vr, &bytes)))
    }

    /// PS3.5 6.2: an OW, OF, OL, OD or OV Value Field is a whole number of
    /// its words.
    fn binary_length(&mut self, vr: VR, length: usize, here: &str) {
        let word = word_size(vr);
        if !length.is_multiple_of(word) {
            self.violation(here, || format!(
                "{vr} Value Field of {length} bytes is not a multiple of {word} bytes (PS3.5 {EDITION} 6.2)"
            ));
        }
    }

    /// F.2.6 BulkDataURI, for the VRs F.2.2 lists, in a form PS3.18
    /// 10.4.1.1.5.1 defines.
    fn bulk_data(
        &mut self,
        tag: Tag,
        vr: VR,
        value: &Json<'_>,
        parent: &InMemDicomObject,
        here: &str,
        depth: usize,
    ) -> Option<Value<InMemDicomObject>> {
        let at = format!("{here}/BulkDataURI");
        if !BULK_VRS.contains(&vr) {
            self.violation(&at, || format!(
                "a {vr} Attribute has a BulkDataURI, which only DS, FL, FD, IS, LT, OB, OD, OF, OL, OV, OW, SL, SS, ST, SV, UC, UL, UN, US, UT and UV Attributes may have ({}, {})",
                cite("F.2.2"),
                cite("10.4.3.3.2")
            ));
            return None;
        }
        let Json::String(uri) = value else {
            self.violation(&at, || {
                format!(
                    "BulkDataURI is {}, not a String ({})",
                    value.kind(),
                    cite("F.2.6")
                )
            });
            return None;
        };
        if let Err(reason) = bulk_uri_form(uri) {
            self.violation(&at, || {
                format!("BulkDataURI {uri:?} {reason} ({})", cite("10.4.1.1.5.1"))
            });
        }
        let pixel_data = depth == 0 && PIXEL_DATA_TAGS.contains(&tag);
        // Group 0002 is kept out of the Data Set, so its Values are not read.
        if tag.group() != 0x0002 {
            self.bulk.push(BulkReference {
                pointer: here.to_string(),
                tag,
                vr,
                uri: uri.clone(),
                pixel_data,
            });
        }
        if let Some(bytes) = self.fetched.get(uri).filter(|_| !pixel_data) {
            return Some(Value::Primitive(fetched_value(vr, bytes)));
        }
        Some(Value::Primitive(stand_in(
            tag, vr, parent, pixel_data, self.stand,
        )))
    }
}

fn empty(vr: VR) -> Value<InMemDicomObject> {
    if vr == VR::SQ {
        Value::from(DataSetSequence::from(Vec::<InMemDicomObject>::new()))
    } else {
        Value::Primitive(PrimitiveValue::Empty)
    }
}

fn is_empty_value(element: &Json<'_>) -> bool {
    matches!(element, Json::Null) || matches!(element, Json::String(text) if text.is_empty())
}

fn render(element: &Json<'_>) -> String {
    match element {
        Json::Null => "null".into(),
        Json::Bool(value) => value.to_string(),
        Json::String(text) => format!("{text:?}"),
        other => other.kind().into(),
    }
}

/// An RFC 8259 number without fraction or exponent.
fn is_integer_literal(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// The exact integer `text` writes: an integer literal, or a Number whose
/// fraction and exponent leave an integer within 2^53.
fn integer_value(text: &str) -> Option<i128> {
    if is_integer_literal(text) {
        return text.parse::<i128>().ok();
    }
    let value = text.parse::<f64>().ok()?;
    (value.is_finite() && value.fract() == 0.0 && value.abs() <= 9_007_199_254_740_992.0)
        .then_some(value as i128)
}

fn integer_primitive(vr: VR, values: &[i128]) -> PrimitiveValue {
    // Values were range-checked against `vr`, so these conversions are exact.
    match vr {
        VR::US => PrimitiveValue::U16(values.iter().map(|value| *value as u16).collect()),
        VR::SS => PrimitiveValue::I16(values.iter().map(|value| *value as i16).collect()),
        VR::UL => PrimitiveValue::U32(values.iter().map(|value| *value as u32).collect()),
        VR::SL => PrimitiveValue::I32(values.iter().map(|value| *value as i32).collect()),
        VR::SV => PrimitiveValue::I64(values.iter().map(|value| *value as i64).collect()),
        _ => PrimitiveValue::U64(values.iter().map(|value| *value as u64).collect()),
    }
}

fn word_size(vr: VR) -> usize {
    match vr {
        VR::OW => 2,
        VR::OF | VR::OL => 4,
        VR::OD | VR::OV => 8,
        _ => 1,
    }
}

/// The `N`-byte words of a Value Field whose length is a multiple of `N`.
fn words<const N: usize>(bytes: &[u8]) -> impl Iterator<Item = [u8; N]> + '_ {
    bytes.as_chunks::<N>().0.iter().copied()
}

/// A binary Value Field as dicom-object holds the VR: Little Endian words.
pub(super) fn binary_value(vr: VR, bytes: &[u8]) -> PrimitiveValue {
    if !bytes.len().is_multiple_of(word_size(vr)) {
        return PrimitiveValue::U8(bytes.iter().copied().collect());
    }
    match vr {
        VR::OW => PrimitiveValue::U16(words(bytes).map(u16::from_le_bytes).collect()),
        VR::OF => PrimitiveValue::F32(words(bytes).map(f32::from_le_bytes).collect()),
        VR::OL => PrimitiveValue::U32(words(bytes).map(u32::from_le_bytes).collect()),
        VR::OD => PrimitiveValue::F64(words(bytes).map(f64::from_le_bytes).collect()),
        VR::OV => PrimitiveValue::U64(words(bytes).map(u64::from_le_bytes).collect()),
        _ => PrimitiveValue::U8(bytes.iter().copied().collect()),
    }
}

/// A fetched Bulk Data Value: "a stream of uncompressed bytes (octets) in
/// Little Endian byte order" (PS3.18 8.7.3.3.1), text as UTF-8.
fn fetched_value(vr: VR, bytes: &[u8]) -> PrimitiveValue {
    let le = |size: usize| bytes.chunks_exact(size);
    match vr {
        VR::US => PrimitiveValue::U16(le(2).map(|w| u16::from_le_bytes([w[0], w[1]])).collect()),
        VR::SS => PrimitiveValue::I16(le(2).map(|w| i16::from_le_bytes([w[0], w[1]])).collect()),
        VR::UL => PrimitiveValue::U32(
            le(4)
                .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
                .collect(),
        ),
        VR::SL => PrimitiveValue::I32(
            le(4)
                .map(|w| i32::from_le_bytes([w[0], w[1], w[2], w[3]]))
                .collect(),
        ),
        VR::FL => PrimitiveValue::F32(
            le(4)
                .map(|w| f32::from_le_bytes([w[0], w[1], w[2], w[3]]))
                .collect(),
        ),
        VR::FD => PrimitiveValue::F64(
            le(8)
                .map(|w| f64::from_le_bytes(w.try_into().unwrap_or([0; 8])))
                .collect(),
        ),
        VR::SV => PrimitiveValue::I64(
            le(8)
                .map(|w| i64::from_le_bytes(w.try_into().unwrap_or([0; 8])))
                .collect(),
        ),
        VR::UV => PrimitiveValue::U64(
            le(8)
                .map(|w| u64::from_le_bytes(w.try_into().unwrap_or([0; 8])))
                .collect(),
        ),
        VR::DS | VR::IS | VR::UC => {
            let text = String::from_utf8_lossy(bytes);
            PrimitiveValue::Strs(
                text.trim_end_matches(['\0', ' '])
                    .split('\\')
                    .map(|value| value.trim().to_string())
                    .collect(),
            )
        }
        VR::LT | VR::ST | VR::UT => PrimitiveValue::Strs(
            [String::from_utf8_lossy(bytes)
                .trim_end_matches(['\0', ' '])
                .to_string()]
            .into_iter()
            .collect(),
        ),
        _ => binary_value(vr, bytes),
    }
}

/// The stand-in for a Value the document gives only as a BulkDataURI.
fn stand_in(
    tag: Tag,
    vr: VR,
    parent: &InMemDicomObject,
    pixel_data: bool,
    stand: Stand,
) -> PrimitiveValue {
    if pixel_data {
        // Present and not empty, as a BulkDataURI implies; never decoded.
        return binary_value(vr, &[0; 8]);
    }
    let plausible = stand == Stand::Plausible;
    if BINARY_VRS.contains(&vr) {
        let bytes = match (plausible, tag) {
            (false, _) => vec![0; 8],
            (true, tags::ICC_PROFILE) => {
                crate::icc::synthetic_srgb_icc_profile().unwrap_or_default()
            }
            (true, tags::PIXEL_DATA) => vec![0; icon_bytes(parent)],
            (true, _) => vec![1; 16],
        };
        return binary_value(vr, &bytes);
    }
    let one = i32::from(plausible);
    match vr {
        VR::DS | VR::IS => PrimitiveValue::from(one.to_string()),
        VR::LT | VR::ST | VR::UC | VR::UT => {
            PrimitiveValue::from(if plausible { "-" } else { "0" })
        }
        VR::FL => PrimitiveValue::from(one as f32),
        VR::FD => PrimitiveValue::from(f64::from(one)),
        VR::US => PrimitiveValue::from(one as u16),
        VR::SS => PrimitiveValue::from(one as i16),
        VR::UL => PrimitiveValue::from(one as u32),
        VR::SL => PrimitiveValue::from(one),
        VR::SV => PrimitiveValue::from(i64::from(one)),
        _ => PrimitiveValue::from(one as u64),
    }
}

/// Native Pixel Data bytes of the image `item` declares, capped at 16 MiB.
fn icon_bytes(item: &InMemDicomObject) -> usize {
    let value = |tag| {
        item.get(tag)
            .and_then(|element| element.to_int::<u64>().ok())
            .unwrap_or(1)
    };
    let bits = value(tags::BITS_ALLOCATED).max(8);
    let bytes = value(tags::ROWS)
        .saturating_mul(value(tags::COLUMNS))
        .saturating_mul(value(tags::SAMPLES_PER_PIXEL))
        .saturating_mul(bits / 8);
    let capped = bytes.min(16 * 1024 * 1024);
    usize::try_from(capped + capped % 2).unwrap_or(2)
}

/// PS3.18 10.4.1.1.5.1: an absolute http(s) URI, or a relative reference with
/// an absolute or relative path, of RFC 3986 characters.
fn bulk_uri_form(uri: &str) -> Result<(), &'static str> {
    if uri.is_empty() {
        return Err("is empty");
    }
    if uri
        .bytes()
        .any(|byte| !(byte.is_ascii_alphanumeric() || b"-._~:/?#[]@!$&'()*+,;=%".contains(&byte)))
    {
        return Err("has a character RFC 3986 does not allow in a URI");
    }
    if uri.starts_with("//") {
        return Err("is a network-path reference, which is not one of the absolute URI, absolute-path and relative-path forms");
    }
    let scheme_end = uri.find(':').filter(|colon| {
        let candidate = &uri[..*colon];
        !candidate.contains('/')
            && candidate
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic())
    });
    match scheme_end {
        Some(colon) => {
            let scheme = uri[..colon].to_ascii_lowercase();
            if (scheme == "http" || scheme == "https") && uri[colon + 1..].starts_with("//") {
                Ok(())
            } else {
                Err("names a scheme other than an http or https absolute URI")
            }
        }
        None => Ok(()),
    }
}
