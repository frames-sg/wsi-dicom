//! Unit tests for the pixel rules on minimal in-memory objects; tests that
//! validate exported files live in src/validation/tests/.

use dicom_core::value::DataSetSequence;
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::mem::InMemElement;
use dicom_object::{DefaultDicomObject, FileMetaTableBuilder, InMemDicomObject};

use super::instance::{validate_icc_rule, validate_image_metadata_rule};
use super::nested::Nested;
use super::pixel_attributes::validate_pixel_attributes_rule;
use super::VL_WSI;

fn unsigned(tag: Tag, value: u16) -> InMemElement {
    DataElement::new(tag, VR::US, PrimitiveValue::from(value))
}

fn unsigned_values(tag: Tag, values: &[u16]) -> InMemElement {
    DataElement::new(
        tag,
        VR::US,
        PrimitiveValue::U16(values.iter().copied().collect()),
    )
}

/// The pixel attributes rule, with the walk of the object's nested Items
/// the dispatch shares.
fn pixel_attributes_rule(object: &DefaultDicomObject, iod: &str) -> Result<(), String> {
    validate_pixel_attributes_rule(object, iod, &Nested::of(object))
}

fn icc_rule(object: &DefaultDicomObject, iod: &str) -> Result<(), String> {
    validate_icc_rule(object, iod, &Nested::of(object))
}

fn text(tag: Tag, vr: VR, value: &str) -> InMemElement {
    DataElement::new(tag, vr, PrimitiveValue::from(value))
}

fn integer_strings(tag: Tag, values: &[&str]) -> InMemElement {
    DataElement::new(
        tag,
        VR::IS,
        PrimitiveValue::Strs(values.iter().map(|value| value.to_string()).collect()),
    )
}

fn words(tag: Tag, values: &[u16]) -> InMemElement {
    DataElement::new(
        tag,
        VR::OW,
        PrimitiveValue::U16(values.iter().copied().collect()),
    )
}

fn bytes(tag: Tag, vr: VR, values: Vec<u8>) -> InMemElement {
    DataElement::new(tag, vr, PrimitiveValue::U8(values.into()))
}

fn sequence(tag: Tag, items: Vec<InMemDicomObject>) -> InMemElement {
    DataElement::new(tag, VR::SQ, DataSetSequence::from(items))
}

fn item(elements: Vec<InMemElement>) -> InMemDicomObject {
    InMemDicomObject::from_element_iter(elements)
}

fn object(transfer_syntax: &str, elements: Vec<InMemElement>) -> DefaultDicomObject {
    item(elements)
        .with_meta(
            FileMetaTableBuilder::new()
                .media_storage_sop_class_uid(crate::VL_WSI_SOP_CLASS_UID)
                .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.999.900")
                .transfer_syntax(transfer_syntax),
        )
        .expect("file meta")
}

/// The top-level sample format of a VL WSI image.
fn pixels(samples: u16, photometric: &str, bits: u16) -> Vec<InMemElement> {
    let mut elements = vec![
        unsigned(tags::SAMPLES_PER_PIXEL, samples),
        text(tags::PHOTOMETRIC_INTERPRETATION, VR::CS, photometric),
        unsigned(tags::ROWS, 16),
        unsigned(tags::COLUMNS, 16),
        unsigned(tags::BITS_ALLOCATED, bits),
        unsigned(tags::BITS_STORED, bits),
        unsigned(tags::HIGH_BIT, bits - 1),
        unsigned(tags::PIXEL_REPRESENTATION, 0),
    ];
    if samples == 3 {
        elements.push(unsigned(tags::PLANAR_CONFIGURATION, 0));
    }
    elements
}

fn image(transfer_syntax: &str, photometric: &str, bits: u16) -> DefaultDicomObject {
    let samples = if photometric == "MONOCHROME2" { 1 } else { 3 };
    object(transfer_syntax, pixels(samples, photometric, bits))
}

#[track_caller]
fn metadata_error(object: &DefaultDicomObject, expected: &str) {
    let error = validate_image_metadata_rule(object, VL_WSI).expect_err("violation");
    assert!(error.contains(expected), "{error}");
}

#[track_caller]
fn attributes_error(object: &DefaultDicomObject, expected: &str) {
    let error = pixel_attributes_rule(object, VL_WSI).expect_err("violation");
    assert!(error.contains(expected), "{error}");
}

#[test]
fn jpeg_extended_admits_only_eight_bit_monochrome() {
    for photometric in ["YBR_FULL_422", "RGB"] {
        metadata_error(
            &image(uids::JPEG_EXTENDED12_BIT, photometric, 8),
            &format!("Photometric Interpretation {photometric} is not permitted with Transfer Syntax 1.2.840.10008.1.2.4.51"),
        );
    }
    metadata_error(
        &image(uids::JPEG_EXTENDED12_BIT, "MONOCHROME2", 16),
        "16-bit form (Bits Stored 12, High Bit 11) is excluded",
    );
    validate_image_metadata_rule(&image(uids::JPEG_EXTENDED12_BIT, "MONOCHROME2", 8), VL_WSI)
        .unwrap();
    for photometric in ["YBR_FULL_422", "RGB", "MONOCHROME2"] {
        validate_image_metadata_rule(&image(uids::JPEG_BASELINE8_BIT, photometric, 8), VL_WSI)
            .unwrap();
    }
    metadata_error(
        &image(uids::JPEG_BASELINE8_BIT, "RGB", 16),
        "admits Bits Allocated 8, Bits Stored 8, High Bit 7",
    );
}

