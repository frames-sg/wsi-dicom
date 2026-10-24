//! Frame payloads of the configuration fixtures: deterministic samples, and
//! their encoding in each Transfer Syntax a VL WSI image may use. Codestreams
//! come from the crate's encoders; JPEG-LS and JPEG XL, which the crate does
//! not encode, use small vectors from dcmtk 3.7 (CharLS) and libjxl 0.11.2.

use std::io::Write;

use dicom_dictionary_std::uids;
use j2k::{
    J2kBlockCodingMode, J2kLosslessEncodeOptions, J2kLosslessSamples, J2kLossyEncodeOptions,
    J2kLossySamples, J2kMarkerSegment, J2kProgressionOrder, ReversibleTransform,
};
use j2k_jpeg::{
    JpegBackend, JpegEncodeOptions, JpegLosslessEncodeOptions, JpegLosslessSamples, JpegSamples,
    JpegSubsampling,
};

/// The sample format of every Frame of a fixture.
#[derive(Clone, Copy, Debug)]
pub(super) struct Format {
    pub(super) rows: u16,
    pub(super) columns: u16,
    pub(super) samples: u16,
    pub(super) bits: u16,
}

impl Format {
    /// Native Format bytes of one Frame (PS3.5 8.1.1), before padding.
    pub(super) fn frame_bytes(self) -> usize {
        usize::from(self.rows)
            * usize::from(self.columns)
            * usize::from(self.samples)
            * usize::from(self.bits / 8)
    }
}

/// Deterministic Native samples of Frame `frame`, little endian.
pub(super) fn native(format: Format, frame: usize) -> Vec<u8> {
    let (rows, columns, samples) = (
        usize::from(format.rows),
        usize::from(format.columns),
        usize::from(format.samples),
    );
    let mut bytes = Vec::with_capacity(format.frame_bytes());
    for y in 0..rows {
        for x in 0..columns {
            for c in 0..samples {
                let value = (x * 7 + y * 13 + c * 61 + frame * 29 + (x ^ y)) as u16;
                if format.bits == 16 {
                    bytes.extend_from_slice(&(value.wrapping_mul(257)).to_le_bytes());
                } else {
                    bytes.push(value as u8);
                }
            }
        }
    }
    bytes
}

/// Frame `frame` encoded for `syntax`, or `None` for a Native syntax.
pub(super) fn encoded(
    syntax: &str,
    photometric: &str,
    format: Format,
    frame: usize,
) -> Option<Vec<u8>> {
    let samples = native(format, frame);
    let (width, height) = (u32::from(format.columns), u32::from(format.rows));
    Some(match syntax {
        uids::IMPLICIT_VR_LITTLE_ENDIAN
        | uids::EXPLICIT_VR_LITTLE_ENDIAN
        | uids::DEFLATED_EXPLICIT_VR_LITTLE_ENDIAN => return None,
        uids::ENCAPSULATED_UNCOMPRESSED_EXPLICIT_VR_LITTLE_ENDIAN => samples,
        uids::DEFLATED_IMAGE_FRAME_COMPRESSION => deflated(&samples),
        uids::RLE_LOSSLESS => rle(&samples, format),
        uids::JPEG_BASELINE8_BIT => baseline(&samples, format),
        uids::JPEG_EXTENDED12_BIT => extended_8_bit(&samples, format),
        uids::JPEG_LOSSLESS | uids::JPEG_LOSSLESS_SV1 => jpeg_lossless(&samples, format),
        uids::JPEGLS_LOSSLESS | uids::JPEGLS_NEAR_LOSSLESS => jpeg_ls(syntax, format),
        uids::JPEG2000_LOSSLESS | uids::JPEG2000MC_LOSSLESS => {
            j2k_lossless(&samples, format, photometric, lossless_options(photometric))
        }
        uids::HTJ2K_LOSSLESS => j2k_lossless(
            &samples,
            format,
            photometric,
            lossless_options(photometric)
                .with_block_coding_mode(J2kBlockCodingMode::HighThroughput),
        ),
        uids::HTJ2K_LOSSLESS_RPCL => j2k_lossless(
            &samples,
            format,
            photometric,
            lossless_options(photometric)
                .with_block_coding_mode(J2kBlockCodingMode::HighThroughput)
                .with_progression(J2kProgressionOrder::Rpcl)
                .with_marker_segments(&[J2kMarkerSegment::Tlm]),
        ),
        uids::JPEG2000 | uids::JPEG2000MC => j2k_lossy(
            &samples,
            (width, height),
            format,
            J2kBlockCodingMode::Classic,
        ),
        uids::HTJ2K => j2k_lossy(
            &samples,
            (width, height),
            format,
            J2kBlockCodingMode::HighThroughput,
        ),
        uids::JPEGXL_LOSSLESS | uids::JPEGXL | uids::JPEGXLJPEG_RECOMPRESSION => {
            jpeg_xl(syntax, format)
        }
        other => panic!("no fixture encoder for Transfer Syntax {other}"),
    })
}

