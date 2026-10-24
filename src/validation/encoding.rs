//! PS3.5 Data Set encoding as written in the file's bytes.
//!
//! dicom-object sorts and deduplicates elements, maps unrecognized VR bytes
//! to UN, discards reserved bytes and delimiter lengths, reads the Items of
//! a UN of Undefined Length as Explicit VR and decodes text by Value 1 of
//! Specific Character Set alone, so these rules re-read the encoded Data Set
//! after the File Meta Information. Attribute Values whose encoding the
//! standard constrains beyond their VR are read from the parsed object.

mod attributes;
mod charset;
mod private;
mod reader;
mod text;
mod walk;

#[cfg(test)]
mod tests;

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use dicom_dictionary_std::uids;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::iod::{data_set_start, Family, EDITION};
use super::wsi_conformance::rule_path;
use super::{intrinsic_check, ValidationCheck, ValidationStatus};
use attributes::Attributes;
use reader::Source;
use walk::{walk, Layout, Syntax};

const ENCODING_RULE: &str = "intrinsic-wsi-dicom-encoding";
/// Explicit VR Big Endian; retired, and its `uids` constant deprecated.
const EXPLICIT_VR_BIG_ENDIAN: &str = "1.2.840.10008.1.2.2";

const LITTLE_ENDIAN: Layout = Layout {
    explicit_vr: true,
    big_endian: false,
};

/// How a Transfer Syntax encodes the Data Set.
struct Encoding {
    syntax: Syntax,
    deflated: bool,
}

/// The PS3.5 Annex A encoding of a DICOM Transfer Syntax; `None` for
/// private and non-binary syntaxes, whose encoding is not known here.
fn encoding(transfer_syntax: &str) -> Option<Encoding> {
    let (layout, encapsulated, annex, deflated) = match transfer_syntax {
        uids::IMPLICIT_VR_LITTLE_ENDIAN => (
            Layout {
                explicit_vr: false,
                big_endian: false,
            },
            false,
            "A.1",
            false,
        ),
        uids::EXPLICIT_VR_LITTLE_ENDIAN => (LITTLE_ENDIAN, false, "A.2", false),
        EXPLICIT_VR_BIG_ENDIAN => (
            Layout {
                explicit_vr: true,
                big_endian: true,
            },
            false,
            "A.3",
            false,
        ),
        uids::DEFLATED_EXPLICIT_VR_LITTLE_ENDIAN => (LITTLE_ENDIAN, false, "A.5", true),
        // JPIP Referenced syntaxes carry no Pixel Data in the Data Set.
        uids::JPIP_REFERENCED => (LITTLE_ENDIAN, false, "A.6", false),
        uids::JPIP_REFERENCED_DEFLATE => (LITTLE_ENDIAN, false, "A.7", true),
        uids::JPIPHTJ2K_REFERENCED => (LITTLE_ENDIAN, false, "A.11", false),
        uids::JPIPHTJ2K_REFERENCED_DEFLATE => (LITTLE_ENDIAN, false, "A.12", true),
        uid if uid.starts_with("1.2.840.10008.1.2.4.")
            || matches!(
                uid,
                uids::ENCAPSULATED_UNCOMPRESSED_EXPLICIT_VR_LITTLE_ENDIAN
                    | uids::RLE_LOSSLESS
                    | uids::DEFLATED_IMAGE_FRAME_COMPRESSION
            ) =>
        {
            (LITTLE_ENDIAN, true, "A.4", false)
        }
        _ => return None,
    };
    Some(Encoding {
        syntax: Syntax {
            layout,
            encapsulated,
            annex,
        },
        deflated,
    })
}

/// Whether `transfer_syntax` encodes Data Elements in Explicit VR Little
/// Endian, as a MAC is calculated (PS3.3 Table C.12-6), and if so whether it
/// encapsulates Pixel Data; otherwise what it is instead. A deflated syntax
/// compresses the Data Set, not the Data Elements a MAC is calculated over.
pub(super) fn explicit_little_endian(transfer_syntax: &str) -> Result<bool, &'static str> {
    let Some(encoding) = encoding(transfer_syntax) else {
        return Err("not a Transfer Syntax whose Data Set encoding PS3.5 Annex A defines");
    };
    let layout = encoding.syntax.layout;
    if !layout.explicit_vr {
        Err("an Implicit VR Transfer Syntax")
    } else if layout.big_endian {
        Err("a Big Endian Transfer Syntax")
    } else {
        Ok(encoding.syntax.encapsulated)
    }
}

fn citation() -> String {
    format!(
        "DICOM PS3.5 {EDITION} 6.1, 6.2, 7, 9 and Annex A, PS3.3 {EDITION} C.12.1 and PS3.10 {EDITION} 7"
    )
}

/// The encoding check of the file at `path`, with the Attribute Value rules
/// when the parser could read it into `object`.
pub(super) fn run_encoding_check(
    path: &Path,
    object: Option<&DefaultDicomObject>,
    max_input_bytes: u64,
) -> ValidationCheck {
    let citation = citation();
    let skipped = |reason: String| {
        intrinsic_check(
            ENCODING_RULE,
            rule_path(path),
            ValidationStatus::Skipped,
            format!("{citation}: not checked: {reason}"),
        )
    };
    let (offset, transfer_syntax) = match data_set_start(path) {
        Ok(start) => start,
        Err(reason) => {
            return skipped(format!(
                "the Data Set cannot be located ({reason}); the File Meta Information check reports why"
            ))
        }
    };
    let Some(encoding) = encoding(&transfer_syntax) else {
        return skipped(format!(
            "Transfer Syntax {transfer_syntax} is not one whose Data Set encoding PS3.5 Annex A defines"
        ));
    };
    let mut findings = Family::default();
    let source = if encoding.deflated {
        Source::deflated(path, offset, max_input_bytes, max_input_bytes)
    } else {
        Source::file(path, offset, max_input_bytes)
    };
    match source {
        Ok(mut source) => {
            let complete = walk(&mut source, encoding.syntax, &mut findings);
            if complete {
                if let Some(deflated) = source.deflated_length() {
                    deflate_padding(path, offset, deflated, &mut findings);
                }
            }
        }
        Err(error) => findings.push(|| format!("cannot read the Data Set: {error}")),
    }
    if let Some(object) = object {
        let root: &InMemDicomObject = object;
        Attributes {
            findings: &mut findings,
            little_endian: (!encoding.syntax.layout.big_endian).then_some(encoding.syntax.annex),
            explicit_vr: encoding.syntax.layout.explicit_vr,
        }
        .check(root);
    }
    findings.check(path, ENCODING_RULE, &citation)
}

/// PS3.5 A.5: an odd-length deflated bit stream of `length` bytes from
/// `offset` is followed by a single NULL byte, an even-length one by nothing.
fn deflate_padding(path: &Path, offset: u64, length: u64, findings: &mut Family) {
    let end = offset + length;
    let mut trailing = Vec::new();
    let read = File::open(path).and_then(|mut file| {
        file.seek(SeekFrom::Start(end))?;
        file.take(2).read_to_end(&mut trailing)
    });
    if let Err(error) = read {
        findings.push(|| format!("cannot read after the deflated bit stream: {error}"));
        return;
    }
    let odd = length % 2 == 1;
    let conformant = match trailing.as_slice() {
        [] => !odd,
        [0] => odd,
        _ => false,
    };
    if !conformant {
        findings.push(|| format!(
            "the deflated bit stream of {length} bytes is followed by {:02X?}; an odd-length stream is followed by a single trailing NULL byte, an even-length one by none (PS3.5 A.5)",
            trailing
        ));
    }
}