#[test]
fn lossless_syntaxes_without_color_transform_admit_monochrome_or_rgb() {
    for syntax in [
        uids::EXPLICIT_VR_LITTLE_ENDIAN,
        uids::ENCAPSULATED_UNCOMPRESSED_EXPLICIT_VR_LITTLE_ENDIAN,
        uids::DEFLATED_IMAGE_FRAME_COMPRESSION,
        uids::RLE_LOSSLESS,
        uids::JPEG_LOSSLESS_SV1,
        uids::JPEGLS_LOSSLESS,
    ] {
        validate_image_metadata_rule(&image(syntax, "RGB", 16), VL_WSI).unwrap();
        validate_image_metadata_rule(&image(syntax, "MONOCHROME2", 8), VL_WSI).unwrap();
        metadata_error(&image(syntax, "YBR_FULL_422", 8), "is not permitted");
        metadata_error(&image(syntax, "YBR_RCT", 8), "is not permitted");
    }
}

#[test]
fn jpeg_xl_follows_table_8_2_15_1() {
    for syntax in [uids::JPEGXL_LOSSLESS, uids::JPEGXL] {
        for photometric in ["MONOCHROME2", "RGB", "YBR_RCT"] {
            validate_image_metadata_rule(&image(syntax, photometric, 16), VL_WSI).unwrap();
        }
        for photometric in ["YBR_FULL_422", "YBR_ICT"] {
            metadata_error(&image(syntax, photometric, 8), "Table 8.2.15-1");
        }
    }
    for photometric in ["MONOCHROME2", "RGB", "YBR_FULL_422"] {
        validate_image_metadata_rule(
            &image(uids::JPEGXLJPEG_RECOMPRESSION, photometric, 8),
            VL_WSI,
        )
        .unwrap();
        metadata_error(
            &image(uids::JPEGXLJPEG_RECOMPRESSION, photometric, 16),
            "admits Bits Allocated 8, Bits Stored 8, High Bit 7",
        );
    }
    metadata_error(
        &image(uids::JPEGXLJPEG_RECOMPRESSION, "YBR_RCT", 8),
        "is not permitted",
    );
}

#[test]
fn jpip_referenced_syntaxes_admit_monochrome_ict_and_rct() {
    for syntax in [
        uids::JPIP_REFERENCED,
        uids::JPIP_REFERENCED_DEFLATE,
        uids::JPIPHTJ2K_REFERENCED,
        uids::JPIPHTJ2K_REFERENCED_DEFLATE,
    ] {
        for photometric in ["MONOCHROME2", "YBR_ICT", "YBR_RCT"] {
            validate_image_metadata_rule(&image(syntax, photometric, 8), VL_WSI).unwrap();
        }
        metadata_error(&image(syntax, "RGB", 8), "MONOCHROME2, YBR_ICT, YBR_RCT");
    }
}

#[test]
fn monochrome_mpeg2_metadata_is_permitted_for_wsi() {
    for syntax in [uids::MPEG2MPML, uids::MPEG2MPMLF] {
        validate_image_metadata_rule(&image(syntax, "MONOCHROME2", 8), VL_WSI).unwrap();
    }
    for syntax in [uids::MPEG2MPHL, uids::MPEG2MPHLF] {
        let mut attributes = pixels(1, "MONOCHROME2", 8);
        attributes.extend([unsigned(tags::ROWS, 720), unsigned(tags::COLUMNS, 1280)]);
        validate_image_metadata_rule(&object(syntax, attributes), VL_WSI).unwrap();
    }
    let mut attributes = pixels(1, "MONOCHROME2", 8);
    attributes.extend([
        unsigned(tags::ROWS, 1080),
        unsigned(tags::COLUMNS, 1920),
        text(tags::CINE_RATE, VR::IS, "30"),
    ]);
    let mut full_hd = object(uids::MPEG2MPHL, attributes);
    validate_image_metadata_rule(&full_hd, VL_WSI).unwrap();
    full_hd.put(text(tags::CINE_RATE, VR::IS, "60"));
    metadata_error(&full_hd, "1080 Rows permit only 25 or 30 frames per second");
}

