//! Certified Timestamps: RFC 3161 tokens built in-process by a test-only TSA.

use std::time::Duration;

use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
use cms::content_info::{CmsVersion, ContentInfo};
use cms::signed_data::{
    CertificateSet, EncapsulatedContentInfo, SignedData, SignerIdentifier, SignerInfo, SignerInfos,
};
use der::asn1::{GeneralizedTime, ObjectIdentifier, OctetString, SetOfVec};
use der::{Any, Decode, Encode, Sequence, Tag as DerTag};
use dicom_core::VR;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;
use x509_cert::attr::Attribute;
use x509_cert::ext::pkix::SubjectKeyIdentifier;
use x509_cert::spki::AlgorithmIdentifierOwned;
use x509_cert::Certificate;

use super::super::keys::RSA_ENCRYPTION;
use super::super::timestamp::{
    MessageImprint, CONTENT_TYPE, MESSAGE_DIGEST, SIGNED_DATA, TST_INFO,
};
use super::super::MacAlgorithm;
use super::{
    assert_verified, assert_violation, binary, edit_first, file, image, sign, signable_tags, text,
    tsa, JPEG_BASELINE,
};

const SHA256_WITH_RSA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.11");
const ECDSA_WITH_SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.2");

/// The TSTInfo fields a token needs (RFC 3161 2.4.2).
#[derive(Sequence)]
struct TstInfo {
    version: u8,
    policy: ObjectIdentifier,
    message_imprint: MessageImprint,
    serial_number: u32,
    gen_time: GeneralizedTime,
}

/// How the test TSA builds a token.
struct Token {
    imprint_algorithm: ObjectIdentifier,
    /// The hash of the Signature the token certifies.
    imprint: Vec<u8>,
    embed_certificate: bool,
    /// Identifies the TSA by Subject Key Identifier, not issuer and serial.
    by_key_identifier: bool,
    /// Signs the TSTInfo itself, without signed attributes.
    signed_attributes: bool,
    /// A messageDigest attribute other than the TSTInfo's hash.
    wrong_message_digest: bool,
    signature_algorithm: ObjectIdentifier,
    /// Flips a bit of the TSA signature.
    break_signature: bool,
}

impl Token {
    /// A token that certifies `signature`, as a TSA makes one.
    fn certifying(signature: &[u8]) -> Self {
        Self {
            imprint_algorithm: MacAlgorithm::Sha256.oid(),
            imprint: MacAlgorithm::Sha256.digest(signature).to_vec(),
            embed_certificate: true,
            by_key_identifier: false,
            signed_attributes: true,
            wrong_message_digest: false,
            signature_algorithm: RSA_ENCRYPTION,
            break_signature: false,
        }
    }
}

fn algorithm(oid: ObjectIdentifier) -> AlgorithmIdentifierOwned {
    AlgorithmIdentifierOwned {
        oid,
        parameters: None,
    }
}

fn attribute(oid: ObjectIdentifier, value: Any) -> Attribute {
    Attribute {
        oid,
        values: SetOfVec::try_from(vec![value]).unwrap(),
    }
}

fn tst_info(request: &Token) -> Vec<u8> {
    TstInfo {
        version: 1,
        policy: ObjectIdentifier::new_unwrap("1.2.826.0.1.3680043.10.999.1"),
        message_imprint: MessageImprint {
            hash_algorithm: algorithm(request.imprint_algorithm),
            hashed_message: OctetString::new(request.imprint.clone()).unwrap(),
        },
        serial_number: 1,
        gen_time: GeneralizedTime::from_unix_duration(Duration::from_secs(1_767_225_600)).unwrap(),
    }
    .to_der()
    .unwrap()
}

