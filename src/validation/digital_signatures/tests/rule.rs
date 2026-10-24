//! The rule over objects signed in-process: every MAC Algorithm, tampering,
//! MAC Parameters references, certificates and Pixel Data encodings.

use dicom_core::value::{PixelFragmentSequence, Value};
use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;
use rsa::traits::PublicKeyParts;
use rsa::{BigUint, RsaPublicKey};
use x509_cert::spki::{AlgorithmIdentifierOwned, ObjectIdentifier, SubjectPublicKeyInfoOwned};

use super::super::keys::signature_value;
use super::super::MacAlgorithm;
use super::{
    assert_verified, assert_violation, binary, certificate, check, edit_first, file, image, sign,
    sign_with, signable_tags, signer, text, unsigned, Edit, Mac, EXPLICIT_LE, JPEG_BASELINE,
};
use crate::validation::ValidationStatus;

/// The test image signed over all its Data Elements.
fn signed_image() -> InMemDicomObject {
    let mut data_set = image();
    let tags = signable_tags(&data_set);
    sign(&mut data_set, JPEG_BASELINE, &tags);
    data_set
}

const DOES_NOT_VERIFY: &str =
    "Digital Signatures Sequence (FFFA,FFFA)[1]: Signature (0400,0120) does not verify";

#[test]
fn an_unsigned_object_passes_with_nothing_to_verify() {
    assert_verified(
        &file(image(), JPEG_BASELINE),
        "no Digital Signatures Sequence (FFFA,FFFA) Item; nothing to verify",
    );
}

#[test]
fn every_mac_algorithm_of_table_c_12_1_1_3_1_2_1_verifies() {
    for &algorithm in MacAlgorithm::ALL {
        let mut data_set = image();
        let tags = signable_tags(&data_set);
        sign_with(
            &mut data_set,
            &Mac {
                algorithm,
                syntax: JPEG_BASELINE,
                tags: &tags,
            },
        );
        let check = check(&file(data_set, JPEG_BASELINE));
        assert_eq!(
            (
                check.status,
                check.message.ends_with("1 Digital Signature verified")
            ),
            (ValidationStatus::Passed, true),
            "{}: {}",
            algorithm.term(),
            check.message
        );
    }
}

#[test]
fn a_changed_signed_data_element_fails() {
    let changes: [(&str, Edit); 4] = [
        ("Patient Name", |data_set| {
            data_set.put(text(tags::PATIENT_NAME, VR::PN, "Muller^Anna"));
        }),
        ("an Item of a signed Sequence", |data_set| {
            edit_first(data_set, tags::SPECIMEN_DESCRIPTION_SEQUENCE, |item| {
                item.put(text(tags::SPECIMEN_IDENTIFIER, VR::LO, "SPECIMEN-2"));
            });
        }),
        ("a Pixel Data Fragment", |data_set| {
            let pixel_data = data_set.take(tags::PIXEL_DATA).unwrap();
            let Value::PixelSequence(pixels) = pixel_data.into_value() else {
                panic!("encapsulated Pixel Data");
            };
            let (offsets, mut fragments) = pixels.into_parts();
            fragments[1][0] ^= 1;
            data_set.put(DataElement::new(
                tags::PIXEL_DATA,
                VR::OB,
                PixelFragmentSequence::new(offsets, fragments),
            ));
        }),
        (
            "the signature's own Digital Signature DateTime",
            |data_set| {
                edit_first(data_set, tags::DIGITAL_SIGNATURES_SEQUENCE, |item| {
                    item.put(text(
                        tags::DIGITAL_SIGNATURE_DATE_TIME,
                        VR::DT,
                        "20260102000000+0000",
                    ));
                });
            },
        ),
    ];
    for (name, change) in changes {
        let mut data_set = signed_image();
        change(&mut data_set);
        let check = check(&file(data_set, JPEG_BASELINE));
        assert_eq!(
            check.status,
            ValidationStatus::Failed,
            "{name}: {}",
            check.message
        );
        assert!(
            check.message.contains(DOES_NOT_VERIFY),
            "{name}: {}",
            check.message
        );
        assert!(
            check
                .message
                .contains("over the SHA256 MAC of the 8 Data Elements Signed and this Item's own Attributes, encoded in 1.2.840.10008.1.2.4.50"),
            "{name}: {}",
            check.message
        );
    }
}

