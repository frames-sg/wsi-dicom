//! Pixel Data structure per Transfer Syntax, on files whose Pixel Data is
//! written byte by byte so that framing the parser tolerates can be encoded.

use super::*;
use crate::validation::ValidationCheck;
use dicom_dictionary_std::uids;
use dicom_object::mem::InMemElement;

const SECONDARY_CAPTURE: &str = "1.2.840.10008.5.1.4.1.1.7";

fn unsigned(tag: dicom_core::Tag, value: u16) -> InMemElement {
    DataElement::new(tag, VR::US, PrimitiveValue::from(value))
}

/// 1 x `columns` single-sample 8-bit Frames.
fn image_pixel(frames: u32, columns: u16) -> Vec<InMemElement> {
    vec![
        unsigned(tags::SAMPLES_PER_PIXEL, 1),
        DataElement::new(
            tags::PHOTOMETRIC_INTERPRETATION,
            VR::CS,
            PrimitiveValue::from("MONOCHROME2"),
        ),
        DataElement::new(
            tags::NUMBER_OF_FRAMES,
            VR::IS,
            PrimitiveValue::from(frames.to_string()),
        ),
        unsigned(tags::ROWS, 1),
        unsigned(tags::COLUMNS, columns),
        unsigned(tags::BITS_ALLOCATED, 8),
        unsigned(tags::BITS_STORED, 8),
        unsigned(tags::HIGH_BIT, 7),
        unsigned(tags::PIXEL_REPRESENTATION, 0),
    ]
}

/// Writes `elements` in `transfer_syntax`, then `tail` as encoded bytes.
fn write_raw(
    root: &Path,
    name: &str,
    transfer_syntax: &str,
    elements: Vec<InMemElement>,
    tail: &[u8],
) -> PathBuf {
    let mut object = InMemDicomObject::from_element_iter(elements);
    object.put(DataElement::new(
        tags::SOP_CLASS_UID,
        VR::UI,
        PrimitiveValue::from(SECONDARY_CAPTURE),
    ));
    object.put(DataElement::new(
        tags::SOP_INSTANCE_UID,
        VR::UI,
        PrimitiveValue::from("1.2.826.0.1.3680043.10.999.500"),
    ));
    let mut bytes = Vec::new();
    object
        .with_meta(
            FileMetaTableBuilder::new()
                .media_storage_sop_class_uid(SECONDARY_CAPTURE)
                .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.999.500")
                .transfer_syntax(transfer_syntax),
        )
        .expect("file meta")
        .write_all(&mut bytes)
        .expect("write data set");
    bytes.extend_from_slice(tail);
    let path = root.join(name);
    std::fs::write(&path, bytes).expect("write file");
    path
}

fn encapsulated_header(vr: &[u8; 2]) -> Vec<u8> {
    let mut bytes = vec![0xE0, 0x7F, 0x10, 0x00, vr[0], vr[1], 0, 0];
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes
}

fn item(payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0xFE, 0xFF, 0x00, 0xE0];
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn delimiter(length: u32) -> Vec<u8> {
    let mut bytes = vec![0xFE, 0xFF, 0xDD, 0xE0];
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes
}

fn offsets(values: &[u32]) -> Vec<u8> {
    item(
        &values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>(),
    )
}

/// OB encapsulated Pixel Data: the Basic Offset Table, the Fragments and
/// the Sequence Delimitation Item.
fn encapsulated(offset_table: &[u32], fragments: &[&[u8]]) -> Vec<u8> {
    let mut bytes = encapsulated_header(b"OB");
    bytes.extend(offsets(offset_table));
    for fragment in fragments {
        bytes.extend(item(fragment));
    }
    bytes.extend(delimiter(0));
    bytes
}

fn native(vr: &[u8; 2], value: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0xE0, 0x7F, 0x10, 0x00, vr[0], vr[1], 0, 0];
    bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
    bytes.extend_from_slice(value);
    bytes
}

fn pixel_structure(path: &Path) -> ValidationCheck {
    validate_dicom_path_with_runner(path, &ValidationOptions::default(), &FakeRunner::default())
        .expect("validation report")
        .checks
        .into_iter()
        .find(|check| check.name == "intrinsic-pixel-structure")
        .expect("pixel structure check")
}

#[track_caller]
fn assert_passes(path: &Path) {
    let check = pixel_structure(path);
    assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);
}

#[track_caller]
fn assert_fails(path: &Path, expected: &str) {
    let check = pixel_structure(path);
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
    assert!(check.message.contains(expected), "{}", check.message);
}

