//! The encoded Data Set, element by element: framing and delimiters, tag
//! order, VR bytes, Group Lengths, reserved and repeating groups, Undefined
//! Lengths, padding and native Pixel Data, with the text and private-block
//! rules of each Data Set and Item.

use std::collections::BTreeSet;

use dicom_core::{Tag, VR};
use dicom_dictionary_std::tags;

use super::charset::{specific_character_set, Repertoire};
use super::private::{is_creator, PrivateBlocks};
use super::reader::{Failure, Source};
use super::text::TextScanner;
use crate::validation::iod::{
    dictionary_vrs, element_label, is_private, item_admits_specific_character_set, long_length,
    raw_tag as raw, tag_text, Family, MAX_NESTING,
};
use crate::validation::pixel_structure::NativeFrame;

const UNDEFINED: u32 = 0xFFFF_FFFF;
const ITEM: Tag = Tag(0xFFFE, 0xE000);
const ITEM_DELIMITATION: Tag = Tag(0xFFFE, 0xE00D);
const SEQUENCE_DELIMITATION: Tag = Tag(0xFFFE, 0xE0DD);
/// Leading bytes of a Value kept for the rules that read it: Specific
/// Character Set, Private Creators, Group Lengths and the image geometry.
const MAX_CAPTURE: usize = 1024;

/// How the Data Set's elements are encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Layout {
    pub(super) explicit_vr: bool,
    pub(super) big_endian: bool,
}

impl Layout {
    /// PS3.5 6.2.2: the Items of a UN of Undefined Length.
    const IMPLICIT_LITTLE_ENDIAN: Self = Self {
        explicit_vr: false,
        big_endian: false,
    };

    fn u16(self, bytes: [u8; 2]) -> u16 {
        if self.big_endian {
            u16::from_be_bytes(bytes)
        } else {
            u16::from_le_bytes(bytes)
        }
    }

    fn u32(self, bytes: [u8; 4]) -> u32 {
        if self.big_endian {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        }
    }
}

/// The Transfer Syntax facts the rules depend on.
#[derive(Clone, Copy, Debug)]
pub(super) struct Syntax {
    pub(super) layout: Layout,
    /// Encapsulated Pixel Data permitted (PS3.5 A.4).
    pub(super) encapsulated: bool,
    /// The Annex A section that sets the Pixel Data VR.
    pub(super) annex: &'static str,
}

/// Framing is lost; why is already recorded.
pub(super) struct Stop;

type Flow<T = ()> = Result<T, Stop>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Extent {
    /// The top-level Data Set, to the end of the stream.
    Stream,
    /// An Item or Sequence of Explicit Length ending at this position.
    Until(u64),
    /// An Item or Sequence of Undefined Length.
    Delimited,
}

#[derive(Debug, Default)]
struct Image {
    rows: Option<u16>,
    columns: Option<u16>,
    samples_per_pixel: Option<u16>,
    bits_allocated: Option<u16>,
    frames: Option<u32>,
    ybr_full_422: bool,
}

impl Image {
    fn native_bytes(&self) -> Option<u64> {
        NativeFrame::new(
            u64::from(self.rows?),
            u64::from(self.columns?),
            u64::from(self.samples_per_pixel?),
            u64::from(self.bits_allocated?),
            self.ybr_full_422,
        )?
        .bytes(u64::from(self.frames.unwrap_or(1)))
        .ok()
    }
}

struct GroupLength {
    group: u16,
    declared: u32,
    /// Position after the Group Length element.
    start: u64,
}

/// One Data Set or Item being read.
struct Context {
    path: String,
    depth: usize,
    /// The Sequence this Item belongs to; `None` for the top-level Data Set.
    sequence: Option<Tag>,
    layout: Layout,
    repertoire: Repertoire,
    in_private_sequence: bool,
    previous: Option<Tag>,
    seen: BTreeSet<Tag>,
    group_length: Option<GroupLength>,
    private: PrivateBlocks,
    image: Image,
}

impl Context {
    fn top(layout: Layout) -> Self {
        Self {
            path: String::new(),
            depth: 0,
            sequence: None,
            layout,
            repertoire: Repertoire::Absent,
            in_private_sequence: false,
            previous: None,
            seen: BTreeSet::new(),
            group_length: None,
            private: PrivateBlocks::default(),
            image: Image::default(),
        }
    }

    /// Item `index` (1-based) of `sequence`, which inherits Specific
    /// Character Set but not Private Creator reservations (PS3.5 7.5.3, 7.8.1).
    fn item(&self, sequence: Tag, index: usize, layout: Layout) -> Self {
        Self {
            path: format!("{}[{index}] > ", element_label(&self.path, sequence)),
            depth: self.depth + 1,
            sequence: Some(sequence),
            layout,
            repertoire: self.repertoire.clone(),
            in_private_sequence: self.in_private_sequence || is_private(sequence),
            ..Self::top(layout)
        }
    }

