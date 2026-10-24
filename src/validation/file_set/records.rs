//! The DICOMDIR File as encoded: its File Meta Information and the Directory
//! Records of the Directory Record Sequence (0004,1220), each with the byte
//! offset of its Item tag, which the offset Attributes of PS3.3 F.3.2.2 count
//! from the first byte of the File Meta Information (the File Preamble).
//!
//! dicom-object discards those offsets, so the file is re-read here. Every
//! length is checked against the bytes that remain, nesting and the number of
//! records are bounded, and parsing stops at the first framing defect.
//!
//! Text is decoded as dicom-parser decodes the referenced files, so a record
//! Key compares with the file's Value: under the character set Value 1 of
//! the record's own Specific Character Set (0008,0005) names, else that of
//! the DICOMDIR's top-level one, else the Default Character Repertoire.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ops::Range;

use dicom_core::{Tag, VR};
use dicom_encoding::text::{SpecificCharacterSet, TextCodec};

use crate::validation::disclosure::shown;
use crate::validation::iod::{long_length, MAX_NESTING};

const PREAMBLE: usize = 128;
const PREFIX: &[u8; 4] = b"DICM";
const UNDEFINED_LENGTH: u32 = 0xFFFF_FFFF;
const ITEM: Tag = Tag(0xFFFE, 0xE000);
const ITEM_DELIMITATION: Tag = Tag(0xFFFE, 0xE00D);
const SEQUENCE_DELIMITATION: Tag = Tag(0xFFFE, 0xE0DD);
pub(super) const DIRECTORY_RECORD_SEQUENCE: Tag = Tag(0x0004, 0x1220);
/// Most Directory Records read from one DICOMDIR.
pub(super) const MAX_RECORDS: usize = 1_000_000;
pub(super) const ICON_IMAGE_SEQUENCE: Tag = Tag(0x0088, 0x0200);
pub(super) const REFERENCED_FILE_ID: Tag = Tag(0x0004, 0x1500);
pub(super) const DIRECTORY_RECORD_TYPE: Tag = Tag(0x0004, 0x1430);
const TRANSFER_SYNTAX_UID: Tag = Tag(0x0002, 0x0010);
const SPECIFIC_CHARACTER_SET: Tag = Tag(0x0008, 0x0005);

/// One Data Element of a Data Set or Directory Record. A Sequence keeps its
/// Item count, and the Icon Image Sequence (0088,0200) also the elements of
/// its Items (PS3.3 F.7), no deeper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Value {
    pub(super) vr: VR,
    pub(super) bytes: Vec<u8>,
    /// For a Directory Record element of a VR whose repertoire Specific
    /// Character Set extends (PS3.5 6.1.2.3), the Value decoded under the
    /// record's character set; `None` otherwise, and when the bytes are not
    /// text of that set, which the encoding check of the DICOMDIR reports.
    pub(super) decoded: Option<String>,
    pub(super) items: usize,
    pub(super) item_sets: Vec<BTreeMap<Tag, Value>>,
}

impl Value {
    /// The Value as text without its padding: as the record's character
    /// set decodes it, else as the Default Character Repertoire.
    pub(super) fn text(&self) -> String {
        let text = match &self.decoded {
            Some(decoded) => Cow::Borrowed(decoded.as_str()),
            None => String::from_utf8_lossy(&self.bytes),
        };
        text.trim_end_matches(['\0', ' '])
            .trim_start_matches(' ')
            .to_string()
    }

    /// The Values of a multi-valued string, without padding.
    pub(super) fn texts(&self) -> Vec<String> {
        self.text()
            .split('\\')
            .map(|value| value.trim_matches([' ', '\0']).to_string())
            .collect()
    }

    /// Value 1 of a UL or US element.
    pub(super) fn unsigned(&self) -> Option<u32> {
        match (self.vr, self.bytes.as_slice()) {
            (VR::UL, [a, b, c, d, ..]) => Some(u32::from_le_bytes([*a, *b, *c, *d])),
            (VR::US, [a, b, ..]) => Some(u32::from(u16::from_le_bytes([*a, *b]))),
            _ => None,
        }
    }
}

