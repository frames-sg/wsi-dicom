//! JPEG XL (ISO/IEC 18181-1 codestream, 18181-2 container) headers of the
//! JPEG XL Transfer Syntaxes (PS3.5 8.2.15 and A.4.12).
//!
//! The SizeHeader and ImageMetadata bundles are read bit by bit; the first
//! frame header is reached when no ICC profile or preview frame precedes it,
//! since an embedded ICC profile is entropy coded.

use std::borrow::Cow;

use super::{signature, Frame, FrameReport};

const CONSISTENT: &str = "PS3.5 8.2.15";
const PROCESS: &str = "PS3.5 A.4.12";

const CODESTREAM_SIGNATURE: [u8; 2] = [0xFF, 0x0A];
const CONTAINER_SIGNATURE: [u8; 12] = [
    0x00, 0x00, 0x00, 0x0C, b'J', b'X', b'L', b' ', 0x0D, 0x0A, 0x87, 0x0A,
];
/// Codestream bytes gathered from partial codestream boxes for the headers.
const MAX_HEADER_BYTES: usize = 64 * 1024;

/// The JPEG XL coding process a Transfer Syntax identifies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    /// 1.2.840.10008.1.2.4.110.
    Lossless,
    /// 1.2.840.10008.1.2.4.111: reversible JPEG transcoding.
    JpegRecompression,
    /// 1.2.840.10008.1.2.4.112: any JPEG XL coding.
    Any,
}

pub(super) fn check(frame: &Frame<'_>, mode: Mode, report: &mut FrameReport<'_>) {
    let bytes = frame.bytes;
    let syntax = frame.declared.syntax.as_str();
    let (codestream, reconstruction) = if bytes.starts_with(&CODESTREAM_SIGNATURE) {
        (Cow::Borrowed(bytes), false)
    } else if bytes.starts_with(&CONTAINER_SIGNATURE) {
        match read_container(bytes) {
            Ok(container) => container,
            Err(reason) => {
                report.violation(PROCESS, || {
                    format!("JPEG XL container is malformed: {reason}")
                });
                return;
            }
        }
    } else {
        report.violation("PS3.10 7.2 and PS3.5 A.4.12", || {
            format!(
                "begins with {}, not the JPEG XL codestream or container signature Transfer Syntax {syntax} identifies",
                signature(bytes)
            )
        });
        return;
    };
    match mode {
        Mode::JpegRecompression if !reconstruction => report.violation(PROCESS, || {
            format!("has no JPEG bitstream reconstruction (jbrd) box, so it is not the reversible JPEG transcoding Transfer Syntax {syntax} identifies")
        }),
        Mode::Lossless if reconstruction => report.violation(PROCESS, || {
            format!("carries JPEG bitstream reconstruction (jbrd) data of a transcoded JPEG, not the lossless JPEG XL coding Transfer Syntax {syntax} identifies")
        }),
        _ => {}
    }
    let header = match Header::read(&codestream) {
        Some(header) => header,
        None => {
            report.violation(CONSISTENT, || {
                "JPEG XL headers are truncated or malformed".into()
            });
            return;
        }
    };
    check_header(frame, &header, report);
    match (mode, header.modular) {
        (Mode::Lossless, Some(false)) => report.violation(PROCESS, || {
            format!("first frame uses VarDCT encoding, which is lossy, not the lossless JPEG XL coding Transfer Syntax {syntax} identifies")
        }),
        (Mode::JpegRecompression, Some(true)) => report.violation(PROCESS, || {
            format!("first frame uses Modular encoding, which cannot hold the DCT coefficients of a transcoded JPEG that Transfer Syntax {syntax} identifies")
        }),
        _ => {}
    }
}

fn check_header(frame: &Frame<'_>, header: &Header, report: &mut FrameReport<'_>) {
    let declared = frame.declared;
    // Orientations 5 to 8 transpose the decoded image (ISO/IEC 18181-1).
    let (width, height) = if header.orientation >= 5 {
        (header.height, header.width)
    } else {
        (header.width, header.height)
    };
    declared
        .columns
        .compare(report, CONSISTENT, "JPEG XL image width", width);
    declared
        .rows
        .compare(report, CONSISTENT, "JPEG XL image height", height);
    let channels = header.color_channels.saturating_add(header.extra_channels);
    declared.samples_per_pixel.compare(
        report,
        CONSISTENT,
        "JPEG XL channel count (color plus extra)",
        channels,
    );
    if header.float_samples {
        report.violation(CONSISTENT, || {
            format!(
                "JPEG XL image holds {}-bit floating point samples, which Bits Stored (0028,0101) and Pixel Representation (0028,0103) cannot describe",
                header.bits_per_sample
            )
        });
    } else {
        declared.bits_stored.compare(
            report,
            CONSISTENT,
            "JPEG XL bits per sample",
            header.bits_per_sample,
        );
    }
}

