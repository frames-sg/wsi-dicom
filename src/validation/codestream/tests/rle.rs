use super::{assert_conformant, assert_violation, check_frame, declared, hex, Declared};

const RLE: &str = "1.2.840.10008.1.2.5";

/// A 6x4 RGB frame that dcmtk 3.7 RLE encoded.
fn dcmtk_rgb() -> Vec<u8> {
    let header = format!("{}{}", "03000000400000005800000060000000", "0".repeat(96));
    hex(&format!(
        "{header}fec80278a0c8fec80278a0c8fec80278a0c8fec80278a0c8fb07fb07fb07fb07fb0005001e3c5a789605003c78b4f02c05005ab40e68c200"
    ))
}

/// PackBits by rows, as G.3.1 asks: repeats of two or more bytes become
/// Replicate Runs and the remaining bytes Literal Runs, then a zero pad.
fn encode_segment(plane: &[u8], columns: usize) -> Vec<u8> {
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

/// An RLE Header followed by `segments`.
fn frame(segments: &[Vec<u8>]) -> Vec<u8> {
    let mut words = [0u32; 16];
    words[0] = u32::try_from(segments.len()).expect("segment count");
    let mut offset = 64;
    for (index, segment) in segments.iter().enumerate() {
        words[index + 1] = offset;
        offset += u32::try_from(segment.len()).expect("segment length");
    }
    let mut bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    for segment in segments {
        bytes.extend_from_slice(segment);
    }
    bytes
}

fn monochrome((rows, columns): (u32, u32)) -> Declared {
    declared(RLE, (rows, columns), 1, 8, "MONOCHROME2")
}

#[test]
fn encoded_frames_are_conformant() {
    assert_conformant(&check_frame(
        &dcmtk_rgb(),
        &declared(RLE, (4, 6), 3, 8, "RGB"),
    ));
    let planes: [Vec<u8>; 3] = [
        (0..24)
            .map(|index| if index % 6 < 3 { 200 } else { index * 9 })
            .collect(),
        vec![7; 24],
        (0..24).map(|index| (index * 37 % 11) as u8).collect(),
    ];
    let segments: Vec<Vec<u8>> = planes
        .iter()
        .map(|plane| encode_segment(plane, 6))
        .collect();
    assert_conformant(&check_frame(
        &frame(&segments),
        &declared(RLE, (4, 6), 3, 8, "RGB"),
    ));
    // 16-bit samples make two Byte Segments, most significant first.
    let high = encode_segment(&[1, 1, 2, 3], 2);
    let low = encode_segment(&[9, 8, 8, 8], 2);
    assert_conformant(&check_frame(
        &frame(&[high, low]),
        &declared(RLE, (2, 2), 1, 16, "MONOCHROME2"),
    ));
}

#[test]
fn header_and_segments_agree_with_the_image_pixel_attributes() {
    assert_violation(
        &check_frame(&dcmtk_rgb(), &declared(RLE, (4, 6), 1, 8, "MONOCHROME2")),
        "Frame 1: RLE Header declares 3 RLE Segments but Samples per Pixel (0028,0002) 1 with Bits Allocated (0028,0100) 8 makes 1 Byte Segments (PS3.5 2026d 8.2.2 and PS3.5 2026d G.2)",
    );
    assert_violation(
        &check_frame(&dcmtk_rgb(), &declared(RLE, (5, 6), 3, 8, "RGB")),
        "RLE Segment 1 decodes to 24 bytes, fewer than the 30 of Rows (0028,0010) by Columns (0028,0011) (PS3.5 2026d 8.2.2)",
    );
    assert_violation(
        &check_frame(&frame(&[vec![0x00, 0x05, 0x00, 0x06]]), &monochrome((1, 1))),
        "RLE Segment 1 has 2 bytes after the runs that decode its 1 bytes",
    );
}

#[test]
fn unused_segment_offsets_are_zero() {
    let mut bytes = dcmtk_rgb();
    bytes[20..24].copy_from_slice(&999u32.to_le_bytes());
    assert_violation(
        &check_frame(&bytes, &declared(RLE, (4, 6), 3, 8, "RGB")),
        "RLE Header offsets after its 3 segments are 0, 999, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, but all unused segment offsets are zero (PS3.5 2026d G.5)",
    );
}

#[test]
fn segments_are_an_even_number_of_bytes_padded_with_zero() {
    assert_violation(
        &check_frame(&frame(&[vec![0x01, 0x2A, 0x2B]]), &monochrome((1, 2))),
        "RLE Segment 1 is 3 bytes; each RLE Segment is an even number of bytes (PS3.5 2026d G.3)",
    );
    assert_violation(
        &check_frame(&frame(&[vec![0x01, 0x2A, 0x2B, 0x07]]), &monochrome((1, 2))),
        "RLE Segment 1 is padded with a nonzero byte; the pad byte is zero (PS3.5 2026d G.3)",
    );
    assert_conformant(&check_frame(
        &frame(&[vec![0x01, 0x2A, 0x2B, 0x00]]),
        &monochrome((1, 2)),
    ));
}

#[test]
fn three_byte_repeats_are_replicate_runs() {
    assert_violation(
        &check_frame(&frame(&[vec![0x03, 5, 5, 5, 6, 0]]), &monochrome((1, 4))),
        "RLE Segment 1 has a literal run at decoded byte 0 that contains a three-byte repeat, which is encoded as a Replicate Run (PS3.5 2026d G.3.1)",
    );
    // A two-byte repeat may stay literal (G.3.1 Note).
    assert_conformant(&check_frame(
        &frame(&[vec![0x03, 5, 5, 6, 7, 0]]),
        &monochrome((1, 4)),
    ));
}

#[test]
fn runs_do_not_cross_row_boundaries() {
    assert_violation(
        &check_frame(&frame(&[vec![0xFB, 9]]), &monochrome((2, 3))),
        "RLE Segment 1 has a run starting at row 0, column 0 that crosses the row boundary; each row is encoded separately (PS3.5 2026d G.3.1)",
    );
    assert_conformant(&check_frame(
        &frame(&[vec![0xFE, 9, 0xFE, 9]]),
        &monochrome((2, 3)),
    ));
    assert_violation(
        &check_frame(
            &frame(&[vec![0x00, 1, 0x02, 2, 3, 4, 0x00]]),
            &monochrome((2, 2)),
        ),
        "RLE Segment 1 has a run starting at row 0, column 1 that crosses the row boundary",
    );
}

#[test]
fn malformed_headers_are_reported() {
    assert_violation(
        &check_frame(&[0; 10], &monochrome((1, 1))),
        "is 10 bytes, shorter than the 64-byte RLE Header (PS3.5 2026d G.5)",
    );
    assert_violation(
        &check_frame(&[0; 64], &monochrome((1, 1))),
        "RLE Header declares 0 RLE Segments; it describes 1 to 15 (PS3.5 2026d G.5)",
    );
    let mut bytes = dcmtk_rgb();
    bytes[8..12].copy_from_slice(&100u32.to_le_bytes());
    assert_violation(
        &check_frame(&bytes, &declared(RLE, (4, 6), 3, 8, "RGB")),
        "RLE Segment offsets 64, 100, 96 do not start at 64 and increase within the Frame",
    );
    let truncated = frame(&[vec![0x05, 1, 2]]);
    assert_violation(
        &check_frame(&truncated, &monochrome((1, 6))),
        "RLE Segment 1 ends inside a literal run of 6 bytes (PS3.5 2026d G.3.1)",
    );
}
