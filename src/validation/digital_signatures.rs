//! Digital Signatures (PS3.3 C.12.1.1.3): each Item of a Digital Signatures
//! Sequence (FFFA,FFFA), in the Data Set or in any Sequence Item, is verified
//! against the Data Set that holds it.
//!
//! The MAC byte stream of C.12.1.1.3.1.2 is rebuilt from the parsed Data Set
//! and hashed as it is written, so Pixel Data is hashed where the parser holds
//! it and the stream is never assembled. The Signature is verified with the
//! RSA public key of Certificate of Signer (PS3.15 Annex C), and a Certified
//! Timestamp against the Signature it certifies. Whether a certificate is
//! trusted depends on the receiver's trust anchors, not on the object, and is
//! not checked.

mod algorithm;
mod keys;
mod stream;
#[cfg(test)]
pub(super) mod tests;
mod timestamp;

use std::borrow::Cow;
use std::path::Path;

use dicom_core::{Tag, VR};
use dicom_dictionary_std::tags;
use dicom_encoding::text::SpecificCharacterSet;
use dicom_object::mem::InMemElement;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::disclosure::shown_or;
use super::encoding::explicit_little_endian;
use super::iod::{
    element_label, integer_value, items, string_value, tag_values, Family, EDITION, MAX_NESTING,
};
use super::wsi_conformance::rule_path;
use super::{intrinsic_check, ValidationCheck, ValidationStatus};
use algorithm::MacAlgorithm;
use stream::{Hashing, MacStream, MacSyntax};
use timestamp::Timestamp;

const DIGITAL_SIGNATURES_RULE: &str = "intrinsic-wsi-dicom-digital-signatures";
/// Length to End (retired) and the Item Delimitation Item, which a signature
/// excludes (PS3.3 C.12.1.1.3.1.1).
const LENGTH_TO_END: Tag = Tag(0x0008, 0x0001);
const ITEM_DELIMITATION: Tag = Tag(0xFFFE, 0xE00D);

fn citation() -> String {
    format!("DICOM PS3.3 {EDITION} C.12.1.1.3 and PS3.15 {EDITION} Annex C")
}

/// The Digital Signatures check of one object. An object without Digital
/// Signatures passes with nothing to verify.
pub(super) fn run_digital_signatures_check(
    path: &Path,
    object: &DefaultDicomObject,
) -> ValidationCheck {
    let citation = citation();
    let mut verifier = Verifier {
        file_syntax: object
            .meta()
            .transfer_syntax()
            .trim_end_matches(['\0', ' ']),
        findings: Family::default(),
        verified: 0,
        timestamps: 0,
        notes: Vec::new(),
    };
    verifier.data_set(object, &SpecificCharacterSet::default(), &mut Vec::new());
    if !verifier.findings.is_empty() {
        return verifier
            .findings
            .check(path, DIGITAL_SIGNATURES_RULE, &citation);
    }
    intrinsic_check(
        DIGITAL_SIGNATURES_RULE,
        rule_path(path),
        ValidationStatus::Passed,
        format!("{citation}: {}", verifier.summary()),
    )
}

/// PS3.3 C.12.1.1.3.1.1: whether, and why, a Data Element is left out of
/// every MAC, whether Data Elements Signed lists it or a signed Sequence holds
/// it.
pub(super) fn excluded_from_signature(
    tag: Tag,
    element: Option<&InMemElement>,
) -> Option<&'static str> {
    if tag == LENGTH_TO_END || tag.element() == 0x0000 {
        return Some("Length to End and group lengths");
    }
    if tag.group() < 0x0008 {
        return Some("a Tag with a group below 0008");
    }
    if tag.group() == 0xFFFA
        || [
            tags::MAC_PARAMETERS_SEQUENCE,
            tags::DATA_SET_TRAILING_PADDING,
            ITEM_DELIMITATION,
        ]
        .contains(&tag)
    {
        return Some("the signature, MAC Parameters, padding and Item delimitation elements");
    }
    element
        .is_some_and(contains_unknown)
        .then_some("a Data Element whose VR is UN, or a Sequence holding one,")
}

fn contains_unknown(element: &InMemElement) -> bool {
    element.vr() == VR::UN
        || element
            .items()
            .is_some_and(|items| items.iter().any(|item| item.iter().any(contains_unknown)))
}