#[test]
fn motion_syntaxes_are_rejected_with_their_attribute_departures() {
    for syntax in [
        uids::MPEG2MPML,
        uids::MPEG2MPMLF,
        uids::MPEG2MPHL,
        uids::MPEG2MPHLF,
        uids::MPEG4HP41,
        uids::MPEG4HP41F,
        uids::MPEG4HP41BD,
        uids::MPEG4HP41BDF,
        uids::MPEG4HP422D,
        uids::MPEG4HP422DF,
        uids::MPEG4HP423D,
        uids::MPEG4HP423DF,
        uids::MPEG4HP42STEREO,
        uids::MPEG4HP42STEREOF,
        uids::HEVCMP51,
        uids::HEVCM10P51,
    ] {
        let error =
            validate_image_metadata_rule(&image(syntax, "RGB", 8), VL_WSI).expect_err(syntax);
        assert!(
            error.contains("is a motion compression syntax")
                && error.contains("Photometric Interpretation \"RGB\" (shall be YBR_PARTIAL_420"),
            "{error}"
        );
    }
    // The multi-component MPEG2 format requires YBR_PARTIAL_420, which WSI excludes.
    let mut conformant = pixels(3, "YBR_PARTIAL_420", 8);
    conformant.extend([unsigned(tags::ROWS, 1080), unsigned(tags::COLUMNS, 1920)]);
    let error =
        validate_image_metadata_rule(&object(uids::MPEG2MPHL, conformant), VL_WSI).unwrap_err();
    assert!(
        error.contains("motion compression syntax") && !error.contains("departs"),
        "{error}"
    );

    metadata_error(
        &image(uids::MPEG2MPML, "MONOCHROME2", 16),
        "Bits Allocated 16 (shall be 8)",
    );
    metadata_error(
        &image(uids::HEVCM10P51, "YBR_PARTIAL_420", 16),
        "Bits Stored 16 (shall be 10), High Bit 15 (shall be 9)",
    );
    metadata_error(
        &image(uids::MPEG2MPHL, "YBR_PARTIAL_420", 8),
        "Rows 16 and Columns 16 (shall be 720 and 1280, or 1080 and 1920)",
    );
    metadata_error(
        &image(uids::MPEG4HP41BD, "YBR_PARTIAL_420", 8),
        "Rows 16 and Columns 16 (1080 and 1920, or 720 and 1280, Table 8-4)",
    );
    let mut large = pixels(3, "YBR_PARTIAL_420", 8);
    large.push(unsigned(tags::ROWS, 600));
    metadata_error(
        &object(uids::MPEG2MPML, large),
        "Rows 600 (at most 576, Table 8-1)",
    );
    // Table 8-1 bounds Rows by the frame rate: 480 at 30 frames per second.
    let mut ntsc = pixels(3, "YBR_PARTIAL_420", 8);
    ntsc.extend([
        unsigned(tags::ROWS, 576),
        text(tags::CINE_RATE, VR::IS, "30"),
    ]);
    metadata_error(
        &object(uids::MPEG2MPML, ntsc),
        "Rows 576 at 30.000 frames per second (at most 480, Table 8-1)",
    );
    // Table 8-4 pairs each format with its frame rates.
    let mut bd = pixels(3, "YBR_PARTIAL_420", 8);
    bd.extend([
        unsigned(tags::ROWS, 1080),
        unsigned(tags::COLUMNS, 1920),
        text(tags::FRAME_TIME, VR::DS, "20"),
    ]);
    metadata_error(
        &object(uids::MPEG4HP41BD, bd),
        "1080 by 1920 at frame rate 50.000 per second (not a format of Table 8-4)",
    );
    // Level 4.1 holds 8192 macroblocks per frame and 245760 per second.
    let mut level = pixels(3, "YBR_PARTIAL_420", 8);
    level.extend([unsigned(tags::ROWS, 2160), unsigned(tags::COLUMNS, 3840)]);
    metadata_error(
        &object(uids::MPEG4HP41, level),
        "Rows 2160 and Columns 3840 (beyond the frame size of MPEG-4 AVC/H.264 Level 4.1",
    );
    let mut rate = pixels(3, "YBR_PARTIAL_420", 8);
    rate.extend([
        unsigned(tags::ROWS, 1080),
        unsigned(tags::COLUMNS, 1920),
        text(tags::CINE_RATE, VR::IS, "60"),
    ]);
    metadata_error(
        &object(uids::MPEG4HP41, rate),
        "1080 by 1920 at frame rate 60.000 per second (beyond the throughput of MPEG-4 AVC/H.264 Level 4.1",
    );
    let mut aspect = pixels(3, "YBR_PARTIAL_420", 8);
    aspect.push(integer_strings(tags::PIXEL_ASPECT_RATIO, &["4", "3"]));
    for syntax in [uids::MPEG4HP41, uids::MPEG4HP422D, uids::HEVCMP51] {
        metadata_error(
            &object(syntax, aspect.clone()),
            "Pixel Aspect Ratio present (shall be absent)",
        );
    }
    let mut stereo = pixels(3, "YBR_PARTIAL_420", 8);
    stereo.push(text(tags::STEREO_PAIRS_PRESENT, VR::CS, "YES"));
    metadata_error(
        &object(uids::MPEG4HP422D, stereo),
        "Stereo Pairs Present YES (shall be NO or absent)",
    );
    for syntax in [uids::MPEG4HP423D, uids::MPEG4HP42STEREO] {
        metadata_error(
            &image(syntax, "YBR_PARTIAL_420", 8),
            "Stereo Pairs Present absent (shall be YES)",
        );
    }
}

