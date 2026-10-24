//! Certified Timestamps (PS3.3 Table C.12-6 and C.12.1.1.3.1.3). With
//! Certified Timestamp Type CMS_TSP the Value is an RFC 3161 TimeStampToken
//! (PS3.15 C.1) whose messageImprint is the hash of the Signature (0400,0120)
//! Value; the token's own signature is verified when it embeds its TSA's
//! certificate. When the token was obtained is not decidable from it.

use std::borrow::Cow;

use cms::cert::CertificateChoices;
use cms::content_info::ContentInfo;
use cms::signed_data::{SignedAttributes, SignedData, SignerIdentifier, SignerInfo};
use der::asn1::{Any, Int, ObjectIdentifier, OctetString};
use der::{Decode, Encode, Reader, Sequence, SliceReader};
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;
use x509_cert::ext::pkix::SubjectKeyIdentifier;
use x509_cert::spki::AlgorithmIdentifierOwned;
use x509_cert::Certificate;

use super::keys::{der_encoding, rsa_key, RSA_ENCRYPTION};
use super::{bytes, quoted, string_value, MacAlgorithm, EDITION};

pub(super) const SIGNED_DATA: ObjectIdentifier =
    ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.2");
pub(super) const TST_INFO: ObjectIdentifier =
    ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.1.4");
pub(super) const CONTENT_TYPE: ObjectIdentifier =
    ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.3");
pub(super) const MESSAGE_DIGEST: ObjectIdentifier =
    ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.4");
/// PKCS #1 v1.5 signature algorithms and the hash each names (RFC 8017
/// A.2.4, the NIST and TeleTrusT arcs); rsaEncryption takes the SignerInfo's
/// digest algorithm.
const RSA_SIGNATURES: [(ObjectIdentifier, MacAlgorithm); 12] = [
    (oid("1.2.840.113549.1.1.4"), MacAlgorithm::Md5),
    (oid("1.2.840.113549.1.1.5"), MacAlgorithm::Sha1),
    (oid("1.2.840.113549.1.1.11"), MacAlgorithm::Sha256),
    (oid("1.2.840.113549.1.1.12"), MacAlgorithm::Sha384),
    (oid("1.2.840.113549.1.1.13"), MacAlgorithm::Sha512),
    (oid("1.2.840.113549.1.1.14"), MacAlgorithm::Sha224),
    (oid("1.2.840.113549.1.1.15"), MacAlgorithm::Sha512_224),
    (oid("1.2.840.113549.1.1.16"), MacAlgorithm::Sha512_256),
    (oid("2.16.840.1.101.3.4.3.13"), MacAlgorithm::Sha3_224),
    (oid("2.16.840.1.101.3.4.3.14"), MacAlgorithm::Sha3_256),
    (oid("2.16.840.1.101.3.4.3.15"), MacAlgorithm::Sha3_384),
    (oid("2.16.840.1.101.3.4.3.16"), MacAlgorithm::Sha3_512),
];

const fn oid(text: &str) -> ObjectIdentifier {
    ObjectIdentifier::new_unwrap(text)
}

