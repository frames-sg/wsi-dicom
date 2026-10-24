use j2k_jpeg::{
    JpegBackend, JpegEncodeOptions, JpegLosslessEncodeOptions, JpegLosslessSamples, JpegSamples12,
    JpegSubsampling,
};

use super::{
    assert_conformant, assert_violation, check_fragments, check_frame, declared, find, insert,
    samples,
};
use crate::test_support::{
    encode_test_gray_jpeg, encode_test_jpeg, encode_test_jpeg_with_subsampling,
};

const BASELINE: &str = "1.2.840.10008.1.2.4.50";
const EXTENDED: &str = "1.2.840.10008.1.2.4.51";
const LOSSLESS: &str = "1.2.840.10008.1.2.4.57";
const SV1: &str = "1.2.840.10008.1.2.4.70";

const ADOBE_UNTRANSFORMED: [u8; 16] = adobe(0);
const ADOBE_YCBCR: [u8; 16] = adobe(1);
const JFIF: [u8; 18] = [
    0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01,
    0x00, 0x00,
];

const fn adobe(transform: u8) -> [u8; 16] {
    [
        0xFF, 0xEE, 0x00, 0x0E, b'A', b'd', b'o', b'b', b'e', 0x00, 0x64, 0x00, 0x00, 0x00, 0x00,
        transform,
    ]
}

/// A 16x8 RGB frame, chrominance subsampled 4:2:2 by the encoder.
fn ybr_422() -> Vec<u8> {
    encode_test_jpeg(16, 8, [200, 120, 40])
}

/// A 16x8 frame with all three components at full resolution.
fn full_resolution() -> Vec<u8> {
    encode_test_jpeg_with_subsampling(16, 8, [200, 120, 40], JpegSubsampling::Ybr444)
}

fn extended_12_bit() -> Vec<u8> {
    let pixels: Vec<u16> = (0..16 * 8)
        .map(|value| (value * 31) as u16 & 0x0FFF)
        .collect();
    j2k_jpeg::encode_jpeg_extended12(
        JpegSamples12::Gray {
            data: &pixels,
            width: 16,
            height: 8,
        },
        JpegEncodeOptions {
            quality: 90,
            subsampling: JpegSubsampling::Gray,
            restart_interval: None,
            backend: JpegBackend::Cpu,
        },
    )
    .expect("12-bit extended JPEG")
    .data
}

fn lossless(predictor: u8) -> Vec<u8> {
    let pixels = samples(16, 8, 1);
    let mut options = JpegLosslessEncodeOptions::new(8);
    options.predictor = predictor;
    j2k_jpeg::encode_jpeg_lossless(
        JpegLosslessSamples::Gray8 {
            data: &pixels,
            width: 16,
            height: 8,
        },
        options,
    )
    .expect("lossless JPEG")
    .data
}

/// `stream` without its marker segments of `marker` before the first scan.
fn without_segments(stream: &[u8], marker: u8) -> Vec<u8> {
    let mut kept = stream[..2].to_vec();
    let mut position = 2;
    while stream[position + 1] != 0xDA {
        let length = usize::from(u16::from_be_bytes([
            stream[position + 2],
            stream[position + 3],
        ]));
        if stream[position + 1] != marker {
            kept.extend_from_slice(&stream[position..position + 2 + length]);
        }
        position += 2 + length;
    }
    kept.extend_from_slice(&stream[position..]);
    kept
}

/// `stream` with `segment` right after SOI.
fn after_soi(stream: &[u8], segment: &[u8]) -> Vec<u8> {
    insert(stream, 2, segment)
}

#[test]
fn encoder_frames_are_conformant_for_the_processes_of_their_transfer_syntaxes() {
    assert_conformant(&check_frame(
        &ybr_422(),
        &declared(BASELINE, (8, 16), 3, 8, "YBR_FULL_422"),
    ));
    assert_conformant(&check_frame(
        &encode_test_gray_jpeg(16, 8, 90),
        &declared(BASELINE, (8, 16), 1, 8, "MONOCHROME2"),
    ));
    assert_conformant(&check_frame(
        &extended_12_bit(),
        &declared(EXTENDED, (8, 16), 1, 12, "MONOCHROME2"),
    ));
    let monochrome = |syntax| declared(syntax, (8, 16), 1, 8, "MONOCHROME2");
    assert_conformant(&check_frame(&lossless(1), &monochrome(SV1)));
    assert_conformant(&check_frame(&lossless(1), &monochrome(LOSSLESS)));
    assert_conformant(&check_frame(&lossless(6), &monochrome(LOSSLESS)));
    // A trailing pad byte after EOI keeps the Item length even.
    let mut padded = ybr_422();
    padded.push(0);
    assert_conformant(&check_frame(
        &padded,
        &declared(BASELINE, (8, 16), 3, 8, "YBR_FULL_422"),
    ));
}