fn wsi_with(elements: Vec<InMemElement>) -> DefaultDicomObject {
    let mut all = pixels(1, "MONOCHROME2", 8);
    all.extend(elements);
    object(uids::EXPLICIT_VR_LITTLE_ENDIAN, all)
}

fn icon(elements: Vec<InMemElement>) -> DefaultDicomObject {
    wsi_with(vec![sequence(
        tags::ICON_IMAGE_SEQUENCE,
        vec![item(icon_fields(elements))],
    )])
}

/// A DICOMDIR whose one IMAGE Directory Record has an Icon Image of
/// `elements` over a valid 2x3 monochrome icon.
fn record_icon(elements: Vec<InMemElement>) -> DefaultDicomObject {
    let record = item(vec![
        text(tags::DIRECTORY_RECORD_TYPE, VR::CS, "IMAGE"),
        sequence(tags::ICON_IMAGE_SEQUENCE, vec![item(icon_fields(elements))]),
    ]);
    object(
        uids::EXPLICIT_VR_LITTLE_ENDIAN,
        vec![sequence(tags::DIRECTORY_RECORD_SEQUENCE, vec![record])],
    )
}

/// A valid 2x3 monochrome icon with `elements` replacing or adding to it.
fn icon_fields(elements: Vec<InMemElement>) -> Vec<InMemElement> {
    let mut fields = vec![
        unsigned(tags::SAMPLES_PER_PIXEL, 1),
        text(tags::PHOTOMETRIC_INTERPRETATION, VR::CS, "MONOCHROME2"),
        unsigned(tags::ROWS, 2),
        unsigned(tags::COLUMNS, 3),
        unsigned(tags::BITS_ALLOCATED, 8),
        unsigned(tags::BITS_STORED, 8),
        unsigned(tags::HIGH_BIT, 7),
        unsigned(tags::PIXEL_REPRESENTATION, 0),
        bytes(tags::PIXEL_DATA, VR::OB, vec![0; 6]),
    ];
    for element in elements {
        fields.retain(|field| field.header().tag != element.header().tag);
        fields.push(element);
    }
    fields
}

#[test]
fn icon_images_follow_the_icon_restrictions() {
    pixel_attributes_rule(&icon(Vec::new()), VL_WSI).unwrap();
    for (element, expected) in [
        (
            unsigned(tags::SAMPLES_PER_PIXEL, 3),
            "Samples per Pixel is 3; Icon Images have a single sample per pixel",
        ),
        (
            text(tags::PHOTOMETRIC_INTERPRETATION, VR::CS, "RGB"),
            "Photometric Interpretation is RGB; Icon Images are MONOCHROME1, MONOCHROME2 or PALETTE COLOR",
        ),
        (
            unsigned(tags::PLANAR_CONFIGURATION, 0),
            "Planar Configuration (0028,0006) is present",
        ),
        (
            unsigned(tags::BITS_ALLOCATED, 16),
            "Bits Allocated is 16; Icon Image pixel samples have 1 or 8 bits",
        ),
        (
            unsigned(tags::BITS_STORED, 7),
            "Bits Stored is 7; Icon Image pixel samples have 1 or 8 bits",
        ),
        (
            unsigned(tags::HIGH_BIT, 8),
            "High Bit is 8; it shall be one less than Bits Stored 8",
        ),
        (
            unsigned(tags::PIXEL_REPRESENTATION, 1),
            "Pixel Representation is 1; Icon Images are unsigned",
        ),
        (
            integer_strings(tags::PIXEL_ASPECT_RATIO, &["1", "1"]),
            "Pixel Aspect Ratio (0028,0034) is present; Icon Image pixels are square",
        ),
        (
            bytes(tags::PIXEL_DATA, VR::OB, vec![0; 4]),
            "Pixel Data (7FE0,0010) holds 4 bytes; Native Format requires 6 bytes for 2 Rows x 3 Columns",
        ),
    ] {
        attributes_error(&icon(vec![element]), expected);
    }
    // A bitmap icon: Bits Allocated 1 packs six pixels in one byte, padded to two.
    pixel_attributes_rule(
        &icon(vec![
            unsigned(tags::BITS_ALLOCATED, 1),
            unsigned(tags::BITS_STORED, 1),
            unsigned(tags::HIGH_BIT, 0),
            bytes(tags::PIXEL_DATA, VR::OB, vec![0; 2]),
        ]),
        VL_WSI,
    )
    .unwrap();
}