/// Framing only the encoded bytes show is the encoding check's: the parsed
/// Pixel Data passes, and the encoding check reports `expected`.
#[track_caller]
fn assert_encoding_fails(path: &Path, expected: &str) {
    assert_passes(path);
    super::encoding::assert_encoding_failure(path, expected);
}

const JPEG: &str = "1.2.840.10008.1.2.4.50";

#[test]
fn encapsulated_fragment_items_have_even_length_of_at_least_two() {
    let tmp = tempfile::tempdir().unwrap();
    let write = |name, fragments: &[&[u8]]| {
        write_raw(
            tmp.path(),
            name,
            JPEG,
            image_pixel(1, 1),
            &encapsulated(&[], fragments),
        )
    };
    assert_passes(&write("even.dcm", &[&[1, 2, 3, 4]]));
    assert_encoding_fails(
        &write("odd.dcm", &[&[1, 2, 3]]),
        "Fragment 1 has Item Length 3; Items containing an encoded Fragment shall be made of an even number of bytes greater or equal to two (PS3.5 A.4)",
    );
    // The parser drops empty Items, so only the encoded framing shows them.
    assert_encoding_fails(
        &write("empty.dcm", &[&[1, 2], &[]]),
        "Fragment 2 has Item Length 0",
    );
}

#[test]
fn encapsulated_pixel_data_is_ob_and_ends_with_a_zero_length_delimiter() {
    let tmp = tempfile::tempdir().unwrap();
    let fragment = item(&[1, 2, 3, 4]);
    let stream = |vr: &[u8; 2], end: &[u8]| {
        let mut bytes = encapsulated_header(vr);
        bytes.extend(offsets(&[]));
        bytes.extend_from_slice(&fragment);
        bytes.extend_from_slice(end);
        bytes
    };
    for syntax in [
        JPEG,
        uids::ENCAPSULATED_UNCOMPRESSED_EXPLICIT_VR_LITTLE_ENDIAN,
    ] {
        let elements = image_pixel(1, 3);
        assert_encoding_fails(
            &write_raw(
                tmp.path(),
                "ow.dcm",
                syntax,
                elements,
                &stream(b"OW", &delimiter(0)),
            ),
            "PixelData (7FE0,0010): encapsulated Pixel Data with VR OW; it shall be OB (PS3.5 A.4)",
        );
    }
    assert_encoding_fails(
        &write_raw(
            tmp.path(),
            "open.dcm",
            JPEG,
            image_pixel(1, 1),
            &stream(b"OB", &[]),
        ),
        "the encapsulated Pixel Data reaches the end of the Data Set without a Sequence Delimitation Item (FFFE,E0DD) (PS3.5 7.5.2 and A.4)",
    );
    assert_encoding_fails(
        &write_raw(
            tmp.path(),
            "long.dcm",
            JPEG,
            image_pixel(1, 1),
            &stream(b"OB", &delimiter(4)),
        ),
        "Sequence Delimitation Item (FFFE,E0DD) has Value Length 00000004H; it shall be 00000000H with no Value (PS3.5 7.5.2)",
    );
}

#[test]
fn bytes_after_the_last_data_element_fail() {
    let tmp = tempfile::tempdir().unwrap();
    let mut tail = encapsulated(&[], &[&[1, 2]]);
    tail.extend_from_slice(&[0, 0, 0]);
    assert_encoding_fails(
        &write_raw(tmp.path(), "encapsulated.dcm", JPEG, image_pixel(1, 1), &tail),
        "3 trailing bytes after the last Data Element are not a Data Element; a File contains a single Data Set (PS3.10 7.2)",
    );
    let mut tail = native(b"OB", &[1, 2]);
    tail.push(9);
    assert_encoding_fails(
        &write_raw(
            tmp.path(),
            "native.dcm",
            uids::EXPLICIT_VR_LITTLE_ENDIAN,
            image_pixel(1, 2),
            &tail,
        ),
        "1 trailing byte after the last Data Element",
    );
}

