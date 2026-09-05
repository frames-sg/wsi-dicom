use std::path::PathBuf;

use dicom_core::value::{PixelFragmentSequence, Value};
use dicom_dictionary_std::tags;
use dicom_object::DefaultDicomObject;

use super::{failed_check, ValidationCheck, ValidationStatus};

pub(super) fn run_intrinsic_pixel_structure_check(
    file: &PathBuf,
    object: &DefaultDicomObject,
    max_frame_bytes: usize,
) -> ValidationCheck {
    let result = (|| -> Result<(), String> {
        let transfer_syntax = object.meta().transfer_syntax.trim_end_matches('\0');
        let frame_count = dicom_frame_count(object)?;
        let pixel_data = object
            .element(tags::PIXEL_DATA)
            .map_err(|err| format!("failed to read Pixel Data: {err}"))?;
        let compressed = pixel_data_transfer_syntax_is_compressed(transfer_syntax);

        match (compressed, pixel_data.value()) {
            (true, Value::PixelSequence(sequence)) => {
                if sequence.fragments().is_empty() {
                    return Err("compressed Pixel Data has no fragments".into());
                }
                let extended_offsets = optional_u64_values(object, tags::EXTENDED_OFFSET_TABLE)?;
                let extended_lengths =
                    optional_u64_values(object, tags::EXTENDED_OFFSET_TABLE_LENGTHS)?;
                assemble_encapsulated_frames(
                    sequence,
                    frame_count,
                    extended_offsets.as_deref(),
                    extended_lengths.as_deref(),
                    0,
                    max_frame_bytes,
                )?;
            }
            (true, _) => {
                return Err(format!(
                    "compressed transfer syntax {transfer_syntax} requires encapsulated Pixel Data"
                ));
            }
            (false, Value::Primitive(_)) => {
                let bytes = pixel_data
                    .to_bytes()
                    .map_err(|err| format!("failed to inspect primitive Pixel Data: {err}"))?;
                if bytes.is_empty() {
                    return Err("primitive Pixel Data is empty".into());
                }
                if primitive_pixel_data_contains_encapsulated_item_stream(&bytes)
                    && !native_pixel_data_length_matches(object, frame_count, bytes.len())
                {
                    return Err(format!(
                        "native transfer syntax {transfer_syntax} cannot contain an encapsulated item stream in Pixel Data"
                    ));
                }
            }
            (false, Value::PixelSequence(_)) => {
                return Err(format!(
                    "native transfer syntax {transfer_syntax} cannot contain encapsulated Pixel Data"
                ));
            }
            (false, _) => return Err("Pixel Data has an unusable value representation".into()),
        }
        Ok(())
    })();

    match result {
        Ok(()) => ValidationCheck {
            execution: None,
            name: "intrinsic-pixel-structure".into(),
            path: Some(file.clone()),
            status: ValidationStatus::Passed,
            command: Vec::new(),
            message: "intrinsic Pixel Data structure is valid".into(),
            stdout: String::new(),
            stderr: String::new(),
        },
        Err(message) => failed_check("intrinsic-pixel-structure", Some(file), message),
    }
}

fn native_pixel_data_length_matches(
    object: &DefaultDicomObject,
    frame_count: usize,
    actual_bytes: usize,
) -> bool {
    let Some(rows) = positive_usize(object, tags::ROWS) else {
        return false;
    };
    let Some(columns) = positive_usize(object, tags::COLUMNS) else {
        return false;
    };
    let Some(samples_per_pixel) = positive_usize(object, tags::SAMPLES_PER_PIXEL) else {
        return false;
    };
    let Some(bits_allocated) = positive_usize(object, tags::BITS_ALLOCATED) else {
        return false;
    };
    let photometric = object
        .element(tags::PHOTOMETRIC_INTERPRETATION)
        .ok()
        .and_then(|element| element.to_str().ok());
    let samples_per_frame = if photometric
        .as_deref()
        .is_some_and(|value| value.trim() == "YBR_FULL_422")
    {
        columns
            .checked_add(1)
            .map(|value| value / 2)
            .and_then(|pairs| pairs.checked_mul(4))
            .and_then(|samples| samples.checked_mul(rows))
    } else {
        rows.checked_mul(columns)
            .and_then(|value| value.checked_mul(samples_per_pixel))
    };
    let Some(sample_count) = samples_per_frame.and_then(|value| value.checked_mul(frame_count))
    else {
        return false;
    };
    let Some(total_bits) = sample_count.checked_mul(bits_allocated) else {
        return false;
    };
    let Some(rounded_bits) = total_bits.checked_add(7) else {
        return false;
    };
    let expected_bytes = rounded_bits / 8;
    let Some(padded_bytes) = expected_bytes.checked_add(expected_bytes % 2) else {
        return false;
    };
    actual_bytes == expected_bytes || actual_bytes == padded_bytes
}