/// The codestream of a container, from its jxlc box or its jxlp boxes in
/// order, and whether a jbrd box is present (ISO/IEC 18181-2).
fn read_container(bytes: &[u8]) -> Result<(Cow<'_, [u8]>, bool), String> {
    let mut position = 0usize;
    let mut whole = None;
    let mut partial: Vec<u8> = Vec::new();
    let mut reconstruction = false;
    while position < bytes.len() {
        let header = bytes
            .get(position..position + 8)
            .ok_or_else(|| format!("box header at byte {position} is truncated"))?;
        let size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let kind = &header[4..8];
        let (start, end) = match size {
            0 => (position + 8, bytes.len()),
            1 => {
                let large = bytes
                    .get(position + 8..position + 16)
                    .and_then(|large| <[u8; 8]>::try_from(large).ok())
                    .map(u64::from_be_bytes)
                    .ok_or("an extended box size is truncated")?;
                let end = usize::try_from(large)
                    .ok()
                    .and_then(|large| position.checked_add(large))
                    .filter(|end| *end >= position + 16)
                    .ok_or("an extended box size is invalid")?;
                (position + 16, end)
            }
            2..=7 => return Err(format!("box at byte {position} declares size {size}")),
            size => {
                let end = usize::try_from(size)
                    .ok()
                    .and_then(|size| position.checked_add(size))
                    .ok_or("a box size is invalid")?;
                (position + 8, end)
            }
        };
        let payload = bytes
            .get(start..end)
            .ok_or_else(|| format!("box at byte {position} runs past the Frame"))?;
        match kind {
            b"jxlc" => whole = Some(payload),
            b"jxlp" if partial.len() < MAX_HEADER_BYTES => {
                let data = payload.get(4..).ok_or("a jxlp box lacks its index")?;
                let room = MAX_HEADER_BYTES - partial.len();
                partial.extend_from_slice(&data[..data.len().min(room)]);
            }
            b"jbrd" => reconstruction = true,
            _ => {}
        }
        position = end;
        // Trailing zero bytes pad the Frame to even length (PS3.5 A.4).
        if bytes[position..].iter().all(|byte| *byte == 0) {
            break;
        }
    }
    match whole {
        Some(codestream) => Ok((Cow::Borrowed(codestream), reconstruction)),
        None if !partial.is_empty() => Ok((Cow::Owned(partial), reconstruction)),
        None => Err("it has no jxlc or jxlp codestream box".into()),
    }
}

/// What the headers say about the decoded image.
struct Header {
    width: u32,
    height: u32,
    orientation: u32,
    bits_per_sample: u32,
    float_samples: bool,
    color_channels: u32,
    extra_channels: u32,
    /// Whether the first frame is Modular, when it can be reached.
    modular: Option<bool>,
}

/// U32 field distributions (ISO/IEC 18181-1 Annex A).
#[derive(Clone, Copy)]
enum Distribution {
    Value(u32),
    Bits(u32),
    Offset(u32, u32),
}

use Distribution::{Bits as B, Offset as O, Value as V};

const ENUM: [Distribution; 4] = [V(0), V(1), O(4, 2), O(6, 18)];
const SIZE: [Distribution; 4] = [B(9), B(13), B(18), B(30)];
const PREVIEW: [Distribution; 4] = [O(6, 1), O(8, 65), O(10, 321), O(12, 1345)];
const PREVIEW_DIV8: [Distribution; 4] = [V(16), V(32), O(5, 1), O(9, 33)];
const CUSTOM_XY: [Distribution; 4] = [B(19), O(19, 524_288), O(20, 1_048_576), O(21, 2_097_152)];
const COLOR_SPACE_GREY: u32 = 1;
const COLOR_SPACE_XYB: u32 = 2;
const CUSTOM: u32 = 2;