#[test]
fn one_fragment_per_frame_syntaxes_reject_frames_split_across_fragments() {
    let tmp = tempfile::tempdir().unwrap();
    for (syntax, section) in [
        (uids::RLE_LOSSLESS, "A.4.2"),
        (uids::HTJ2K_LOSSLESS, "A.4.4"),
        (uids::HTJ2K, "A.4.4"),
        (uids::JPEGXL_LOSSLESS, "A.4.12"),
        (uids::DEFLATED_IMAGE_FRAME_COMPRESSION, "A.4.13"),
    ] {
        let split = write_raw(
            tmp.path(),
            "split.dcm",
            syntax,
            image_pixel(2, 4),
            &encapsulated(&[0, 20], &[&[1, 2], &[3, 4], &[5, 6]]),
        );
        assert_fails(
            &split,
            &format!(
                "3 Fragments encode 2 Frames; each Frame shall be encoded in one and only one Fragment (PS3.5 2026c {section})"
            ),
        );
        let whole = write_raw(
            tmp.path(),
            "whole.dcm",
            syntax,
            image_pixel(2, 4),
            &encapsulated(&[0, 10], &[&[1, 2], &[3, 4]]),
        );
        assert_passes(&whole);
    }
    // JPEG lets a Frame span Fragments.
    assert_passes(&write_raw(
        tmp.path(),
        "jpeg.dcm",
        JPEG,
        image_pixel(2, 4),
        &encapsulated(&[0, 20], &[&[1, 2], &[3, 4], &[5, 6]]),
    ));
}

#[test]
fn encapsulated_uncompressed_fragments_hold_native_frames() {
    let tmp = tempfile::tempdir().unwrap();
    let syntax = uids::ENCAPSULATED_UNCOMPRESSED_EXPLICIT_VR_LITTLE_ENDIAN;
    // Two 1 x 3 Frames of 3 bytes, each padded to 4.
    assert_passes(&write_raw(
        tmp.path(),
        "frames.dcm",
        syntax,
        image_pixel(2, 3),
        &encapsulated(&[], &[&[1, 2, 3, 0], &[4, 5, 6, 0]]),
    ));
    assert_fails(
        &write_raw(
            tmp.path(),
            "short.dcm",
            syntax,
            image_pixel(2, 3),
            &encapsulated(&[], &[&[1, 2, 3, 0], &[4, 5]]),
        ),
        "Fragment 2 holds 2 bytes; each Fragment holds one Frame encoded as in Native Format, 3 bytes for 1 Rows x 3 Columns x 1 Samples per Pixel x 1 Frame(s) at 8 Bits Allocated padded to even length (PS3.5 2026c A.4.11)",
    );
    assert_fails(
        &write_raw(
            tmp.path(),
            "split.dcm",
            syntax,
            image_pixel(1, 3),
            &encapsulated(&[], &[&[1, 2], &[3, 0]]),
        ),
        "2 Fragments encode 1 Frames; each Frame shall be encoded in one and only one Fragment (PS3.5 2026c A.4.11)",
    );
    assert_fails(
        &write_raw(
            tmp.path(),
            "native.dcm",
            syntax,
            image_pixel(1, 2),
            &native(b"OB", &[1, 2]),
        ),
        "requires Pixel Data (7FE0,0010) in Encapsulated Format",
    );
}

#[test]
fn deflated_image_frames_are_encapsulated_one_per_frame() {
    let tmp = tempfile::tempdir().unwrap();
    let syntax = uids::DEFLATED_IMAGE_FRAME_COMPRESSION;
    assert_passes(&write_raw(
        tmp.path(),
        "frames.dcm",
        syntax,
        image_pixel(2, 64),
        &encapsulated(&[], &[&[0x63, 0x00], &[0x63, 0x60]]),
    ));
    assert_fails(
        &write_raw(
            tmp.path(),
            "native.dcm",
            syntax,
            image_pixel(1, 2),
            &native(b"OB", &[1, 2]),
        ),
        "requires Pixel Data (7FE0,0010) in Encapsulated Format",
    );
}