/// PS3.3 F.7 restricts the Icon Image of a Directory Record in place of
/// C.7.6.1.1.6 and admits Pixel Aspect Ratio 1:1; the File-set check holds
/// F.7, and the Image Pixel Macro's own constraints still hold here.
#[test]
fn directory_record_icons_follow_annex_f_and_the_image_pixel_macro() {
    let file_set = super::FILE_SET;
    let square = integer_strings(tags::PIXEL_ASPECT_RATIO, &["1", "1"]);
    pixel_attributes_rule(&record_icon(vec![square.clone()]), file_set).unwrap();
    let error = pixel_attributes_rule(&icon(vec![square]), VL_WSI).unwrap_err();
    assert!(error.contains("Icon Image pixels are square"), "{error}");
    for (element, expected) in [
        (
            unsigned(tags::BITS_ALLOCATED, 12),
            "Bits Allocated is 12; it shall be either 1 or a multiple of 8 (PS3.3",
        ),
        (
            unsigned(tags::HIGH_BIT, 8),
            "High Bit is 8; it shall be one less than Bits Stored 8 (PS3.3",
        ),
        (
            integer_strings(tags::PIXEL_ASPECT_RATIO, &["0", "1"]),
            "Pixel Aspect Ratio",
        ),
        (
            bytes(tags::PIXEL_DATA, VR::OB, vec![0; 4]),
            "Pixel Data (7FE0,0010) holds 4 bytes; Native Format requires 6 bytes for 2 Rows x 3 Columns",
        ),
    ] {
        let error = pixel_attributes_rule(&record_icon(vec![element]), file_set)
            .expect_err("violation");
        assert!(
            error.contains("DirectoryRecordSequence (0004,1220)[1] > Icon Image Sequence (0088,0200) Item 1"),
            "{error}"
        );
        assert!(error.contains(expected), "{error}");
    }
}

fn palette_icon(descriptor: [u16; 3], entries: Vec<u8>) -> DefaultDicomObject {
    icon(vec![
        text(tags::PHOTOMETRIC_INTERPRETATION, VR::CS, "PALETTE COLOR"),
        unsigned_values(tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR, &descriptor),
        unsigned_values(
            tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
            &descriptor,
        ),
        unsigned_values(
            tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
            &descriptor,
        ),
        bytes(
            tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
            VR::OW,
            entries.clone(),
        ),
        bytes(
            tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
            VR::OW,
            entries.clone(),
        ),
        bytes(tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA, VR::OW, entries),
    ])
}