/// Whether Specific Character Set (0008,0005) extends or replaces the
/// repertoire of `vr` (PS3.5 6.1.2.3).
fn extended_repertoire(vr: VR) -> bool {
    matches!(
        vr,
        VR::SH | VR::LO | VR::UC | VR::ST | VR::LT | VR::UT | VR::PN
    )
}

/// One Item of the Directory Record Sequence.
#[derive(Debug, Clone, Default)]
pub(super) struct Record {
    /// Byte offset of the Item tag from the start of the file.
    pub(super) offset: u64,
    /// The bytes of the Item's Data Set in the file: after the Item tag and
    /// length, before any Item Delimitation Item.
    pub(super) content: Range<usize>,
    pub(super) elements: BTreeMap<Tag, Value>,
    /// The character set the record's text is decoded with.
    pub(super) charset: SpecificCharacterSet,
}

impl Record {
    pub(super) fn get(&self, tag: Tag) -> Option<&Value> {
        self.elements.get(&tag)
    }

    /// Directory Record Type (0004,1430), or "" when absent.
    pub(super) fn record_type(&self) -> String {
        self.get(DIRECTORY_RECORD_TYPE)
            .map(Value::text)
            .unwrap_or_default()
    }
}

/// The parsed DICOMDIR File.
#[derive(Debug, Default)]
pub(super) struct Directory {
    /// Group 0002 elements as encoded.
    pub(super) meta: BTreeMap<Tag, Value>,
    /// Top-level Data Set elements other than the Directory Record Sequence.
    pub(super) data_set: BTreeMap<Tag, Value>,
    /// Whether the Data Set holds a Directory Record Sequence, i.e. the
    /// Directory Information Module.
    pub(super) has_records: bool,
    pub(super) records: Vec<Record>,
}

/// Parses `bytes`, a whole DICOMDIR File. The error names the first framing
/// defect; nothing after it is read.
pub(super) fn parse(bytes: &[u8]) -> Result<Directory, String> {
    if bytes.len() < PREAMBLE + PREFIX.len() || &bytes[PREAMBLE..PREAMBLE + 4] != PREFIX {
        return Err(
            "the file has no \"DICM\" prefix after a 128-byte File Preamble (PS3.10 7.1)".into(),
        );
    }
    let mut reader = Reader {
        bytes,
        position: PREAMBLE + PREFIX.len(),
    };
    let mut directory = Directory::default();
    while reader.peek_group() == Some(0x0002) {
        let (tag, value) = reader.element(0)?;
        directory.meta.insert(tag, value);
    }
    let syntax = directory
        .meta
        .get(&TRANSFER_SYNTAX_UID)
        .map(Value::text)
        .unwrap_or_default();
    if syntax != "1.2.840.10008.1.2.1" {
        return Err(format!(
            "Transfer Syntax UID (0002,0010) is {}, but the DICOMDIR File shall use \
             Explicit VR Little Endian 1.2.840.10008.1.2.1 (PS3.10 8.6); its records are not read",
            shown(TRANSFER_SYNTAX_UID, &syntax)
        ));
    }
    while !reader.at_end() {
        let start = reader.position;
        let tag = reader.tag()?;
        if tag == DIRECTORY_RECORD_SEQUENCE {
            reader.position = start;
            directory.has_records = true;
            directory.records = reader.record_sequence()?;
            continue;
        }
        reader.position = start;
        let (tag, value) = reader.element(0)?;
        directory.data_set.insert(tag, value);
    }
    decode_records(&mut directory);
    Ok(directory)
}

/// The character set Value 1 of the Specific Character Set (0008,0005) of
/// `elements` names, which is the one dicom-parser decodes a Data Set's text
/// with; `None` when it is absent or names no character set dicom-encoding
/// decodes, as for an empty Value 1 before ISO 2022 Code Extensions.
fn declared_charset(elements: &BTreeMap<Tag, Value>) -> Option<SpecificCharacterSet> {
    let term = elements
        .get(&SPECIFIC_CHARACTER_SET)?
        .texts()
        .into_iter()
        .next()?;
    SpecificCharacterSet::from_code(&term)
}