fn positive_usize(object: &DefaultDicomObject, tag: dicom_core::Tag) -> Option<usize> {
    object
        .element(tag)
        .ok()?
        .to_int::<usize>()
        .ok()
        .filter(|value| *value > 0)
}

pub(super) fn dicom_frame_count(
    object: &dicom_object::DefaultDicomObject,
) -> Result<usize, String> {
    match object.element(tags::NUMBER_OF_FRAMES) {
        Ok(element) => match element.to_int::<usize>() {
            Ok(frame_count) if frame_count > 0 => Ok(frame_count),
            Ok(_) => Err("DICOM Number of Frames must be greater than zero".into()),
            Err(err) => Err(format!("failed to read DICOM Number of Frames: {err}")),
        },
        Err(err) => Err(format!("DICOM Number of Frames is missing: {err}")),
    }
}

fn pixel_data_transfer_syntax_is_compressed(transfer_syntax: &str) -> bool {
    transfer_syntax.starts_with("1.2.840.10008.1.2.4.") || transfer_syntax == "1.2.840.10008.1.2.5"
}

fn primitive_pixel_data_contains_encapsulated_item_stream(bytes: &[u8]) -> bool {
    const ITEM_TAG: [u8; 4] = [0xFE, 0xFF, 0x00, 0xE0];
    const SEQUENCE_DELIMITATION_TAG: [u8; 4] = [0xFE, 0xFF, 0xDD, 0xE0];
    const ITEM_HEADER_BYTES: usize = 8;

    // A malformed writer can assign a native transfer syntax and then serialize encapsulated
    // items with a defined Pixel Data length. In that case the object parser exposes primitive
    // bytes, so require a complete item-stream parse rather than relying on the first tag alone.
    let Some(mut cursor) = item_end(bytes, 0) else {
        return false;
    };
    let basic_offset_table_length = cursor - ITEM_HEADER_BYTES;
    if !basic_offset_table_length.is_multiple_of(4) {
        return false;
    }

    let mut fragment_count = 0_usize;
    while cursor < bytes.len() {
        if bytes[cursor..].starts_with(&ITEM_TAG) {
            let Some(next_cursor) = item_end(bytes, cursor) else {
                return false;
            };
            if !(next_cursor - cursor - ITEM_HEADER_BYTES).is_multiple_of(2) {
                return false;
            }
            fragment_count += 1;
            cursor = next_cursor;
            continue;
        }
        if bytes[cursor..].starts_with(&SEQUENCE_DELIMITATION_TAG) {
            let Some(header_end) = cursor.checked_add(ITEM_HEADER_BYTES) else {
                return false;
            };
            if header_end != bytes.len() {
                return false;
            }
            let Ok(length_bytes) = bytes[cursor + 4..header_end].try_into() else {
                return false;
            };
            let length = u32::from_le_bytes(length_bytes);
            return fragment_count > 0 && length == 0;
        }
        return false;
    }

    fragment_count > 0
}

fn item_end(bytes: &[u8], offset: usize) -> Option<usize> {
    const ITEM_TAG: [u8; 4] = [0xFE, 0xFF, 0x00, 0xE0];
    const ITEM_HEADER_BYTES: usize = 8;

    let header_end = offset.checked_add(ITEM_HEADER_BYTES)?;
    let header = bytes.get(offset..header_end)?;
    if !header.starts_with(&ITEM_TAG) {
        return None;
    }
    let length = u32::from_le_bytes(header[4..8].try_into().ok()?);
    if length == u32::MAX {
        return None;
    }
    header_end
        .checked_add(usize::try_from(length).ok()?)
        .filter(|end| *end <= bytes.len())
}