#[test]
fn palette_icons_need_eight_bit_samples_and_consistent_tables() {
    pixel_attributes_rule(&palette_icon([256, 0, 8], vec![0; 256]), VL_WSI).unwrap();
    pixel_attributes_rule(&palette_icon([256, 0, 16], vec![0; 512]), VL_WSI).unwrap();
    // 2^16 entries are encoded as 0.
    pixel_attributes_rule(&palette_icon([0, 0, 8], vec![0; 65_536]), VL_WSI).unwrap();
    attributes_error(
        &palette_icon([65_535, 0, 8], vec![0; 65_536]),
        "holds 65536 entries but the Red Palette Color Lookup Table Descriptor first Value is 65535; 2^16 entries are encoded as 0",
    );
    // 8-bit entries padded to 16 bits allocated, as some writers do.
    attributes_error(
        &palette_icon([256, 0, 8], vec![0; 512]),
        "holds 512 bytes; 256 entries stored as 8 bits allocated occupy 256 bytes",
    );
    attributes_error(
        &palette_icon([256, 0, 12], vec![0; 512]),
        "third Value is 12; it shall be 8 or 16",
    );
    let mut sixteen_bit = palette_icon([256, 0, 8], vec![0; 256]);
    let mut icons = sixteen_bit.take(tags::ICON_IMAGE_SEQUENCE).unwrap();
    let icon_item = &mut icons.items_mut().unwrap()[0];
    icon_item.put(unsigned(tags::BITS_ALLOCATED, 16));
    icon_item.put(unsigned_values(
        tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
        &[128, 1, 16],
    ));
    sixteen_bit.put(icons);
    let error = pixel_attributes_rule(&sixteen_bit, VL_WSI).unwrap_err();
    for expected in [
        "Bits Allocated is 16; an Icon Image with a Palette Color Lookup Table has 8",
        "the first Values of the Red, Green and Blue Palette Color Lookup Table Descriptors differ (Red 256, Green 128, Blue 256)",
        "the second Values of the Red, Green and Blue Palette Color Lookup Table Descriptors differ",
        "the third Values of the Red, Green and Blue Palette Color Lookup Table Descriptors differ",
    ] {
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

fn optical_path_palette(samples: u16, palette: Vec<InMemElement>) -> DefaultDicomObject {
    let photometric = if samples == 1 { "MONOCHROME2" } else { "RGB" };
    let mut elements = pixels(samples, photometric, 8);
    elements.push(sequence(
        tags::OPTICAL_PATH_SEQUENCE,
        vec![item(vec![sequence(
            tags::PALETTE_COLOR_LOOKUP_TABLE_SEQUENCE,
            vec![item(palette)],
        )])],
    ));
    object(uids::EXPLICIT_VR_LITTLE_ENDIAN, elements)
}

fn segmented_palette(descriptor: [u16; 3], segments: &[u16]) -> Vec<InMemElement> {
    vec![
        unsigned_values(tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR, &descriptor),
        unsigned_values(
            tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
            &descriptor,
        ),
        unsigned_values(
            tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
            &descriptor,
        ),
        words(
            tags::SEGMENTED_RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
            segments,
        ),
        words(
            tags::SEGMENTED_GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
            segments,
        ),
        words(
            tags::SEGMENTED_BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA,
            segments,
        ),
    ]
}

#[test]
fn optical_path_palettes_are_sixteen_bit_single_sample_tables() {
    let plain = |bits: u16, samples: u16| {
        let entries = vec![0u8; 256 * usize::from(bits / 8)];
        optical_path_palette(
            samples,
            vec![
                unsigned_values(
                    tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
                    &[256, 0, bits],
                ),
                unsigned_values(
                    tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
                    &[256, 0, bits],
                ),
                unsigned_values(
                    tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
                    &[256, 0, bits],
                ),
                bytes(
                    tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
                    VR::OW,
                    entries.clone(),
                ),
                bytes(
                    tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
                    VR::OW,
                    entries.clone(),
                ),
                bytes(tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA, VR::OW, entries),
            ],
        )
    };
    pixel_attributes_rule(&plain(16, 1), VL_WSI).unwrap();
    attributes_error(
        &plain(8, 1),
        "third Value is 8; a Palette Color Lookup Table Module in an Image IOD has 16-bit entries",
    );
    attributes_error(
        &plain(16, 3),
        "Palette Color Lookup Tables are used only with a single sample per pixel",
    );
    attributes_error(
        &optical_path_palette(3, segmented_palette([3, 0, 16], &[0, 3, 1, 2, 3])),
        "Palette Color Lookup Tables are used only with a single sample per pixel",
    );
}

#[test]
fn segmented_palettes_expand_to_the_declared_entries() {
    // Discrete 2 entries, linear 3, then an indirect copy of both: 10 entries.
    let segments = [0, 2, 0, 100, 1, 3, 400, 2, 2, 0, 0];
    pixel_attributes_rule(
        &optical_path_palette(1, segmented_palette([10, 0, 16], &segments)),
        VL_WSI,
    )
    .unwrap();
    for (descriptor, segments, expected) in [
        (
            [9, 0, 16],
            segments.as_slice(),
            "expands to 10 entries; the Red Palette Color Lookup Table Descriptor declares 9",
        ),
        (
            [4, 0, 16],
            &[0, 2, 0, 1, 2, 2, 0, 0, 2, 1, 8, 0][..],
            "copies another indirect segment",
        ),
        (
            [4, 0, 16],
            &[0, 2, 0, 1, 2, 1, 2, 0][..],
            "pointing to byte offset 2, which does not begin a segment",
        ),
        (
            [4, 0, 16],
            &[0, 2, 0, 1, 2, 3, 0, 0][..],
            "copying 3 segments from byte offset 0, beyond the last segment",
        ),
        ([4, 0, 16], &[1, 4, 9][..], "begins with a linear segment"),
        (
            [4, 0, 16],
            &[0, 5, 1, 2][..],
            "Segment Length exceeds the remaining data",
        ),
        ([4, 0, 16], &[0, 1, 7, 1, 3][..], "truncated linear segment"),
        (
            [4, 0, 16],
            &[0, 1, 7, 2, 1, 0][..],
            "truncated indirect segment",
        ),
        (
            [4, 0, 16],
            &[3, 4][..],
            "opcode 3 at byte 0; opcodes 3 and above are reserved",
        ),
    ] {
        attributes_error(
            &optical_path_palette(1, segmented_palette(descriptor, segments)),
            expected,
        );
    }
    // 2^16 expanded entries require a first descriptor Value of 0.
    let full = [0, 1, 0, 1, 65_535, 65_535];
    pixel_attributes_rule(
        &optical_path_palette(1, segmented_palette([0, 0, 16], &full)),
        VL_WSI,
    )
    .unwrap();
    attributes_error(
        &optical_path_palette(1, segmented_palette([65_535, 0, 16], &full)),
        "expands to 65536 entries but the Red Palette Color Lookup Table Descriptor first Value is 65535",
    );
}

#[test]
fn pixel_padding_lies_in_the_stored_range_with_ordered_limits() {
    let padding = |value: u16, limit: Option<u16>, photometric: &str| {
        let mut elements = vec![
            unsigned(tags::PIXEL_PADDING_VALUE, value),
            text(tags::PHOTOMETRIC_INTERPRETATION, VR::CS, photometric),
        ];
        elements.extend(limit.map(|limit| unsigned(tags::PIXEL_PADDING_RANGE_LIMIT, limit)));
        wsi_with(elements)
    };
    pixel_attributes_rule(&padding(0, Some(10), "MONOCHROME2"), VL_WSI).unwrap();
    pixel_attributes_rule(&padding(255, None, "MONOCHROME2"), VL_WSI).unwrap();
    attributes_error(
        &padding(256, None, "MONOCHROME2"),
        "Pixel Padding Value (0028,0120) 256 lies outside the stored sample range 0..=255",
    );
    attributes_error(
        &padding(0, Some(300), "MONOCHROME2"),
        "Pixel Padding Range Limit (0028,0121) 300 lies outside the stored sample range 0..=255",
    );
    attributes_error(
        &padding(10, Some(0), "MONOCHROME2"),
        "Pixel Padding Value (0028,0120) 10 and Pixel Padding Range Limit (0028,0121) 0 are in the wrong order for Photometric Interpretation MONOCHROME2",
    );
    pixel_attributes_rule(&padding(10, Some(0), "MONOCHROME1"), VL_WSI).unwrap();
}

#[test]
fn pixel_aspect_ratio_values_are_positive() {
    pixel_attributes_rule(
        &wsi_with(vec![integer_strings(tags::PIXEL_ASPECT_RATIO, &["6", "5"])]),
        VL_WSI,
    )
    .unwrap();
    attributes_error(
        &wsi_with(vec![integer_strings(tags::PIXEL_ASPECT_RATIO, &["0", "1"])]),
        "Pixel Aspect Ratio (0028,0034) is 0\\1; both integer Values shall be greater than 0",
    );
}

#[test]
fn icon_icc_profiles_follow_the_dicom_profile_constraints() {
    let srgb = crate::icc::synthetic_srgb_icc_profile().unwrap();
    let icc_icon = |profile: Vec<u8>| {
        let mut elements = pixels(1, "MONOCHROME2", 8);
        elements.push(sequence(
            tags::OPTICAL_PATH_SEQUENCE,
            vec![item(Vec::new())],
        ));
        elements.push(sequence(
            tags::ICON_IMAGE_SEQUENCE,
            vec![item(vec![bytes(tags::ICC_PROFILE, VR::OB, profile)])],
        ));
        object(uids::EXPLICIT_VR_LITTLE_ENDIAN, elements)
    };
    icc_rule(&icc_icon(srgb.clone()), VL_WSI).unwrap();
    for (range, value, expected) in [
        (12..16, b"mntr", "input device ('scnr')"),
        (16..20, b"GRAY", "input color space must be RGB"),
        (20..24, b"RGB ", "PCS must be XYZ or Lab"),
    ] {
        let mut profile = srgb.clone();
        profile[range].copy_from_slice(value);
        let error = icc_rule(&icc_icon(profile), VL_WSI).unwrap_err();
        assert!(
            error.contains("Icon Image Sequence item 0 has an invalid ICC Profile")
                && error.contains(expected),
            "{error}"
        );
    }
}

fn tag_value(tag: Tag, value: Tag) -> InMemElement {
    DataElement::new(tag, VR::AT, PrimitiveValue::Tags(vec![value].into()))
}

/// One Frame for each (Dimension Index Value, Z Offset in Slide Coordinate
/// System) of `frames`, the Attribute of the Plane Position (Slide) Functional
/// Group that the one Dimension Index Sequence Item indexes.
fn focal_planes(frames: &[(u32, &str)]) -> DefaultDicomObject {
    let frames: Vec<(u32, InMemElement)> = frames
        .iter()
        .map(|(index, depth)| (*index, z_offset(depth)))
        .collect();
    plane_positions(&frames, Vec::new(), Vec::new())
}

fn z_offset(depth: &str) -> InMemElement {
    text(tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, VR::DS, depth)
}

/// One Frame for each (Dimension Index Value, Plane Position (Slide)
/// Attribute) of `frames`; the one Dimension Index Sequence Item indexes that
/// Attribute. `shared` goes in the Shared Item and `top` at the top level.
fn plane_positions(
    frames: &[(u32, InMemElement)],
    shared: Vec<InMemElement>,
    top: Vec<InMemElement>,
) -> DefaultDicomObject {
    let pointer = frames[0].1.header().tag;
    let per_frame = frames
        .iter()
        .map(|(index, position)| {
            item(vec![
                sequence(
                    tags::FRAME_CONTENT_SEQUENCE,
                    vec![item(vec![DataElement::new(
                        tags::DIMENSION_INDEX_VALUES,
                        VR::UL,
                        PrimitiveValue::from(*index),
                    )])],
                ),
                sequence(
                    tags::PLANE_POSITION_SLIDE_SEQUENCE,
                    vec![item(vec![position.clone()])],
                ),
            ])
        })
        .collect();
    let dimension = item(vec![
        tag_value(tags::DIMENSION_INDEX_POINTER, pointer),
        tag_value(
            tags::FUNCTIONAL_GROUP_POINTER,
            tags::PLANE_POSITION_SLIDE_SEQUENCE,
        ),
    ]);
    let mut elements = vec![
        text(tags::NUMBER_OF_FRAMES, VR::IS, &frames.len().to_string()),
        sequence(tags::DIMENSION_INDEX_SEQUENCE, vec![dimension]),
        sequence(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, vec![item(shared)]),
        sequence(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE, per_frame),
    ];
    elements.extend(top);
    object(uids::EXPLICIT_VR_LITTLE_ENDIAN, elements)
}

#[test]
fn integer_index_values_are_compared_exactly() {
    use super::geometry::validate_dimension_rule;
    let column = |index: u32, position: i32| {
        (
            index,
            DataElement::new(
                tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
                VR::SL,
                PrimitiveValue::from(position),
            ),
        )
    };
    validate_dimension_rule(&plane_positions(
        &[column(1, 1), column(1, 1), column(2, 257)],
        Vec::new(),
        Vec::new(),
    ))
    .unwrap();
    // Column 2 is far from index 2's column 257, but a pixel position is
    // discrete: one pixel off is another Value.
    let error = validate_dimension_rule(&plane_positions(
        &[column(1, 1), column(1, 2), column(2, 257)],
        Vec::new(),
        Vec::new(),
    ))
    .unwrap_err();
    assert!(
        error.contains("Frames with index 1 have different Values, 1 and 2, for (0048,021E)"),
        "{error}"
    );
    // A lone index has nothing to compete with, and is compared exactly too.
    let error = validate_dimension_rule(&plane_positions(
        &[column(1, 1), column(1, 3)],
        Vec::new(),
        Vec::new(),
    ))
    .unwrap_err();
    assert!(
        error.contains("Frames with index 1 have different Values, 1 and 3,"),
        "{error}"
    );
}

#[test]
fn a_lone_focal_plane_spreads_only_within_the_imaged_volume_depth() {
    use super::geometry::validate_dimension_rule;
    let depth = || {
        vec![DataElement::new(
            tags::IMAGED_VOLUME_DEPTH,
            VR::FL,
            PrimitiveValue::from(4.0f32),
        )]
    };
    let lone = |depths: [&str; 2]| -> Vec<(u32, InMemElement)> {
        depths.iter().map(|value| (1, z_offset(value))).collect()
    };
    validate_dimension_rule(&plane_positions(
        &lone(["10.0", "12.5"]),
        Vec::new(),
        depth(),
    ))
    .unwrap();
    let error =
        validate_dimension_rule(&plane_positions(&lone(["10.0", "15"]), Vec::new(), depth()))
            .unwrap_err();
    assert!(
        error.contains("Frames with index 1 have Values from 10 to 15, which spread more than the Imaged Volume Depth (0048,0003), 4, although no other index competes with it, for (0040,074A)"),
        "{error}"
    );
    // Without a depth there is no scale, so the Values are compared exactly.
    validate_dimension_rule(&focal_planes(&[(1, "10.0"), (1, "10")])).unwrap();
    let error = validate_dimension_rule(&focal_planes(&[(1, "10.0"), (1, "10.5")])).unwrap_err();
    assert!(
        error.contains("Frames with index 1 have different Values, 10 and 10.5,"),
        "{error}"
    );
}

#[test]
fn a_lone_index_of_x_offsets_spreads_only_within_half_a_pixel() {
    use super::geometry::validate_dimension_rule;
    let measures = || {
        vec![sequence(
            tags::PIXEL_MEASURES_SEQUENCE,
            vec![item(vec![DataElement::new(
                tags::PIXEL_SPACING,
                VR::DS,
                PrimitiveValue::Strs(["0.0005", "0.00025"].map(String::from).into()),
            )])],
        )]
    };
    let lone = |offsets: [&str; 2]| -> Vec<(u32, InMemElement)> {
        offsets
            .iter()
            .map(|value| {
                (
                    1,
                    text(tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, VR::DS, value),
                )
            })
            .collect()
    };
    validate_dimension_rule(&plane_positions(
        &lone(["1.0", "1.0002"]),
        measures(),
        Vec::new(),
    ))
    .unwrap();
    let error = validate_dimension_rule(&plane_positions(
        &lone(["1.0", "1.0003"]),
        measures(),
        Vec::new(),
    ))
    .unwrap_err();
    assert!(
        error.contains("which spread more than half the Pixel Spacing (0028,0030), 0.00025,"),
        "{error}"
    );
}

#[test]
fn frames_of_one_index_may_spread_until_they_reach_another_index() {
    use super::geometry::validate_dimension_rule;
    // Each tile of a focal plane has its own focus: index 1 spreads around
    // 10 µm and index 2 around 20 µm, and no Frame comes near the other's.
    validate_dimension_rule(&focal_planes(&[
        (1, "10.0"),
        (1, "10.4"),
        (1, "9.7"),
        (2, "20.2"),
        (2, "19.9"),
    ]))
    .unwrap();
    // A Frame of index 1 at 14.9 µm is nearer index 1's typical 12.45 µm
    // than half the gap to index 2's 20 µm.
    validate_dimension_rule(&focal_planes(&[(1, "10"), (1, "14.9"), (2, "20")])).unwrap();
    // A Frame of index 1 at 16 µm is within half the 10 µm gap of index 2.
    let error =
        validate_dimension_rule(&focal_planes(&[(1, "10"), (1, "10"), (1, "16"), (2, "20")]))
            .unwrap_err();
    assert!(
        error.contains("a Frame with index 1 has Value 16, nearer to the typical Value 20 of index 2 than half the gap from the typical Value 10 of index 1"),
        "{error}"
    );
}
