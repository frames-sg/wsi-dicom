use j2k::{
    J2kBlockCodingMode, J2kLosslessEncodeOptions, J2kLosslessSamples, J2kLossyEncodeOptions,
    J2kLossySamples, J2kMarkerSegment, J2kProgressionOrder, ReversibleTransform,
};

use super::{
    assert_conformant, assert_violation, check_fragments, check_frame, declared, find, insert,
    samples, Declared,
};

const PART1_LOSSLESS: &str = "1.2.840.10008.1.2.4.90";
const PART1: &str = "1.2.840.10008.1.2.4.91";
const PART2_LOSSLESS: &str = "1.2.840.10008.1.2.4.92";
const HTJ2K_LOSSLESS: &str = "1.2.840.10008.1.2.4.201";
const HTJ2K_RPCL: &str = "1.2.840.10008.1.2.4.202";
const HTJ2K: &str = "1.2.840.10008.1.2.4.203";

/// Offsets in SIZ (ISO/IEC 15444-1 A.5.1).
const RSIZ: usize = 6;
const COMPONENTS: usize = 42;

fn lossless(
    width: u32,
    height: u32,
    components: u16,
    options: J2kLosslessEncodeOptions,
) -> Vec<u8> {
    let data = samples(width, height, u32::from(components));
    let samples = J2kLosslessSamples::new(&data, width, height, components, 8, false)
        .expect("lossless samples");
    j2k::encode_j2k_lossless(samples, &options.with_cpu_only_backend())
        .expect("lossless JPEG 2000")
        .codestream
}

fn lossy(width: u32, height: u32, components: u16, mode: J2kBlockCodingMode) -> Vec<u8> {
    let data = samples(width, height, u32::from(components));
    let samples =
        J2kLossySamples::new(&data, width, height, components, 8, false).expect("lossy samples");
    let options = J2kLossyEncodeOptions::default()
        .with_cpu_only_backend()
        .with_block_coding_mode(mode);
    j2k::encode_j2k_lossy(samples, &options)
        .expect("lossy JPEG 2000")
        .codestream
}

/// A Part 1 codestream with the reversible color transformation.
pub(super) fn lossless_rgb(width: u32, height: u32) -> Vec<u8> {
    lossless(width, height, 3, J2kLosslessEncodeOptions::default())
}

fn high_throughput() -> J2kLosslessEncodeOptions {
    J2kLosslessEncodeOptions::default().with_block_coding_mode(J2kBlockCodingMode::HighThroughput)
}

fn rpcl() -> J2kLosslessEncodeOptions {
    high_throughput()
        .with_progression(J2kProgressionOrder::Rpcl)
        .with_marker_segments(&[J2kMarkerSegment::Tlm])
}

fn untransformed(mut options: J2kLosslessEncodeOptions) -> J2kLosslessEncodeOptions {
    options.reversible_transform = ReversibleTransform::None53;
    options
}

fn color(syntax: &str, (rows, columns): (u32, u32), photometric: &str) -> Declared {
    declared(syntax, (rows, columns), 3, 8, photometric)
}

/// Offset of a main header marker, which precedes the first SOT.
fn main_marker(codestream: &[u8], marker: u16) -> usize {
    let tile = find(codestream, &[0xFF, 0x90]);
    find(&codestream[..tile], &marker.to_be_bytes())
}

/// `codestream` with `segment` inserted before its first SOT.
fn with_main_segment(codestream: &[u8], segment: &[u8]) -> Vec<u8> {
    insert(codestream, find(codestream, &[0xFF, 0x90]), segment)
}

fn with_rsiz(codestream: &[u8], rsiz: u16) -> Vec<u8> {
    let mut patched = codestream.to_vec();
    patched[RSIZ..RSIZ + 2].copy_from_slice(&rsiz.to_be_bytes());
    patched
}