/// Decodes the text of every record: under its own Specific Character Set,
/// else the DICOMDIR's, which follows the Directory Record Sequence in the
/// file and so is read only after it.
fn decode_records(directory: &mut Directory) {
    let inherited = declared_charset(&directory.data_set).unwrap_or_default();
    for record in &mut directory.records {
        record.charset = declared_charset(&record.elements).unwrap_or_else(|| inherited.clone());
        for value in record.elements.values_mut() {
            if extended_repertoire(value.vr) {
                value.decoded = record.charset.decode(&value.bytes).ok();
            }
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Reader<'_> {
    fn at_end(&self) -> bool {
        self.position >= self.bytes.len()
    }

    fn take(&mut self, count: usize, what: &str) -> Result<&[u8], String> {
        let end = self
            .position
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| {
                format!(
                    "{what} at byte {} needs {count} bytes, but only {} remain",
                    self.position,
                    self.bytes.len().saturating_sub(self.position)
                )
            })?;
        let slice = &self.bytes[self.position..end];
        self.position = end;
        Ok(slice)
    }

    fn u16(&mut self, what: &str) -> Result<u16, String> {
        let bytes = self.take(2, what)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self, what: &str) -> Result<u32, String> {
        let bytes = self.take(4, what)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn peek_group(&self) -> Option<u16> {
        let bytes = self.bytes.get(self.position..self.position + 2)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn tag(&mut self) -> Result<Tag, String> {
        let group = self.u16("a tag")?;
        let element = self.u16("a tag")?;
        Ok(Tag(group, element))
    }

    /// Reads one Explicit VR Little Endian element; a Sequence's Items are
    /// skipped and counted.
    fn element(&mut self, depth: usize) -> Result<(Tag, Value), String> {
        let start = self.position;
        let tag = self.tag()?;
        if tag.group() == 0xFFFE {
            return Err(format!(
                "{} at byte {start} is a delimiter where a Data Element is expected",
                tag_label(tag)
            ));
        }
        let vr_bytes = self.take(2, "a VR")?;
        let vr_bytes = [vr_bytes[0], vr_bytes[1]];
        let vr = VR::from_binary(vr_bytes).ok_or_else(|| {
            format!(
                "{} at byte {start} has VR bytes {:02X} {:02X}, which name no VR (PS3.5 7.1.2)",
                tag_label(tag),
                vr_bytes[0],
                vr_bytes[1]
            )
        })?;
        let length = if long_length(vr) {
            self.take(2, "reserved bytes")?;
            self.u32("a Value Length")?
        } else {
            u32::from(self.u16("a Value Length")?)
        };
        if vr == VR::SQ {
            let (items, item_sets) = self.sequence_items(tag, length, depth)?;
            return Ok((
                tag,
                Value {
                    vr,
                    bytes: Vec::new(),
                    decoded: None,
                    items,
                    item_sets,
                },
            ));
        }
        if length == UNDEFINED_LENGTH {
            return Err(format!(
                "{} at byte {start} has an undefined length, which only a Sequence may have here",
                tag_label(tag)
            ));
        }
        let bytes = self.take(length as usize, "a Value")?.to_vec();
        Ok((
            tag,
            Value {
                vr,
                bytes,
                decoded: None,
                items: 0,
                item_sets: Vec::new(),
            },
        ))
    }

    /// Reads the Items of a Sequence whose header was just read; returns
    /// their count and, for the Icon Image Sequence, their elements.
    fn sequence_items(
        &mut self,
        tag: Tag,
        length: u32,
        depth: usize,
    ) -> Result<(usize, Vec<BTreeMap<Tag, Value>>), String> {
        if depth >= MAX_NESTING {
            return Err(format!(
                "{} nests Sequences deeper than {MAX_NESTING} levels",
                tag_label(tag)
            ));
        }
        let end = self.end_of(length, tag)?;
        let keep = tag == ICON_IMAGE_SEQUENCE;
        let mut items = 0usize;
        let mut item_sets = Vec::new();
        while self.within(end) {
            let start = self.position;
            let item = self.tag()?;
            let item_length = self.u32("an Item length")?;
            if item == SEQUENCE_DELIMITATION && end.is_none() {
                return Ok((items, item_sets));
            }
            if item != ITEM {
                return Err(format!(
                    "{} at byte {start} is not an Item tag in {}",
                    tag_label(item),
                    tag_label(tag)
                ));
            }
            items += 1;
            let mut elements = BTreeMap::new();
            self.item_elements(item_length, depth + 1, &mut |tag, value| {
                if keep {
                    elements.insert(tag, value);
                }
                Ok(())
            })?;
            if keep {
                item_sets.push(elements);
            }
        }
        self.close(end, tag)?;
        Ok((items, item_sets))
    }

    /// Reads the elements of an Item whose header was just read, passing
    /// each to `visit`; returns where its Data Set ends, before any Item
    /// Delimitation Item.
    fn item_elements(
        &mut self,
        length: u32,
        depth: usize,
        visit: &mut dyn FnMut(Tag, Value) -> Result<(), String>,
    ) -> Result<usize, String> {
        let end = self.end_of(length, ITEM)?;
        while self.within(end) {
            if end.is_none() && self.peek_tag() == Some(ITEM_DELIMITATION) {
                let content_end = self.position;
                self.tag()?;
                self.u32("an Item Delimitation length")?;
                return Ok(content_end);
            }
            let (tag, value) = self.element(depth)?;
            visit(tag, value)?;
        }
        self.close(end, ITEM)?;
        Ok(self.position)
    }

    /// Reads the Directory Record Sequence, keeping each Item's offset and
    /// elements.
    fn record_sequence(&mut self) -> Result<Vec<Record>, String> {
        self.tag()?;
        let vr = self.take(2, "a VR")?;
        if vr != b"SQ" {
            return Err("Directory Record Sequence (0004,1220) is not encoded with VR SQ".into());
        }
        self.take(2, "reserved bytes")?;
        let length = self.u32("a Value Length")?;
        let end = self.end_of(length, DIRECTORY_RECORD_SEQUENCE)?;
        let mut records = Vec::new();
        while self.within(end) {
            let offset = self.position as u64;
            let item = self.tag()?;
            let item_length = self.u32("an Item length")?;
            if item == SEQUENCE_DELIMITATION && end.is_none() {
                return Ok(records);
            }
            if item != ITEM {
                return Err(format!(
                    "{} at byte {offset} is not an Item tag in the Directory Record Sequence",
                    tag_label(item)
                ));
            }
            if records.len() == MAX_RECORDS {
                return Err(format!(
                    "the Directory Record Sequence has more than {MAX_RECORDS} records"
                ));
            }
            let start = self.position;
            let mut elements = BTreeMap::new();
            let end = self.item_elements(item_length, 1, &mut |tag, value| {
                elements.insert(tag, value);
                Ok(())
            })?;
            records.push(Record {
                offset,
                content: start..end,
                elements,
                charset: SpecificCharacterSet::default(),
            });
        }
        self.close(end, DIRECTORY_RECORD_SEQUENCE)?;
        Ok(records)
    }

    fn peek_tag(&self) -> Option<Tag> {
        let bytes = self.bytes.get(self.position..self.position + 4)?;
        Some(Tag(
            u16::from_le_bytes([bytes[0], bytes[1]]),
            u16::from_le_bytes([bytes[2], bytes[3]]),
        ))
    }

    /// The end of a defined length starting here, or `None` for an
    /// undefined length.
    fn end_of(&self, length: u32, tag: Tag) -> Result<Option<usize>, String> {
        if length == UNDEFINED_LENGTH {
            return Ok(None);
        }
        let end = self.position.saturating_add(length as usize);
        if end > self.bytes.len() {
            return Err(format!(
                "{} at byte {} has length {length}, beyond the end of the file",
                tag_label(tag),
                self.position
            ));
        }
        Ok(Some(end))
    }

    fn within(&self, end: Option<usize>) -> bool {
        match end {
            Some(end) => self.position < end,
            None => !self.at_end(),
        }
    }

    /// Fails when a defined length overran or an undefined one ended without
    /// its delimiter.
    fn close(&self, end: Option<usize>, tag: Tag) -> Result<(), String> {
        match end {
            Some(end) if self.position == end => Ok(()),
            Some(end) => Err(format!(
                "the content of {} overruns its length, ending at byte {} past byte {end}",
                tag_label(tag),
                self.position
            )),
            None => Err(format!(
                "{} of undefined length has no delimiter before the end of the file",
                tag_label(tag)
            )),
        }
    }
}

pub(super) fn tag_label(tag: Tag) -> String {
    format!("({:04X},{:04X})", tag.group(), tag.element())
}
