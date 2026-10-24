//! The encoded bytes of SH, LO, UC, ST, LT, UT and PN Values under the
//! repertoire of their Data Set, before any decoding: ISO 2022 Code
//! Extension (PS3.5 6.1.2.5), the C1 and shift prohibitions (6.1.1), new
//! lines (6.1.3), minimal UTF-8 (PS3.3 C.12.1.1.2), Person Name component
//! groups (6.2.1.2) and Value lengths without escape sequences (6.2).

use std::collections::BTreeMap;

use dicom_core::VR;
use dicom_encoding::text::{SpecificCharacterSet, TextCodec};

use super::charset::{Element, Repertoire, Set, Unextendable};
use crate::validation::iod::Family;

const ESC: u8 = 0x1B;
const LF: u8 = 0x0A;
const FF: u8 = 0x0C;
const CR: u8 = 0x0D;
const SO: u8 = 0x0E;
const SI: u8 = 0x0F;
const SPACE: u8 = 0x20;
const EQUALS: u8 = 0x3D;
const BACKSLASH: u8 = 0x5C;
const CARET: u8 = 0x5E;
/// Intermediate bytes accepted in one escape sequence before it is malformed.
const MAX_INTERMEDIATES: usize = 3;
/// Alphabetic component group bytes kept for decoding: 64 characters of up
/// to four bytes each.
const MAX_ALPHABETIC_BYTES: usize = 256;
/// Component groups recorded per Person Name Value; more already violate.
const MAX_GROUPS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// SH, LO, UC: 05/12 separates Values.
    Strings,
    /// ST, LT, UT: single-valued text with lines and pages.
    Paragraphs,
    /// PN: Values of component groups of components.
    PersonName,
}

/// The text VRs whose repertoire Specific Character Set governs, with each
/// Value's (or PN component group's) character maximum.
fn kind(vr: VR) -> Option<(Kind, Option<usize>)> {
    Some(match vr {
        VR::SH => (Kind::Strings, Some(16)),
        VR::LO => (Kind::Strings, Some(64)),
        VR::UC => (Kind::Strings, None),
        VR::ST => (Kind::Paragraphs, Some(1024)),
        VR::LT => (Kind::Paragraphs, Some(10240)),
        VR::UT => (Kind::Paragraphs, None),
        VR::PN => (Kind::PersonName, Some(64)),
        _ => return None,
    })
}

/// One kind of defect; each is reported once per element, at its first
/// occurrence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Issue {
    OutsideRepertoire,
    C1,
    Shift,
    ControlSet,
    UnknownEscape,
    IncompleteEscape,
    ExtensionNotPermitted,
    Undeclared,
    NotReturned,
    NoG1,
    IncompleteCharacter,
    Backslash,
    AlphabeticEscape,
    AlphabeticCodePoint,
    Overlong,
    Invalid,
    BareLineFeed,
    BareCarriageReturn,
    TooLong,
    TooManyGroups,
    TooManyComponents,
}

/// "ESC 02/04 04/02", the column/row notation of PS3.3 Tables C.12-3/C.12-4.
fn escape_text(sequence: &[u8]) -> String {
    std::iter::once("ESC".to_string())
        .chain(
            sequence
                .iter()
                .map(|byte| format!("{:02}/{:02}", byte >> 4, byte & 0x0F)),
        )
        .collect::<Vec<_>>()
        .join(" ")
}

/// A UTF-8, GB18030 or GBK character in progress.
#[derive(Clone, Copy, Debug, Default)]
struct Multibyte {
    lead: u8,
    /// Bytes of the character seen so far.
    seen: u8,
    /// Bytes the character takes.
    length: u8,
    code_point: u32,
    overlong: bool,
    /// Offset of the lead byte in the Value.
    offset: u64,
}

enum Step {
    More,
    Done { code_point: Option<u32>, trail: u8 },
    Overlong,
    Invalid,
}

