//! The rule over whole objects: Frame iteration, bounded reports, and the
//! ICC Profile each JPEG Frame is compared with.

use std::path::Path;

use dicom_core::value::{DataSetSequence, PixelFragmentSequence};
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::tags;
use dicom_object::mem::InMemElement;
use dicom_object::{DefaultDicomObject, FileMetaTableBuilder, InMemDicomObject};

use super::super::run_codestream_check;
use super::{assert_conformant, assert_violation, insert};
use crate::validation::{ValidationCheck, ValidationStatus};

const VL_WSI: &str = "1.2.840.10008.5.1.4.1.1.77.1.6";
const BASELINE: &str = "1.2.840.10008.1.2.4.50";
const J2K_LOSSLESS: &str = "1.2.840.10008.1.2.4.90";

fn unsigned(tag: Tag, value: u16) -> InMemElement {
    DataElement::new(tag, VR::US, PrimitiveValue::from(value))
}

fn text(tag: Tag, vr: VR, value: &str) -> InMemElement {
    DataElement::new(tag, vr, PrimitiveValue::from(value))
}

fn sequence(tag: Tag, items: Vec<InMemDicomObject>) -> InMemElement {
    DataElement::new(tag, VR::SQ, DataSetSequence::from(items))
}

/// A VL WSI Data Set whose Pixel Data holds `frames`, one Fragment each.
pub(super) fn image(
    (rows, columns): (u16, u16),
    samples: u16,
    photometric: &str,
    frames: Vec<Vec<u8>>,
) -> InMemDicomObject {
    let count = frames.len().to_string();
    InMemDicomObject::from_element_iter([
        text(tags::SOP_CLASS_UID, VR::UI, VL_WSI),
        unsigned(tags::ROWS, rows),
        unsigned(tags::COLUMNS, columns),
        unsigned(tags::SAMPLES_PER_PIXEL, samples),
        unsigned(tags::BITS_ALLOCATED, 8),
        unsigned(tags::BITS_STORED, 8),
        unsigned(tags::HIGH_BIT, 7),
        unsigned(tags::PIXEL_REPRESENTATION, 0),
        text(tags::PHOTOMETRIC_INTERPRETATION, VR::CS, photometric),
        text(tags::NUMBER_OF_FRAMES, VR::IS, &count),
        DataElement::new(
            tags::PIXEL_DATA,
            VR::OB,
            PixelFragmentSequence::new_fragments(frames),
        ),
    ])
}

fn file(data_set: InMemDicomObject, syntax: &str) -> DefaultDicomObject {
    data_set
        .with_meta(
            FileMetaTableBuilder::new()
                .media_storage_sop_class_uid(VL_WSI)
                .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.999.700")
                .transfer_syntax(syntax),
        )
        .expect("file meta")
}

fn check(object: &DefaultDicomObject) -> ValidationCheck {
    run_codestream_check(Path::new("object.dcm"), object, 1 << 20)
}

fn grey_jpeg() -> Vec<u8> {
    crate::test_support::encode_test_gray_jpeg(16, 8, 90)
}

/// APP2 "ICC_PROFILE" segments holding `profile` in `chunks` pieces.
fn icc_segments(profile: &[u8], chunks: usize) -> Vec<u8> {
    let size = profile.len().div_ceil(chunks);
    let count = u8::try_from(chunks).expect("chunk count");
    profile
        .chunks(size)
        .zip(1..=count)
        .flat_map(|(data, sequence)| {
            let length = u16::try_from(2 + 12 + 2 + data.len()).expect("segment length");
            [
                &[0xFF, 0xE2][..],
                &length.to_be_bytes(),
                b"ICC_PROFILE\0",
                &[sequence, count],
                data,
            ]
            .concat()
        })
        .collect()
}

fn with_icc(stream: &[u8], profile: &[u8], chunks: usize) -> Vec<u8> {
    insert(stream, 2, &icc_segments(profile, chunks))
}

fn profile(seed: u8) -> Vec<u8> {
    (0..301u32)
        .map(|index| (index as u8).wrapping_mul(seed))
        .collect()
}