/// Bits read least significant first; `None` past the end.
struct BitReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl BitReader<'_> {
    fn bits(&mut self, count: u32) -> Option<u64> {
        let mut value = 0u64;
        for index in 0..count {
            let byte = *self.bytes.get(self.position / 8)?;
            value |= u64::from((byte >> (self.position % 8)) & 1) << index;
            self.position += 1;
        }
        Some(value)
    }

    fn bit(&mut self) -> Option<bool> {
        self.bits(1).map(|bit| bit == 1)
    }

    fn u32(&mut self, distribution: [Distribution; 4]) -> Option<u32> {
        let selector = usize::try_from(self.bits(2)?).ok()?;
        Some(match distribution[selector] {
            Distribution::Value(value) => value,
            Distribution::Bits(count) => u32::try_from(self.bits(count)?).ok()?,
            Distribution::Offset(count, offset) => {
                u32::try_from(self.bits(count)?).ok()?.checked_add(offset)?
            }
        })
    }

    fn u64(&mut self) -> Option<u64> {
        Some(match self.bits(2)? {
            0 => 0,
            1 => 1 + self.bits(4)?,
            2 => 17 + self.bits(8)?,
            _ => {
                let mut value = self.bits(12)?;
                let mut shift = 12;
                while self.bit()? {
                    if shift == 60 {
                        value |= self.bits(4)? << 60;
                        break;
                    }
                    value |= self.bits(8)? << shift;
                    shift += 8;
                }
                value
            }
        })
    }

    fn skip(&mut self, count: u64) -> Option<()> {
        let count = usize::try_from(count).ok()?;
        let end = self.position.checked_add(count)?;
        let available = self.bytes.len().checked_mul(8)?;
        (end <= available).then(|| self.position = end)
    }

    fn skip_f16(&mut self, count: u64) -> Option<()> {
        self.skip(16 * count)
    }

    fn align_to_byte(&mut self) {
        self.position = self.position.div_ceil(8) * 8;
    }
}

impl Header {
    fn read(codestream: &[u8]) -> Option<Self> {
        let mut reader = BitReader {
            bytes: codestream,
            position: 16,
        };
        let (width, height) = read_size(&mut reader)?;
        let mut header = Self {
            width,
            height,
            orientation: 1,
            bits_per_sample: 8,
            float_samples: false,
            color_channels: 3,
            extra_channels: 0,
            modular: None,
        };
        let mut want_icc = false;
        let mut preview = false;
        let mut xyb_encoded = true;
        // ImageMetadata (ISO/IEC 18181-1 Annex A).
        if !reader.bit()? {
            let extra_fields = reader.bit()?;
            if extra_fields {
                header.orientation = 1 + u32::try_from(reader.bits(3)?).ok()?;
                if reader.bit()? {
                    read_size(&mut reader)?;
                }
                preview = reader.bit()?;
                if preview {
                    read_preview(&mut reader)?;
                }
                if reader.bit()? {
                    reader.u32([V(100), V(1000), O(10, 1), O(30, 1)])?;
                    reader.u32([V(1), V(1001), O(8, 1), O(10, 1)])?;
                    reader.u32([V(0), B(3), B(16), B(32)])?;
                    reader.bit()?;
                }
            }
            (header.bits_per_sample, header.float_samples) = read_bit_depth(&mut reader)?;
            reader.bit()?;
            header.extra_channels = reader.u32([V(0), V(1), O(4, 2), O(12, 1)])?;
            for _ in 0..header.extra_channels {
                read_extra_channel(&mut reader)?;
            }
            xyb_encoded = reader.bit()?;
            let (icc, color_space) = read_color_encoding(&mut reader)?;
            want_icc = icc;
            header.color_channels = if color_space == COLOR_SPACE_GREY {
                1
            } else {
                3
            };
            if extra_fields && !reader.bit()? {
                // ToneMapping: intensity target, minimum nits, a flag, linear below.
                reader.skip_f16(2)?;
                reader.bit()?;
                reader.skip_f16(1)?;
            }
            skip_extensions(&mut reader)?;
        }
        // CustomTransformData (ISO/IEC 18181-1 Annex A).
        if !reader.bit()? {
            if xyb_encoded && !reader.bit()? {
                reader.skip_f16(16)?;
            }
            let weights = reader.bits(3)?;
            for (mask, count) in [(1, 15), (2, 55), (4, 210)] {
                if weights & mask != 0 {
                    reader.skip_f16(count)?;
                }
            }
        }
        if !want_icc && !preview {
            reader.align_to_byte();
            // FrameHeader: all_default means a regular VarDCT frame.
            header.modular = Some(
                !reader.bit()? && {
                    reader.bits(2)?;
                    reader.bit()?
                },
            );
        }
        Some(header)
    }
}