#[test]
fn encoder_codestreams_are_conformant_for_their_transfer_syntaxes() {
    let rct = lossless_rgb(16, 8);
    assert_conformant(&check_frame(
        &rct,
        &color(PART1_LOSSLESS, (8, 16), "YBR_RCT"),
    ));
    assert_conformant(&check_frame(&rct, &color(PART1, (8, 16), "YBR_RCT")));
    assert_conformant(&check_frame(
        &lossless(16, 8, 3, untransformed(J2kLosslessEncodeOptions::default())),
        &color(PART1_LOSSLESS, (8, 16), "RGB"),
    ));
    assert_conformant(&check_frame(
        &lossless(16, 8, 1, J2kLosslessEncodeOptions::default()),
        &declared(PART1_LOSSLESS, (8, 16), 1, 8, "MONOCHROME2"),
    ));
    assert_conformant(&check_frame(
        &lossy(16, 8, 3, J2kBlockCodingMode::Classic),
        &color(PART1, (8, 16), "YBR_ICT"),
    ));
    let ht = lossless(16, 8, 3, high_throughput());
    assert_conformant(&check_frame(
        &ht,
        &color(HTJ2K_LOSSLESS, (8, 16), "YBR_RCT"),
    ));
    assert_conformant(&check_frame(&ht, &color(HTJ2K, (8, 16), "YBR_RCT")));
    assert_conformant(&check_frame(
        &lossless(128, 128, 3, rpcl()),
        &color(HTJ2K_RPCL, (128, 128), "YBR_RCT"),
    ));
    assert_conformant(&check_frame(
        &lossy(16, 8, 3, J2kBlockCodingMode::HighThroughput),
        &color(HTJ2K, (8, 16), "YBR_ICT"),
    ));
    let tiled = lossless(
        64,
        64,
        3,
        J2kLosslessEncodeOptions::default().with_tile_size(Some((32, 32))),
    );
    assert_conformant(&check_frame(
        &tiled,
        &color(PART1_LOSSLESS, (64, 64), "YBR_RCT"),
    ));
    let mut padded = rct.clone();
    padded.push(0);
    assert_conformant(&check_frame(
        &padded,
        &color(PART1_LOSSLESS, (8, 16), "YBR_RCT"),
    ));
}

#[test]
fn image_and_tile_size_agrees_with_the_image_pixel_attributes() {
    let codestream = lossless_rgb(16, 8);
    for (declared, expected) in [
        (
            color(PART1_LOSSLESS, (16, 16), "YBR_RCT"),
            "Frame 1: SIZ height (Ysiz - YOsiz) is 8 but Rows (0028,0010) is 16 (PS3.5 2026c 8.2.4)",
        ),
        (
            color(PART1_LOSSLESS, (8, 8), "YBR_RCT"),
            "SIZ width (Xsiz - XOsiz) is 16 but Columns (0028,0011) is 8 (PS3.5 2026c 8.2.4)",
        ),
        (
            declared(PART1_LOSSLESS, (8, 16), 1, 8, "MONOCHROME2"),
            "SIZ Csiz is 3 but Samples per Pixel (0028,0002) is 1 (PS3.5 2026c 8.2.4)",
        ),
        (
            declared(PART1_LOSSLESS, (8, 16), 3, 12, "YBR_RCT"),
            "SIZ component 0 precision (Ssiz) is 8 but Bits Stored (0028,0101) is 12 (PS3.5 2026c 8.2.4)",
        ),
    ] {
        assert_violation(&check_frame(&codestream, &declared), expected);
    }
    let mut signed = color(PART1_LOSSLESS, (8, 16), "YBR_RCT");
    signed.pixel_representation.value = Some(1);
    assert_violation(
        &check_frame(&codestream, &signed),
        "SIZ component 0 sign (Ssiz) is 0 but Pixel Representation (0028,0103) is 1 (PS3.5 2026c 8.2.4)",
    );
    assert_violation(
        &check_frame(
            &lossless(16, 8, 3, high_throughput()),
            &color(HTJ2K_LOSSLESS, (8, 32), "YBR_RCT"),
        ),
        "SIZ width (Xsiz - XOsiz) is 16 but Columns (0028,0011) is 32 (PS3.5 2026c 8.2.14)",
    );
}

#[test]
fn all_components_share_size_precision_and_sign() {
    let codestream = lossless_rgb(16, 8);
    let declared = color(PART1_LOSSLESS, (8, 16), "YBR_RCT");
    let mut precision = codestream.clone();
    precision[COMPONENTS + 6] = 0x0B;
    assert_violation(
        &check_frame(&precision, &declared),
        "SIZ component precisions differ (8, 8, 12 bits), but every sample has the same Bits Allocated, Bits Stored and High Bit (PS3.3 2026c C.7-11c and PS3.3 2026c C.7.6.3.1.1)",
    );
    let mut sign = codestream.clone();
    sign[COMPONENTS + 3] |= 0x80;
    assert_violation(
        &check_frame(&sign, &declared),
        "SIZ component signs differ (0, 1, 0), but every sample has the same Pixel Representation (PS3.3 2026c C.7-11c and PS3.3 2026c C.7.6.3.1.1)",
    );
    let mut separation = codestream.clone();
    separation[COMPONENTS + 4] = 2;
    assert_violation(
        &check_frame(&separation, &declared),
        "SIZ component separations (1x1, 2x1, 1x1) encode image planes of different sizes, but all image planes have the same Rows and Columns (PS3.3 2026c C.7.6.3.1.1)",
    );
}