/// The Sequence Items enclosing a Data Set, outermost first.
type Trail = Vec<(Tag, usize)>;

/// The signatures verified so far, and why the others fail.
struct Verifier<'o> {
    /// The Transfer Syntax the parsed Pixel Data is encoded in.
    file_syntax: &'o str,
    findings: Family,
    verified: usize,
    /// Verified signatures whose Certified Timestamp is verified too.
    timestamps: usize,
    /// What a verified signature leaves unverified.
    notes: Vec<String>,
}

impl Verifier<'_> {
    /// Verifies the signatures of `data_set` and, recursively, of every Item
    /// of its Sequences.
    fn data_set(
        &mut self,
        data_set: &InMemDicomObject,
        inherited: &SpecificCharacterSet,
        trail: &mut Trail,
    ) {
        let charset = character_set(data_set).unwrap_or_else(|| inherited.clone());
        for (index, signature) in items(data_set, tags::DIGITAL_SIGNATURES_SEQUENCE)
            .iter()
            .enumerate()
        {
            let label = || {
                format!(
                    "{}Digital Signatures Sequence (FFFA,FFFA)[{}]",
                    path(trail),
                    index + 1
                )
            };
            match self.verify(data_set, signature, &charset) {
                Ok(Timestamp::Absent) => self.verified += 1,
                Ok(Timestamp::Verified) => {
                    self.verified += 1;
                    self.timestamps += 1;
                }
                Ok(Timestamp::Unverified(reason)) => {
                    self.verified += 1;
                    self.notes.push(format!(
                        "{}: its Certified Timestamp certifies the Signature, but {reason}, so the token's own signature is not verified",
                        label()
                    ));
                }
                Err(reason) => self.findings.push(|| format!("{}: {reason}", label())),
            }
        }
        if trail.len() >= MAX_NESTING {
            return;
        }
        for element in data_set.iter() {
            let Some(items) = element.items() else {
                continue;
            };
            for (index, item) in items.iter().enumerate() {
                trail.push((element.header().tag, index));
                self.data_set(item, &charset, trail);
                trail.pop();
            }
        }
    }

    /// Recomputes the MAC of one Digital Signatures Sequence Item and verifies
    /// its Signature, then its Certified Timestamp.
    fn verify(
        &self,
        data_set: &InMemDicomObject,
        signature: &InMemDicomObject,
        charset: &SpecificCharacterSet,
    ) -> Result<Timestamp, String> {
        let parameters = mac_parameters(data_set, signature)?;
        let syntax = mac_syntax(parameters, self.file_syntax)?;
        let algorithm = mac_algorithm(parameters)?;
        let key = keys::signer_key(signature)?;
        let value = bytes(signature, tags::SIGNATURE).ok_or_else(|| {
            format!("Signature (0400,0120) is absent, so there is nothing to verify (PS3.3 {EDITION} Table C.12-6)")
        })?;
        let value = keys::signature_value(&value, &key);
        let signed = tag_values(parameters, tags::DATA_ELEMENTS_SIGNED);

        let mut hasher = algorithm.hasher();
        let mut stream = MacStream::new(Hashing(&mut *hasher), &syntax);
        stream.signed(data_set, &signed, charset)?;
        stream.signature_item(signature, charset)?;
        stream.finish()?;
        let mac = hasher.finalize();
        key.verify(algorithm.padding(), &mac, value).map_err(|_| {
            format!(
                "Signature (0400,0120) does not verify with the RSA public key of Certificate of Signer (0400,0115) over the {} MAC of the {} Data Elements Signed and this Item's own Attributes, encoded in {}; a signed Data Element or the Signature differs from what was signed (PS3.3 {EDITION} C.12.1.1.3.1.2 and PS3.15 {EDITION} C.1)",
                algorithm.term(),
                signed.len(),
                syntax.uid
            )
        })?;
        timestamp::check(signature, value)
    }

    fn summary(&self) -> String {
        let mut summary = match self.verified {
            0 => "no Digital Signatures Sequence (FFFA,FFFA) Item; nothing to verify".to_string(),
            1 => "1 Digital Signature verified".to_string(),
            count => format!("{count} Digital Signatures verified"),
        };
        if self.timestamps > 0 {
            summary.push_str(&format!(
                ", {} with a verified Certified Timestamp",
                self.timestamps
            ));
        }
        for note in &self.notes {
            summary.push_str(&format!("; {note}"));
        }
        summary
    }
}