#[test]
fn unsigned_data_elements_and_excluded_attributes_may_change() {
    let mut data_set = image();
    sign(
        &mut data_set,
        JPEG_BASELINE,
        &[tags::SOP_INSTANCE_UID, tags::PIXEL_DATA],
    );
    data_set.put(text(tags::PATIENT_NAME, VR::PN, "Other^Name"));
    edit_first(&mut data_set, tags::DIGITAL_SIGNATURES_SEQUENCE, |item| {
        item.put(text(tags::CERTIFIED_TIMESTAMP_TYPE, VR::CS, "CMS_TSP"));
    });
    edit_first(&mut data_set, tags::MAC_PARAMETERS_SEQUENCE, |item| {
        item.put(text(tags::PATIENT_ID, VR::LO, "outside the MAC"));
    });
    // Certified Timestamp Type without a Certified Timestamp is unread.
    assert_verified(
        &file(data_set, JPEG_BASELINE),
        "1 Digital Signature verified",
    );
}

#[test]
fn a_changed_signature_fails() {
    let mut data_set = signed_image();
    edit_first(&mut data_set, tags::DIGITAL_SIGNATURES_SEQUENCE, |item| {
        let mut value = item
            .get(tags::SIGNATURE)
            .unwrap()
            .to_bytes()
            .unwrap()
            .into_owned();
        value[10] ^= 0x40;
        item.put(binary(tags::SIGNATURE, value));
    });
    assert_violation(&file(data_set, JPEG_BASELINE), DOES_NOT_VERIFY);
}

#[test]
fn a_mac_id_number_must_identify_one_mac_parameters_item() {
    let mut data_set = signed_image();
    edit_first(&mut data_set, tags::DIGITAL_SIGNATURES_SEQUENCE, |item| {
        item.put(unsigned(tags::MACID_NUMBER, 7));
    });
    assert_violation(
        &file(data_set, JPEG_BASELINE),
        "Digital Signatures Sequence (FFFA,FFFA)[1]: MAC ID Number (0400,0005) 7 identifies no MAC Parameters Sequence (4FFE,0001) Item at its level, so its MAC cannot be recomputed (PS3.3 2026d Table C.12-6)",
    );

    let mut data_set = signed_image();
    let tags = signable_tags(&image());
    sign(&mut data_set, JPEG_BASELINE, &tags);
    edit_first(&mut data_set, tags::MAC_PARAMETERS_SEQUENCE, |item| {
        item.put(unsigned(tags::MACID_NUMBER, 1));
    });
    assert_violation(
        &file(data_set, JPEG_BASELINE),
        "MAC ID Number (0400,0005) 1 identifies more than one MAC Parameters Sequence (4FFE,0001) Item; it shall be unique within the SOP Instance",
    );

    let mut data_set = signed_image();
    edit_first(&mut data_set, tags::DIGITAL_SIGNATURES_SEQUENCE, |item| {
        item.remove_element(tags::MACID_NUMBER);
    });
    assert_violation(
        &file(data_set, JPEG_BASELINE),
        "MAC ID Number (0400,0005) is absent, so no MAC Parameters Sequence (4FFE,0001) Item says how its MAC was calculated",
    );
}