impl Multibyte {
    fn active(&self) -> bool {
        self.seen > 0
    }

    /// Starts a character at a byte of 80H-FFH; `None` when no character
    /// starts with it.
    fn start(byte: u8, code: Unextendable, offset: u64) -> Option<Self> {
        let (length, code_point, overlong) = match code {
            Unextendable::Utf8 => match byte {
                0xC0 | 0xC1 => (2, 0, true),
                0xC2..=0xDF => (2, u32::from(byte & 0x1F), false),
                0xE0..=0xEF => (3, u32::from(byte & 0x0F), false),
                0xF0..=0xF4 => (4, u32::from(byte & 0x07), false),
                _ => return None,
            },
            // GBK, as Code Page 936, encodes the euro sign as the single byte 80H.
            Unextendable::Gbk if byte == 0x80 => (1, 0, false),
            Unextendable::Gb18030 | Unextendable::Gbk => match byte {
                0x81..=0xFE => (2, 0, false),
                _ => return None,
            },
        };
        Some(Self {
            lead: byte,
            seen: 1,
            length,
            code_point,
            overlong,
            offset,
        })
    }

    fn next(&mut self, byte: u8, code: Unextendable) -> Step {
        match code {
            Unextendable::Utf8 => self.next_utf8(byte),
            Unextendable::Gb18030 | Unextendable::Gbk => self.next_gb(byte, code),
        }
    }

    fn next_utf8(&mut self, byte: u8) -> Step {
        let (low, high) = match (self.seen, self.lead) {
            (1, 0xE0) => (0xA0, 0xBF),
            (1, 0xED) => (0x80, 0x9F),
            (1, 0xF0) => (0x90, 0xBF),
            (1, 0xF4) => (0x80, 0x8F),
            _ => (0x80, 0xBF),
        };
        if !(0x80..=0xBF).contains(&byte) {
            return Step::Invalid;
        }
        if !(low..=high).contains(&byte) {
            // Below the range after E0 or F0 the sequence is overlong; above it
            // after ED or F4 it encodes a surrogate or exceeds U+10FFFF.
            if byte < low {
                self.overlong = true;
            } else {
                return Step::Invalid;
            }
        }
        self.code_point = self.code_point << 6 | u32::from(byte & 0x3F);
        self.seen += 1;
        if self.seen < self.length {
            return Step::More;
        }
        if self.overlong {
            Step::Overlong
        } else {
            Step::Done {
                code_point: Some(self.code_point),
                trail: byte,
            }
        }
    }

    fn next_gb(&mut self, byte: u8, code: Unextendable) -> Step {
        let valid = match (self.seen, self.length) {
            (1, _) if (0x30..=0x39).contains(&byte) && code == Unextendable::Gb18030 => {
                self.length = 4;
                true
            }
            (1, _) => matches!(byte, 0x40..=0x7E | 0x80..=0xFE),
            (2, 4) => (0x81..=0xFE).contains(&byte),
            _ => (0x30..=0x39).contains(&byte),
        };
        if !valid {
            return Step::Invalid;
        }
        self.seen += 1;
        if self.seen < self.length {
            Step::More
        } else {
            Step::Done {
                code_point: None,
                trail: byte,
            }
        }
    }
}

/// The counts of one Person Name component group.
#[derive(Clone, Copy, Debug, Default)]
struct Group {
    characters: usize,
    components: usize,
}

/// Scans one element's Value bytes; `finish` reports what it found.
pub(super) struct TextScanner<'r> {
    repertoire: &'r Repertoire,
    kind: Kind,
    limit: Option<usize>,
    issues: BTreeMap<Issue, String>,
    offset: u64,
    initial: (Set, Option<Set>),
    g0: Set,
    g1: Option<Set>,
    /// First byte of a two-byte ISO 2022 character, and whether it is GR.
    pending: Option<(u8, bool)>,
    multibyte: Multibyte,
    /// Bytes after ESC so far, and the offset of the ESC.
    escape: Option<(Vec<u8>, u64)>,
    after_cr: bool,
    /// 1-based Value number.
    value: usize,
    characters: usize,
    /// The Value uses Code Extension; its length is checked here.
    extension: bool,
    trailing_space: bool,
    groups: Vec<Group>,
    alphabetic: Vec<u8>,
}