#[test]
fn extended_offset_table_needs_an_empty_basic_offset_table_and_whole_frames() {
    let tmp = tempfile::tempdir().unwrap();
    let with_table = |offsets: &[u64], lengths: &[u64]| {
        let mut elements = image_pixel(2, 4);
        elements.push(DataElement::new(
            tags::EXTENDED_OFFSET_TABLE,
            VR::OV,
            PrimitiveValue::U64(offsets.iter().copied().collect()),
        ));
        elements.push(DataElement::new(
            tags::EXTENDED_OFFSET_TABLE_LENGTHS,
            VR::OV,
            PrimitiveValue::U64(lengths.iter().copied().collect()),
        ));
        elements
    };
    assert_passes(&write_raw(
        tmp.path(),
        "eot.dcm",
        JPEG,
        with_table(&[0, 10], &[2, 2]),
        &encapsulated(&[], &[&[1, 2], &[3, 4]]),
    ));
    assert_fails(
        &write_raw(
            tmp.path(),
            "bot.dcm",
            JPEG,
            with_table(&[0, 10], &[2, 2]),
            &encapsulated(&[0, 10], &[&[1, 2], &[3, 4]]),
        ),
        "the Basic Offset Table holds 2 offsets although Extended Offset Table (7FE0,0001) is present; the Basic Offset Table Item shall then be empty",
    );
    assert_fails(
        &write_raw(
            tmp.path(),
            "span.dcm",
            JPEG,
            with_table(&[0, 20], &[4, 2]),
            &encapsulated(&[], &[&[1, 2], &[3, 4], &[5, 6]]),
        ),
        "Extended Offset Table (7FE0,0001) is present, but 3 Fragments encode 2 Frames; it addresses the one and only Fragment of every Frame (PS3.3 2026c C.7.6.3.1.8)",
    );
}

#[test]
fn a_missing_basic_offset_table_item_misaligns_the_frames() {
    let tmp = tempfile::tempdir().unwrap();
    // Two Frames written without the leading Basic Offset Table Item: the
    // first Fragment is read as the offset table and the Frames do not map.
    let mut tail = encapsulated_header(b"OB");
    tail.extend(item(&[0xFF, 0xD8, 0xFF, 0xD9]));
    tail.extend(item(&[0xFF, 0xD8, 0xFF, 0xD9]));
    tail.extend(delimiter(0));
    assert_fails(
        &write_raw(tmp.path(), "no-bot.dcm", JPEG, image_pixel(2, 4), &tail),
        "Pixel Data offset table has 1 entries for 2 frames",
    );
}

#[test]
fn video_streams_follow_their_fragment_and_offset_table_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let write = |syntax, frames, offset_table: &[u32], fragments: &[&[u8]]| {
        write_raw(
            tmp.path(),
            "video.dcm",
            syntax,
            image_pixel(frames, 4),
            &encapsulated(offset_table, fragments),
        )
    };
    // A stream of several Frames needs no Fragment per Frame.
    for syntax in [uids::MPEG2MPML, uids::MPEG4HP41, uids::HEVCMP51] {
        assert_passes(&write(syntax, 3, &[], &[&[0, 0, 1, 0xB3]]));
    }
    for (syntax, section) in [
        (uids::MPEG2MPML, "8.2.5"),
        (uids::MPEG2MPHL, "8.2.6"),
        (uids::MPEG4HP41, "8.2.7"),
        (uids::MPEG4HP422D, "8.2.8"),
        (uids::MPEG4HP42STEREO, "8.2.9"),
    ] {
        assert_fails(
            &write(syntax, 3, &[], &[&[0, 0], &[1, 0xB3]]),
            &format!(
                "Pixel Data holds 2 Fragments; for a Non-Fragmentable Transfer Syntax one Fragment shall contain the whole bit stream (PS3.5 2026c {section})"
            ),
        );
    }
    for syntax in [uids::MPEG2MPMLF, uids::MPEG4HP41F, uids::HEVCM10P51] {
        assert_passes(&write(syntax, 3, &[], &[&[0, 0], &[1, 0xB3]]));
    }
    for (syntax, section) in [(uids::MPEG2MPML, "8.2.5"), (uids::MPEG2MPHLF, "8.2.6")] {
        assert_fails(
            &write(syntax, 1, &[0], &[&[0, 0, 1, 0xB3]]),
            &format!(
                "the Basic Offset Table holds 1 offsets; it shall be empty (present but zero length) for this MPEG2 Transfer Syntax (PS3.5 2026c {section})"
            ),
        );
    }
}

