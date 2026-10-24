//! PS3.6 VR/VM and PS3.5 6.2 Value format checks over every Data Element.

use dicom_core::value::Value;
use dicom_core::{PrimitiveValue, Tag, VR};
use dicom_dictionary_std::StandardDataDictionary;
use dicom_object::mem::InMemElement;
use dicom_object::InMemDicomObject;

use super::dictionary::{DICTIONARY, DICTIONARY_REPEATING};
use super::file_meta::long_length;
use super::model::{DictionaryEntry, ValueMultiplicity};
use super::report::{excerpt, tag_text, Family};
use super::scope::{declared_length, is_private, raw_tag};
use super::MAX_NESTING;
use crate::uid::is_valid_dicom_uid;
use dicom_core::dictionary::{DataDictionary, DataDictionaryEntry};

const VM_1: ValueMultiplicity = ValueMultiplicity {
    min: 1,
    max: Some(1),
    step: 1,
};
const GROUP_LENGTH: &[VR] = &[VR::UL];
const PRIVATE_CREATOR: &[VR] = &[VR::LO];
const ESC: char = '\u{1b}';
/// Longest Value quoted in a message.
const MAX_VALUE_EXCERPT: usize = 48;

/// Encoding facts that shape the Value rules.
#[derive(Clone, Copy, Debug)]
pub(super) struct Encoding {
    /// Explicit VR Transfer Syntax: the encoded VR is checked against PS3.6.
    pub(super) explicit_vr: bool,
    /// Specific Character Set present: text may leave the Default Repertoire.
    pub(super) extended_characters: bool,
}

pub(super) struct ValueChecks<'f> {
    pub(super) vr_vm: &'f mut Family,
    pub(super) format: &'f mut Family,
    pub(super) encoding: Encoding,
}

impl ValueChecks<'_> {
    pub(super) fn data_set(&mut self, object: &InMemDicomObject, path: &str, depth: usize) {
        for element in object.iter() {
            let tag = element.header().tag;
            let label = Label {
                path,
                tag,
                retired: false,
            };
            self.element(element, label);
            let Some(items) = element.items() else {
                continue;
            };
            if depth >= MAX_NESTING {
                self.format.push(|| format!(
                    "{label}: Sequence nesting exceeds {MAX_NESTING} levels; deeper Items not checked"
                ));
                continue;
            }
            for (index, item) in items.iter().enumerate() {
                let item_path = format!("{label}[{}] > ", index + 1);
                self.data_set(item, &item_path, depth + 1);
            }
        }
    }

    fn element(&mut self, element: &InMemElement, label: Label<'_>) {
        let vr = element.vr();
        if let Some((allowed, vm, retired)) = expected(label.tag) {
            let label = Label { retired, ..label };
            if self.encoding.explicit_vr
                && !allowed.is_empty()
                && !allowed.contains(&vr)
                && !unknown_for_long_value(vr, allowed, element)
            {
                self.vr_vm.push(|| {
                    format!(
                        "{label}: VR {vr:?} where PS3.6 specifies {}",
                        allowed
                            .iter()
                            .map(|vr| format!("{vr:?}"))
                            .collect::<Vec<_>>()
                            .join(" or ")
                    )
                });
            }
            if let Some(count) = value_count(element) {
                if count > 0 && !vm.admits(count) {
                    self.vr_vm.push(|| {
                        format!(
                            "{label}: {count} Value{} where PS3.6 VM is {}",
                            if count == 1 { "" } else { "s" },
                            vm.describe()
                        )
                    });
                }
            }
        } else if !is_private(label.tag) {
            self.vr_vm.push(|| {
                format!(
                    "{label}: not a Data Element of the PS3.6 {} registry",
                    super::tables::EDITION
                )
            });
        }
        self.length(element, label);
        if let Value::Primitive(primitive) = element.value() {
            self.format_values(vr, primitive, label);
        }
    }

    fn length(&mut self, element: &InMemElement, label: Label<'_>) {
        let Some(length) = declared_length(element) else {
            return;
        };
        if matches!(
            element.value(),
            Value::Sequence(_) | Value::PixelSequence(_)
        ) {
            return;
        }
        if length % 2 == 1 {
            self.format.push(|| {
                format!(
                    "{label}: odd Value Length {length}; Value Lengths shall be even (PS3.5 7.1.1)"
                )
            });
        }
        if let Some(size) = binary_size(element.vr()) {
            if length % size != 0 {
                self.format.push(|| format!(
                    "{label}: Value Length {length} is not a multiple of {size} bytes for VR {:?} (PS3.5 6.2)",
                    element.vr()
                ));
            }
        }
    }

    fn format_values(&mut self, vr: VR, primitive: &PrimitiveValue, label: Label<'_>) {
        let values: &[String] = match primitive {
            PrimitiveValue::Strs(values) => values,
            PrimitiveValue::Str(value) => std::slice::from_ref(value),
            _ => return,
        };
        let last = values.len().saturating_sub(1);
        for (index, raw) in values.iter().enumerate() {
            let value = if index == last {
                strip_padding(vr, raw)
            } else {
                raw
            };
            if let Err(reason) = check_value(vr, value, self.encoding.extended_characters) {
                let which = if values.len() > 1 {
                    format!(" Value {}", index + 1)
                } else {
                    String::new()
                };
                self.format.push(|| {
                    format!(
                        "{label}{which} {:?}: {reason} for VR {vr:?} (PS3.5 6.2)",
                        excerpt(value, MAX_VALUE_EXCERPT)
                    )
                });
            }
        }
    }
}