/// RFC 1951 raw deflate (PS3.5 A.4.13); the Fragment pads an odd-length
/// stream with the single trailing NUL.
fn deflated(samples: &[u8]) -> Vec<u8> {
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(samples).expect("deflate");
    encoder.finish().expect("deflate")
}

/// PackBits by rows, one Segment per byte plane (PS3.5 Annex G).
fn rle(samples: &[u8], format: Format) -> Vec<u8> {
    let bytes_per_sample = usize::from(format.bits / 8);
    let planes = usize::from(format.samples) * bytes_per_sample;
    let pixels = usize::from(format.rows) * usize::from(format.columns);
    let stride = usize::from(format.samples) * bytes_per_sample;
    let segments: Vec<Vec<u8>> = (0..planes)
        .map(|plane| {
            let sample = plane / bytes_per_sample;
            // Segments run from the most significant byte (G.2).
            let byte = bytes_per_sample - 1 - plane % bytes_per_sample;
            let values: Vec<u8> = (0..pixels)
                .map(|pixel| samples[pixel * stride + sample * bytes_per_sample + byte])
                .collect();
            pack_bits(&values, usize::from(format.columns))
        })
        .collect();
    let mut header = [0u32; 16];
    header[0] = segments.len() as u32;
    let mut offset = 64u32;
    for (index, segment) in segments.iter().enumerate() {
        header[index + 1] = offset;
        offset += segment.len() as u32;
    }
    let mut frame: Vec<u8> = header.iter().flat_map(|word| word.to_le_bytes()).collect();
    for segment in segments {
        frame.extend(segment);
    }
    frame
}

fn pack_bits(plane: &[u8], columns: usize) -> Vec<u8> {
    let mut encoded = Vec::new();
    for row in plane.chunks(columns) {
        let mut position = 0;
        while position < row.len() {
            let value = row[position];
            let run = row[position..]
                .iter()
                .take(128)
                .take_while(|byte| **byte == value)
                .count();
            if run >= 2 {
                encoded.extend_from_slice(&[(1 - run as i16) as u8, value]);
                position += run;
                continue;
            }
            let start = position;
            while position < row.len()
                && position - start < 128
                && row.get(position + 1) != Some(&row[position])
            {
                position += 1;
            }
            encoded.push((position - start - 1) as u8);
            encoded.extend_from_slice(&row[start..position]);
        }
    }
    if encoded.len() % 2 == 1 {
        encoded.push(0);
    }
    encoded
}

fn baseline(samples: &[u8], format: Format) -> Vec<u8> {
    let (width, height) = (u32::from(format.columns), u32::from(format.rows));
    let (samples, subsampling) = if format.samples == 1 {
        (
            JpegSamples::Gray8 {
                data: samples,
                width,
                height,
            },
            JpegSubsampling::Gray,
        )
    } else {
        (
            JpegSamples::Rgb8 {
                data: samples,
                width,
                height,
            },
            JpegSubsampling::Ybr422,
        )
    };
    j2k_jpeg::encode_jpeg_baseline(
        samples,
        JpegEncodeOptions {
            quality: 90,
            subsampling,
            restart_interval: None,
            backend: JpegBackend::Cpu,
        },
    )
    .expect("JPEG baseline")
    .data
}

/// An 8-bit Process 2 stream: a baseline stream is extended sequential
/// Huffman coding with SOF1 in place of SOF0 (ISO/IEC 10918-1 B.2.2).
fn extended_8_bit(samples: &[u8], format: Format) -> Vec<u8> {
    let mut stream = baseline(samples, format);
    let sof = stream
        .windows(2)
        .position(|marker| marker == [0xFF, 0xC0])
        .expect("SOF0");
    stream[sof + 1] = 0xC1;
    stream
}

fn jpeg_lossless(samples: &[u8], format: Format) -> Vec<u8> {
    let (width, height) = (u32::from(format.columns), u32::from(format.rows));
    let words: Vec<u16> = samples
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    let input = match (format.samples, format.bits) {
        (1, 8) => JpegLosslessSamples::Gray8 {
            data: samples,
            width,
            height,
        },
        (1, _) => JpegLosslessSamples::Gray16 {
            data: &words,
            width,
            height,
        },
        (_, 8) => JpegLosslessSamples::Rgb8 {
            data: samples,
            width,
            height,
        },
        _ => JpegLosslessSamples::Rgb16 {
            data: &words,
            width,
            height,
        },
    };
    let mut options = JpegLosslessEncodeOptions::new(format.bits as u8);
    options.predictor = 1;
    j2k_jpeg::encode_jpeg_lossless(input, options)
        .expect("lossless JPEG")
        .data
}

fn lossless_options(photometric: &str) -> J2kLosslessEncodeOptions {
    let mut options = J2kLosslessEncodeOptions::default();
    if photometric != "YBR_RCT" {
        options.reversible_transform = ReversibleTransform::None53;
    }
    options
}