#[test]
fn native_pixel_data_holds_exactly_the_frames_padded_to_even_length() {
    let tmp = tempfile::tempdir().unwrap();
    let explicit = uids::EXPLICIT_VR_LITTLE_ENDIAN;
    assert_passes(&write_raw(
        tmp.path(),
        "frames.dcm",
        explicit,
        image_pixel(2, 2),
        &native(b"OB", &[1, 2, 3, 4]),
    ));
    // Three 1 x 1 Frames are 3 bytes, padded to 4.
    assert_passes(&write_raw(
        tmp.path(),
        "padded.dcm",
        explicit,
        image_pixel(3, 1),
        &native(b"OB", &[1, 2, 3, 0]),
    ));
    for (name, value) in [
        ("short.dcm", &[1, 2][..]),
        ("long.dcm", &[1, 2, 3, 4, 5, 6][..]),
    ] {
        assert_fails(
            &write_raw(tmp.path(), name, explicit, image_pixel(2, 2), &native(b"OB", value)),
            &format!(
                "Pixel Data (7FE0,0010) holds {} bytes; Native Format requires 4 bytes for 1 Rows x 2 Columns x 1 Samples per Pixel x 2 Frame(s) at 8 Bits Allocated, padded only to the next even length (PS3.5 2026c 8.1.1)",
                value.len()
            ),
        );
    }
    let mut sixteen = image_pixel(1, 2);
    sixteen.extend([
        unsigned(tags::BITS_ALLOCATED, 16),
        unsigned(tags::BITS_STORED, 16),
        unsigned(tags::HIGH_BIT, 15),
    ]);
    assert_fails(
        &write_raw(tmp.path(), "ob16.dcm", explicit, sixteen.clone(), &native(b"OB", &[0; 4])),
        "Native Pixel Data (7FE0,0010) has VR OB with Bits Allocated 16; Bits Allocated greater than 8 requires OW (PS3.5 2026c A.2)",
    );
    assert_passes(&write_raw(
        tmp.path(),
        "ow16.dcm",
        explicit,
        sixteen,
        &native(b"OW", &[0; 4]),
    ));
    let mut implicit = vec![0xE0, 0x7F, 0x10, 0x00, 4, 0, 0, 0];
    implicit.extend_from_slice(&[1, 2, 3, 4]);
    assert_passes(&write_raw(
        tmp.path(),
        "implicit.dcm",
        uids::IMPLICIT_VR_LITTLE_ENDIAN,
        image_pixel(2, 2),
        &implicit,
    ));
}

#[test]
fn pixel_elements_follow_the_transfer_syntax() {
    let tmp = tempfile::tempdir().unwrap();
    let float = || {
        DataElement::new(
            tags::FLOAT_PIXEL_DATA,
            VR::OF,
            PrimitiveValue::F32(vec![0.0f32; 1].into()),
        )
    };
    let url = |value: &str| {
        DataElement::new(
            tags::PIXEL_DATA_PROVIDER_URL,
            VR::UR,
            PrimitiveValue::from(value),
        )
    };
    let with = |extra: Vec<InMemElement>| {
        let mut elements = image_pixel(1, 1);
        elements.extend(extra);
        elements
    };
    assert_fails(
        &write_raw(tmp.path(), "float.dcm", JPEG, with(vec![float()]), &[]),
        "Float Pixel Data (7FE0,0008) is present, but Encapsulated Transfer Syntax 1.2.840.10008.1.2.4.50 is used only with integer Pixel Data (7FE0,0010) in the top-level Data Set (PS3.5 2026c A.4)",
    );
    assert_fails(
        &write_raw(
            tmp.path(),
            "both.dcm",
            JPEG,
            with(vec![float()]),
            &encapsulated(&[], &[&[1, 2]]),
        ),
        "Pixel Data (7FE0,0010) and Float Pixel Data (7FE0,0008) are present together",
    );
    for syntax in [uids::JPIP_REFERENCED, uids::JPIPHTJ2K_REFERENCED] {
        assert_passes(&write_raw(
            tmp.path(),
            "jpip.dcm",
            syntax,
            with(vec![url("https://example.org/jpip?target=slide.jph")]),
            &[],
        ));
        assert_fails(
            &write_raw(
                tmp.path(),
                "ftp.dcm",
                syntax,
                with(vec![url("ftp://example.org/slide.jp2")]),
                &[],
            ),
            "does not use HTTP or HTTPS; the JPIP transport protocol shall be HTTP or HTTPS (PS3.5 2026c 8.4.1)",
        );
        assert_fails(
            &write_raw(tmp.path(), "no-url.dcm", syntax, with(Vec::new()), &[]),
            "Pixel Data Provider URL (0028,7FE0) is missing",
        );
        assert_fails(
            &write_raw(tmp.path(), "jpip-float.dcm", syntax, with(vec![float()]), &[]),
            &format!("Float Pixel Data (7FE0,0008) is present, but JPIP Referenced Transfer Syntax {syntax} references integer Pixel Data through Pixel Data Provider URL (0028,7FE0) instead"),
        );
        assert_fails(
            &write_raw(
                tmp.path(),
                "jpip-pixels.dcm",
                syntax,
                with(Vec::new()),
                &encapsulated(&[], &[&[1, 2]]),
            ),
            "Pixel Data (7FE0,0010) is present, but JPIP Referenced Transfer Syntax",
        );
    }
}

