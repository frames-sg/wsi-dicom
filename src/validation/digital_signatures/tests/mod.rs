//! Objects signed in-process with test-only keys, and the rule over them.
//! The keys are generated from fixed seeds when the tests first need them
//! and never leave memory. The byte stream is pinned by hand in stream.rs;
//! interoperability with dcmsign is tested on exported files in
//! src/validation/tests/digital_signatures.rs.

mod rule;
mod stream;
mod timestamp;

use std::path::Path;
use std::str::FromStr;
use std::sync::OnceLock;
use std::time::Duration;

use dicom_core::value::{DataSetSequence, PixelFragmentSequence};
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::tags;
use dicom_object::mem::InMemElement;
use dicom_object::{DefaultDicomObject, FileMetaTableBuilder, InMemDicomObject};
use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha8Rng;
use rsa::pkcs1v15::{Signature, SigningKey};
use rsa::RsaPrivateKey;
use sha2::Sha256;
use x509_cert::builder::{Builder, CertificateBuilder, Profile};
use x509_cert::der::Encode;
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::SubjectPublicKeyInfoOwned;
use x509_cert::time::Validity;

use super::stream::{MacStream, MacSyntax};
use super::{character_set, items, run_digital_signatures_check, MacAlgorithm};
use crate::validation::encoding::explicit_little_endian;
use crate::validation::{ValidationCheck, ValidationStatus};

const VL_WSI: &str = "1.2.840.10008.5.1.4.1.1.77.1.6";
const EXPLICIT_LE: &str = "1.2.840.10008.1.2.1";
const JPEG_BASELINE: &str = "1.2.840.10008.1.2.4.50";
/// Large enough for every DigestInfo in a PKCS #1 v1.5 block, quick to generate.
const TEST_KEY_BITS: usize = 1024;

/// A change to a Data Set or Item.
type Edit = fn(&mut InMemDicomObject);

/// A test-only RSA key and its self-signed X.509 certificate (DER).
struct TestKey {
    key: RsaPrivateKey,
    certificate: Vec<u8>,
}

/// The key that signs test objects.
fn signer() -> &'static TestKey {
    static KEY: OnceLock<TestKey> = OnceLock::new();
    KEY.get_or_init(|| test_key(1, "CN=wsi-dicom test signer (test only)"))
}

/// The key of the test Time-Stamp Authority.
fn tsa() -> &'static TestKey {
    static KEY: OnceLock<TestKey> = OnceLock::new();
    KEY.get_or_init(|| test_key(2, "CN=wsi-dicom test TSA (test only)"))
}

fn test_key(seed: u64, subject: &str) -> TestKey {
    let key =
        RsaPrivateKey::new(&mut ChaCha8Rng::seed_from_u64(seed), TEST_KEY_BITS).expect("test key");
    let public = SubjectPublicKeyInfoOwned::from_key(key.to_public_key()).expect("public key");
    let certificate = certificate(&key, subject, public);
    TestKey { key, certificate }
}

/// A self-signed certificate for `public`, signed by `key`.
fn certificate(key: &RsaPrivateKey, subject: &str, public: SubjectPublicKeyInfoOwned) -> Vec<u8> {
    let signing = SigningKey::<Sha256>::new(key.clone());
    CertificateBuilder::new(
        Profile::Root,
        SerialNumber::from(7_u32),
        Validity::from_now(Duration::from_secs(3600)).expect("validity"),
        Name::from_str(subject).expect("subject"),
        public,
        &signing,
    )
    .expect("certificate builder")
    .build::<Signature>()
    .expect("certificate")
    .to_der()
    .expect("certificate DER")
}

fn unsigned(tag: Tag, value: u16) -> InMemElement {
    DataElement::new(tag, VR::US, PrimitiveValue::from(value))
}

fn text(tag: Tag, vr: VR, value: &str) -> InMemElement {
    DataElement::new(tag, vr, PrimitiveValue::from(value))
}

/// An OB Value, padded to even length with a NULL byte.
fn binary(tag: Tag, mut value: Vec<u8>) -> InMemElement {
    if value.len() % 2 == 1 {
        value.push(0);
    }
    DataElement::new(tag, VR::OB, PrimitiveValue::from(value))
}

fn sequence(tag: Tag, items: Vec<InMemDicomObject>) -> InMemElement {
    DataElement::new(tag, VR::SQ, DataSetSequence::from(items))
}