#[test]
fn frame_header_agrees_with_rows_columns_samples_and_bits_stored() {
    let stream = ybr_422();
    for (declared, expected) in [
        (
            declared(BASELINE, (16, 16), 3, 8, "YBR_FULL_422"),
            "Frame 1: SOF0 number of lines is 8 but Rows (0028,0010) is 16 (PS3.5 2026c 8.2.1)",
        ),
        (
            declared(BASELINE, (8, 8), 3, 8, "YBR_FULL_422"),
            "SOF0 samples per line is 16 but Columns (0028,0011) is 8 (PS3.5 2026c 8.2.1)",
        ),
        (
            declared(BASELINE, (8, 16), 1, 8, "MONOCHROME2"),
            "SOF0 number of components is 3 but Samples per Pixel (0028,0002) is 1",
        ),
    ] {
        assert_violation(&check_frame(&stream, &declared), expected);
    }
    assert_violation(
        &check_frame(
            &lossless(1),
            &declared(LOSSLESS, (8, 16), 1, 12, "MONOCHROME2"),
        ),
        "SOF3 sample precision is 8 but Bits Stored (0028,0101) is 12 (PS3.5 2026c 8.2.1)",
    );
}

#[test]
fn coding_process_is_the_one_its_transfer_syntax_identifies() {
    assert_violation(
        &check_frame(&ybr_422(), &declared(EXTENDED, (8, 16), 3, 8, "YBR_FULL_422")),
        "SOF0 (baseline DCT) is not the JPEG coding process that Transfer Syntax 1.2.840.10008.1.2.4.51 identifies (SOF1 (extended sequential DCT, Huffman)) (PS3.5 2026c A.4.1 Table A.4-3)",
    );
    let twelve_bit_as_baseline = check_frame(
        &extended_12_bit(),
        &declared(BASELINE, (8, 16), 1, 12, "MONOCHROME2"),
    );
    assert_violation(&twelve_bit_as_baseline, "SOF1 (extended sequential DCT, Huffman) is not the JPEG coding process that Transfer Syntax 1.2.840.10008.1.2.4.50 identifies");
    assert_violation(
        &twelve_bit_as_baseline,
        "sample precision 12 is not the 8 (Process 1)",
    );
    assert_violation(
        &check_frame(&lossless(7), &declared(SV1, (8, 16), 1, 8, "MONOCHROME2")),
        "SOS predictor selection value 7 is not the first-order prediction (Selection Value 1) of Transfer Syntax 1.2.840.10008.1.2.4.70 (PS3.5 2026c A.4.1 Table A.4-3)",
    );
    assert_violation(
        &check_frame(
            &encode_test_gray_jpeg(16, 8, 90),
            &declared(SV1, (8, 16), 1, 8, "MONOCHROME2"),
        ),
        "SOF0 (baseline DCT) is not the JPEG coding process that Transfer Syntax 1.2.840.10008.1.2.4.70 identifies (SOF3 (lossless, Huffman))",
    );
}

#[test]
fn frames_carry_their_table_specifications() {
    let ybr = declared(BASELINE, (8, 16), 3, 8, "YBR_FULL_422");
    let without_huffman = check_frame(&without_segments(&ybr_422(), 0xC4), &ybr);
    assert_violation(&without_huffman, "of the first scan uses DC Huffman table 0 (DHT) and AC Huffman table 0 (DHT), which the stream does not specify: abbreviated format instead of the interchange format (PS3.5 2026c A.4.1)");
    assert_violation(
        &check_frame(&without_segments(&ybr_422(), 0xDB), &ybr),
        "uses quantization table 0 (DQT), which the stream does not specify",
    );
    assert_violation(
        &check_frame(
            &without_segments(&lossless(1), 0xC4),
            &declared(SV1, (8, 16), 1, 8, "MONOCHROME2"),
        ),
        "uses DC Huffman table 0 (DHT), which the stream does not specify",
    );
    // An abbreviated table-specification stream holds tables but no image.
    let stream = ybr_422();
    let tables_only = [&stream[..find(&stream, &[0xFF, 0xC0])], &[0xFF, 0xD9][..]].concat();
    assert_violation(
        &check_frame(&tables_only, &ybr),
        "reaches EOI without a scan",
    );
}