#[test]
fn multiple_component_transformation_matches_photometric_interpretation() {
    assert_violation(
        &check_frame(&lossless_rgb(16, 8), &color(PART1_LOSSLESS, (8, 16), "RGB")),
        "main header COD applies the Part 1 reversible (RCT) multiple component transformation, so Photometric Interpretation is YBR_RCT, not RGB (PS3.5 2026c 8.2.4)",
    );
    assert_violation(
        &check_frame(
            &lossy(16, 8, 3, J2kBlockCodingMode::Classic),
            &color(PART1, (8, 16), "YBR_RCT"),
        ),
        "main header COD applies the Part 1 irreversible (ICT) multiple component transformation, so Photometric Interpretation is YBR_ICT, not YBR_RCT (PS3.5 2026c 8.2.4)",
    );
    assert_violation(
        &check_frame(
            &lossless(16, 8, 3, untransformed(J2kLosslessEncodeOptions::default())),
            &color(PART1_LOSSLESS, (8, 16), "YBR_RCT"),
        ),
        "main header COD applies no multiple component transformation, so the components are not the YBR_RCT that Photometric Interpretation declares (PS3.5 2026c 8.2.4)",
    );
}

#[test]
fn htj2k_multiple_component_transformation_matches_photometric_interpretation() {
    assert_violation(
        &check_frame(
            &lossless(16, 8, 3, high_throughput()),
            &color(HTJ2K_LOSSLESS, (8, 16), "RGB"),
        ),
        "applies the Part 1 reversible (RCT) multiple component transformation, so Photometric Interpretation is YBR_RCT, not RGB (PS3.5 2026c 8.2.14)",
    );
    assert_violation(
        &check_frame(
            &lossy(16, 8, 3, J2kBlockCodingMode::HighThroughput),
            &color(HTJ2K, (8, 16), "RGB"),
        ),
        "applies the Part 1 irreversible (ICT) multiple component transformation, so Photometric Interpretation is YBR_ICT, not RGB (PS3.5 2026c 8.2.14)",
    );
    assert_violation(
        &check_frame(
            &lossless(16, 8, 3, untransformed(high_throughput())),
            &color(HTJ2K_LOSSLESS, (8, 16), "YBR_ICT"),
        ),
        "applies no multiple component transformation, so the components are not the YBR_ICT that Photometric Interpretation declares (PS3.5 2026c 8.2.14)",
    );
}

#[test]
fn lossless_transfer_syntaxes_use_the_reversible_path() {
    let irreversible = check_frame(
        &lossy(16, 8, 3, J2kBlockCodingMode::Classic),
        &color(PART1_LOSSLESS, (8, 16), "YBR_ICT"),
    );
    assert_violation(&irreversible, "main header COD selects the 9-7 irreversible wavelet transformation, but lossless Transfer Syntax 1.2.840.10008.1.2.4.90 uses the reversible 5-3 transformation (PS3.5 2026c A.4.4)");
    assert_violation(
        &irreversible,
        "quantizes, but lossless Transfer Syntax 1.2.840.10008.1.2.4.90 uses no quantization",
    );
    assert_violation(
        &check_frame(
            &lossy(16, 8, 3, J2kBlockCodingMode::HighThroughput),
            &color(HTJ2K_RPCL, (8, 16), "YBR_ICT"),
        ),
        "selects the 9-7 irreversible wavelet transformation, but lossless Transfer Syntax 1.2.840.10008.1.2.4.202 uses the reversible 5-3 transformation",
    );
}

#[test]
fn part1_transfer_syntaxes_admit_no_htj2k_or_part2_coding() {
    let ht = check_frame(
        &lossless(16, 8, 3, high_throughput()),
        &color(PART1_LOSSLESS, (8, 16), "YBR_RCT"),
    );
    assert_violation(&ht, "declares HTJ2K (ISO/IEC 15444-15) coding, which Transfer Syntax 1.2.840.10008.1.2.4.90 does not identify (PS3.5 2026c A.4.4)");
    assert_violation(
        &ht,
        "CAP marker segment declares HTJ2K (Part 15) capabilities",
    );
    assert_violation(&ht, "selects HT code-blocks (ISO/IEC 15444-15)");

    let codestream = lossless_rgb(16, 8);
    let declared = color(PART1, (8, 16), "YBR_RCT");
    assert_violation(
        &check_frame(&with_rsiz(&codestream, 0x8000), &declared),
        "SIZ Rsiz 8000H declares JPEG 2000 Part 2 capabilities, extensions that Part 1 decoders cannot decode, in Part 1 Transfer Syntax 1.2.840.10008.1.2.4.91 (PS3.5 2026c A.4.4)",
    );
    let offset = with_main_segment(&codestream, &[0xFF, 0x70, 0x00, 0x03, 0x00]);
    assert_violation(
        &check_frame(&offset, &declared),
        "main header DCO (FF70H) is a JPEG 2000 Part 2 extension that Transfer Syntax 1.2.840.10008.1.2.4.91 does not admit (PS3.5 2026c A.4.4)",
    );
    let mut array_transform = codestream.clone();
    array_transform[main_marker(&codestream, 0xFF52) + 8] = 2;
    assert_violation(
        &check_frame(&array_transform, &declared),
        "main header COD selects a Part 2 array-based multiple component transformation, which Transfer Syntax 1.2.840.10008.1.2.4.91 does not admit",
    );
}