#[test]
fn mac_parameters_the_rule_cannot_recompute_fail() {
    let cases: [(Edit, &str); 4] = [
        (
            |item| {
                item.put(text(tags::MAC_ALGORITHM, VR::CS, "BLAKE3"));
            },
            "MAC Algorithm (0400,0015) is \"BLAKE3\", not a Defined Term of PS3.3 2026d Table C.12.1.1.3.1.2-1, so its MAC cannot be computed",
        ),
        (
            |item| {
                item.put(text(tags::MAC_CALCULATION_TRANSFER_SYNTAX_UID, VR::UI, "1.2.840.10008.1.2"));
            },
            "MAC Calculation Transfer Syntax UID (0400,0010) 1.2.840.10008.1.2 is an Implicit VR Transfer Syntax; a MAC is encoded in an Explicit VR Little Endian Transfer Syntax, so this one is not recomputed (PS3.3 2026d C.12.1.1.3.1.2 and Table C.12-6)",
        ),
        (
            |item| {
                item.put(text(tags::MAC_CALCULATION_TRANSFER_SYNTAX_UID, VR::UI, "1.2.840.10008.1.2.2"));
            },
            "1.2.840.10008.1.2.2 is a Big Endian Transfer Syntax",
        ),
        (
            |item| {
                item.put(text(tags::MAC_CALCULATION_TRANSFER_SYNTAX_UID, VR::UI, "1.2.826.0.1.3680043.10.999.1"));
            },
            "1.2.826.0.1.3680043.10.999.1 is not a Transfer Syntax whose Data Set encoding PS3.5 Annex A defines",
        ),
    ];
    for (edit, expected) in cases {
        let mut data_set = signed_image();
        edit_first(&mut data_set, tags::MAC_PARAMETERS_SEQUENCE, edit);
        assert_violation(&file(data_set, JPEG_BASELINE), expected);
    }
}

#[test]
fn pixel_data_is_hashed_only_in_the_encoding_the_file_holds() {
    // The MAC Calculation Transfer Syntax would carry the Frames natively.
    let mut data_set = image();
    let tags = signable_tags(&data_set);
    sign_with(
        &mut data_set,
        &Mac {
            algorithm: MacAlgorithm::Sha256,
            syntax: EXPLICIT_LE,
            tags: &tags,
        },
    );
    assert_violation(
        &file(data_set, JPEG_BASELINE),
        "PixelData (7FE0,0010) is encapsulated as the file's Transfer Syntax 1.2.840.10008.1.2.4.50 encodes it, and MAC Calculation Transfer Syntax UID (0400,0010) 1.2.840.10008.1.2.1 encodes it otherwise; its Frames are not re-encoded to recompute the MAC (PS3.3 2026d C.12.1.1.3.1.2)",
    );

    // Without Pixel Data among the signed elements, only the Data Element
    // encoding matters, which is the same.
    let mut data_set = image();
    sign_with(
        &mut data_set,
        &Mac {
            algorithm: MacAlgorithm::Sha256,
            syntax: EXPLICIT_LE,
            tags: &[tags::SOP_INSTANCE_UID, tags::SPECIMEN_DESCRIPTION_SEQUENCE],
        },
    );
    assert_verified(
        &file(data_set, JPEG_BASELINE),
        "1 Digital Signature verified",
    );

    // Native Pixel Data in a native file, signed in another native syntax.
    let mut data_set = image();
    data_set.put(DataElement::new(
        tags::PIXEL_DATA,
        VR::OB,
        PrimitiveValue::from(vec![9_u8; 64]),
    ));
    let tags = signable_tags(&data_set);
    sign_with(
        &mut data_set,
        &Mac {
            algorithm: MacAlgorithm::Sha512,
            syntax: EXPLICIT_LE,
            tags: &tags,
        },
    );
    assert_verified(
        &file(data_set, "1.2.840.10008.1.2"),
        "1 Digital Signature verified",
    );
}

#[test]
fn certificates_the_rule_cannot_read_fail() {
    let cases: [(Edit, &str); 3] = [
        (
            |item| {
                item.put(text(tags::CERTIFICATE_TYPE, VR::CS, "X509_1993_ENC"));
            },
            "Certificate Type (0400,0110) is \"X509_1993_ENC\"; X509_1993_SIG is the certificate type of the Digital Signature Profiles, so Certificate of Signer (0400,0115) cannot be read (PS3.15 2026d C.1)",
        ),
        (
            |item| {
                item.put(binary(tags::CERTIFICATE_OF_SIGNER, b"-----BEGIN CERTIFICATE-----".to_vec()));
            },
            "Certificate of Signer (0400,0115) is not an X.509 certificate: ",
        ),
        (
            |item| {
                let mut value = super::signer().certificate.clone();
                value.extend([0, 0]);
                item.put(binary(tags::CERTIFICATE_OF_SIGNER, value));
            },
            "Certificate of Signer (0400,0115) is not an X.509 certificate: 2 bytes follow its",
        ),
    ];
    for (edit, expected) in cases {
        let mut data_set = signed_image();
        edit_first(&mut data_set, tags::DIGITAL_SIGNATURES_SEQUENCE, edit);
        assert_violation(&file(data_set, JPEG_BASELINE), expected);
    }
}