/// An Optical Path Sequence Item; its ICC Profile padded to even length.
fn optical_path(identifier: &str, profile: &[u8]) -> InMemDicomObject {
    let mut padded = profile.to_vec();
    if padded.len() % 2 == 1 {
        padded.push(0);
    }
    InMemDicomObject::from_element_iter([
        text(tags::OPTICAL_PATH_IDENTIFIER, VR::SH, identifier),
        DataElement::new(tags::ICC_PROFILE, VR::OB, PrimitiveValue::from(padded)),
    ])
}

fn identifying(identifier: &str) -> InMemDicomObject {
    InMemDicomObject::from_element_iter([sequence(
        tags::OPTICAL_PATH_IDENTIFICATION_SEQUENCE,
        vec![InMemDicomObject::from_element_iter([text(
            tags::OPTICAL_PATH_IDENTIFIER,
            VR::SH,
            identifier,
        )])],
    )])
}

#[test]
fn every_frame_is_checked_against_the_attributes_that_define_it() {
    let frames = vec![
        super::j2k::lossless_rgb(16, 8),
        super::j2k::lossless_rgb(32, 8),
    ];
    let result = check(&file(image((8, 16), 3, "YBR_RCT", frames), J2K_LOSSLESS));
    assert_violation(
        &result,
        "DICOM PS3.5 2026d 8.2.4 and A.4.4: 1 violation: Frame 2: SIZ width (Xsiz - XOsiz) is 32 but Columns (0028,0011) is 16 (PS3.5 2026d 8.2.4)",
    );
}

#[test]
fn reports_show_the_first_violations_and_count_the_rest() {
    let frames = vec![grey_jpeg(); 25];
    let result = check(&file(image((9, 16), 1, "MONOCHROME2", frames), BASELINE));
    assert_violation(
        &result,
        "25 violations: Frame 1: SOF0 number of lines is 8 but Rows (0028,0010) is 9",
    );
    assert_violation(&result, "Frame 20: SOF0 number of lines");
    assert_violation(&result, "; and 5 more");
    assert!(!result.message.contains("Frame 21:"), "{}", result.message);
}

#[test]
fn transfer_syntaxes_without_a_known_codestream_pass_with_nothing_to_check() {
    // Deflated Image Frame Compression is inflated; see tests/deflate.rs.
    for syntax in [
        "1.2.840.10008.1.2.1",
        "1.2.840.10008.1.2.4.94",
        "1.2.840.10008.1.2.4.100",
    ] {
        let object = file(image((8, 16), 1, "MONOCHROME2", vec![grey_jpeg()]), syntax);
        let result = check(&object);
        assert_eq!(result.status, ValidationStatus::Passed, "{syntax}");
        assert!(
            result.message.contains("nothing to check"),
            "{}",
            result.message
        );
    }
}

#[test]
fn frames_that_cannot_be_delimited_are_reported() {
    let mut data_set = image((8, 16), 1, "MONOCHROME2", vec![grey_jpeg(); 3]);
    data_set.put(text(tags::NUMBER_OF_FRAMES, VR::IS, "2"));
    assert_violation(
        &check(&file(data_set, BASELINE)),
        "Pixel Data Frames cannot be delimited, so no codestream was checked: cannot map 3 Pixel Data fragments to 2 frames without an offset table (PS3.5 2026d A.4)",
    );
    let mut native = image((8, 16), 1, "MONOCHROME2", vec![]);
    native.put(DataElement::new(
        tags::PIXEL_DATA,
        VR::OB,
        PrimitiveValue::from(vec![0u8; 128]),
    ));
    assert_violation(
        &check(&file(native, BASELINE)),
        "Pixel Data (7FE0,0010) is not encapsulated although Transfer Syntax 1.2.840.10008.1.2.4.50 requires it",
    );
}