pub(super) fn optional_u64_values(
    object: &dicom_object::DefaultDicomObject,
    tag: dicom_core::Tag,
) -> Result<Option<Vec<u64>>, String> {
    let Ok(element) = object.element(tag) else {
        return Ok(None);
    };
    element
        .to_multi_int::<u64>()
        .map(Some)
        .map_err(|err| format!("failed to read DICOM element {tag}: {err}"))
}

pub(super) fn assemble_encapsulated_frames(
    sequence: &PixelFragmentSequence<Vec<u8>>,
    frame_count: usize,
    extended_offsets: Option<&[u64]>,
    extended_lengths: Option<&[u64]>,
    max_frames: usize,
    max_frame_bytes: usize,
) -> Result<Vec<Vec<u8>>, String> {
    let fragments = sequence.fragments();
    if fragments.is_empty() {
        return Err("Pixel Data has no fragments".to_string());
    }

    let basic_offsets = sequence.offset_table();
    let offsets = match extended_offsets {
        Some(offsets) if !offsets.is_empty() => EncapsulatedFrameOffsets::Extended(offsets),
        _ if !basic_offsets.is_empty() => EncapsulatedFrameOffsets::Basic(basic_offsets),
        _ => EncapsulatedFrameOffsets::None,
    };
    let lengths = extended_lengths.filter(|lengths| !lengths.is_empty());
    if lengths.is_some() && extended_offsets.is_none_or(<[u64]>::is_empty) {
        return Err("Extended Offset Table Lengths requires an Extended Offset Table".to_string());
    }
    if let Some(lengths) = lengths {
        if lengths.len() != frame_count {
            return Err(format!(
                "Extended Offset Table Lengths has {} entries for {frame_count} frames",
                lengths.len()
            ));
        }
    }

    visit_encapsulated_frame_spans(
        fragments,
        frame_count,
        offsets,
        lengths,
        max_frame_bytes,
        |_, _, _, _| Ok(()),
    )?;

    let selected_frames = max_frames.min(frame_count);
    let mut frames = Vec::new();
    frames
        .try_reserve_exact(selected_frames)
        .map_err(|_| "Pixel Data frame list exceeds available memory".to_string())?;
    visit_encapsulated_frame_spans(
        fragments,
        frame_count,
        offsets,
        lengths,
        max_frame_bytes,
        |frame_index, start, end, output_len| {
            if frame_index < selected_frames {
                let mut frame = Vec::new();
                frame.try_reserve_exact(output_len).map_err(|_| {
                    format!("Pixel Data frame {frame_index} exceeds available memory")
                })?;
                for fragment in &fragments[start..end] {
                    let remaining = output_len.saturating_sub(frame.len());
                    if remaining == 0 {
                        break;
                    }
                    frame.extend_from_slice(&fragment[..fragment.len().min(remaining)]);
                }
                frames.push(frame);
            }
            Ok(())
        },
    )?;
    Ok(frames)
}

#[derive(Clone, Copy)]
enum EncapsulatedFrameOffsets<'a> {
    None,
    Basic(&'a [u32]),
    Extended(&'a [u64]),
}

impl EncapsulatedFrameOffsets<'_> {
    fn len(self) -> usize {
        match self {
            Self::None => 0,
            Self::Basic(offsets) => offsets.len(),
            Self::Extended(offsets) => offsets.len(),
        }
    }

    fn get(self, index: usize) -> Option<u64> {
        match self {
            Self::None => None,
            Self::Basic(offsets) => offsets.get(index).copied().map(u64::from),
            Self::Extended(offsets) => offsets.get(index).copied(),
        }
    }
}

