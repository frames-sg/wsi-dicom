//! Functional-group requirements for the selected single-path TILED_FULL profile.

use dicom_core::header::Header;
use dicom_dictionary_std::tags;
use dicom_object::DefaultDicomObject;

use super::fields::{required_string, sequence_items};

pub(super) fn validate_functional_groups(object: &DefaultDicomObject) -> Result<(), String> {
    let shared = sequence_items(
        object,
        tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
        "Shared Functional Groups Sequence",
    )?;
    if shared.len() != 1 {
        return Err("Shared Functional Groups Sequence must contain one item".into());
    }
    let shared = &shared[0];
    for (tag, name) in [
        (tags::PIXEL_MEASURES_SEQUENCE, "Pixel Measures Sequence"),
        (
            tags::WHOLE_SLIDE_MICROSCOPY_IMAGE_FRAME_TYPE_SEQUENCE,
            "Whole Slide Microscopy Image Frame Type Sequence",
        ),
    ] {
        if sequence_items(shared, tag, name)?.len() != 1 {
            return Err(format!("{name} must contain one shared item"));
        }
    }
    let frame_type = &sequence_items(
        shared,
        tags::WHOLE_SLIDE_MICROSCOPY_IMAGE_FRAME_TYPE_SEQUENCE,
        "Whole Slide Microscopy Image Frame Type Sequence",
    )?[0];
    let image_type = required_string(object, tags::IMAGE_TYPE, "Image Type")?;
    if required_string(frame_type, tags::FRAME_TYPE, "Frame Type")? != image_type {
        return Err("shared Frame Type must match Image Type in the core profile".into());
    }
    let frames = object
        .element(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .ok()
        .map(|element| {
            element
                .items()
                .ok_or_else(|| "Per-frame Functional Groups Sequence is not a sequence".to_string())
        })
        .transpose()?;
    if let Some(frames) = frames {
        for frame in frames {
            for element in shared {
                if frame.element(element.tag()).is_ok() {
                    return Err(format!(
                        "functional group {} occurs in both shared and per-frame sequences",
                        element.tag()
                    ));
                }
            }
        }
    }
    // DERIVED alone does not imply another SOP Instance exists. A source reference does.
    let references_source = image_type.starts_with("DERIVED")
        && (object.element(tags::SOURCE_IMAGE_SEQUENCE).is_ok()
            || object.element(tags::REFERENCED_SERIES_SEQUENCE).is_ok());
    let shared_derivation = shared.element(tags::DERIVATION_IMAGE_SEQUENCE).is_ok();
    if references_source
        && !shared_derivation
        && !frames.is_some_and(|frames| {
            !frames.is_empty()
                && frames
                    .iter()
                    .all(|frame| frame.element(tags::DERIVATION_IMAGE_SEQUENCE).is_ok())
        })
    {
        return Err("image derived from another SOP Instance requires the Derivation Image functional group".into());
    }
    for group in std::iter::once(shared).chain(frames.into_iter().flatten()) {
        if group.element(tags::DERIVATION_IMAGE_SEQUENCE).is_err() {
            continue;
        }
        let derivations = sequence_items(
            group,
            tags::DERIVATION_IMAGE_SEQUENCE,
            "Derivation Image Sequence",
        )?;
        if references_source && derivations.is_empty() {
            return Err("referenced derived image has an empty Derivation Image Sequence".into());
        }
        for derivation in derivations {
            if sequence_items(
                derivation,
                tags::DERIVATION_CODE_SEQUENCE,
                "Derivation Code Sequence",
            )?
            .is_empty()
            {
                return Err("Derivation Code Sequence is empty".into());
            }
            let sources = sequence_items(
                derivation,
                tags::SOURCE_IMAGE_SEQUENCE,
                "Derivation Source Image Sequence",
            )?;
            if sources.is_empty() {
                return Err("Derivation Source Image Sequence is empty".into());
            }
            for source in sources {
                required_string(
                    source,
                    tags::REFERENCED_SOP_CLASS_UID,
                    "Referenced SOP Class UID",
                )?;
                required_string(
                    source,
                    tags::REFERENCED_SOP_INSTANCE_UID,
                    "Referenced SOP Instance UID",
                )?;
                if sequence_items(
                    source,
                    tags::PURPOSE_OF_REFERENCE_CODE_SEQUENCE,
                    "Purpose of Reference Code Sequence",
                )?
                .is_empty()
                {
                    return Err("Purpose of Reference Code Sequence is empty".into());
                }
            }
        }
    }
    Ok(())
}