    fn scope(&self) -> &'static str {
        if self.depth == 0 {
            "Data Set"
        } else {
            "Item"
        }
    }

    fn label(&self, tag: Tag) -> String {
        element_label(&self.path, tag)
    }

    /// PS3.5 7.2: a Group Length covers the elements of its group that
    /// follow it, up to `position`.
    fn close_group_length(
        &mut self,
        next_group: Option<u16>,
        position: u64,
        findings: &mut Family,
    ) {
        let Some(open) = &self.group_length else {
            return;
        };
        if next_group == Some(open.group) {
            return;
        }
        let actual = position - open.start;
        if actual != u64::from(open.declared) {
            let label = self.label(Tag(open.group, 0x0000));
            let declared = open.declared;
            findings.push(|| format!(
                "{label}: Group Length {declared} but the group's elements take {actual} bytes; a Group Length shall be consistent with the encoding (PS3.5 7.2)"
            ));
        }
        self.group_length = None;
    }
}

/// Even groups 50xx, 60xx and 7Fxx that PS3.6 defines only as repeating
/// groups, outside the overlay groups 6000-601E.
fn retired_repeating_group(group: u16) -> bool {
    group.is_multiple_of(2)
        && match group & 0xFF00 {
            0x5000 => true,
            0x6000 => group > 0x601E,
            0x7F00 => group != 0x7FE0,
            _ => false,
        }
}

/// PS3.5 7.8.2: Attributes excluded from Private Sequence Items.
fn is_bulk_pixel_data(tag: Tag) -> bool {
    matches!(
        tag,
        tags::PIXEL_DATA
            | tags::FLOAT_PIXEL_DATA
            | tags::DOUBLE_FLOAT_PIXEL_DATA
            | tags::WAVEFORM_DATA
    ) || ((0x6000..=0x601E).contains(&tag.group())
        && tag.group().is_multiple_of(2)
        && tag.element() == 0x3000)
}

/// VRs whose Values are character strings padded with SPACE (PS3.5 6.2).
fn space_padded(vr: VR) -> bool {
    matches!(
        vr,
        VR::AE
            | VR::AS
            | VR::CS
            | VR::DA
            | VR::DS
            | VR::DT
            | VR::IS
            | VR::LO
            | VR::LT
            | VR::PN
            | VR::SH
            | VR::ST
            | VR::TM
            | VR::UC
            | VR::UR
            | VR::UT
    )
}

/// The VR an Implicit VR element has: the PS3.6 VR, Group Length UL, or
/// Private Creator LO; `None` for other private and unregistered tags.
fn implicit_vr(tag: Tag) -> Option<VR> {
    if tag.element() == 0x0000 {
        return Some(VR::UL);
    }
    if is_creator(tag) {
        return Some(VR::LO);
    }
    if is_private(tag) {
        return None;
    }
    let vrs = dictionary_vrs(tag)?;
    if vrs.contains(&VR::SQ) {
        Some(VR::SQ)
    } else if vrs.contains(&VR::OW) {
        // PS3.5 A.1: an "OB or OW" element such as Pixel Data is OW in Implicit VR.
        Some(VR::OW)
    } else {
        vrs.first().copied()
    }
}

struct Header {
    tag: Tag,
    /// The encoded VR, or in Implicit VR the dictionary's; `None` when unknown.
    vr: Option<VR>,
    length: u32,
}

/// What a Value's bytes are read for.
struct Plan<'s, 'r> {
    head: usize,
    tail: usize,
    scanner: Option<&'s mut TextScanner<'r>>,
}

#[derive(Default)]
struct ValueBytes {
    head: Vec<u8>,
    tail: Vec<u8>,
}

pub(super) struct Walker<'a> {
    source: &'a mut Source,
    findings: &'a mut Family,
    syntax: Syntax,
}

/// Walks the whole Data Set; returns whether it was read to its end.
pub(super) fn walk(source: &mut Source, syntax: Syntax, findings: &mut Family) -> bool {
    let mut walker = Walker {
        source,
        findings,
        syntax,
    };
    let mut root = Context::top(syntax.layout);
    walker.data_set(&mut root, Extent::Stream).is_ok()
}