#[test]
fn htj2k_transfer_syntaxes_identify_htj2k_codestreams() {
    let classic = check_frame(
        &lossless_rgb(16, 8),
        &color(HTJ2K_LOSSLESS, (8, 16), "YBR_RCT"),
    );
    assert_violation(&classic, "lacks the HTJ2K capability bit 14 of an ISO/IEC 15444-15 codestream, which Transfer Syntax 1.2.840.10008.1.2.4.201 identifies (PS3.5 2026c A.4.4)");
    assert_violation(
        &classic,
        "main header has no CAP marker segment declaring Part 15 capabilities",
    );
    assert_violation(
        &check_frame(
            &lossy(16, 8, 3, J2kBlockCodingMode::HighThroughput),
            &color(HTJ2K_LOSSLESS, (8, 16), "YBR_ICT"),
        ),
        "selects the 9-7 irreversible wavelet transformation, but lossless Transfer Syntax 1.2.840.10008.1.2.4.201",
    );
}

#[test]
fn part2_transfer_syntaxes_admit_only_annex_j_extensions() {
    let codestream = with_rsiz(&lossless_rgb(16, 8), 0x8100);
    let declared = color(PART2_LOSSLESS, (8, 16), "YBR_RCT");
    assert_conformant(&check_frame(&codestream, &declared));
    let annex_j = with_main_segment(
        &codestream,
        &[0xFF, 0x78, 0x00, 0x03, 0x00, 0xFF, 0x77, 0x00, 0x03, 0x00],
    );
    assert_conformant(&check_frame(&annex_j, &declared));
    assert_violation(
        &check_frame(&with_rsiz(&codestream, 0x8101), &declared),
        "SIZ Rsiz 8101H declares Part 2 extensions other than the Annex J multiple component transformations (bits 0001H) (PS3.5 2026c A.4.4)",
    );
    assert_violation(
        &check_frame(&with_main_segment(&codestream, &[0xFF, 0x79, 0x00, 0x03, 0x00]), &declared),
        "main header ATK (FF79H) belongs to a JPEG 2000 Part 2 Annex other than the Annex J multiple component transformations (PS3.5 2026c A.4.4)",
    );
}

#[test]
fn a_fragment_holds_one_component_collection() {
    let codestream = with_rsiz(&lossless_rgb(16, 8), 0x8100);
    let declared = color(PART2_LOSSLESS, (8, 16), "YBR_RCT");
    let collection = |index: u8| [0xFF, 0x75, 0x00, 0x05, 0x00, 0x00, index];
    let one = with_main_segment(&codestream, &collection(0));
    assert_conformant(&check_frame(&one, &declared));
    let two = with_main_segment(&one, &collection(1));
    assert_violation(
        &check_frame(&two, &declared),
        "MCC marker segments define 2 component collections in a codestream held by a single Fragment, but each Fragment contains data of one component collection (PS3.5 2026c A.4.4)",
    );
    let middle = two.len() / 2;
    assert_conformant(&check_fragments(
        &[&two[..middle], &two[middle..]],
        &declared,
    ));
}

#[test]
fn jp2_file_format_header_is_not_included() {
    let codestream = lossless_rgb(16, 8);
    let mut jp2 = vec![
        0x00, 0x00, 0x00, 0x0C, b'j', b'P', b' ', b' ', 0x0D, 0x0A, 0x87, 0x0A, 0x00, 0x00, 0x00,
        0x14, b'f', b't', b'y', b'p', b'j', b'p', b'2', b' ', 0x00, 0x00, 0x00, 0x00, b'j', b'p',
        b'2', b' ',
    ];
    let length = u32::try_from(codestream.len() + 8).expect("box length");
    jp2.extend_from_slice(&length.to_be_bytes());
    jp2.extend_from_slice(b"jp2c");
    jp2.extend_from_slice(&codestream);
    assert_violation(
        &check_frame(&jp2, &color(PART1_LOSSLESS, (8, 16), "YBR_RCT")),
        "begins with the JP2 signature box: the optional JP2 file format header shall not be included (PS3.5 2026c A.4.4)",
    );
}

