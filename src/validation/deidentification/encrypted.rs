//! PS3.15 E.1.1 Encrypted Attributes Sequence (0400,0500): each Item holds
//! an Encrypted Attributes Data Set encoded with a DICOM Transfer Syntax,
//! which Encrypted Content Transfer Syntax UID (0400,0510) names, and
//! encrypted as CMS EnvelopedData in Encrypted Content (0400,0520) (PS3.3
//! C.12.1.1.4.1), with RSA key transport [RFC 2313] and Triple DES [RFC
//! 3370] or AES [RFC 3565] content encryption. The ciphertext itself cannot
//! be decrypted without the recipient's key, so the Transfer Syntax of the
//! Data Set inside is not checked.

use cms::content_info::ContentInfo;
use cms::enveloped_data::{EnvelopedData, RecipientInfo};
use der::asn1::ObjectIdentifier;
use der::{Decode, Encode, Header, SliceReader};
use dicom_core::VR;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::Findings;
use crate::validation::disclosure::shown_or;
use crate::validation::encoding::defines_data_set_encoding;
use crate::validation::iod::{check_value_format, items, string_value};

const ENVELOPED_DATA: ObjectIdentifier = oid("1.2.840.113549.1.7.3");
/// RFC 2313 rsaEncryption, the RSA key transport of RFC 3370 4.2.1.
const RSA_ENCRYPTION: ObjectIdentifier = oid("1.2.840.113549.1.1.1");
/// RFC 3370 5.1 Triple-DES and RFC 3565 4.1 AES content encryption, in
/// every key length.
const CONTENT_ENCRYPTION: [(ObjectIdentifier, &str); 4] = [
    (oid("1.2.840.113549.3.7"), "des-ede3-cbc"),
    (oid("2.16.840.1.101.3.4.1.2"), "id-aes128-CBC"),
    (oid("2.16.840.1.101.3.4.1.22"), "id-aes192-CBC"),
    (oid("2.16.840.1.101.3.4.1.42"), "id-aes256-CBC"),
];

const fn oid(text: &str) -> ObjectIdentifier {
    ObjectIdentifier::new_unwrap(text)
}

/// Holds each Item of the Encrypted Attributes Sequence to PS3.15 E.1.1.
pub(super) fn check_encrypted_attributes(object: &InMemDicomObject, findings: &mut Findings) {
    let sequence = items(object, tags::ENCRYPTED_ATTRIBUTES_SEQUENCE);
    for (index, item) in sequence.iter().enumerate() {
        check_transfer_syntax(item, index, findings);
        if let Err(reason) = check_enveloped_data(item) {
            findings
                .violations
                .push(|| format!("{}EncryptedContent (0400,0520) {reason}", item_label(index)));
        }
    }
}

/// The path of the `index`th (0-based) Item, as messages prefix its
/// Attributes.
fn item_label(index: usize) -> String {
    format!("EncryptedAttributesSequence (0400,0500)[{}] > ", index + 1)
}

/// E.1.1: "all instances of the Encrypted Attributes Data Set shall be
/// encoded with a DICOM Transfer Syntax", which narrows the Standard or
/// Private syntax PS3.3 C.12.1.1.4.1 otherwise admits. That PS3.3 Table
/// C.12-1 admits only Explicit VR Little Endian ones is the encoding check's.
fn check_transfer_syntax(item: &InMemDicomObject, index: usize, findings: &mut Findings) {
    let uid = string_value(item, tags::ENCRYPTED_CONTENT_TRANSFER_SYNTAX_UID, 1);
    if uid.as_deref().is_some_and(is_dicom_transfer_syntax) {
        return;
    }
    findings.violations.push(|| {
        format!(
            "{}EncryptedContentTransferSyntaxUID (0400,0510) is {}, not a Transfer Syntax that PS3.6 Annex A registers and whose Data Set encoding PS3.5 Annex A defines; E.1.1 encodes the Encrypted Attributes Data Set with a DICOM Transfer Syntax",
            item_label(index),
            shown_or(
                tags::ENCRYPTED_CONTENT_TRANSFER_SYNTAX_UID,
                uid.as_deref(),
                "absent"
            )
        )
    });
}