#[test]
fn embedded_icc_profiles_equal_the_icc_profile_of_the_frame() {
    let icc = profile(3);
    let object = |frame: Vec<u8>| {
        let mut data_set = image((8, 16), 1, "MONOCHROME2", vec![frame]);
        data_set.put(sequence(
            tags::OPTICAL_PATH_SEQUENCE,
            vec![optical_path("1", &icc)],
        ));
        file(data_set, BASELINE)
    };
    // The DICOM Value's trailing pad byte is excluded.
    assert_conformant(&check(&object(with_icc(&grey_jpeg(), &icc, 2))));
    assert_conformant(&check(&object(grey_jpeg())));
    assert_violation(
        &check(&object(with_icc(&grey_jpeg(), &profile(5), 2))),
        "Frame 1: APP2 ICC_PROFILE segments concatenate to a 301-byte profile that is not identical to the 301-byte ICC Profile (0028,2000) of Optical Path Sequence Item 1 (PS3.5 2026d 8.2.1)",
    );
    // A zero-length ICC Profile is the same as none (PS3.5 7.4.5).
    let mut empty = image(
        (8, 16),
        1,
        "MONOCHROME2",
        vec![with_icc(&grey_jpeg(), &icc, 2)],
    );
    empty.put(sequence(
        tags::OPTICAL_PATH_SEQUENCE,
        vec![optical_path("1", &[])],
    ));
    assert_conformant(&check(&file(empty, BASELINE)));
    let first_chunk_only = icc_segments(&icc, 2);
    let first_chunk_only = &first_chunk_only[..4 + 14 + 151];
    assert_violation(
        &check(&object(insert(&grey_jpeg(), 2, first_chunk_only))),
        "its 1 APP2 ICC_PROFILE segments are not one complete chunk sequence of 2 (ISO 15076-1 Annex B)",
    );
}

#[test]
fn each_frame_is_compared_with_the_icc_profile_of_its_optical_path() {
    let (first, second) = (profile(3), profile(7));
    let paths = || {
        sequence(
            tags::OPTICAL_PATH_SEQUENCE,
            vec![optical_path("A", &first), optical_path("B", &second)],
        )
    };
    let frames =
        |a: &[u8], b: &[u8]| vec![with_icc(&grey_jpeg(), a, 1), with_icc(&grey_jpeg(), b, 1)];

    // Per-Frame Functional Groups identify each Frame's optical path.
    let identified = |frames: Vec<Vec<u8>>| {
        let mut data_set = image((8, 16), 1, "MONOCHROME2", frames);
        data_set.put(paths());
        data_set.put(sequence(
            tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
            vec![identifying("B"), identifying("A")],
        ));
        check(&file(data_set, BASELINE))
    };
    assert_conformant(&identified(frames(&second, &first)));
    let swapped = identified(frames(&first, &second));
    assert_violation(&swapped, "Frame 1: APP2 ICC_PROFILE segments concatenate to a 301-byte profile that is not identical to the 301-byte ICC Profile (0028,2000) of Optical Path Sequence Item 2");
    assert_violation(&swapped, "Frame 2: APP2 ICC_PROFILE segments concatenate to a 301-byte profile that is not identical to the 301-byte ICC Profile (0028,2000) of Optical Path Sequence Item 1");

    // TILED_FULL orders Frames by optical path after the tiles of each.
    let tiled_full = |frames: Vec<Vec<u8>>| {
        let mut data_set = image((8, 16), 1, "MONOCHROME2", frames);
        data_set.put(paths());
        data_set.put(text(
            tags::DIMENSION_ORGANIZATION_TYPE,
            VR::CS,
            "TILED_FULL",
        ));
        data_set.put(DataElement::new(
            tags::TOTAL_PIXEL_MATRIX_COLUMNS,
            VR::UL,
            PrimitiveValue::from(16u32),
        ));
        data_set.put(DataElement::new(
            tags::TOTAL_PIXEL_MATRIX_ROWS,
            VR::UL,
            PrimitiveValue::from(8u32),
        ));
        check(&file(data_set, BASELINE))
    };
    assert_conformant(&tiled_full(frames(&first, &second)));
    assert_violation(
        &tiled_full(frames(&second, &first)),
        "Frame 2: APP2 ICC_PROFILE segments concatenate to a 301-byte profile that is not identical to the 301-byte ICC Profile (0028,2000) of Optical Path Sequence Item 2",
    );
}