/// What a Digital Signatures Sequence Item's Certified Timestamp establishes.
pub(super) enum Timestamp {
    Absent,
    /// It certifies the Signature, and its TSA signature verifies.
    Verified,
    /// It certifies the Signature; its TSA signature is not verified, why.
    Unverified(&'static str),
}

/// MessageImprint (RFC 3161 2.4.1).
#[derive(Clone, Debug, Eq, PartialEq, Sequence)]
pub(super) struct MessageImprint {
    pub(super) hash_algorithm: AlgorithmIdentifierOwned,
    pub(super) hashed_message: OctetString,
}

/// Checks the Certified Timestamp of a Digital Signatures Sequence Item
/// against the `signature` it certifies.
pub(super) fn check(item: &InMemDicomObject, signature: &[u8]) -> Result<Timestamp, String> {
    let Some(value) = bytes(item, tags::CERTIFIED_TIMESTAMP).filter(|value| !value.is_empty())
    else {
        return Ok(Timestamp::Absent);
    };
    let kind = string_value(item, tags::CERTIFIED_TIMESTAMP_TYPE, 1);
    if kind.as_deref() != Some("CMS_TSP") {
        return Err(format!(
            "Certified Timestamp Type (0400,0305) is {}; CMS_TSP, the RFC 3161 token of PS3.15 {EDITION} C.1, is the type whose Certified Timestamp (0400,0310) can be read (PS3.3 {EDITION} Table C.12-6)",
            quoted(kind.as_deref())
        ));
    }
    let (signed, tst_info) = token(&value).map_err(|reason| {
        format!("Certified Timestamp (0400,0310) is not an RFC 3161 TimeStampToken: {reason} (PS3.15 {EDITION} C.1)")
    })?;
    imprint(&tst_info, signature)?;
    let [signer] = signed.signer_infos.0.as_slice() else {
        return Err(format!(
            "Certified Timestamp (0400,0310) has {} SignerInfos; a TimeStampToken has one, its TSA's (RFC 3161 2.4.2)",
            signed.signer_infos.0.len()
        ));
    };
    let Some(certificate) = tsa_certificate(&signed, &signer.sid) else {
        return Ok(Timestamp::Unverified(
            "the token embeds no certificate of its TSA",
        ));
    };
    tsa_signature(signer, certificate, &tst_info).map_err(|reason| {
        format!("Certified Timestamp (0400,0310) {reason} (RFC 5652 5.4 and 5.6)")
    })?;
    Ok(Timestamp::Verified)
}

/// The SignedData of a TimeStampToken and the DER TSTInfo it encapsulates
/// (RFC 3161 2.4.2).
fn token(value: &[u8]) -> Result<(SignedData, Vec<u8>), String> {
    let content = ContentInfo::from_der(der_encoding(value)?).map_err(|error| error.to_string())?;
    if content.content_type != SIGNED_DATA {
        return Err(format!(
            "its content type is {}, not id-signedData",
            content.content_type
        ));
    }
    let signed: SignedData = content
        .content
        .decode_as()
        .map_err(|error| format!("its SignedData is malformed: {error}"))?;
    let encapsulated = &signed.encap_content_info;
    if encapsulated.econtent_type != TST_INFO {
        return Err(format!(
            "it encapsulates {}, not id-ct-TSTInfo",
            encapsulated.econtent_type
        ));
    }
    let tst_info = encapsulated
        .econtent
        .as_ref()
        .ok_or_else(|| "it encapsulates no TSTInfo".to_string())?
        .decode_as::<OctetString>()
        .map_err(|error| format!("its encapsulated TSTInfo is malformed: {error}"))?
        .into_bytes();
    Ok((signed, tst_info))
}

/// RFC 3161 2.4.2: the messageImprint is the hash of the timestamped data,
/// here the Signature Value.
fn imprint(tst_info: &[u8], signature: &[u8]) -> Result<(), String> {
    let imprint = message_imprint(tst_info).map_err(|error| {
        format!("Certified Timestamp (0400,0310) has a malformed TSTInfo: {error} (RFC 3161 2.4.2)")
    })?;
    let oid = imprint.hash_algorithm.oid;
    let algorithm = MacAlgorithm::of_oid(oid).ok_or_else(|| {
        format!("Certified Timestamp (0400,0310) messageImprint hashes with {oid}, not a hash of PS3.3 {EDITION} Table C.12.1.1.3.1.2-1, so it cannot be compared with the Signature")
    })?;
    if *algorithm.digest(signature) != *imprint.hashed_message.as_bytes() {
        return Err(format!(
            "Certified Timestamp (0400,0310) messageImprint is not the {} hash of Signature (0400,0120), so the token does not certify this Signature (PS3.3 {EDITION} Table C.12-6 and RFC 3161 2.4.2)",
            algorithm.term()
        ));
    }
    Ok(())
}

/// The messageImprint of a TSTInfo; the fields after it are not needed.
fn message_imprint(tst_info: &[u8]) -> der::Result<MessageImprint> {
    let mut reader = SliceReader::new(tst_info)?;
    let imprint = reader.sequence(|fields| {
        Int::decode(fields)?;
        ObjectIdentifier::decode(fields)?;
        let imprint = MessageImprint::decode(fields)?;
        while !fields.is_finished() {
            Any::decode(fields)?;
        }
        Ok(imprint)
    })?;
    reader.finish(imprint)
}

/// The certificate of the TSA that `sid` identifies, when the token embeds it.
fn tsa_certificate<'s>(signed: &'s SignedData, sid: &SignerIdentifier) -> Option<&'s Certificate> {
    signed
        .certificates
        .as_ref()?
        .0
        .iter()
        .find_map(|choice| match choice {
            CertificateChoices::Certificate(certificate) if identifies(sid, certificate) => {
                Some(certificate)
            }
            _ => None,
        })
}