impl<'r> TextScanner<'r> {
    /// A scanner for an element of `vr`, or `None` when the VR's repertoire
    /// is not Specific Character Set's or the repertoire cannot be read.
    pub(super) fn new(repertoire: &'r Repertoire, vr: VR) -> Option<Self> {
        let (kind, limit) = kind(vr)?;
        if *repertoire == Repertoire::Unknown {
            return None;
        }
        let initial = repertoire.initial();
        Some(Self {
            repertoire,
            kind,
            limit,
            issues: BTreeMap::new(),
            offset: 0,
            initial,
            g0: initial.0,
            g1: initial.1,
            pending: None,
            multibyte: Multibyte::default(),
            escape: None,
            after_cr: false,
            value: 1,
            characters: 0,
            extension: false,
            trailing_space: false,
            groups: vec![Group::default()],
            alphabetic: Vec::new(),
        })
    }

    pub(super) fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.byte(byte);
            self.offset += 1;
        }
    }

    /// Ends the Value and reports each defect found, once, under `label`.
    pub(super) fn finish(mut self, label: &str, findings: &mut Family) {
        if let Some((sequence, start)) = self.escape.take() {
            self.incomplete_escape(&sequence, start);
        }
        if self.multibyte.active() {
            self.invalid(self.multibyte.offset);
            self.multibyte = Multibyte::default();
        }
        if self.after_cr {
            self.bare_carriage_return();
        }
        self.before_reset("the end of the Value");
        self.end_value(true);
        for message in self.issues.into_values() {
            findings.push(|| format!("{label}{message}"));
        }
    }

    fn issue(&mut self, issue: Issue, message: impl FnOnce() -> String) {
        self.issues.entry(issue).or_insert_with(message);
    }

    fn byte(&mut self, byte: u8) {
        if self.kind == Kind::Paragraphs {
            if self.after_cr && byte != LF {
                self.bare_carriage_return();
            }
            if byte == LF && !self.after_cr {
                let offset = self.offset;
                self.issue(Issue::BareLineFeed, || format!(
                    ": LF at byte {offset} without a preceding CR; a new line in ST, LT and UT shall be CR LF (PS3.5 6.1.3)"
                ));
            }
            self.after_cr = byte == CR;
        }
        if let Some((sequence, start)) = self.escape.as_mut() {
            match byte {
                0x20..=0x2F if sequence.len() < MAX_INTERMEDIATES => {
                    sequence.push(byte);
                    return;
                }
                0x30..=0x7E => {
                    sequence.push(byte);
                    let (sequence, start) = (std::mem::take(sequence), *start);
                    self.escape = None;
                    self.escape_sequence(&sequence, start);
                    return;
                }
                _ => {
                    let (sequence, start) = (std::mem::take(sequence), *start);
                    self.escape = None;
                    self.incomplete_escape(&sequence, start);
                }
            }
        }
        match self.repertoire {
            Repertoire::Unextendable(code) => {
                let code = *code;
                self.multibyte_byte(byte, code);
            }
            _ => self.iso2022_byte(byte),
        }
    }

    fn bare_carriage_return(&mut self) {
        let offset = self.offset.saturating_sub(1);
        self.issue(Issue::BareCarriageReturn, || format!(
            ": CR at byte {offset} not followed by LF; a new line in ST, LT and UT shall be CR LF (PS3.5 6.1.3)"
        ));
    }

    /// A byte of 00H-7FH outside any multi-byte character: controls,
    /// delimiters and single-byte graphic characters.
    fn left(&mut self, byte: u8) {
        self.trailing_space = byte == SPACE;
        match byte {
            ESC => {
                self.cut_character("ESC");
                self.escape = Some((Vec::new(), self.offset));
            }
            CR | LF | FF => {
                self.before_reset(match byte {
                    CR => "CR",
                    LF => "LF",
                    _ => "FF",
                });
                self.reset();
                self.character();
            }
            SO | SI => {
                let offset = self.offset;
                self.issue(Issue::Shift, || format!(
                    ": {} at byte {offset} is a locking shift; locking shifts shall not be used (PS3.5 6.1.1 and 6.1.2.5.2)",
                    if byte == SO { "SO (0EH)" } else { "SI (0FH)" }
                ));
                self.before_reset("a control character");
                self.character();
            }
            0x00..=0x1F => {
                self.before_reset("a control character");
                self.character();
            }
            BACKSLASH if self.kind != Kind::Paragraphs => {
                self.before_reset("the 05/12 Value delimiter");
                self.end_value(false);
                self.value += 1;
                self.reset();
            }
            CARET if self.kind == Kind::PersonName => {
                self.before_reset("the \"^\" component delimiter");
                if let Some(group) = self.groups.last_mut() {
                    group.components += 1;
                }
                self.character();
                self.reset();
            }
            // The group delimiter is not counted, as the value-format check
            // does not count it.
            EQUALS if self.kind == Kind::PersonName => {
                self.before_reset("the \"=\" component group delimiter");
                self.end_group();
                if self.groups.len() < MAX_GROUPS {
                    self.groups.push(Group::default());
                }
                self.reset();
            }
            _ => self.character(),
        }
    }

    fn character(&mut self) {
        self.characters += 1;
        if let Some(group) = self.groups.last_mut() {
            group.characters += 1;
        }
    }

    fn group_index(&self) -> usize {
        self.groups.len() - 1
    }

    fn iso2022_byte(&mut self, byte: u8) {
        match byte {
            0x21..=0x7E if self.g0.double_byte() => self.pair(byte, false),
            0x00..=0x7F => {
                if byte == SPACE {
                    self.cut_character("SPACE");
                }
                self.left(byte);
            }
            _ if *self.repertoire == Repertoire::Absent => {
                let offset = self.offset;
                self.issue(Issue::OutsideRepertoire, || format!(
                    ": byte {byte:02X}H at byte {offset} is outside the Default Character Repertoire, but Specific Character Set (0008,0005) is absent (PS3.5 6.1.2.2)"
                ));
                self.trailing_space = false;
                self.character();
            }
            0x80..=0x9F => {
                let offset = self.offset;
                self.issue(Issue::C1, || format!(
                    ": byte {byte:02X}H at byte {offset} is a C1 control character; the C1 set shall not be used (PS3.5 6.1.1)"
                ));
                self.trailing_space = false;
                self.character();
            }
            _ => match self.g1 {
                Some(set) if set.double_byte() => self.pair(byte, true),
                Some(_) => {
                    self.trailing_space = false;
                    self.character();
                }
                None => {
                    self.no_g1(byte);
                    self.trailing_space = false;
                    self.character();
                }
            },
        }
    }

    fn no_g1(&mut self, byte: u8) {
        let offset = self.offset;
        if let Repertoire::Single { term, .. } = self.repertoire {
            self.issue(Issue::OutsideRepertoire, || format!(
                ": byte {byte:02X}H at byte {offset} is outside the repertoire of Specific Character Set \"{term}\" (PS3.5 6.1.2.3)"
            ));
        } else {
            self.issue(Issue::NoG1, || format!(
                ": GR byte {byte:02X}H at byte {offset} with no character set designated to G1; Value 1 of Specific Character Set designates none, so an escape sequence shall designate one before its first use in each Value, line, page and name component (PS3.5 6.1.2.3 and 6.1.2.5.3)"
            ));
        }
    }

    /// One byte of a two-byte ISO 2022 character.
    fn pair(&mut self, byte: u8, gr: bool) {
        self.trailing_space = false;
        match self.pending.take() {
            Some((first, first_gr)) if first_gr == gr => {
                if (first == BACKSLASH || byte == BACKSLASH) && self.kind != Kind::Paragraphs {
                    let offset = self.offset;
                    let set = self.g0.name();
                    self.issue(Issue::Backslash, || format!(
                        ": byte 5CH at byte {offset} belongs to a two-byte {set} character; it would separate Values, and 5CH shall not be present in SH, LO, UC and PN (PS3.5 6.2 and 6.1.2.5.3)"
                    ));
                }
                self.character();
            }
            Some(_) => {
                self.incomplete_character("a byte of the other code table half");
                self.pending = Some((byte, gr));
            }
            None => self.pending = Some((byte, gr)),
        }
    }

    /// A two-byte character cut by `by`.
    fn cut_character(&mut self, by: &str) {
        if self.pending.is_some() {
            self.incomplete_character(by);
        }
    }

    fn incomplete_character(&mut self, by: &str) {
        let offset = self.offset;
        let set = match self.pending {
            Some((_, true)) => self.g1.map_or("G1", Set::name),
            _ => self.g0.name(),
        };
        self.pending = None;
        self.issue(Issue::IncompleteCharacter, || format!(
            ": two-byte {set} character cut by {by} at byte {offset} (PS3.3 C.12.1.1.2 Table C.12-4)"
        ));
    }

    /// The Value 1 designations shall be in force before line and page
    /// ends, control characters, delimiters and the end of the Value; a set
    /// that has only G0 need not be returned to in G1.
    fn before_reset(&mut self, at: &str) {
        self.cut_character(at);
        if !matches!(self.repertoire, Repertoire::Extended { .. }) {
            return;
        }
        let (g0, g1) = self.initial;
        let still = if self.g0 != g0 {
            Some((self.g0, "G0"))
        } else if g1.is_some() && self.g1 != g1 {
            self.g1.map(|set| (set, "G1"))
        } else {
            None
        };
        if let Some((set, element)) = still {
            let offset = self.offset;
            let pn = if self.kind == Kind::PersonName {
                " and 6.2.1.2"
            } else {
                ""
            };
            self.issue(Issue::NotReturned, || format!(
                ": {} is still designated to {element} before {at} at byte {offset}; the Value 1 character set shall be active there (PS3.5 6.1.2.5.3{pn})",
                set.name()
            ));
        }
    }

    fn reset(&mut self) {
        (self.g0, self.g1) = self.initial;
        self.pending = None;
    }

    fn incomplete_escape(&mut self, sequence: &[u8], start: u64) {
        let text = escape_text(sequence);
        self.issue(Issue::IncompleteEscape, || format!(
            ": incomplete escape sequence {text} at byte {start}; ESC shall only begin ISO 2022 character set control sequences (PS3.5 6.1.3)"
        ));
    }

    fn escape_sequence(&mut self, sequence: &[u8], start: u64) {
        self.extension = true;
        let text = escape_text(sequence);
        if self.kind == Kind::PersonName && self.group_index() == 0 {
            self.issue(Issue::AlphabeticEscape, || format!(
                ": {text} at byte {start} in the alphabetic component group; ISO 2022 escapes shall not be used there (PS3.5 6.2.1.2)"
            ));
        }
        let declared = match self.repertoire {
            Repertoire::Extended { declared, .. } => declared,
            other => {
                let why = match other {
                    Repertoire::Absent => {
                        "Specific Character Set (0008,0005) is absent".to_string()
                    }
                    Repertoire::Single { term, .. } => {
                        format!("Specific Character Set \"{term}\" has a single Value")
                    }
                    Repertoire::Unextendable(code) => {
                        format!(
                            "Specific Character Set \"{}\" prohibits Code Extension",
                            code.term()
                        )
                    }
                    _ => String::new(),
                };
                self.issue(Issue::ExtensionNotPermitted, || format!(
                    ": {text} at byte {start}, but {why}; Code Extension shall not be used (PS3.5 6.1.2.5.4 and 6.1.3)"
                ));
                return;
            }
        };
        if let Some(set) = Set::designated_by(sequence) {
            if !declared.contains(&set) {
                self.issue(Issue::Undeclared, || format!(
                    ": {text} at byte {start} designates {}, which no Value of Specific Character Set names (PS3.5 6.1.2.3 and 6.1.2.4)",
                    set.name()
                ));
            }
            self.cut_character("an escape sequence");
            match set.element() {
                Element::G0 => self.g0 = set,
                Element::G1 => self.g1 = Some(set),
            }
            return;
        }
        let (issue, why) = match sequence {
            [b'N' | b'O'] => (
                Issue::Shift,
                "is a single shift; code elements G2 and G3 are not used (PS3.5 6.1.2.5.2)",
            ),
            [b'n' | b'o' | b'|' | b'}' | b'~'] => (
                Issue::Shift,
                "is a locking shift; G0 and G1 are always invoked in GL and GR and locking shifts shall not be used (PS3.5 6.1.1 and 6.1.2.5.2)",
            ),
            [0x40..=0x5F] => (
                Issue::C1,
                "is a C1 control function; the C1 code element shall not be used (PS3.5 6.1.1 and 6.1.2.5.1)",
            ),
            [b'!', ..] | [b'"', ..] => (
                Issue::ControlSet,
                "designates a control set; C0 shall be the primary control set and C1 shall not be used (PS3.5 6.1.2.5.1)",
            ),
            _ => (
                Issue::UnknownEscape,
                "is not an escape sequence of PS3.3 Tables C.12-3 and C.12-4 (PS3.5 6.1.2.5.2 and 6.1.3)",
            ),
        };
        self.issue(issue, || format!(": {text} at byte {start} {why}"));
    }

    fn multibyte_byte(&mut self, byte: u8, code: Unextendable) {
        self.trailing_space = byte == SPACE;
        if self.kind == Kind::PersonName
            && self.group_index() == 0
            && code != Unextendable::Utf8
            && self.alphabetic.len() < MAX_ALPHABETIC_BYTES
        {
            self.alphabetic.push(byte);
        }
        if self.multibyte.active() {
            match self.multibyte.next(byte, code) {
                Step::More => return,
                Step::Done { code_point, trail } => {
                    self.multibyte = Multibyte::default();
                    if trail == BACKSLASH && self.kind != Kind::Paragraphs {
                        let offset = self.offset;
                        self.issue(Issue::Backslash, || format!(
                            ": byte 5CH at byte {offset} ends a {} character; it would separate Values, and 5CH shall not be present in SH, LO, UC and PN (PS3.5 6.2)",
                            code.term()
                        ));
                    }
                    if let Some(code_point) = code_point {
                        self.alphabetic_code_point(code_point);
                    }
                    self.character();
                    return;
                }
                Step::Overlong => {
                    let offset = self.multibyte.offset;
                    self.multibyte = Multibyte::default();
                    self.issue(Issue::Overlong, || format!(
                        ": overlong UTF-8 sequence at byte {offset}; the minimal length UTF-8 encoding shall be used (PS3.3 C.12.1.1.2)"
                    ));
                    self.character();
                    return;
                }
                Step::Invalid => {
                    self.invalid(self.multibyte.offset);
                    self.multibyte = Multibyte::default();
                }
            }
        }
        if byte < 0x80 {
            self.left(byte);
            return;
        }
        match Multibyte::start(byte, code, self.offset) {
            Some(started) if started.length == 1 => self.character(),
            Some(started) => self.multibyte = started,
            None => {
                self.invalid(self.offset);
                self.character();
            }
        }
    }

    fn invalid(&mut self, offset: u64) {
        let Repertoire::Unextendable(code) = self.repertoire else {
            return;
        };
        let term = code.term();
        self.issue(Issue::Invalid, || {
            format!(": bytes at byte {offset} are not a {term} character (PS3.3 C.12.1.1.2)")
        });
    }

    /// PS3.5 6.2.1.2: alphabetic characters under a multi-byte character set
    /// without Code Extension.
    fn alphabetic_code_point(&mut self, code_point: u32) {
        if self.kind != Kind::PersonName || self.group_index() != 0 {
            return;
        }
        let permitted = matches!(
            code_point,
            0x20..=0x1FFF | 0x3001 | 0x3002 | 0x300C | 0x300D | 0x3099..=0x309C | 0x30A0..=0x30FF
        );
        // Control characters are the value-format check's finding.
        if !permitted && code_point >= 0x20 {
            let offset = self.offset;
            self.issue(Issue::AlphabeticCodePoint, || format!(
                ": U+{code_point:04X} at byte {offset} in the alphabetic component group, outside U+0020-U+1FFF and the permitted Japanese punctuation and kana (PS3.5 6.2.1.2)"
            ));
        }
    }

    /// Decodes the GB18030 or GBK alphabetic component group, which needs the
    /// character set's code table.
    fn end_alphabetic(&mut self) {
        let Repertoire::Unextendable(code) = self.repertoire else {
            return;
        };
        let bytes = std::mem::take(&mut self.alphabetic);
        if *code == Unextendable::Utf8 || bytes.is_empty() {
            return;
        }
        let decoded =
            SpecificCharacterSet::from_code(code.term()).and_then(|set| set.decode(&bytes).ok());
        for character in decoded.unwrap_or_default().chars() {
            self.alphabetic_code_point(u32::from(character));
        }
    }

    fn end_group(&mut self) {
        if self.group_index() == 0 {
            self.end_alphabetic();
        }
    }

    /// Checks the Value's length and Person Name structure when it uses Code
    /// Extension, which only these bytes show (the value-format check
    /// counts the other Values).
    fn end_value(&mut self, last: bool) {
        if self.kind == Kind::PersonName {
            self.end_group();
        }
        let padding = usize::from(last && self.trailing_space);
        if self.extension {
            let value = self.value;
            match (self.kind, self.limit) {
                (Kind::PersonName, Some(limit)) => self.person_name(value, limit, padding),
                (_, Some(limit)) if self.characters.saturating_sub(padding) > limit => {
                    let count = self.characters - padding;
                    self.issue(Issue::TooLong, || format!(
                        " Value {value}: {count} characters, not counting escape sequences, exceed the {limit}-character maximum (PS3.5 6.2)"
                    ));
                }
                _ => {}
            }
        }
        self.characters = 0;
        self.extension = false;
        self.trailing_space = false;
        self.groups = vec![Group::default()];
        self.alphabetic.clear();
    }

    fn person_name(&mut self, value: usize, limit: usize, padding: usize) {
        let groups = std::mem::take(&mut self.groups);
        if groups.len() > 3 {
            self.issue(Issue::TooManyGroups, || {
                format!(" Value {value}: more than three component groups (PS3.5 6.2)")
            });
        }
        let last = groups.len() - 1;
        for (index, group) in groups.iter().enumerate() {
            let components = group.components + 1;
            if components > 5 {
                self.issue(Issue::TooManyComponents, || format!(
                    " Value {value} component group {}: {components} components exceed five (PS3.5 6.2)",
                    index + 1
                ));
            }
            let count = if index == last {
                group.characters.saturating_sub(padding)
            } else {
                group.characters
            };
            if count > limit {
                self.issue(Issue::TooLong, || format!(
                    " Value {value} component group {}: {count} characters, not counting escape sequences, exceed the {limit}-character maximum (PS3.5 6.2 and 6.2.1.2)",
                    index + 1
                ));
            }
        }
    }
}