/// Calls `predicate` on every text Value whose repertoire Specific Character Set governs.
pub(super) fn any_text(object: &InMemDicomObject, predicate: impl Fn(&str) -> bool + Copy) -> bool {
    any_text_bounded(object, predicate, 0)
}

fn any_text_bounded(
    object: &InMemDicomObject,
    predicate: impl Fn(&str) -> bool + Copy,
    depth: usize,
) -> bool {
    object.iter().any(|element| match element.value() {
        Value::Primitive(PrimitiveValue::Strs(values))
            if matches!(element.vr(), VR::SH | VR::LO | VR::PN | VR::UC) =>
        {
            values.iter().any(|value| predicate(value))
        }
        Value::Primitive(PrimitiveValue::Str(value))
            if matches!(element.vr(), VR::ST | VR::LT | VR::UT) =>
        {
            predicate(value)
        }
        Value::Sequence(sequence) if depth < MAX_NESTING => sequence
            .items()
            .iter()
            .any(|item| any_text_bounded(item, predicate, depth + 1)),
        _ => false,
    })
}

/// PS3.5 6.2.2: a Standard Data Element whose VR has a 16-bit Value Length
/// Field is encoded as UN in Explicit VR once its Value exceeds 65534 bytes.
fn unknown_for_long_value(vr: VR, allowed: &[VR], element: &InMemElement) -> bool {
    vr == VR::UN
        && allowed.iter().all(|allowed| !long_length(*allowed))
        && declared_length(element).is_some_and(|length| length > 65534)
}

/// `<path><Keyword> (gggg,eeee)` as the value checks name an element.
pub(in crate::validation) fn element_label(path: &str, tag: Tag) -> String {
    Label {
        path,
        tag,
        retired: false,
    }
    .to_string()
}

/// `<path><Keyword> (gggg,eeee)`, without a keyword for unregistered tags;
/// formatted only when a message names the element.
#[derive(Clone, Copy)]
struct Label<'a> {
    path: &'a str,
    tag: Tag,
    retired: bool,
}

impl std::fmt::Display for Label<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.path)?;
        if let Some(entry) = StandardDataDictionary.by_tag(self.tag) {
            write!(f, "{} ", entry.alias())?;
        }
        f.write_str(&tag_text(raw_tag(self.tag)))?;
        if self.retired {
            f.write_str(" (retired)")?;
        }
        Ok(())
    }
}

/// VR alternatives, VM and retirement from PS3.6 for a public tag or Private Creator.
fn expected(tag: Tag) -> Option<(&'static [VR], ValueMultiplicity, bool)> {
    if is_private(tag) {
        return (0x0010..=0x00FF).contains(&tag.element()).then_some((
            PRIVATE_CREATOR,
            VM_1,
            false,
        ));
    }
    if tag.element() == 0x0000 {
        // Group Length (PS3.5 7.2), retired outside group 0002.
        return Some((GROUP_LENGTH, VM_1, tag.group() != 0x0002));
    }
    entry(tag).map(|entry| (entry.vr, entry.vm, entry.retired))
}