fn identifies(sid: &SignerIdentifier, certificate: &Certificate) -> bool {
    let tbs = &certificate.tbs_certificate;
    match sid {
        SignerIdentifier::IssuerAndSerialNumber(id) => {
            id.issuer == tbs.issuer && id.serial_number == tbs.serial_number
        }
        SignerIdentifier::SubjectKeyIdentifier(key) => tbs
            .get::<SubjectKeyIdentifier>()
            .ok()
            .flatten()
            .is_some_and(|(_, own)| own == *key),
    }
}

/// RFC 5652 5.4 and 5.6: the TSA signs its signed attributes, which carry the
/// hash of the TSTInfo, or the TSTInfo itself when it has none. The reason
/// reads after "Certified Timestamp (0400,0310)".
fn tsa_signature(
    signer: &SignerInfo,
    certificate: &Certificate,
    tst_info: &[u8],
) -> Result<(), String> {
    let key = rsa_key(&certificate.tbs_certificate.subject_public_key_info)
        .map_err(|reason| format!("is signed by a TSA whose certificate {reason}; TSA signatures are verified when they are RSA PKCS #1 v1.5"))?;
    let digest_oid = signer.digest_alg.oid;
    let digest = MacAlgorithm::of_oid(digest_oid).ok_or_else(|| {
        format!("is signed over digest algorithm {digest_oid}, not a hash of PS3.3 {EDITION} Table C.12.1.1.3.1.2-1")
    })?;
    let signature_oid = signer.signature_algorithm.oid;
    let algorithm = if signature_oid == RSA_ENCRYPTION {
        Some(digest)
    } else {
        RSA_SIGNATURES
            .iter()
            .find(|(oid, _)| *oid == signature_oid)
            .map(|(_, algorithm)| *algorithm)
    }
    .ok_or_else(|| {
        format!("is signed with {signature_oid}; TSA signatures are verified when they are RSA PKCS #1 v1.5")
    })?;
    let message = match &signer.signed_attrs {
        None => Cow::Borrowed(tst_info),
        Some(attributes) => {
            signed_attributes(attributes, digest, tst_info)?;
            Cow::Owned(attributes.to_der().map_err(|error| {
                format!("has signed attributes that cannot be encoded: {error}")
            })?)
        }
    };
    key.verify(
        algorithm.padding(),
        &algorithm.digest(&message),
        signer.signature.as_bytes(),
    )
    .map_err(|_| "has a TSA signature that does not verify with the public key of the TSA certificate it embeds".to_string())
}

/// The contentType and messageDigest signed attributes name and hash the
/// TSTInfo (RFC 5652 11.1 and 11.2).
fn signed_attributes(
    attributes: &SignedAttributes,
    digest: MacAlgorithm,
    tst_info: &[u8],
) -> Result<(), String> {
    let value = |oid: ObjectIdentifier| {
        attributes
            .iter()
            .find(|attribute| attribute.oid == oid)
            .and_then(|attribute| attribute.values.iter().next())
    };
    let content_type = value(CONTENT_TYPE).and_then(|any| any.decode_as::<ObjectIdentifier>().ok());
    if content_type != Some(TST_INFO) {
        return Err("has a contentType signed attribute that is not id-ct-TSTInfo".to_string());
    }
    let message_digest = value(MESSAGE_DIGEST)
        .and_then(|any| any.decode_as::<OctetString>().ok())
        .ok_or_else(|| "has no messageDigest signed attribute".to_string())?;
    if *digest.digest(tst_info) != *message_digest.as_bytes() {
        return Err(format!(
            "has a messageDigest signed attribute that is not the {} hash of its TSTInfo",
            digest.term()
        ));
    }
    Ok(())
}