impl Walker<'_> {
    /// Records why reading stopped.
    fn fail(&mut self, failure: Failure, what: impl FnOnce() -> String) -> Stop {
        match failure {
            Failure::Truncated { needed, available } => self.findings.push(|| format!(
                "{} needs {needed} bytes but the Data Set ends after {available}; a Value Length shall equal the length of its Value Field (PS3.5 7.1.2 and 7.1.3)",
                what()
            )),
            Failure::Limit(limit) => self.findings.push(|| format!(
                "the inflated Data Set exceeds {limit} bytes (max_input_bytes); the rest is not checked"
            )),
            Failure::Io(error) => self.findings.push(|| format!("{}: cannot read the Data Set: {error}", what())),
        }
        Stop
    }

    fn array<const N: usize>(&mut self, what: impl FnOnce() -> String) -> Flow<[u8; N]> {
        self.source
            .read_array::<N>()
            .map_err(|failure| self.fail(failure, what))
    }

    fn tag(&mut self, layout: Layout, what: impl FnOnce() -> String) -> Flow<Tag> {
        let bytes = self.array::<4>(what)?;
        Ok(Tag(
            layout.u16([bytes[0], bytes[1]]),
            layout.u16([bytes[2], bytes[3]]),
        ))
    }

    fn length(&mut self, layout: Layout, what: impl FnOnce() -> String) -> Flow<u32> {
        self.array::<4>(what).map(|bytes| layout.u32(bytes))
    }

    fn data_set(&mut self, context: &mut Context, extent: Extent) -> Flow {
        let end = loop {
            let start = self.source.position();
            match extent {
                Extent::Until(end) if start > end => return Err(self.overrun(context, start - end)),
                Extent::Until(end) if start == end => break start,
                Extent::Stream => match self.source.at_end() {
                    Ok(true) => break start,
                    Ok(false) => {}
                    Err(failure) => return Err(self.fail(failure, || "the next element".into())),
                },
                _ => {}
            }
            if extent == Extent::Stream && self.trailing_bytes()? {
                break start;
            }
            let layout = context.layout;
            let path = context.path.clone();
            let tag = self.tag(layout, || format!("{path}the next element tag"))?;
            if tag.group() == 0xFFFE {
                if self.delimiter_in_data_set(context, tag, extent)? {
                    break start;
                }
                continue;
            }
            let header = self.header(context, tag)?;
            context.close_group_length(Some(tag.group()), start, self.findings);
            self.observe(context, &header);
            let limit = match extent {
                Extent::Until(end) => Some(end),
                _ => None,
            };
            self.value(context, &header, limit)?;
        };
        context.close_group_length(None, end, self.findings);
        context
            .private
            .finish(&context.path, context.depth > 0, self.findings);
        Ok(())
    }

    /// An Item of Undefined Length ran past the Explicit Length around it.
    fn overrun(&mut self, context: &Context, beyond: u64) -> Stop {
        let path = &context.path;
        let scope = context.scope();
        self.findings.push(|| format!(
            "{path}its elements extend {beyond} bytes beyond the {scope}'s Explicit Length; the rest is not checked (PS3.5 7.5.1)"
        ));
        Stop
    }

    /// PS3.10 7.2: bytes after the last element that cannot hold another.
    fn trailing_bytes(&mut self) -> Flow<bool> {
        let available = match self.source.peek(4) {
            Ok(bytes) => bytes.len(),
            Err(failure) => return Err(self.fail(failure, || "the next element".into())),
        };
        if available == 0 || available >= 4 {
            return Ok(false);
        }
        let (bytes, are) = if available == 1 {
            ("byte", "is")
        } else {
            ("bytes", "are")
        };
        self.findings.push(|| format!(
            "{available} trailing {bytes} after the last Data Element {are} not a Data Element; a File contains a single Data Set (PS3.10 7.2)"
        ));
        Ok(true)
    }

    /// An (FFFE,eeee) tag where a Data Element is expected; returns whether
    /// it ends the Item.
    fn delimiter_in_data_set(&mut self, context: &Context, tag: Tag, extent: Extent) -> Flow<bool> {
        let path = context.path.clone();
        let length = self.length(context.layout, || format!("{path}{}", tag_text(raw(tag))))?;
        match (tag, extent) {
            (ITEM_DELIMITATION, Extent::Delimited) => {
                self.zero_length(&path, tag, length);
                Ok(true)
            }
            (ITEM_DELIMITATION | SEQUENCE_DELIMITATION, Extent::Stream) => {
                self.findings.push(|| {
                    format!(
                        "{} outside any Sequence; a File contains a single Data Set (PS3.10 7.2)",
                        tag_text(raw(tag))
                    )
                });
                Ok(false)
            }
            (ITEM, Extent::Stream) if length != UNDEFINED => {
                self.findings.push(|| {
                    "Item (FFFE,E000) outside any Sequence; a File contains a single Data Set (PS3.10 7.2)"
                        .to_string()
                });
                self.source
                    .skip(u64::from(length))
                    .map_err(|failure| self.fail(failure, || "Item (FFFE,E000)".into()))?;
                Ok(false)
            }
            (ITEM_DELIMITATION, Extent::Until(_)) => {
                self.findings.push(|| format!(
                    "{path}Item Delimitation Item (FFFE,E00D) in an Item of Explicit Length; it ends Items of Undefined Length only (PS3.5 7.5.1)"
                ));
                self.zero_length(&path, tag, length);
                Ok(false)
            }
            (SEQUENCE_DELIMITATION, Extent::Until(_)) => {
                self.findings.push(|| {
                    format!(
                        "{path}Sequence Delimitation Item (FFFE,E0DD) inside an Item (PS3.5 7.5.2)"
                    )
                });
                Ok(false)
            }
            _ => {
                let scope = context.scope();
                self.findings.push(|| format!(
                    "{path}{} where a Data Element of the {scope} is expected; the rest is not checked (PS3.5 7.5)",
                    tag_text(raw(tag))
                ));
                Err(Stop)
            }
        }
    }

    /// PS3.5 7.5.1 and 7.5.2: delimiters carry no Value. `path` precedes
    /// the delimiter's name in the message.
    fn zero_length(&mut self, path: &str, tag: Tag, length: u32) {
        if length == 0 {
            return;
        }
        let (name, section) = if tag == ITEM_DELIMITATION {
            ("Item Delimitation Item (FFFE,E00D)", "7.5.1")
        } else {
            ("Sequence Delimitation Item (FFFE,E0DD)", "7.5.2")
        };
        self.findings.push(|| format!(
            "{path}{name} has Value Length {length:08X}H; it shall be 00000000H with no Value (PS3.5 {section})"
        ));
    }

    fn header(&mut self, context: &Context, tag: Tag) -> Flow<Header> {
        let layout = context.layout;
        let label = || context.label(tag);
        if !layout.explicit_vr {
            let length = self.length(layout, label)?;
            return Ok(Header {
                tag,
                vr: implicit_vr(tag),
                length,
            });
        }
        let bytes = self.array::<2>(label)?;
        let vr = VR::from_binary(bytes);
        if vr.is_none() {
            self.invalid_vr(context, tag, bytes);
        }
        // PS3.5 6.2: an unrecognized VR has the 32-bit Value Length of the
        // newer VRs.
        if !vr.is_none_or(long_length) {
            let length = self.array::<2>(label)?;
            return Ok(Header {
                tag,
                vr,
                length: u32::from(layout.u16(length)),
            });
        }
        let reserved = self.array::<2>(label)?;
        if reserved != [0, 0] {
            self.findings.push(|| format!(
                "{}: reserved bytes after VR {} are {:02X}{:02X}H; they shall be 0000H (PS3.5 7.1.2)",
                label(),
                String::from_utf8_lossy(&bytes),
                reserved[0],
                reserved[1]
            ));
        }
        let length = self.length(layout, label)?;
        Ok(Header { tag, vr, length })
    }

    fn invalid_vr(&mut self, context: &Context, tag: Tag, bytes: [u8; 2]) {
        let label = context.label(tag);
        let upper = [bytes[0].to_ascii_uppercase(), bytes[1].to_ascii_uppercase()];
        let annex = self.syntax.annex;
        if VR::from_binary(upper).is_some() {
            self.findings.push(|| format!(
                "{label}: VR bytes \"{}\" are not upper case; the VR shall be two upper case letters of the Default Character Repertoire (PS3.5 7.1.1)",
                String::from_utf8_lossy(&bytes)
            ));
        } else {
            let private = if is_private(tag) { " 7.8, 7.8.2," } else { "" };
            self.findings.push(|| format!(
                "{label}: VR bytes {:02X}H {:02X}H are not a VR of PS3.5 6.2; Data Elements shall be encoded with an Explicit VR of PS3.5 6.2 (PS3.5 7.1.2,{private} and {annex})",
                bytes[0], bytes[1]
            ));
        }
    }

    /// Rules on the tag itself, before its Value.
    fn observe(&mut self, context: &mut Context, header: &Header) {
        let tag = header.tag;
        let duplicate = !context.seen.insert(tag);
        let previous = context.previous;
        context.previous = previous.max(Some(tag));
        if is_private(tag) && !matches!(tag.group(), 0x0001 | 0x0003 | 0x0005 | 0x0007 | 0xFFFF) {
            context.private.element(tag, &context.path, self.findings);
        }
        let context = &*context;
        let label = || context.label(tag);
        let scope = context.scope();
        if duplicate {
            self.findings.push(|| format!(
                "{}: occurs more than once in this {scope}; a Data Element shall occur at most once (PS3.5 7.1 and 7.5.1)",
                label()
            ));
        } else if let Some(previous) = previous.filter(|previous| tag < *previous) {
            let section = if is_private(tag) {
                "7.1 and 7.8"
            } else {
                "7.1 and 7.5.1"
            };
            self.findings.push(|| format!(
                "{}: follows {}; the Data Elements of a {scope} shall be in increasing tag order (PS3.5 {section})",
                label(),
                tag_text(raw(previous))
            ));
        }
        let group = tag.group();
        match group {
            0x0001 | 0x0003 | 0x0005 | 0x0007 | 0xFFFF => self.findings.push(|| {
                format!(
                    "{}: group {group:04X} shall not be used (PS3.5 7.1, 7.5.1 and 7.8.1)",
                    label()
                )
            }),
            0x0000 | 0x0002 | 0x0006 if context.depth > 0 => {
                let file_meta = if group == 0x0002 {
                    "; group 0002 belongs only to the File Meta Information (PS3.10 7.1)"
                } else {
                    ""
                };
                self.findings.push(|| format!(
                    "{}: group {group:04X} shall not be present within Sequence Items (PS3.5 7.5.1){file_meta}",
                    label()
                ));
            }
            _ if retired_repeating_group(group) => self.findings.push(|| {
                format!(
                "{}: Repeating Groups are only allowed in the even groups 6000-601E (PS3.5 7.6)",
                label()
            )
            }),
            _ => {}
        }
        if context.in_private_sequence && is_bulk_pixel_data(tag) {
            self.findings.push(|| format!(
                "{}: shall not be included in a Private Sequence Item or an Item nested within one (PS3.5 7.8.2)",
                label()
            ));
        }
        if tag == tags::SPECIFIC_CHARACTER_SET
            && context
                .sequence
                .is_some_and(|sequence| item_admits_specific_character_set(sequence) == Some(false))
        {
            self.findings.push(|| format!(
                "{}: the IOD does not define Specific Character Set for the Items of this Sequence (PS3.5 7.5.3)",
                label()
            ));
        }
    }

    fn value(&mut self, context: &mut Context, header: &Header, limit: Option<u64>) -> Flow {
        if header.length == UNDEFINED {
            return self.undefined_length(context, header);
        }
        let end = self.source.position() + u64::from(header.length);
        if let Some(limit) = limit.filter(|limit| end > *limit) {
            let label = context.label(header.tag);
            let length = header.length;
            let scope = context.scope();
            self.findings.push(|| format!(
                "{label}: Value Length {length} extends {} bytes beyond its {scope}; the rest is not checked (PS3.5 7.5.1)",
                end - limit
            ));
            return Err(Stop);
        }
        if header.vr == Some(VR::SQ) {
            let layout = context.layout;
            return self.sequence(context, header.tag, Extent::Until(end), layout);
        }
        self.primitive(context, header)
    }

    fn undefined_length(&mut self, context: &mut Context, header: &Header) -> Flow {
        let tag = header.tag;
        let layout = context.layout;
        if tag == tags::PIXEL_DATA {
            let annex = self.syntax.annex;
            if !layout.explicit_vr || !self.syntax.encapsulated {
                self.findings.push(|| format!(
                    "{}: Undefined Length, but the Transfer Syntax encodes Pixel Data in Native Format with an Explicit Length (PS3.5 7.1.1 and {annex})",
                    context.label(tag)
                ));
            } else if let Some(vr) = header.vr.filter(|vr| *vr != VR::OB) {
                self.findings.push(|| {
                    format!(
                        "{}: encapsulated Pixel Data with VR {vr:?}; it shall be OB (PS3.5 A.4)",
                        context.label(tag)
                    )
                });
            }
            return self.fragments(context);
        }
        match header.vr {
            Some(VR::SQ) => self.sequence(context, tag, Extent::Delimited, layout),
            Some(VR::UN) if layout.explicit_vr => {
                let items = self.unknown_items_layout(context, tag)?;
                self.sequence(context, tag, Extent::Delimited, items)
            }
            None if !layout.explicit_vr => self.sequence(context, tag, Extent::Delimited, layout),
            vr => {
                self.findings.push(|| format!(
                    "{}: Undefined Length on VR {}; it is permitted only for SQ, UN and encapsulated Pixel Data (PS3.5 7.1.1)",
                    context.label(tag),
                    vr.map_or("UN", VR::to_string)
                ));
                // As the parser does, the Value is read as Items.
                self.sequence(context, tag, Extent::Delimited, layout)
            }
        }
    }

    /// PS3.5 6.2.2: the Items of a UN of Undefined Length stay in Implicit
    /// VR Little Endian. Explicit VR bytes in its first element's header
    /// show the Items were re-encoded; they are then read as encoded.
    fn unknown_items_layout(&mut self, context: &Context, tag: Tag) -> Flow<Layout> {
        let bytes = match self.source.peek(16) {
            Ok(bytes) => bytes.to_vec(),
            Err(failure) => return Err(self.fail(failure, || context.label(tag))),
        };
        let implicit = Layout::IMPLICIT_LITTLE_ENDIAN;
        let [i0, i1, i2, i3, _, _, _, _, g0, g1, e0, e1, v0, v1, l0, l1] = bytes[..] else {
            return Ok(implicit);
        };
        let item = Tag(implicit.u16([i0, i1]), implicit.u16([i2, i3]));
        let first = Tag(implicit.u16([g0, g1]), implicit.u16([e0, e1]));
        let Some(vr) = VR::from_binary([v0, v1]) else {
            return Ok(implicit);
        };
        // An Implicit VR length whose low bytes spell a VR is implausibly
        // long unless the dictionary has that very VR for the tag.
        let implicit_length = u64::from(u32::from_le_bytes([v0, v1, l0, l1]));
        let explicit_form = item == ITEM
            && first.group() != 0xFFFE
            && (implicit_vr(first) == Some(vr)
                || self
                    .source
                    .remaining()
                    .is_some_and(|remaining| implicit_length > remaining));
        if !explicit_form {
            return Ok(implicit);
        }
        self.findings.push(|| format!(
            "{}: UN of Undefined Length whose Items are in Explicit VR; they shall remain in Implicit VR form (PS3.5 6.2.2)",
            context.label(tag)
        ));
        Ok(context.layout)
    }

    /// The Items of a Sequence, or of an element read as one.
    fn sequence(
        &mut self,
        parent: &mut Context,
        sequence: Tag,
        extent: Extent,
        layout: Layout,
    ) -> Flow {
        let label = parent.label(sequence);
        if parent.depth >= MAX_NESTING {
            self.findings.push(|| {
                format!(
                "{label}: Sequence nesting exceeds {MAX_NESTING} levels; deeper Items not checked"
            )
            });
            return match extent {
                Extent::Until(end) => {
                    let length = end - self.source.position();
                    self.source
                        .skip(length)
                        .map_err(|failure| self.fail(failure, || label.clone()))
                }
                _ => Err(Stop),
            };
        }
        let mut index = 0;
        loop {
            if let Extent::Until(end) = extent {
                let position = self.source.position();
                if position > end {
                    self.findings.push(|| format!(
                        "{label}: its Items extend {} bytes beyond its Value Length; the rest is not checked (PS3.5 7.5.2)",
                        position - end
                    ));
                    return Err(Stop);
                }
                if position == end {
                    return Ok(());
                }
            }
            let tag = self.tag(layout, || format!("{label} Item tag"))?;
            let length = self.length(layout, || format!("{label} Item length"))?;
            match tag {
                ITEM => {
                    index += 1;
                    let mut item = parent.item(sequence, index, layout);
                    let item_extent = if length == UNDEFINED {
                        Extent::Delimited
                    } else {
                        let end = self.source.position() + u64::from(length);
                        if let Extent::Until(limit) = extent {
                            if end > limit {
                                self.findings.push(|| format!(
                                    "{label}[{index}]: Item Length {length} extends {} bytes beyond its Sequence; the rest is not checked (PS3.5 7.5.1)",
                                    end - limit
                                ));
                                return Err(Stop);
                            }
                        }
                        Extent::Until(end)
                    };
                    self.data_set(&mut item, item_extent)?;
                }
                SEQUENCE_DELIMITATION => {
                    self.zero_length(&format!("{label}: "), tag, length);
                    if extent == Extent::Delimited {
                        return Ok(());
                    }
                    self.findings.push(|| format!(
                        "{label}: Sequence Delimitation Item (FFFE,E0DD) in a Sequence of Explicit Length (PS3.5 7.5.2)"
                    ));
                }
                ITEM_DELIMITATION => {
                    self.findings.push(|| {
                        format!(
                        "{label}: Item Delimitation Item (FFFE,E00D) outside an Item (PS3.5 7.5.1)"
                    )
                    });
                    self.zero_length(&format!("{label}: "), tag, length);
                }
                other => {
                    self.findings.push(|| format!(
                        "{label}: {} where an Item (FFFE,E000) is expected; the rest is not checked (PS3.5 7.5)",
                        tag_text(raw(other))
                    ));
                    return Err(Stop);
                }
            }
        }
    }

    /// PS3.5 A.4: the Basic Offset Table Item, fragment Items of Explicit,
    /// even and non-zero Length, then a Sequence Delimitation Item.
    fn fragments(&mut self, context: &Context) -> Flow {
        let layout = context.layout;
        let label = context.label(tags::PIXEL_DATA);
        // Item 0 is the Basic Offset Table; Fragments are numbered from 1.
        let mut index = 0u64;
        loop {
            match self.source.at_end() {
                Ok(true) => {
                    self.findings.push(|| format!(
                        "{label}: the encapsulated Pixel Data reaches the end of the Data Set without a Sequence Delimitation Item (FFFE,E0DD) (PS3.5 7.5.2 and A.4)"
                    ));
                    return Err(Stop);
                }
                Ok(false) => {}
                Err(failure) => return Err(self.fail(failure, || format!("{label} fragment Item"))),
            }
            let tag = self.tag(layout, || {
                format!("{label} Sequence Delimitation Item (FFFE,E0DD)")
            })?;
            let length = self.length(layout, || format!("{label} fragment Item length"))?;
            match tag {
                ITEM if length != UNDEFINED => {
                    if index > 0 && (length == 0 || length % 2 == 1) {
                        self.findings.push(|| format!(
                            "{label}: Fragment {index} has Item Length {length}; Items containing an encoded Fragment shall be made of an even number of bytes greater or equal to two (PS3.5 A.4)"
                        ));
                    }
                    self.source
                        .skip(u64::from(length))
                        .map_err(|failure| self.fail(failure, || format!("{label} fragment")))?;
                    index += 1;
                }
                SEQUENCE_DELIMITATION => {
                    self.zero_length(&format!("{label}: "), tag, length);
                    return Ok(());
                }
                other => {
                    self.findings.push(|| format!(
                        "{label}: {} of length {length:08X}H where a fragment Item of Explicit Length or the Sequence Delimitation Item is expected; the rest is not checked (PS3.5 A.4)",
                        tag_text(raw(other))
                    ));
                    return Err(Stop);
                }
            }
        }
    }

    /// Reads `length` bytes as `plan` asks, skipping the rest.
    fn read(
        &mut self,
        length: u64,
        plan: Plan<'_, '_>,
        what: impl Fn() -> String,
    ) -> Flow<ValueBytes> {
        if let Some(available) = self
            .source
            .remaining()
            .filter(|available| length > *available)
        {
            let failure = Failure::Truncated {
                needed: length,
                available,
            };
            return Err(self.fail(failure, what));
        }
        let mut bytes = ValueBytes::default();
        let Plan {
            head,
            tail,
            scanner,
        } = plan;
        if let Some(scanner) = scanner {
            let mut last = Vec::with_capacity(tail);
            self.source
                .chunks(length, |chunk| {
                    scanner.feed(chunk);
                    let room = head.saturating_sub(bytes.head.len()).min(chunk.len());
                    bytes.head.extend_from_slice(&chunk[..room]);
                    last.extend_from_slice(&chunk[chunk.len().saturating_sub(tail)..]);
                    let excess = last.len().saturating_sub(tail);
                    last.drain(..excess);
                })
                .map_err(|failure| self.fail(failure, &what))?;
            bytes.tail = last;
            return Ok(bytes);
        }
        let head_length = length.min(head as u64);
        let tail_length = (length - head_length).min(tail as u64);
        self.source
            .chunks(head_length, |chunk| bytes.head.extend_from_slice(chunk))
            .map_err(|failure| self.fail(failure, &what))?;
        self.source
            .skip(length - head_length - tail_length)
            .map_err(|failure| self.fail(failure, &what))?;
        self.source
            .chunks(tail_length, |chunk| bytes.tail.extend_from_slice(chunk))
            .map_err(|failure| self.fail(failure, &what))?;
        if bytes.tail.len() < tail {
            let mut joined = bytes.head.clone();
            joined.extend_from_slice(&bytes.tail);
            bytes.tail = joined[joined.len().saturating_sub(tail)..].to_vec();
        }
        Ok(bytes)
    }

    fn primitive(&mut self, context: &mut Context, header: &Header) -> Flow {
        let Header { tag, vr, length } = *header;
        let length = u64::from(length);
        let label = || context.label(tag);
        let repertoire = context.repertoire.clone();
        let mut scanner = vr.and_then(|vr| TextScanner::new(&repertoire, vr));
        let captured = tag == tags::SPECIFIC_CHARACTER_SET
            || tag.element() == 0x0000
            || is_creator(tag)
            || matches!(
                tag,
                tags::ROWS
                    | tags::COLUMNS
                    | tags::SAMPLES_PER_PIXEL
                    | tags::BITS_ALLOCATED
                    | tags::NUMBER_OF_FRAMES
                    | tags::PHOTOMETRIC_INTERPRETATION
                    | tags::ICC_PROFILE
            );
        let padded = vr.is_some_and(|vr| space_padded(vr) || vr == VR::UI);
        let pad_byte = vr == Some(VR::OB) && matches!(tag, tags::PIXEL_DATA | tags::ICC_PROFILE);
        let plan = Plan {
            head: if captured { MAX_CAPTURE } else { 0 },
            tail: if padded { 2 } else { usize::from(pad_byte) },
            scanner: scanner.as_mut(),
        };
        let bytes = self.read(length, plan, label)?;
        if let Some(scanner) = scanner {
            scanner.finish(&context.label(tag), self.findings);
        }
        if let Some(vr) = vr.filter(|_| padded) {
            self.padding(context, tag, vr, &bytes.tail);
        }
        if tag == tags::PIXEL_DATA {
            self.native_pixel_data(context, header, &bytes.tail);
        }
        if captured {
            self.captured(context, header, &bytes);
        }
        Ok(())
    }

    /// PS3.5 6.2: SPACE pads character strings, a single NULL pads UIDs.
    fn padding(&mut self, context: &Context, tag: Tag, vr: VR, tail: &[u8]) {
        let label = || context.label(tag);
        match (vr, tail) {
            (VR::UI, [_, b' ']) => self.findings.push(|| format!(
                "{}: UID Value padded with SPACE (20H); UI Values are padded with a single trailing NULL (00H) (PS3.5 6.2)",
                label()
            )),
            (VR::UI, [0, 0]) => self.findings.push(|| format!(
                "{}: UID Value padded with more than a single trailing NULL (00H) (PS3.5 6.2)",
                label()
            )),
            (VR::UI, _) => {}
            (_, [.., 0]) => self.findings.push(|| format!(
                "{}: {vr:?} Value padded with NULL (00H); character string Values are padded with SPACE (20H) (PS3.5 6.2)",
                label()
            )),
            _ => {}
        }
    }

    /// PS3.5 A.2 to A.5: native Pixel Data with Bits Allocated above 8 is
    /// OW; PS3.5 6.2: an OB Value is padded with 00H.
    fn native_pixel_data(&mut self, context: &Context, header: &Header, tail: &[u8]) {
        let label = || context.label(header.tag);
        let bits = context.image.bits_allocated;
        if context.layout.explicit_vr
            && header.vr == Some(VR::OB)
            && bits.is_some_and(|bits| bits > 8)
        {
            let annex = self.syntax.annex;
            self.findings.push(|| format!(
                "{}: VR OB with Bits Allocated (0028,0100) {}; native Pixel Data with Bits Allocated above 8 shall be OW (PS3.5 {annex})",
                label(),
                bits.unwrap_or_default()
            ));
        }
        if header.vr == Some(VR::OB) {
            let expected = context.image.native_bytes();
            self.pad_byte(label, expected, header.length, tail);
        }
    }

    /// An OB Value one byte longer than its content ends in its pad byte.
    fn pad_byte(
        &mut self,
        label: impl FnOnce() -> String,
        content: Option<u64>,
        length: u32,
        tail: &[u8],
    ) {
        let Some(&last) = tail.last() else {
            return;
        };
        if content.is_some_and(|content| content % 2 == 1 && content + 1 == u64::from(length))
            && last != 0
        {
            self.findings.push(|| format!(
                "{}: OB Value padded with {last:02X}H; OB Values are padded with a single trailing NULL (00H) (PS3.5 6.2)",
                label()
            ));
        }
    }

    /// The Values other rules of this Data Set depend on.
    fn captured(&mut self, context: &mut Context, header: &Header, bytes: &ValueBytes) {
        let tag = header.tag;
        let head = &bytes.head;
        let layout = context.layout;
        let unsigned = || (head.len() >= 2).then(|| layout.u16([head[0], head[1]]));
        let text = || {
            String::from_utf8_lossy(head)
                .trim_matches([' ', '\0'])
                .to_string()
        };
        match tag {
            tags::SPECIFIC_CHARACTER_SET => {
                context.repertoire = if header.length as usize > MAX_CAPTURE {
                    Repertoire::Unknown
                } else {
                    specific_character_set(head, &context.label(tag), self.findings)
                };
            }
            tags::ROWS => context.image.rows = unsigned(),
            tags::COLUMNS => context.image.columns = unsigned(),
            tags::SAMPLES_PER_PIXEL => context.image.samples_per_pixel = unsigned(),
            tags::BITS_ALLOCATED => context.image.bits_allocated = unsigned(),
            tags::NUMBER_OF_FRAMES => context.image.frames = text().parse().ok(),
            tags::PHOTOMETRIC_INTERPRETATION => {
                context.image.ybr_full_422 = text() == "YBR_FULL_422"
            }
            tags::ICC_PROFILE if header.vr == Some(VR::OB) => {
                // ICC.1 7.2.2: the profile size is the header's first field, big-endian.
                let size = (head.len() >= 4)
                    .then(|| u64::from(u32::from_be_bytes([head[0], head[1], head[2], head[3]])));
                let label = || context.label(tag);
                self.pad_byte(label, size, header.length, &bytes.tail);
            }
            _ if is_creator(tag) => {
                let implicit = !layout.explicit_vr;
                context
                    .private
                    .creator(tag, head, implicit, &context.path, self.findings);
            }
            _ if tag.element() == 0x0000 && head.len() == 4 => {
                context.group_length = Some(GroupLength {
                    group: tag.group(),
                    declared: layout.u32([head[0], head[1], head[2], head[3]]),
                    start: self.source.position(),
                });
            }
            _ => {}
        }
    }
}