/// `Sequence[n] > ` for each enclosing Item, formatted only for a message.
fn path(trail: &Trail) -> String {
    trail
        .iter()
        .map(|(tag, index)| format!("{}[{}] > ", element_label("", *tag), index + 1))
        .collect()
}

/// The MAC Parameters Sequence Item that a signature's MAC ID Number
/// identifies, among those at the signature's level (PS3.3 Table C.12-6).
fn mac_parameters<'d>(
    data_set: &'d InMemDicomObject,
    signature: &InMemDicomObject,
) -> Result<&'d InMemDicomObject, String> {
    let Some(id) = integer_value(signature, tags::MACID_NUMBER) else {
        return Err(format!(
            "MAC ID Number (0400,0005) is absent, so no MAC Parameters Sequence (4FFE,0001) Item says how its MAC was calculated (PS3.3 {EDITION} Table C.12-6)"
        ));
    };
    let mut matching = items(data_set, tags::MAC_PARAMETERS_SEQUENCE)
        .iter()
        .filter(|item| integer_value(item, tags::MACID_NUMBER) == Some(id));
    match (matching.next(), matching.next()) {
        (Some(parameters), None) => Ok(parameters),
        (None, _) => Err(format!(
            "MAC ID Number (0400,0005) {id} identifies no MAC Parameters Sequence (4FFE,0001) Item at its level, so its MAC cannot be recomputed (PS3.3 {EDITION} Table C.12-6)"
        )),
        (Some(_), Some(_)) => Err(format!(
            "MAC ID Number (0400,0005) {id} identifies more than one MAC Parameters Sequence (4FFE,0001) Item; it shall be unique within the SOP Instance (PS3.3 {EDITION} Table C.12-6)"
        )),
    }
}

/// MAC Calculation Transfer Syntax UID (0400,0010): an Explicit VR Little
/// Endian Transfer Syntax (PS3.3 C.12.1.1.3.1.2 and Table C.12-6).
fn mac_syntax<'f>(parameters: &InMemDicomObject, file: &'f str) -> Result<MacSyntax<'f>, String> {
    let Some(uid) = string_value(parameters, tags::MAC_CALCULATION_TRANSFER_SYNTAX_UID, 1) else {
        return Err(format!(
            "its MAC Parameters Sequence (4FFE,0001) Item has no MAC Calculation Transfer Syntax UID (0400,0010), so its MAC cannot be encoded (PS3.3 {EDITION} C.12.1.1.3.1.2)"
        ));
    };
    match explicit_little_endian(&uid) {
        Ok(encapsulated) => Ok(MacSyntax {
            uid,
            encapsulated,
            file,
        }),
        Err(kind) => Err(format!(
            "MAC Calculation Transfer Syntax UID (0400,0010) {uid} is {kind}; a MAC is encoded in an Explicit VR Little Endian Transfer Syntax, so this one is not recomputed (PS3.3 {EDITION} C.12.1.1.3.1.2 and Table C.12-6)"
        )),
    }
}

/// MAC Algorithm (0400,0015), a Defined Term of PS3.3 Table
/// C.12.1.1.3.1.2-1.
fn mac_algorithm(parameters: &InMemDicomObject) -> Result<MacAlgorithm, String> {
    let term = string_value(parameters, tags::MAC_ALGORITHM, 1);
    term.as_deref().and_then(MacAlgorithm::of_term).ok_or_else(|| {
        format!(
            "MAC Algorithm (0400,0015) is {}, not a Defined Term of PS3.3 {EDITION} Table C.12.1.1.3.1.2-1, so its MAC cannot be computed",
            shown_or(tags::MAC_ALGORITHM, term.as_deref(), "absent")
        )
    })
}

/// The character set a Data Set's own Specific Character Set (0008,0005)
/// declares by its first Value, which is how the parser decodes its text.
fn character_set(data_set: &InMemDicomObject) -> Option<SpecificCharacterSet> {
    string_value(data_set, tags::SPECIFIC_CHARACTER_SET, 1)
        .and_then(|term| SpecificCharacterSet::from_code(&term))
}

/// The bytes of a binary Attribute's Value.
fn bytes(object: &InMemDicomObject, tag: Tag) -> Option<Cow<'_, [u8]>> {
    object.get(tag).and_then(|element| element.to_bytes().ok())
}