/// The test TSA's SignerInfo over `tst_info`, with SHA256.
fn signer_info(request: &Token, certificate: &Certificate, tst_info: &[u8]) -> SignerInfo {
    let sha256 = MacAlgorithm::Sha256;
    let mut digest = sha256.digest(tst_info).to_vec();
    if request.wrong_message_digest {
        digest[0] ^= 1;
    }
    let attributes = request.signed_attributes.then(|| {
        SetOfVec::try_from(vec![
            attribute(CONTENT_TYPE, Any::encode_from(&TST_INFO).unwrap()),
            attribute(
                MESSAGE_DIGEST,
                Any::encode_from(&OctetString::new(digest).unwrap()).unwrap(),
            ),
        ])
        .unwrap()
    });
    let message = match &attributes {
        Some(attributes) => attributes.to_der().unwrap(),
        None => tst_info.to_vec(),
    };
    let mut signature = tsa()
        .key
        .sign(sha256.padding(), &sha256.digest(&message))
        .unwrap();
    if request.break_signature {
        signature[3] ^= 1;
    }
    let tbs = &certificate.tbs_certificate;
    let sid = if request.by_key_identifier {
        let (_, key) = tbs.get::<SubjectKeyIdentifier>().unwrap().unwrap();
        SignerIdentifier::SubjectKeyIdentifier(key)
    } else {
        SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
            issuer: tbs.issuer.clone(),
            serial_number: tbs.serial_number.clone(),
        })
    };
    SignerInfo {
        version: CmsVersion::V1,
        sid,
        digest_alg: algorithm(sha256.oid()),
        signed_attrs: attributes,
        signature_algorithm: algorithm(request.signature_algorithm),
        signature: OctetString::new(signature).unwrap(),
        unsigned_attrs: None,
    }
}

/// A TimeStampToken as `request` describes it.
fn token(request: &Token) -> Vec<u8> {
    let tst_info = tst_info(request);
    let certificate = Certificate::from_der(&tsa().certificate).unwrap();
    let signer = signer_info(request, &certificate, &tst_info);
    let certificates = request.embed_certificate.then(|| {
        CertificateSet(
            SetOfVec::try_from(vec![CertificateChoices::Certificate(certificate)]).unwrap(),
        )
    });
    let signed = SignedData {
        version: CmsVersion::V3,
        digest_algorithms: SetOfVec::try_from(vec![algorithm(MacAlgorithm::Sha256.oid())]).unwrap(),
        encap_content_info: EncapsulatedContentInfo {
            econtent_type: TST_INFO,
            econtent: Some(Any::new(DerTag::OctetString, tst_info).unwrap()),
        },
        certificates,
        crls: None,
        signer_infos: SignerInfos(SetOfVec::try_from(vec![signer]).unwrap()),
    };
    ContentInfo {
        content_type: SIGNED_DATA,
        content: Any::encode_from(&signed).unwrap(),
    }
    .to_der()
    .unwrap()
}

/// The test image signed, then timestamped by `stamp` from its Signature.
fn timestamped(stamp: impl FnOnce(&[u8]) -> Vec<u8>) -> InMemDicomObject {
    let mut data_set = image();
    let tags = signable_tags(&data_set);
    sign(&mut data_set, JPEG_BASELINE, &tags);
    edit_first(&mut data_set, tags::DIGITAL_SIGNATURES_SEQUENCE, |item| {
        let signature = item
            .get(tags::SIGNATURE)
            .unwrap()
            .to_bytes()
            .unwrap()
            .into_owned();
        item.put(text(tags::CERTIFIED_TIMESTAMP_TYPE, VR::CS, "CMS_TSP"));
        item.put(binary(tags::CERTIFIED_TIMESTAMP, stamp(&signature)));
    });
    data_set
}

#[track_caller]
fn assert_token_verified(change: fn(&mut Token)) {
    let data_set = timestamped(|signature| {
        let mut request = Token::certifying(signature);
        change(&mut request);
        token(&request)
    });
    assert_verified(
        &file(data_set, JPEG_BASELINE),
        "1 Digital Signature verified, 1 with a verified Certified Timestamp",
    );
}