fn visit_encapsulated_frame_spans(
    fragments: &[Vec<u8>],
    frame_count: usize,
    offsets: EncapsulatedFrameOffsets<'_>,
    lengths: Option<&[u64]>,
    max_frame_bytes: usize,
    mut visit: impl FnMut(usize, usize, usize, usize) -> Result<(), String>,
) -> Result<(), String> {
    if offsets.len() == 0 {
        if frame_count == 1 {
            return validate_and_visit_frame_span(
                fragments,
                lengths,
                max_frame_bytes,
                0,
                0,
                fragments.len(),
                &mut visit,
            );
        }
        if frame_count != fragments.len() {
            return Err(format!(
                "cannot map {} Pixel Data fragments to {frame_count} frames without an offset table",
                fragments.len()
            ));
        }
        for frame_index in 0..frame_count {
            validate_and_visit_frame_span(
                fragments,
                lengths,
                max_frame_bytes,
                frame_index,
                frame_index,
                frame_index + 1,
                &mut visit,
            )?;
        }
        return Ok(());
    }

    if offsets.len() != frame_count {
        return Err(format!(
            "Pixel Data offset table has {} entries for {frame_count} frames",
            offsets.len()
        ));
    }
    if offsets.get(0) != Some(0)
        || (1..frame_count).any(|index| offsets.get(index - 1) >= offsets.get(index))
    {
        return Err("Pixel Data frame offsets are not strictly increasing from zero".to_string());
    }

    let mut fragment_index = 0usize;
    let mut fragment_offset = 0u64;
    for frame_index in 0..frame_count {
        let expected_start = offsets.get(frame_index).expect("validated offset count");
        if fragment_offset != expected_start {
            return Err(format!(
                "Pixel Data frame offset {expected_start} does not identify a fragment boundary"
            ));
        }
        let start = fragment_index;
        let expected_end = offsets.get(frame_index + 1);
        while fragment_index < fragments.len()
            && expected_end.is_none_or(|end| fragment_offset < end)
        {
            let fragment_len = u64::try_from(fragments[fragment_index].len())
                .map_err(|_| "Pixel Data fragment length exceeds u64".to_string())?;
            fragment_offset = fragment_offset
                .checked_add(8)
                .and_then(|offset| offset.checked_add(fragment_len))
                .ok_or_else(|| "Pixel Data fragment offsets overflow u64".to_string())?;
            fragment_index += 1;
        }
        if expected_end.is_some_and(|end| fragment_offset != end) {
            return Err(format!(
                "Pixel Data frame offset {} does not identify a fragment boundary",
                expected_end.expect("checked Some")
            ));
        }
        validate_and_visit_frame_span(
            fragments,
            lengths,
            max_frame_bytes,
            frame_index,
            start,
            fragment_index,
            &mut visit,
        )?;
    }
    if fragment_index != fragments.len() {
        return Err("Pixel Data offset table does not map all fragments".to_string());
    }
    Ok(())
}

fn validate_and_visit_frame_span(
    fragments: &[Vec<u8>],
    lengths: Option<&[u64]>,
    max_frame_bytes: usize,
    frame_index: usize,
    start: usize,
    end: usize,
    visit: &mut impl FnMut(usize, usize, usize, usize) -> Result<(), String>,
) -> Result<(), String> {
    if start >= end || end > fragments.len() {
        return Err(format!(
            "Pixel Data frame {frame_index} has an empty or invalid fragment span"
        ));
    }
    let assembled_len = fragments[start..end]
        .iter()
        .try_fold(0usize, |total, fragment| {
            total
                .checked_add(fragment.len())
                .ok_or_else(|| "assembled Pixel Data frame length overflows usize".to_string())
        })?;
    let output_len = match lengths {
        Some(lengths) => usize::try_from(lengths[frame_index]).map_err(|_| {
            format!("Pixel Data frame {frame_index} length exceeds platform limits")
        })?,
        None => assembled_len,
    };
    if output_len == 0 {
        return Err(format!("Pixel Data frame {frame_index} is empty"));
    }
    if output_len > assembled_len {
        if end == start + 1 && output_len.checked_sub(assembled_len) == Some(8) {
            return Err(format!(
                "Pixel Data frame {frame_index} Extended Offset Table Lengths entry includes the 8-byte Item header; DICOM requires only Frame payload bytes"
            ));
        }
        return Err(format!(
            "Pixel Data frame {frame_index} declares {output_len} bytes but only {assembled_len} are available"
        ));
    }
    if output_len > max_frame_bytes {
        return Err(format!(
            "Pixel Data frame {frame_index} exceeds {max_frame_bytes} byte validation limit"
        ));
    }
    visit(frame_index, start, end, output_len)
}

#[cfg(any(test, feature = "bench-internals"))]
pub(crate) fn fragment_payload_without_padding(fragment: &[u8]) -> &[u8] {
    fragment
}