#[test]
fn each_frame_is_one_complete_separately_encoded_stream() {
    let ybr = declared(BASELINE, (8, 16), 3, 8, "YBR_FULL_422");
    let stream = ybr_422();
    assert_violation(
        &check_frame(&stream[..stream.len() - 2], &ybr),
        "does not end with EOI (FFD9H), so the Frame is not one complete, separately encoded JPEG stream (PS3.5 2026c A.4.1)",
    );
    let mut first = stream.clone();
    if first.len() % 2 == 1 {
        first.push(0);
    }
    assert_violation(
        &check_fragments(&[&first, &stream], &ybr),
        "its Fragment 2 begins another JPEG stream after the previous Fragment ended one, so its Fragments hold data of more than one Frame",
    );
    let middle = stream.len() / 2;
    assert_conformant(&check_fragments(
        &[&stream[..middle], &stream[middle..]],
        &ybr,
    ));
    assert_violation(
        &check_frame(&after_soi(&stream, &[0xFF, 0xD8]), &ybr),
        "a second SOI precedes the first scan",
    );
}

#[test]
fn color_declarations_agree_with_photometric_interpretation() {
    let rgb = declared(BASELINE, (8, 16), 3, 8, "RGB");
    let stream = full_resolution();
    assert_conformant(&check_frame(
        &after_soi(&stream, &ADOBE_UNTRANSFORMED),
        &rgb,
    ));
    assert_violation(
        &check_frame(&after_soi(&stream, &ADOBE_YCBCR), &rgb),
        "APP14 Adobe transform 1 (YCbCr) declares color-transformed components but Photometric Interpretation is RGB (PS3.5 2026c 8.2.1 and PS3.3 2026c C.8.12.4.1.5)",
    );
    assert_violation(
        &check_frame(&after_soi(&stream, &JFIF), &rgb),
        "APP0 JFIF, which implies YCbCr components declares color-transformed components but Photometric Interpretation is RGB",
    );
    // The explicit Adobe transform flag outranks the JFIF implication.
    let both = after_soi(&after_soi(&stream, &ADOBE_UNTRANSFORMED), &JFIF);
    assert_conformant(&check_frame(&both, &rgb));
    assert_conformant(&check_frame(
        &after_soi(&ybr_422(), &JFIF),
        &declared(BASELINE, (8, 16), 3, 8, "YBR_FULL_422"),
    ));
}

#[test]
fn subsampled_planes_are_described_only_by_ybr_full_422() {
    let stream = after_soi(&ybr_422(), &ADOBE_UNTRANSFORMED);
    assert_violation(
        &check_frame(&stream, &declared(BASELINE, (8, 16), 3, 8, "RGB")),
        "SOF0 sampling factors 2x1, 1x1, 1x1 encode planes of different sizes, which Photometric Interpretation RGB does not describe (PS3.3 2026c C.7.6.3.1.1)",
    );
    assert_conformant(&check_frame(
        &full_resolution(),
        &declared(BASELINE, (8, 16), 3, 8, "RGB"),
    ));
}

#[test]
fn stream_kind_is_the_one_its_transfer_syntax_identifies() {
    let codestream = super::j2k::lossless_rgb(16, 8);
    assert_violation(
        &check_frame(&codestream, &declared(BASELINE, (8, 16), 3, 8, "YBR_FULL_422")),
        "begins with bytes FF 4F FF 51 (a JPEG 2000 codestream: SOC, SIZ), not the SOI marker of the JPEG stream Transfer Syntax 1.2.840.10008.1.2.4.50 identifies (PS3.10 2026c 7.2 and PS3.5 2026c A.4.1)",
    );
    let jpeg_ls = super::jpeg_ls::dcmtk_lossless();
    assert_violation(
        &check_frame(&jpeg_ls, &declared(BASELINE, (4, 6), 3, 8, "RGB")),
        "SOF55 (JPEG-LS) is not the JPEG coding process",
    );
}

#[test]
fn malformed_headers_are_reported_without_panicking() {
    let ybr = declared(BASELINE, (8, 16), 3, 8, "YBR_FULL_422");
    let stream = ybr_422();
    for length in 0..stream.len() - 1 {
        let check = check_frame(&stream[..length], &ybr);
        assert_eq!(
            check.status,
            crate::validation::ValidationStatus::Failed,
            "{length}"
        );
    }
    let mut overlong = stream.clone();
    let quantization = find(&overlong, &[0xFF, 0xDB]);
    overlong[quantization + 2..quantization + 4].copy_from_slice(&[0xFF, 0xFF]);
    assert_violation(
        &check_frame(&overlong, &ybr),
        "JPEG header is malformed: marker FFDBH declares length 65535, beyond the Frame",
    );
}
