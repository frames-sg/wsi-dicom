//! The MAC byte stream of PS3.3 C.12.1.1.3.1.2, encoded in Explicit VR Little
//! Endian and written to the MAC Algorithm as it is produced. Primitive Data
//! Elements are encoded as the parser's writer encodes them; Sequences and
//! encapsulated Pixel Data are written without Value Lengths, Item Tags
//! separating their Items and Fragments, and a Sequence Delimitation Item Tag
//! after them.

use std::io::{self, BufWriter, Write};

use dicom_core::value::Value;
use dicom_core::{DataElementHeader, Tag, VR};
use dicom_dictionary_std::tags;
use dicom_encoding::encode::explicit_le::ExplicitVRLittleEndianEncoder;
use dicom_encoding::encode::EncoderFor;
use dicom_encoding::text::SpecificCharacterSet;
use dicom_object::mem::InMemElement;
use dicom_object::InMemDicomObject;
use dicom_parser::stateful::encode::StatefulEncoder;
use sha2::digest::DynDigest;

use super::{character_set, excluded_from_signature, EDITION, MAX_NESTING};
use crate::validation::iod::element_label;

/// The Item and Sequence Delimitation Item Tags, without a Value Length.
const ITEM: [u8; 4] = [0xFE, 0xFF, 0x00, 0xE0];
const SEQUENCE_DELIMITATION: [u8; 4] = [0xFE, 0xFF, 0xDD, 0xE0];
/// The Digital Signatures Sequence Item Attributes outside its own MAC.
const UNSIGNED: [Tag; 4] = [
    tags::CERTIFICATE_OF_SIGNER,
    tags::SIGNATURE,
    tags::CERTIFIED_TIMESTAMP_TYPE,
    tags::CERTIFIED_TIMESTAMP,
];
/// Headers and short Values are gathered; Fragments, and Values at least
/// this long, reach the hasher from where the parser holds them.
const BUFFER: usize = 64 * 1024;

/// Feeds written bytes to a MAC Algorithm.
pub(super) struct Hashing<'h>(pub(super) &'h mut dyn DynDigest);

impl Write for Hashing<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// MAC Calculation Transfer Syntax UID (0400,0010), with the Transfer Syntax
/// the parsed Pixel Data is encoded in.
pub(super) struct MacSyntax<'f> {
    pub(super) uid: String,
    /// Whether it encapsulates Pixel Data (PS3.5 A.4).
    pub(super) encapsulated: bool,
    pub(super) file: &'f str,
}

impl MacSyntax<'_> {
    /// Pixel Data is hashed as the file encodes it, which is how the MAC
    /// Calculation Transfer Syntax encodes it when the two are the same, or
    /// when both leave it native.
    fn pixel_data(&self, element: &InMemElement) -> Result<(), String> {
        let encapsulated = matches!(element.value(), Value::PixelSequence(_));
        if self.uid == self.file || !(encapsulated || self.encapsulated) {
            return Ok(());
        }
        let form = if encapsulated {
            "encapsulated"
        } else {
            "native"
        };
        Err(format!(
            "{} is {form} as the file's Transfer Syntax {} encodes it, and MAC Calculation Transfer Syntax UID (0400,0010) {} encodes it otherwise; its Frames are not re-encoded to recompute the MAC (PS3.3 {EDITION} C.12.1.1.3.1.2)",
            element_label("", element.header().tag),
            self.file,
            self.uid
        ))
    }
}

/// One MAC byte stream, written to `W`.
pub(super) struct MacStream<'s, W: Write> {
    out: BufWriter<W>,
    syntax: &'s MacSyntax<'s>,
}

impl<'s, W: Write> MacStream<'s, W> {
    pub(super) fn new(out: W, syntax: &'s MacSyntax<'s>) -> Self {
        Self {
            out: BufWriter::with_capacity(BUFFER, out),
            syntax,
        }
    }

    /// The Data Elements of `data_set` that Data Elements Signed lists, in
    /// Data Set order, except those no MAC includes (C.12.1.1.3.1.1).
    pub(super) fn signed(
        &mut self,
        data_set: &InMemDicomObject,
        signed: &[Tag],
        charset: &SpecificCharacterSet,
    ) -> Result<(), String> {
        for element in data_set
            .iter()
            .filter(|element| signed.contains(&element.header().tag) && signable(element))
        {
            self.element(element, charset, 0)?;
        }
        Ok(())
    }