#[test]
fn a_certificate_without_an_rsa_key_fails() {
    // An EC point under id-ecPublicKey with prime256v1; the certificate's own
    // signature is RSA and is not what is read.
    let elliptic = SubjectPublicKeyInfoOwned {
        algorithm: AlgorithmIdentifierOwned {
            oid: ObjectIdentifier::new_unwrap("1.2.840.10045.2.1"),
            parameters: Some(
                x509_cert::der::Any::encode_from(&ObjectIdentifier::new_unwrap(
                    "1.2.840.10045.3.1.7",
                ))
                .unwrap(),
            ),
        },
        subject_public_key: x509_cert::der::asn1::BitString::from_bytes(&[4; 65]).unwrap(),
    };
    let foreign = certificate(
        &signer().key,
        "CN=wsi-dicom test EC subject (test only)",
        elliptic,
    );
    let mut data_set = signed_image();
    edit_first(&mut data_set, tags::DIGITAL_SIGNATURES_SEQUENCE, |item| {
        item.put(binary(tags::CERTIFICATE_OF_SIGNER, foreign));
    });
    assert_violation(
        &file(data_set, JPEG_BASELINE),
        "Certificate of Signer (0400,0115) holds a public key of algorithm 1.2.840.10045.2.1, not rsaEncryption; the Digital Signature Profiles sign with RSA (PS3.15 2026d Annex C)",
    );
}

#[test]
fn a_signature_in_a_sequence_item_is_verified_against_its_item() {
    let mut data_set = image();
    edit_first(&mut data_set, tags::SPECIMEN_DESCRIPTION_SEQUENCE, |item| {
        let tags = [tags::SPECIMEN_IDENTIFIER, tags::SPECIMEN_TYPE_CODE_SEQUENCE];
        sign(item, JPEG_BASELINE, &tags);
    });
    // The outer signature covers the Item, whose own signature it excludes.
    let tags = signable_tags(&data_set);
    sign(&mut data_set, JPEG_BASELINE, &tags);
    assert_verified(
        &file(data_set.clone(), JPEG_BASELINE),
        "2 Digital Signatures verified",
    );

    edit_first(&mut data_set, tags::SPECIMEN_DESCRIPTION_SEQUENCE, |item| {
        item.put(text(tags::SPECIMEN_IDENTIFIER, VR::LO, "SPECIMEN-9"));
    });
    let check = check(&file(data_set, JPEG_BASELINE));
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
    assert!(check.message.contains("2 violations"), "{}", check.message);
    assert!(
        check.message.contains(
            "SpecimenDescriptionSequence (0040,0560)[1] > Digital Signatures Sequence (FFFA,FFFA)[1]: Signature (0400,0120) does not verify"
        ),
        "{}",
        check.message
    );
}

#[test]
fn a_signature_of_an_odd_length_modulus_is_read_without_its_pad_byte() {
    // A 1032-bit modulus: signatures are 129 bytes, padded to 130 in OB.
    let mut modulus = vec![0x80];
    modulus.extend([0; 127]);
    modulus.push(1);
    let key =
        RsaPublicKey::new(BigUint::from_bytes_be(&modulus), BigUint::from(65_537_u32)).unwrap();
    assert_eq!(key.size(), 129);
    let mut padded = vec![1_u8; 129];
    padded.push(0);
    assert_eq!(signature_value(&padded, &key), &padded[..129]);
    let unpadded = vec![1_u8; 130];
    assert_eq!(signature_value(&unpadded, &key), unpadded.as_slice());
}
