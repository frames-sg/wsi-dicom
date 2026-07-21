use std::path::PathBuf;

use dicom_core::value::{PixelFragmentSequence, Value};
use dicom_dictionary_std::tags;

use super::{failed_check, ValidationCheck, ValidationStatus};

pub(super) fn run_intrinsic_pixel_structure_check(
    file: &PathBuf,
    max_frame_bytes: usize,
) -> ValidationCheck {
    let result = (|| -> Result<(), String> {
        let object = dicom_object::open_file(file)
            .map_err(|err| format!("failed to read DICOM file: {err}"))?;
        let transfer_syntax = object.meta().transfer_syntax.trim_end_matches('\0');
        let frame_count = dicom_frame_count(&object)?;
        let pixel_data = object
            .element(tags::PIXEL_DATA)
            .map_err(|err| format!("failed to read Pixel Data: {err}"))?;
        let compressed = pixel_data_transfer_syntax_is_compressed(transfer_syntax);

        match (compressed, pixel_data.value()) {
            (true, Value::PixelSequence(sequence)) => {
                if sequence.fragments().is_empty() {
                    return Err("compressed Pixel Data has no fragments".into());
                }
                let extended_offsets = optional_u64_values(&object, tags::EXTENDED_OFFSET_TABLE)?;
                let extended_lengths =
                    optional_u64_values(&object, tags::EXTENDED_OFFSET_TABLE_LENGTHS)?;
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