    /// The Attributes of the Digital Signatures Sequence Item but Certificate
    /// of Signer, Signature, Certified Timestamp Type and Certified Timestamp.
    pub(super) fn signature_item(
        &mut self,
        item: &InMemDicomObject,
        inherited: &SpecificCharacterSet,
    ) -> Result<(), String> {
        let charset = character_set(item).unwrap_or_else(|| inherited.clone());
        for element in item
            .iter()
            .filter(|element| !UNSIGNED.contains(&element.header().tag) && signable(element))
        {
            self.element(element, &charset, 0)?;
        }
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<(), String> {
        self.out.flush().map_err(stream_error)
    }

    fn element(
        &mut self,
        element: &InMemElement,
        charset: &SpecificCharacterSet,
        depth: usize,
    ) -> Result<(), String> {
        let header = element.header();
        if header.tag == tags::PIXEL_DATA {
            self.syntax.pixel_data(element)?;
        }
        match element.value() {
            Value::Sequence(sequence) => self.sequence(header, sequence.items(), charset, depth),
            Value::Primitive(_) if header.vr == VR::SQ => {
                self.sequence(header, &[], charset, depth)
            }
            Value::Primitive(value) => StatefulEncoder::new(
                &mut self.out,
                EncoderFor::new(ExplicitVRLittleEndianEncoder::default()),
                charset.clone(),
            )
            .encode_primitive_element(header, value)
            .map_err(|error| {
                format!(
                    "{} cannot be encoded for its MAC: {error} (PS3.3 {EDITION} C.12.1.1.3.1.2)",
                    element_label("", header.tag)
                )
            }),
            Value::PixelSequence(pixels) => {
                self.head(header)?;
                self.raw(&ITEM)?;
                for offset in pixels.offset_table() {
                    self.raw(&offset.to_le_bytes())?;
                }
                for fragment in pixels.fragments() {
                    self.raw(&ITEM)?;
                    self.unbuffered(fragment)?;
                }
                self.raw(&SEQUENCE_DELIMITATION)
            }
        }
    }

    /// A Sequence and, recursively, every signable Data Element of its Items.
    fn sequence(
        &mut self,
        header: &DataElementHeader,
        items: &[InMemDicomObject],
        charset: &SpecificCharacterSet,
        depth: usize,
    ) -> Result<(), String> {
        if depth >= MAX_NESTING {
            return Err(format!(
                "{} nests Sequences deeper than {MAX_NESTING} levels, so its MAC is not recomputed",
                element_label("", header.tag)
            ));
        }
        self.head(header)?;
        for item in items {
            self.raw(&ITEM)?;
            let charset = character_set(item).unwrap_or_else(|| charset.clone());
            for element in item.iter().filter(|element| signable(element)) {
                self.element(element, &charset, depth + 1)?;
            }
        }
        self.raw(&SEQUENCE_DELIMITATION)
    }

    /// The Tag, VR and reserved field, without the Value Length.
    fn head(&mut self, header: &DataElementHeader) -> Result<(), String> {
        let [group_low, group_high] = header.tag.group().to_le_bytes();
        let [element_low, element_high] = header.tag.element().to_le_bytes();
        let [vr_first, vr_second] = header.vr.to_bytes();
        self.raw(&[
            group_low,
            group_high,
            element_low,
            element_high,
            vr_first,
            vr_second,
            0,
            0,
        ])
    }

    fn raw(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.out.write_all(bytes).map_err(stream_error)
    }

    /// Bytes written from where the parser holds them, without the buffer.
    fn unbuffered(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.out.flush().map_err(stream_error)?;
        self.out.get_mut().write_all(bytes).map_err(stream_error)
    }
}

fn signable(element: &InMemElement) -> bool {
    excluded_from_signature(element.header().tag, Some(element)).is_none()
}

fn stream_error(error: io::Error) -> String {
    format!("the MAC byte stream cannot be written: {error}")
}