fn code(value: &str, meaning: &str) -> InMemDicomObject {
    InMemDicomObject::from_element_iter([
        text(tags::CODE_VALUE, VR::SH, value),
        text(tags::CODING_SCHEME_DESIGNATOR, VR::SH, "SCT"),
        text(tags::CODE_MEANING, VR::LO, meaning),
    ])
}

/// A small VL WSI Data Set with text, a nested Sequence and encapsulated
/// Pixel Data of two Fragments.
fn image() -> InMemDicomObject {
    let specimen = InMemDicomObject::from_element_iter([
        text(tags::SPECIMEN_IDENTIFIER, VR::LO, "SPECIMEN-1"),
        text(tags::SPECIMEN_UID, VR::UI, "1.2.826.0.1.3680043.10.999.801"),
        sequence(
            tags::SPECIMEN_TYPE_CODE_SEQUENCE,
            vec![code("430861001", "Tissue section")],
        ),
    ]);
    InMemDicomObject::from_element_iter([
        text(tags::SPECIFIC_CHARACTER_SET, VR::CS, "ISO_IR 100"),
        text(tags::SOP_CLASS_UID, VR::UI, VL_WSI),
        text(
            tags::SOP_INSTANCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.800",
        ),
        text(tags::PATIENT_NAME, VR::PN, "Müller^Anna"),
        unsigned(tags::ROWS, 8),
        unsigned(tags::COLUMNS, 8),
        sequence(tags::SPECIMEN_DESCRIPTION_SEQUENCE, vec![specimen]),
        DataElement::new(
            tags::PIXEL_DATA,
            VR::OB,
            PixelFragmentSequence::new(
                vec![0_u32],
                vec![vec![0xFF, 0xD8, 0x01, 0x02], vec![0x03, 0x04, 0xFF, 0xD9]],
            ),
        ),
    ])
}

/// The Tags of `data_set` that a MAC may include.
fn signable_tags(data_set: &InMemDicomObject) -> Vec<Tag> {
    data_set
        .iter()
        .map(|element| element.header().tag)
        .filter(|tag| {
            tag.group() >= 0x0008 && tag.group() != 0xFFFA && *tag != tags::MAC_PARAMETERS_SEQUENCE
        })
        .collect()
}

fn file(data_set: InMemDicomObject, syntax: &str) -> DefaultDicomObject {
    data_set
        .with_meta(
            FileMetaTableBuilder::new()
                .media_storage_sop_class_uid(VL_WSI)
                .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.999.800")
                .transfer_syntax(syntax),
        )
        .expect("file meta")
}

fn check(object: &DefaultDicomObject) -> ValidationCheck {
    run_digital_signatures_check(Path::new("object.dcm"), object)
}

#[track_caller]
fn assert_verified(object: &DefaultDicomObject, summary: &str) {
    let check = check(object);
    assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);
    assert!(check.message.ends_with(summary), "{}", check.message);
}

#[track_caller]
fn assert_violation(object: &DefaultDicomObject, expected: &str) {
    let check = check(object);
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
    assert!(
        check.message.contains(expected),
        "message lacks {expected:?}: {}",
        check.message
    );
}

/// How a test signature's MAC is calculated.
struct Mac<'t> {
    algorithm: MacAlgorithm,
    /// MAC Calculation Transfer Syntax UID.
    syntax: &'t str,
    tags: &'t [Tag],
}

/// Signs `tags` of `data_set` with SHA256 in MAC Calculation Transfer Syntax
/// `syntax`, and appends the MAC Parameters and Digital Signatures Items.
pub(in crate::validation) fn sign(data_set: &mut InMemDicomObject, syntax: &str, tags: &[Tag]) {
    sign_with(
        data_set,
        &Mac {
            algorithm: MacAlgorithm::Sha256,
            syntax,
            tags,
        },
    );
}