#[test]
fn frames_hold_a_jpeg_2000_codestream() {
    let declared = color(PART1_LOSSLESS, (8, 16), "YBR_RCT");
    assert_violation(
        &check_frame(&crate::test_support::encode_test_jpeg(16, 8, [1, 2, 3]), &declared),
        "begins with bytes FF D8 FF DB (a JPEG stream: SOI), not the SOC and SIZ markers of the JPEG 2000 codestream Transfer Syntax 1.2.840.10008.1.2.4.90 identifies (PS3.10 2026c 7.2 and PS3.5 2026c A.4.4)",
    );
    let mut malformed = lossless_rgb(16, 8);
    malformed[5] += 3;
    assert_violation(
        &check_frame(&malformed, &declared),
        "SIZ marker segment is malformed",
    );
}

#[test]
fn each_frame_is_one_complete_separately_encoded_codestream() {
    let codestream = lossless_rgb(16, 8);
    let declared = color(PART1_LOSSLESS, (8, 16), "YBR_RCT");
    let doubled = [codestream.as_slice(), &codestream].concat();
    assert_violation(
        &check_frame(&doubled, &declared),
        &format!(
            "data follows EOC at byte {}, so the Frame holds more than one codestream (PS3.5 2026c A.4.4)",
            codestream.len() - 2
        ),
    );
    assert_violation(
        &check_frame(&codestream[..codestream.len() - 10], &declared),
        "tile-part length Psot",
    );
    assert_violation(
        &check_frame(&codestream[..codestream.len() - 2], &declared),
        "codestream ends without EOC (FFD9H), so the Frame is not one complete codestream",
    );
}

#[test]
fn htj2k_rpcl_codestreams_progress_by_resolution_with_tile_lengths() {
    let declared = color(HTJ2K_RPCL, (128, 128), "YBR_RCT");
    let layered = high_throughput().with_marker_segments(&[J2kMarkerSegment::Tlm]);
    assert_violation(
        &check_frame(&lossless(128, 128, 3, layered), &declared),
        "main header COD progression order LRCP is not RPCL, which HTJ2K Lossless RPCL requires (PS3.5 2026c 10.18.1)",
    );
    // The encoder writes TLM for RPCL; removing it leaves a decodable codestream.
    let with_tile_lengths = lossless(128, 128, 3, rpcl());
    let tile_lengths = main_marker(&with_tile_lengths, 0xFF55);
    let length = usize::from(u16::from_be_bytes([
        with_tile_lengths[tile_lengths + 2],
        with_tile_lengths[tile_lengths + 3],
    ]));
    let without_tile_lengths = [
        &with_tile_lengths[..tile_lengths],
        &with_tile_lengths[tile_lengths + 2 + length..],
    ]
    .concat();
    assert_violation(
        &check_frame(&without_tile_lengths, &declared),
        "main header has no TLM (tile-part length) marker segment, which HTJ2K Lossless RPCL requires (PS3.5 2026c 10.18.1)",
    );
    let undecomposed = rpcl().with_max_decomposition_levels(Some(0));
    assert_violation(
        &check_frame(&lossless(128, 128, 3, undecomposed), &declared),
        "main header COD has 0 decomposition levels, leaving a 128x128 base resolution with neither side at most 64 (PS3.5 2026c 10.18.1)",
    );
    // The statement asks for the width or the height to reach 64.
    assert_conformant(&check_frame(
        &lossless(128, 32, 3, undecomposed),
        &color(HTJ2K_RPCL, (32, 128), "YBR_RCT"),
    ));
}

#[test]
fn malformed_codestreams_are_reported_without_panicking() {
    let codestream = lossless(
        32,
        32,
        3,
        J2kLosslessEncodeOptions::default().with_tile_size(Some((16, 16))),
    );
    let declared = color(PART1_LOSSLESS, (32, 32), "YBR_RCT");
    for length in 0..codestream.len() - 1 {
        let check = check_frame(&codestream[..length], &declared);
        assert_eq!(
            check.status,
            crate::validation::ValidationStatus::Failed,
            "{length}"
        );
    }
    let mut overlong = codestream.clone();
    let tile_part = find(&overlong, &[0xFF, 0x90]);
    overlong[tile_part + 6..tile_part + 10].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_violation(&check_frame(&overlong, &declared), "runs past the Frame");
}