/// SizeHeader (ISO/IEC 18181-1 Annex A).
fn read_size(reader: &mut BitReader<'_>) -> Option<(u32, u32)> {
    let small = reader.bit()?;
    let dimension = |reader: &mut BitReader<'_>| -> Option<u32> {
        if small {
            Some((u32::try_from(reader.bits(5)?).ok()? + 1) * 8)
        } else {
            reader.u32(SIZE)?.checked_add(1)
        }
    };
    let height = dimension(reader)?;
    let ratio = reader.bits(3)?;
    let width = match ratio {
        0 => dimension(reader)?,
        ratio => {
            let (numerator, denominator) =
                [(1, 1), (12, 10), (4, 3), (3, 2), (16, 9), (5, 4), (2, 1)]
                    [usize::try_from(ratio).ok()? - 1];
            u32::try_from(u64::from(height) * numerator / denominator).ok()?
        }
    };
    Some((width, height))
}

/// PreviewHeader (ISO/IEC 18181-1 Annex A).
fn read_preview(reader: &mut BitReader<'_>) -> Option<()> {
    let div8 = reader.bit()?;
    let distribution = if div8 { PREVIEW_DIV8 } else { PREVIEW };
    reader.u32(distribution)?;
    if reader.bits(3)? == 0 {
        reader.u32(distribution)?;
    }
    Some(())
}

/// BitDepth: (bits per sample, floating point).
fn read_bit_depth(reader: &mut BitReader<'_>) -> Option<(u32, bool)> {
    if reader.bit()? {
        let bits = reader.u32([V(32), V(16), V(24), O(6, 1)])?;
        reader.bits(4)?;
        Some((bits, true))
    } else {
        Some((reader.u32([V(8), V(10), V(12), O(6, 1)])?, false))
    }
}

/// ExtraChannelInfo (ISO/IEC 18181-1 Annex A).
fn read_extra_channel(reader: &mut BitReader<'_>) -> Option<()> {
    if reader.bit()? {
        return Some(());
    }
    let kind = reader.u32(ENUM)?;
    read_bit_depth(reader)?;
    reader.u32([V(0), V(3), V(4), O(3, 1)])?;
    let name = reader.u32([V(0), B(4), O(5, 16), O(10, 48)])?;
    reader.skip(8 * u64::from(name))?;
    match kind {
        0 => {
            reader.bit()?;
        }
        2 => reader.skip_f16(4)?,
        5 => {
            reader.u32([V(1), B(2), O(4, 3), O(8, 19)])?;
        }
        _ => {}
    }
    Some(())
}

/// ColorEncoding (ISO/IEC 18181-1 Annex A): (want ICC, color space).
fn read_color_encoding(reader: &mut BitReader<'_>) -> Option<(bool, u32)> {
    if reader.bit()? {
        return Some((false, 0));
    }
    let want_icc = reader.bit()?;
    let color_space = reader.u32(ENUM)?;
    if !want_icc && color_space != COLOR_SPACE_XYB {
        if reader.u32(ENUM)? == CUSTOM {
            reader.u32(CUSTOM_XY)?;
            reader.u32(CUSTOM_XY)?;
        }
        if color_space != COLOR_SPACE_GREY && reader.u32(ENUM)? == CUSTOM {
            for _ in 0..6 {
                reader.u32(CUSTOM_XY)?;
            }
        }
    }
    if !want_icc {
        // The XYB transfer function is implicit, so it is not coded.
        if color_space != COLOR_SPACE_XYB {
            if reader.bit()? {
                reader.bits(24)?;
            } else {
                reader.u32(ENUM)?;
            }
        }
        reader.u32(ENUM)?;
    }
    Some((want_icc, color_space))
}

/// Extensions: a bit set of extensions, then each one's length in bits.
fn skip_extensions(reader: &mut BitReader<'_>) -> Option<()> {
    let extensions = reader.u64()?;
    let mut total = 0u64;
    for bit in 0..64 {
        if extensions & (1 << bit) != 0 {
            total = total.checked_add(reader.u64()?)?;
        }
    }
    reader.skip(total)
}
