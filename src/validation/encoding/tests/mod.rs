//! Hand-built byte streams for each encoding rule; exported files are
//! checked in src/validation/tests/encoding.rs.

mod attributes;
mod character_sets;
mod private_blocks;
mod structure;

use std::io::Write;

use super::run_encoding_check;
use crate::validation::{ValidationCheck, ValidationStatus};

const EXPLICIT_LE: &str = "1.2.840.10008.1.2.1";
const IMPLICIT_LE: &str = "1.2.840.10008.1.2";
const EXPLICIT_BE: &str = "1.2.840.10008.1.2.2";
const DEFLATED: &str = "1.2.840.10008.1.2.1.99";
const JPEG_BASELINE: &str = "1.2.840.10008.1.2.4.50";

/// PS3.5 Table 7.1-1 VRs, with a 32-bit Value Length.
fn long(vr: &[u8; 2]) -> bool {
    matches!(
        vr,
        b"OB"
            | b"OD"
            | b"OF"
            | b"OL"
            | b"OV"
            | b"OW"
            | b"SQ"
            | b"SV"
            | b"UC"
            | b"UN"
            | b"UR"
            | b"UT"
            | b"UV"
    ) || dicom_core::VR::from_binary(*vr).is_none()
}

/// Writes Data Set bytes as one Transfer Syntax encodes them.
#[derive(Clone, Default)]
struct Stream {
    implicit: bool,
    big_endian: bool,
    bytes: Vec<u8>,
}

impl Stream {
    fn explicit() -> Self {
        Self::default()
    }

    fn implicit() -> Self {
        Self {
            implicit: true,
            ..Self::default()
        }
    }

    fn big_endian() -> Self {
        Self {
            big_endian: true,
            ..Self::default()
        }
    }

    fn u16(&mut self, value: u16) -> &mut Self {
        let bytes = if self.big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        };
        self.raw(&bytes)
    }

    fn u32(&mut self, value: u32) -> &mut Self {
        let bytes = if self.big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        };
        self.raw(&bytes)
    }

    fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.bytes.extend_from_slice(bytes);
        self
    }

    fn tag(&mut self, (group, element): (u16, u16)) -> &mut Self {
        self.u16(group).u16(element)
    }

    /// An element header with `length`, which may be Undefined Length.
    fn header(&mut self, tag: (u16, u16), vr: &[u8; 2], length: u32) -> &mut Self {
        self.tag(tag);
        if self.implicit {
            return self.u32(length);
        }
        self.raw(vr);
        if long(vr) {
            self.raw(&[0, 0]).u32(length)
        } else {
            self.u16(u16::try_from(length).expect("16-bit Value Length"))
        }
    }

    fn element(&mut self, tag: (u16, u16), vr: &[u8; 2], value: &[u8]) -> &mut Self {
        self.header(tag, vr, value.len() as u32).raw(value)
    }

    fn text(&mut self, tag: (u16, u16), vr: &[u8; 2], value: &str) -> &mut Self {
        self.element(tag, vr, value.as_bytes())
    }

    fn us(&mut self, tag: (u16, u16), value: u16) -> &mut Self {
        let bytes = if self.big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        };
        self.element(tag, b"US", &bytes)
    }

    fn item(&mut self, length: u32) -> &mut Self {
        self.tag((0xFFFE, 0xE000)).u32(length)
    }

    fn item_end(&mut self) -> &mut Self {
        self.tag((0xFFFE, 0xE00D)).u32(0)
    }

    fn sequence_end(&mut self) -> &mut Self {
        self.tag((0xFFFE, 0xE0DD)).u32(0)
    }

    /// A Sequence of Undefined Length holding `items` of Undefined Length.
    fn sequence(&mut self, tag: (u16, u16), items: &[Stream]) -> &mut Self {
        self.header(tag, b"SQ", u32::MAX);
        for item in items {
            self.item(u32::MAX).raw(&item.bytes).item_end();
        }
        self.sequence_end()
    }

    /// A Sequence of Explicit Length holding `items` of Explicit Length.
    fn defined_sequence(&mut self, tag: (u16, u16), items: &[Stream]) -> &mut Self {
        let content: usize = items.iter().map(|item| 8 + item.bytes.len()).sum();
        self.header(tag, b"SQ", content as u32);
        for item in items {
            self.item(item.bytes.len() as u32).raw(&item.bytes);
        }
        self
    }

    fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.bytes)
    }
}

/// A Part 10 file: preamble, prefix and File Meta Information declaring
/// `transfer_syntax`, then `data_set` as given.
fn file(transfer_syntax: &str, data_set: &[u8]) -> Vec<u8> {
    let mut uid = transfer_syntax.as_bytes().to_vec();
    if uid.len() % 2 == 1 {
        uid.push(0);
    }
    let mut meta = Stream::explicit();
    meta.element((0x0002, 0x0001), b"OB", &[0, 1])
        .element((0x0002, 0x0010), b"UI", &uid);
    let group = meta.take();
    let mut bytes = vec![0; 128];
    bytes.extend_from_slice(b"DICM");
    let mut head = Stream::explicit();
    head.element((0x0002, 0x0000), b"UL", &(group.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&head.bytes);
    bytes.extend_from_slice(&group);
    bytes.extend_from_slice(data_set);
    bytes
}

fn check(transfer_syntax: &str, data_set: &[u8]) -> ValidationCheck {
    check_file(&file(transfer_syntax, data_set))
}

fn check_file(bytes: &[u8]) -> ValidationCheck {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(bytes).unwrap();
    file.flush().unwrap();
    run_encoding_check(file.path(), None, u64::MAX)
}

#[track_caller]
fn assert_violation(transfer_syntax: &str, data_set: &[u8], expected: &str) {
    let check = check(transfer_syntax, data_set);
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
    assert!(
        check.message.contains(expected),
        "message lacks {expected:?}: {}",
        check.message
    );
}

#[track_caller]
fn assert_conformant(transfer_syntax: &str, data_set: &[u8]) {
    let check = check(transfer_syntax, data_set);
    assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);
}
