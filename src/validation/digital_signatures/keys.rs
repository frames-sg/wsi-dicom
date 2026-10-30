//! The RSA public keys a Digital Signature and its Certified Timestamp are
//! verified with, read from X.509 certificates (PS3.15 C.1).

use der::{Decode, Encode, Header, SliceReader};
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;
use rsa::pkcs1::RsaPublicKey as Pkcs1PublicKey;
use rsa::traits::PublicKeyParts;
use rsa::{BigUint, RsaPublicKey};
use x509_cert::spki::{ObjectIdentifier, SubjectPublicKeyInfoOwned};
use x509_cert::Certificate;

use super::{bytes, string_value, EDITION};
use crate::validation::disclosure::shown_or;

/// rsaEncryption (RFC 8017 A.1).
pub(super) const RSA_ENCRYPTION: ObjectIdentifier =
    ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");
/// The longest modulus verified; RSA verification time grows with it.
const MAX_MODULUS_BITS: usize = 16384;

/// The RSA public key of Certificate of Signer (0400,0115), an X.509
/// signature certificate of Certificate Type X509_1993_SIG (PS3.15 C.1).
pub(super) fn signer_key(signature: &InMemDicomObject) -> Result<RsaPublicKey, String> {
    let certificate_type = string_value(signature, tags::CERTIFICATE_TYPE, 1);
    if certificate_type.as_deref() != Some("X509_1993_SIG") {
        return Err(format!(
            "Certificate Type (0400,0110) is {}; X509_1993_SIG is the certificate type of the Digital Signature Profiles, so Certificate of Signer (0400,0115) cannot be read (PS3.15 {EDITION} C.1)",
            shown_or(tags::CERTIFICATE_TYPE, certificate_type.as_deref(), "absent")
        ));
    }
    let value = bytes(signature, tags::CERTIFICATE_OF_SIGNER).ok_or_else(|| {
        format!("Certificate of Signer (0400,0115) is absent, so the Signature cannot be verified (PS3.3 {EDITION} Table C.12-6)")
    })?;
    let certificate = certificate(&value).map_err(|reason| {
        format!("Certificate of Signer (0400,0115) is not an X.509 certificate: {reason} (PS3.15 {EDITION} C.1)")
    })?;
    rsa_key(&certificate.tbs_certificate.subject_public_key_info).map_err(|reason| {
        format!("Certificate of Signer (0400,0115) {reason}; the Digital Signature Profiles sign with RSA (PS3.15 {EDITION} Annex C)")
    })
}

/// The RSA signature in Signature (0400,0120): as long as the modulus, or
/// one NULL byte longer where an odd length is padded (PS3.5 7.1).
pub(super) fn signature_value<'v>(value: &'v [u8], key: &RsaPublicKey) -> &'v [u8] {
    let size = key.size();
    match value {
        [signature @ .., 0] if signature.len() == size && size % 2 == 1 => signature,
        _ => value,
    }
}

/// A DER certificate in an OB Value.
pub(super) fn certificate(value: &[u8]) -> Result<Certificate, String> {
    Certificate::from_der(der_encoding(value)?).map_err(|error| error.to_string())
}

/// The RSA public key of a certificate's SubjectPublicKeyInfo; the reason
/// reads after the certificate's name.
pub(super) fn rsa_key(spki: &SubjectPublicKeyInfoOwned) -> Result<RsaPublicKey, String> {
    let algorithm = spki.algorithm.oid;
    if algorithm != RSA_ENCRYPTION {
        return Err(format!(
            "holds a public key of algorithm {algorithm}, not rsaEncryption"
        ));
    }
    let key = spki
        .subject_public_key
        .as_bytes()
        .ok_or_else(|| "holds a public key that is not whole bytes".to_string())
        .and_then(|bytes| {
            Pkcs1PublicKey::from_der(bytes)
                .map_err(|error| format!("holds a malformed RSA public key: {error}"))
        })?;
    RsaPublicKey::new_with_max_size(
        BigUint::from_bytes_be(key.modulus.as_bytes()),
        BigUint::from_bytes_be(key.public_exponent.as_bytes()),
        MAX_MODULUS_BITS,
    )
    .map_err(|error| format!("holds an unusable RSA public key: {error}"))
}

/// The DER encoding an OB Value holds: an odd-length encoding is followed by
/// one NULL byte, an even-length one by nothing (PS3.5 7.1).
pub(super) fn der_encoding(value: &[u8]) -> Result<&[u8], String> {
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