/// Signs as `mac` says, taking the Pixel Data to be encoded in the MAC
/// Calculation Transfer Syntax.
fn sign_with(data_set: &mut InMemDicomObject, mac: &Mac<'_>) {
    let id = u16::try_from(items(data_set, tags::MAC_PARAMETERS_SEQUENCE).len()).unwrap();
    let parameters = InMemDicomObject::from_element_iter([
        unsigned(tags::MACID_NUMBER, id),
        text(
            tags::MAC_CALCULATION_TRANSFER_SYNTAX_UID,
            VR::UI,
            mac.syntax,
        ),
        text(tags::MAC_ALGORITHM, VR::CS, mac.algorithm.term()),
        DataElement::new(
            tags::DATA_ELEMENTS_SIGNED,
            VR::AT,
            PrimitiveValue::Tags(mac.tags.iter().copied().collect()),
        ),
    ]);
    let mut signature = InMemDicomObject::from_element_iter([
        unsigned(tags::MACID_NUMBER, id),
        text(
            tags::DIGITAL_SIGNATURE_UID,
            VR::UI,
            &format!("1.2.826.0.1.3680043.10.999.9{id}"),
        ),
        text(
            tags::DIGITAL_SIGNATURE_DATE_TIME,
            VR::DT,
            "20260101000000+0000",
        ),
        text(tags::CERTIFICATE_TYPE, VR::CS, "X509_1993_SIG"),
        binary(tags::CERTIFICATE_OF_SIGNER, signer().certificate.clone()),
    ]);
    let digest = mac_of(data_set, &signature, mac);
    let value = signer()
        .key
        .sign(mac.algorithm.padding(), &digest)
        .expect("signature");
    signature.put(binary(tags::SIGNATURE, value));
    append(data_set, tags::MAC_PARAMETERS_SEQUENCE, parameters);
    append(data_set, tags::DIGITAL_SIGNATURES_SEQUENCE, signature);
}

/// The MAC of `signature` over `data_set`, through the stream the rule
/// recomputes; stream.rs pins its bytes independently.
fn mac_of(data_set: &InMemDicomObject, signature: &InMemDicomObject, mac: &Mac<'_>) -> Box<[u8]> {
    let mut hasher = mac.algorithm.hasher();
    let bytes = stream_bytes(data_set, signature, mac.syntax, mac.syntax, mac.tags);
    hasher.update(&bytes);
    hasher.finalize()
}

/// The MAC byte stream of `signature` over `data_set`.
fn stream_bytes(
    data_set: &InMemDicomObject,
    signature: &InMemDicomObject,
    file_syntax: &str,
    mac_syntax: &str,
    signed: &[Tag],
) -> Vec<u8> {
    let syntax = MacSyntax {
        uid: mac_syntax.to_string(),
        encapsulated: explicit_little_endian(mac_syntax).unwrap_or(false),
        file: file_syntax,
    };
    let charset = character_set(data_set).unwrap_or_default();
    let mut bytes = Vec::new();
    let mut stream = MacStream::new(&mut bytes, &syntax);
    stream
        .signed(data_set, signed, &charset)
        .expect("signed elements");
    stream
        .signature_item(signature, &charset)
        .expect("signature item");
    stream.finish().expect("stream");
    bytes
}

/// The MAC byte stream of the first top-level Digital Signature of `object`,
/// as the rule recomputes it.
pub(in crate::validation) fn first_signature_stream(object: &DefaultDicomObject) -> Vec<u8> {
    let root: &InMemDicomObject = object;
    let signature = &items(root, tags::DIGITAL_SIGNATURES_SEQUENCE)[0];
    let parameters = &items(root, tags::MAC_PARAMETERS_SEQUENCE)[0];
    let mac_syntax = super::string_value(parameters, tags::MAC_CALCULATION_TRANSFER_SYNTAX_UID, 1)
        .expect("MAC Calculation Transfer Syntax UID");
    let signed = super::tag_values(parameters, tags::DATA_ELEMENTS_SIGNED);
    let file_syntax = object.meta().transfer_syntax().trim_end_matches('\0');
    stream_bytes(root, signature, file_syntax, &mac_syntax, &signed)
}

/// Appends `item` to the Sequence `tag`, creating it.
fn append(data_set: &mut InMemDicomObject, tag: Tag, item: InMemDicomObject) {
    let mut present = data_set
        .take(tag)
        .and_then(|element| element.items().map(<[_]>::to_vec))
        .unwrap_or_default();
    present.push(item);
    data_set.put(sequence(tag, present));
}

/// Applies `edit` to the first Item of the Sequence `tag`.
fn edit_first(data_set: &mut InMemDicomObject, tag: Tag, edit: impl FnOnce(&mut InMemDicomObject)) {
    let mut element = data_set.take(tag).expect("sequence");
    edit(&mut element.items_mut().expect("items")[0]);
    data_set.put(element);
}