#[test]
fn data_sets_not_encoded_in_the_declared_transfer_syntax_fail() {
    let tmp = tempfile::tempdir().unwrap();
    // Explicit VR elements under Implicit VR Little Endian: the VR bytes
    // are read as part of a Value Length.
    let mut explicit = vec![0x08, 0x00, 0x08, 0x00, b'C', b'S', 0x0A, 0x00];
    explicit.extend_from_slice(b"ORIGINAL\\P");
    explicit.extend(native(b"OB", &[1, 2]));
    let path = write_raw(
        tmp.path(),
        "explicit.dcm",
        uids::IMPLICIT_VR_LITTLE_ENDIAN,
        Vec::new(),
        &explicit,
    );
    assert_eq!(pixel_structure(&path).status, ValidationStatus::Failed);
    // Big Endian tags and lengths under a Little Endian syntax.
    let mut swapped = vec![0x00, 0x28, 0x00, 0x10, b'U', b'S', 0x00, 0x02, 0x00, 0x01];
    swapped.extend_from_slice(&[0x7F, 0xE0, 0x00, 0x10, b'O', b'B', 0, 0, 0, 0, 0, 2, 1, 2]);
    for syntax in [
        uids::EXPLICIT_VR_LITTLE_ENDIAN,
        JPEG,
        uids::JPIP_REFERENCED,
        uids::JPIPHTJ2K_REFERENCED,
    ] {
        let path = write_raw(tmp.path(), "swapped.dcm", syntax, Vec::new(), &swapped);
        assert_eq!(
            pixel_structure(&path).status,
            ValidationStatus::Failed,
            "{syntax}"
        );
    }
}

#[test]
fn deflated_data_sets_inflate_and_undeflated_bytes_fail() {
    let tmp = tempfile::tempdir().unwrap();
    let mut elements = image_pixel(1, 2);
    elements.push(DataElement::new(
        tags::PIXEL_DATA,
        VR::OB,
        PrimitiveValue::from(vec![1u8, 2]),
    ));
    let deflated = write_raw(
        tmp.path(),
        "deflated.dcm",
        uids::DEFLATED_EXPLICIT_VR_LITTLE_ENDIAN,
        elements,
        &[],
    );
    assert_passes(&deflated);
    // Explicit VR Little Endian bytes after a File Meta group that declares
    // the deflated syntax: they are not an RFC 1951 stream.
    let meta = FileMetaTableBuilder::new()
        .media_storage_sop_class_uid(SECONDARY_CAPTURE)
        .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.999.501")
        .transfer_syntax(uids::DEFLATED_EXPLICIT_VR_LITTLE_ENDIAN)
        .build()
        .unwrap();
    let mut bytes = vec![0u8; 128];
    bytes.extend_from_slice(b"DICM");
    meta.write(&mut bytes).unwrap();
    bytes.extend(native(b"OB", &[1, 2]));
    let undeflated = tmp.path().join("undeflated.dcm");
    std::fs::write(&undeflated, bytes).unwrap();
    assert_eq!(
        pixel_structure(&undeflated).status,
        ValidationStatus::Failed
    );
}

#[test]
fn objects_without_image_pixels_or_multiple_frames_are_not_failed_for_them() {
    let tmp = tempfile::tempdir().unwrap();
    let write = |name, elements, tail: &[u8]| {
        write_raw(
            tmp.path(),
            name,
            uids::EXPLICIT_VR_LITTLE_ENDIAN,
            elements,
            tail,
        )
    };
    // An SR or RT object has no Image Pixel Module and no Pixel Data.
    assert_passes(&write(
        "document.dcm",
        vec![DataElement::new(
            tags::MODALITY,
            VR::CS,
            PrimitiveValue::from("SR"),
        )],
        &[],
    ));
    // A single-frame image has no Number of Frames.
    let mut single_frame = image_pixel(1, 2);
    single_frame.retain(|element| element.header().tag != tags::NUMBER_OF_FRAMES);
    assert_passes(&write(
        "single-frame.dcm",
        single_frame,
        &native(b"OB", &[0, 0]),
    ));
    assert_fails(
        &write("no-pixels.dcm", image_pixel(1, 2), &[]),
        "Pixel Data (7FE0,0010) is missing",
    );
}