/// Whether `uid` is registered in PS3.6 Annex A and names a Transfer Syntax
/// whose Data Set encoding PS3.5 Annex A defines. The encoding table admits
/// its JPEG arc by prefix, so the registration makes the lookup exact; a
/// syntax that encodes no Data Set, such as SMPTE ST 2110, is not one.
fn is_dicom_transfer_syntax(uid: &str) -> bool {
    defines_data_set_encoding(uid) && check_value_format(VR::UI, uid).is_ok()
}

/// E.1.1: the Data Set is encrypted, as CMS EnvelopedData, with RSA key
/// transport to every recipient and Triple DES or AES content encryption.
fn check_enveloped_data(item: &InMemDicomObject) -> Result<(), String> {
    let bytes = item
        .get(tags::ENCRYPTED_CONTENT)
        .ok_or("is absent; E.1.1 stores the encrypted Data Set in it")?
        .to_bytes()
        .map_err(|error| format!("cannot be read: {error}"))?;
    let content = der_encoding(&bytes)
        .and_then(|der| ContentInfo::from_der(der).map_err(|error| error.to_string()))
        .map_err(|reason| format!("is not a CMS ContentInfo ({reason}); E.1.1 encrypts the Data Set as CMS EnvelopedData"))?;
    if content.content_type != ENVELOPED_DATA {
        return Err(format!(
            "has content type {}, not id-envelopedData ({ENVELOPED_DATA}); E.1.1 encrypts the Data Set as CMS EnvelopedData",
            content.content_type
        ));
    }
    let enveloped = content
        .content
        .decode_as::<EnvelopedData>()
        .map_err(|error| format!("is not CMS EnvelopedData ({error})"))?;
    if enveloped.encrypted_content.encrypted_content.is_none() {
        return Err("holds EnvelopedData without encrypted content; E.1.1 stores the encrypted Data Set in the Item".into());
    }
    let algorithm = enveloped.encrypted_content.content_enc_alg.oid;
    if !CONTENT_ENCRYPTION
        .iter()
        .any(|(known, _)| *known == algorithm)
    {
        let permitted: Vec<&str> = CONTENT_ENCRYPTION.iter().map(|(_, name)| *name).collect();
        return Err(format!(
            "encrypts its content with algorithm {algorithm}; E.1.1 requires Triple DES [RFC 3370] or AES [RFC 3565] content encryption ({})",
            permitted.join(", ")
        ));
    }
    for (index, recipient) in enveloped.recip_infos.0.iter().enumerate() {
        match recipient {
            RecipientInfo::Ktri(transport) if transport.key_enc_alg.oid == RSA_ENCRYPTION => {}
            RecipientInfo::Ktri(transport) => {
                return Err(format!(
                    "RecipientInfo {} transports its key with algorithm {}; E.1.1 requires RSA [RFC 2313] key transport (rsaEncryption, {RSA_ENCRYPTION})",
                    index + 1,
                    transport.key_enc_alg.oid
                ))
            }
            _ => {
                return Err(format!(
                    "RecipientInfo {} is not a KeyTransRecipientInfo; E.1.1 requires RSA [RFC 2313] key transport of the content-encryption key",
                    index + 1
                ))
            }
        }
    }
    Ok(())
}

/// The DER encoding at the start of an OB Value, which may end in one
/// padding byte (PS3.5 7.1.1).
fn der_encoding(value: &[u8]) -> Result<&[u8], String> {
    let header = SliceReader::new(value)
        .and_then(|mut reader| Header::decode(&mut reader))
        .map_err(|error| error.to_string())?;
    let length = header
        .encoded_len()
        .and_then(|head| head + header.length)
        .and_then(usize::try_from)
        .map_err(|error| error.to_string())?;
    match value.get(length..) {
        Some([] | [0]) => Ok(&value[..length]),
        Some(rest) => Err(format!(
            "{} bytes follow its {length}-byte DER encoding",
            rest.len()
        )),
        None => Err(format!(
            "its DER encoding declares {length} bytes but the Value holds {}",
            value.len()
        )),
    }
}