fn j2k_lossless(
    samples: &[u8],
    format: Format,
    _photometric: &str,
    options: J2kLosslessEncodeOptions,
) -> Vec<u8> {
    let input = J2kLosslessSamples::new(
        samples,
        u32::from(format.columns),
        u32::from(format.rows),
        format.samples,
        format.bits as u8,
        false,
    )
    .expect("lossless samples");
    j2k::encode_j2k_lossless(input, &options.with_cpu_only_backend())
        .expect("lossless JPEG 2000")
        .codestream
}

fn j2k_lossy(
    samples: &[u8],
    (width, height): (u32, u32),
    format: Format,
    mode: J2kBlockCodingMode,
) -> Vec<u8> {
    let input = J2kLossySamples::new(
        samples,
        width,
        height,
        format.samples,
        format.bits as u8,
        false,
    )
    .expect("lossy samples");
    let options = J2kLossyEncodeOptions::default()
        .with_cpu_only_backend()
        .with_block_coding_mode(mode);
    j2k::encode_j2k_lossy(input, &options)
        .expect("lossy JPEG 2000")
        .codestream
}

/// A 6x4 RGB frame that dcmtk 3.7 (CharLS) encoded, lossless or with NEAR 3.
fn jpeg_ls(syntax: &str, format: Format) -> Vec<u8> {
    assert_eq!(
        (format.rows, format.columns, format.samples, format.bits),
        (4, 6, 3, 8),
        "the JPEG-LS vectors are 6x4 8-bit RGB"
    );
    hex(if syntax == uids::JPEGLS_LOSSLESS {
        "ffd8fff70011080004000603011100021100031100ffda000c030100020003000001000000016d9000000cf9897ad95fe6924faedcb76acf957800000276000002ee367400000177ca3700a72360001c0c80ffd9"
    } else {
        "ffd8fff70011080004000603011100021100031100ffda000c03010002000300030100005400000273a6ff6d55f5c3ff7f3f810040088002cfc100200020c0000017c0ffffd9"
    })
}

/// 8x8 8-bit frames that libjxl 0.11.2 (cjxl) encoded: grey lossless, grey
/// recompressed from JPEG, or RGB lossless in a container.
fn jpeg_xl(syntax: &str, format: Format) -> Vec<u8> {
    assert_eq!(
        (format.rows, format.columns, format.bits),
        (8, 8, 8),
        "the JPEG XL vectors are 8x8 8-bit"
    );
    hex(match (syntax, format.samples) {
        (uids::JPEGXLJPEG_RECOMPRESSION, 1) => "0000000c4a584c200d0a870a00000014667479706a786c20000000006a786c20000000136a786c7000000000ff0a414050dc08000001046a627264c10d20082d030200848090ab4a000040c851d982a138ff27888c11ca38a714b41f214398a50e32cc40e90e35581f329c7ccc80430e2a2c72d0b003db470dfdfb23cb1c76dc8186a2fb5f8c71c624976cf2c928a73c33cd35db7c33ce390b3d34d1451b7d34d2494b3d35d5555b7d35d6598b3d36d9659b7d36da69cb3d37dd75db7d37de790b3e38e1851b7e38e289472ef9e494576ef9e598671ebae8a3935ebae9a7a39e7aecb2cf4e7bedb6df8e7beec10b3f3cf1c51b7f3cf2c9472ffdf4d4576ffdf5d8670f7ef8e28f4f7ef9e69f8f7efaf0c72ffffcf4d76ffffdf8e7ff0ff0030000000b0880e000104a4649460001010000010001000003000000fd6a786c7080000001e05b001000a803087808780878feff0e0000180008000100000000008080258963c4dda487a8ea1a830b10fc0a040a242928284c43566c0c8e36c6c622b4c3c69ac3e8ce0d00900b8ed790c301b9a013404c96ab3f000cababffe43ed8114f85b3f1f4972452998dcb98580d52292f84143cc9283fc5f412f0b42114b1a41250a5fabff21a8096b0e68e21f75532aa41c1653c2d020b7411e2089e6c04a6a22832605ba38cc15589a39ae044f98a5603452f8711f82411d7446a34be9280032a081f8541ea445371f71cd039f019c12a05000000000050924809d9edb6df5408047adcbfeb347b4b6dc74902d248806d03",
        (_, 1) => "ff0a414050dc080804010068004b2825d6818282a4b86a3b1c509f75777737000000000000e01c",
        _ => "0000000c4a584c200d0a870a00000014667479706a786c20000000006a786c200000006d6a786c63ff0a4140240804010068014b28b06db43fafc2282828290a0a20bbb1318c8a8a61540ca3630c1e4ad363440c630cdee51863f45c207a00df0007000c6924db8edbe849bfcb2d5a20a9bedeff7ffd48eb075686eef0b3bfaa90df70c7c680b1fc421610c801",
    })
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).expect("hex digits"))
        .collect()
}