/// The PS3.6 VR alternatives of a public tag (an Implicit VR element's VR).
pub(in crate::validation) fn dictionary_vrs(tag: Tag) -> Option<&'static [VR]> {
    entry(tag).map(|entry| entry.vr)
}

pub(super) fn entry(tag: Tag) -> Option<&'static DictionaryEntry> {
    let value = raw_tag(tag);
    DICTIONARY
        .binary_search_by_key(&value, |entry| entry.tag)
        .ok()
        .map(|index| &DICTIONARY[index])
        .or_else(|| {
            DICTIONARY_REPEATING
                .iter()
                .find(|entry| value & entry.mask == entry.tag)
        })
}

fn value_count(element: &InMemElement) -> Option<u32> {
    match element.vr() {
        VR::OB | VR::OW | VR::OD | VR::OF | VR::OL | VR::OV | VR::UN | VR::SQ => None,
        _ => match element.value() {
            Value::Primitive(primitive) => Some(primitive.multiplicity()),
            _ => None,
        },
    }
}

fn binary_size(vr: VR) -> Option<u32> {
    match vr {
        VR::US | VR::SS | VR::OW => Some(2),
        VR::UL | VR::SL | VR::FL | VR::AT | VR::OF | VR::OL => Some(4),
        VR::FD | VR::SV | VR::UV | VR::OD | VR::OV => Some(8),
        _ => None,
    }
}

/// Removes the single character PS3.5 6.2 appends to reach even length.
fn strip_padding(vr: VR, value: &str) -> &str {
    match vr {
        VR::UI => value.strip_suffix('\0').unwrap_or(value),
        _ => value.strip_suffix(' ').unwrap_or(value),
    }
}

pub(super) fn check_value(vr: VR, value: &str, extended: bool) -> Result<(), String> {
    match vr {
        VR::AE => {
            max_bytes(value, 16)?;
            repertoire(value, |c| (' '..='~').contains(&c) && c != '\\')?;
            if !value.is_empty() && value.trim_matches(' ').is_empty() {
                return Err("a Value consisting solely of spaces".into());
            }
            Ok(())
        }
        VR::AS => {
            let bytes = value.as_bytes();
            if bytes.len() == 4
                && bytes[..3].iter().all(u8::is_ascii_digit)
                && matches!(bytes[3], b'D' | b'W' | b'M' | b'Y')
            {
                Ok(())
            } else {
                Err("not of the form nnnD, nnnW, nnnM or nnnY".into())
            }
        }
        VR::CS => {
            max_bytes(value, 16)?;
            repertoire(value, |c| {
                c.is_ascii_uppercase() || c.is_ascii_digit() || c == ' ' || c == '_'
            })
        }
        VR::DA => date(value),
        VR::DS => decimal(value),
        VR::DT => date_time(value),
        VR::IS => integer(value),
        VR::LO => text(value, 64, extended, false),
        VR::SH => text(value, 16, extended, false),
        VR::UC => text(value, usize::MAX, extended, false),
        VR::ST => text(value, 1024, extended, true),
        VR::LT => text(value, 10240, extended, true),
        VR::UT => text(value, usize::MAX, extended, true),
        VR::PN => person_name(value, extended),
        VR::TM => time(value.trim_end_matches(' ')),
        VR::UI => {
            if !value.is_empty() && !is_valid_dicom_uid(value) {
                Err(
                    "not a UID of at most 64 digits and periods without leading-zero components"
                        .into(),
                )
            } else if value.starts_with("1.2.840.10008.")
                && super::dictionary::REGISTERED_UIDS
                    .binary_search(&value)
                    .is_err()
            {
                Err("under the DICOM root 1.2.840.10008 but not registered in PS3.6 Annex A (PS3.5 9.2)".into())
            } else {
                Ok(())
            }
        }
        VR::UR => uri(value),
        _ => Ok(()),
    }
}

fn max_bytes(value: &str, limit: usize) -> Result<(), String> {
    if value.len() > limit {
        Err(format!(
            "{} bytes exceed the {limit}-byte maximum",
            value.len()
        ))
    } else {
        Ok(())
    }
}