#[track_caller]
fn assert_token_violation(change: fn(&mut Token), expected: &str) {
    let data_set = timestamped(|signature| {
        let mut request = Token::certifying(signature);
        change(&mut request);
        token(&request)
    });
    assert_violation(&file(data_set, JPEG_BASELINE), expected);
}

#[test]
fn a_token_certifying_the_signature_with_its_tsa_certificate_verifies() {
    assert_token_verified(|_| {});
    assert_token_verified(|request| request.by_key_identifier = true);
    assert_token_verified(|request| request.signed_attributes = false);
    assert_token_verified(|request| request.signature_algorithm = SHA256_WITH_RSA);
}

#[test]
fn a_token_without_its_tsa_certificate_is_matched_but_not_verified() {
    let data_set = timestamped(|signature| {
        token(&Token {
            embed_certificate: false,
            ..Token::certifying(signature)
        })
    });
    assert_verified(
        &file(data_set, JPEG_BASELINE),
        "1 Digital Signature verified; Digital Signatures Sequence (FFFA,FFFA)[1]: its Certified Timestamp certifies the Signature, but the token embeds no certificate of its TSA, so the token's own signature is not verified",
    );
}

#[test]
fn a_token_of_another_signature_fails() {
    assert_token_violation(
        |request| request.imprint[0] ^= 1,
        "Digital Signatures Sequence (FFFA,FFFA)[1]: Certified Timestamp (0400,0310) messageImprint is not the SHA256 hash of Signature (0400,0120), so the token does not certify this Signature (PS3.3 2026d Table C.12-6 and RFC 3161 2.4.2)",
    );
}

#[test]
fn a_token_whose_tsa_signature_does_not_verify_fails() {
    assert_token_violation(
        |request| request.break_signature = true,
        "Certified Timestamp (0400,0310) has a TSA signature that does not verify with the public key of the TSA certificate it embeds (RFC 5652 5.4 and 5.6)",
    );
    assert_token_violation(
        |request| request.wrong_message_digest = true,
        "Certified Timestamp (0400,0310) has a messageDigest signed attribute that is not the SHA256 hash of its TSTInfo (RFC 5652 5.4 and 5.6)",
    );
    assert_token_violation(
        |request| request.signature_algorithm = ECDSA_WITH_SHA256,
        "Certified Timestamp (0400,0310) is signed with 1.2.840.10045.4.3.2; TSA signatures are verified when they are RSA PKCS #1 v1.5 (RFC 5652 5.4 and 5.6)",
    );
}

#[test]
fn a_timestamp_the_rule_cannot_read_fails() {
    let data_set = timestamped(|_| b"not a token".to_vec());
    assert_violation(
        &file(data_set, JPEG_BASELINE),
        "Certified Timestamp (0400,0310) is not an RFC 3161 TimeStampToken: ",
    );

    assert_token_violation(
        |request| request.imprint_algorithm = ObjectIdentifier::new_unwrap("1.2.826.0.1.3680043.10.999.4"),
        "Certified Timestamp (0400,0310) messageImprint hashes with 1.2.826.0.1.3680043.10.999.4, not a hash of PS3.3 2026d Table C.12.1.1.3.1.2-1, so it cannot be compared with the Signature",
    );

    let mut data_set = timestamped(|signature| token(&Token::certifying(signature)));
    edit_first(&mut data_set, tags::DIGITAL_SIGNATURES_SEQUENCE, |item| {
        item.put(text(tags::CERTIFIED_TIMESTAMP_TYPE, VR::CS, "PGP"));
    });
    assert_violation(
        &file(data_set, JPEG_BASELINE),
        "Certified Timestamp Type (0400,0305) is \"PGP\"; CMS_TSP, the RFC 3161 token of PS3.15 2026d C.1, is the type whose Certified Timestamp (0400,0310) can be read (PS3.3 2026d Table C.12-6)",
    );
}