fn max_chars(value: &str, limit: usize) -> Result<(), String> {
    let count = value.chars().count();
    if count > limit {
        Err(format!(
            "{count} characters exceed the {limit}-character maximum"
        ))
    } else {
        Ok(())
    }
}

fn repertoire(value: &str, allowed: impl Fn(char) -> bool) -> Result<(), String> {
    match value.chars().find(|c| !allowed(*c)) {
        Some(c) => Err(format!("character {c:?} outside the permitted repertoire")),
        None => Ok(()),
    }
}

/// SH, LO, UC, ST, LT, UT: Default Repertoire unless Specific Character Set is
/// present; control characters only as PS3.5 6.2 permits.
fn text(value: &str, limit: usize, extended: bool, paragraphs: bool) -> Result<(), String> {
    // A Value with Code Extension is decoded by Value 1 of Specific Character
    // Set alone, so its escape sequences and multi-byte characters miscount
    // here; the encoding check counts it from the encoded bytes, without the
    // escape sequences (PS3.5 6.2).
    if !value.contains(ESC) {
        max_chars(value, limit)?;
    }
    repertoire(value, |c| {
        permitted_text_char(c, extended, paragraphs, false)
    })
}

fn permitted_text_char(c: char, extended: bool, paragraphs: bool, person_name: bool) -> bool {
    if c == ESC {
        return true;
    }
    if c.is_control() {
        return paragraphs && matches!(c, '\t' | '\n' | '\u{0c}' | '\r');
    }
    if c == '\\' {
        return paragraphs && !person_name;
    }
    c.is_ascii() || extended
}

fn person_name(value: &str, extended: bool) -> Result<(), String> {
    repertoire(value, |c| permitted_text_char(c, extended, false, true))?;
    // With Code Extension, "^" and "=" bytes may belong to multi-byte
    // characters; the encoding check finds the delimiters from the encoded
    // bytes and counts the component groups there.
    if value.contains(ESC) {
        return Ok(());
    }
    let groups: Vec<&str> = value.split('=').collect();
    if groups.len() > 3 {
        return Err(format!("{} component groups exceed three", groups.len()));
    }
    for group in groups {
        max_chars(group, 64)?;
        let components = group.split('^').count();
        if components > 5 {
            return Err(format!("{components} components exceed five"));
        }
    }
    Ok(())
}

fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn number(value: &str) -> u32 {
    value
        .bytes()
        .fold(0, |acc, byte| acc * 10 + u32::from(byte - b'0'))
}

fn valid_date(year: u32, month: u32, day: u32) -> bool {
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

fn date(value: &str) -> Result<(), String> {
    if value.len() != 8 || !digits(value) {
        return Err("not of the form YYYYMMDD".into());
    }
    if valid_date(
        number(&value[..4]),
        number(&value[4..6]),
        number(&value[6..8]),
    ) {
        Ok(())
    } else {
        Err("not a Gregorian calendar date".into())
    }
}

fn time(value: &str) -> Result<(), String> {
    max_bytes(value, 14)?;
    let (clock, fraction) = match value.split_once('.') {
        Some((clock, fraction)) => (clock, Some(fraction)),
        None => (value, None),
    };
    if !digits(clock) || !matches!(clock.len(), 2 | 4 | 6) {
        return Err("not of the form HH[MM[SS[.F{1,6}]]]".into());
    }
    if let Some(fraction) = fraction {
        if clock.len() != 6 || !digits(fraction) || fraction.len() > 6 {
            return Err("fractional seconds need HHMMSS and 1 to 6 digits".into());
        }
    }
    clock_ranges(clock)
}

fn clock_ranges(clock: &str) -> Result<(), String> {
    let limits = [23, 59, 60];
    for (index, limit) in limits.iter().enumerate() {
        let start = index * 2;
        if clock.len() >= start + 2 && number(&clock[start..start + 2]) > *limit {
            return Err("hour, minute or second out of range".into());
        }
    }
    Ok(())
}

fn date_time(value: &str) -> Result<(), String> {
    let value = value.trim_end_matches(' ');
    max_bytes(value, 26)?;
    let (main, offset) = match value.get(4..).and_then(|rest| rest.find(['+', '-'])) {
        Some(position) => (&value[..position + 4], Some(&value[position + 4..])),
        None => (value, None),
    };
    let (stamp, fraction) = match main.split_once('.') {
        Some((stamp, fraction)) => (stamp, Some(fraction)),
        None => (main, None),
    };
    if !digits(stamp) || !matches!(stamp.len(), 4 | 6 | 8 | 10 | 12 | 14) {
        return Err("not of the form YYYY[MM[DD[HH[MM[SS[.F]]]]]][&ZZXX]".into());
    }
    if let Some(fraction) = fraction {
        if stamp.len() != 14 || !digits(fraction) || fraction.len() > 6 {
            return Err("fractional seconds need YYYYMMDDHHMMSS and 1 to 6 digits".into());
        }
    }
    let month = (stamp.len() >= 6).then(|| number(&stamp[4..6]));
    let day = (stamp.len() >= 8).then(|| number(&stamp[6..8]));
    if month.is_some_and(|month| !(1..=12).contains(&month)) {
        return Err("month out of range".into());
    }
    if let (Some(month), Some(day)) = (month, day) {
        if !valid_date(number(&stamp[..4]), month, day) {
            return Err("not a Gregorian calendar date".into());
        }
    }
    if stamp.len() > 8 {
        clock_ranges(&stamp[8..])?;
    }
    if let Some(offset) = offset {
        utc_offset(offset)?;
    }
    Ok(())
}

fn utc_offset(offset: &str) -> Result<(), String> {
    let (sign, digits_part) = offset.split_at(1);
    if digits_part.len() != 4 || !digits(digits_part) {
        return Err("UTC offset is not &ZZXX".into());
    }
    let minutes = number(&digits_part[..2]) * 60 + number(&digits_part[2..]);
    let valid = number(&digits_part[2..]) < 60
        && match sign {
            "+" => minutes <= 14 * 60,
            _ => minutes <= 12 * 60 && minutes != 0,
        };
    if valid {
        Ok(())
    } else {
        Err("UTC offset outside -1200..+1400 or -0000".into())
    }
}

fn decimal(value: &str) -> Result<(), String> {
    max_bytes(value, 16)?;
    let number = value.trim_matches(' ');
    if number.is_empty() {
        return Ok(());
    }
    let unsigned = number.strip_prefix(['+', '-']).unwrap_or(number);
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(position) => (&unsigned[..position], Some(&unsigned[position + 1..])),
        None => (unsigned, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let mantissa_ok = (whole.is_empty() || digits(whole))
        && (fraction.is_empty() || digits(fraction))
        && !(whole.is_empty() && fraction.is_empty());
    let exponent_ok = exponent.is_none_or(|exponent| {
        let digits_part = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        digits(digits_part)
    });
    if mantissa_ok && exponent_ok {
        Ok(())
    } else {
        Err("not a fixed or floating point decimal".into())
    }
}

fn integer(value: &str) -> Result<(), String> {
    max_bytes(value, 12)?;
    let number = value.trim_matches(' ');
    if number.is_empty() {
        return Ok(());
    }
    let unsigned = number.strip_prefix(['+', '-']).unwrap_or(number);
    if !digits(unsigned) {
        return Err("not a base-10 integer".into());
    }
    match number.parse::<i64>() {
        Ok(parsed) if i32::try_from(parsed).is_ok() => Ok(()),
        _ => Err("outside -2^31..2^31-1".into()),
    }
}

fn uri(value: &str) -> Result<(), String> {
    if value.starts_with(' ') {
        return Err("leading space".into());
    }
    let value = value.trim_end_matches(' ');
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            let escaped = bytes
                .get(index + 1..index + 3)
                .is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit));
            if !escaped {
                return Err("percent sign not followed by two hex digits".into());
            }
            index += 3;
            continue;
        }
        let permitted = byte.is_ascii_alphanumeric() || b"-._~:/?#[]@!$&'()*+,;=".contains(&byte);
        if !permitted {
            return Err(format!(
                "character {:?} outside the RFC 3986 repertoire",
                char::from(byte)
            ));
        }
        index += 1;
    }
    Ok(())
}
