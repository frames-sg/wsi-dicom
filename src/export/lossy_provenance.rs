use std::path::Path;

use dicom_core::value::Value;
use dicom_dictionary_std::tags;
use dicom_object::DefaultDicomObject;

use crate::lossy::{
    method_for_lossy_transfer_syntax, uncompressed_pixel_bytes, LossyCompressionHistory,
};
use crate::Error;

pub(super) fn declared_source_lossy_history(
    source_path: &Path,
) -> Result<LossyCompressionHistory, Error> {
    let object = match dicom_object::open_file(source_path) {
        Ok(object) => object,
        Err(_) => return Ok(LossyCompressionHistory::default()),
    };
    let flag = object
        .element(tags::LOSSY_IMAGE_COMPRESSION)
        .ok()
        .map(|element| {
            element
                .to_str()
                .map(|value| value.trim().to_string())
                .map_err(|err| Error::Metadata {
                    reason: format!(
                        "source DICOM {} has invalid Lossy Image Compression: {err}",
                        source_path.display()
                    ),
                })
        })
        .transpose()?;
    match flag.as_deref() {
        Some("01") => declared_lossy_history(&object, source_path),
        Some("00") | None => {
            let Some(method) = method_for_lossy_transfer_syntax(&object.meta().transfer_syntax)
            else {
                return Ok(LossyCompressionHistory::default());
            };
            inferred_transfer_syntax_history(&object, source_path, method)
        }
        Some(value) => Err(Error::Metadata {
            reason: format!(
                "source DICOM {} has invalid Lossy Image Compression value {value:?}",
                source_path.display()
            ),
        }),
    }
}

fn declared_lossy_history(
    object: &DefaultDicomObject,
    source_path: &Path,
) -> Result<LossyCompressionHistory, Error> {
    let methods = object
        .element(tags::LOSSY_IMAGE_COMPRESSION_METHOD)
        .map_err(|_| Error::Metadata {
            reason: format!(
                "source DICOM {} declares lossy pixels without Lossy Image Compression Method",
                source_path.display()
            ),
        })?
        .to_multi_str()
        .map_err(|err| Error::Metadata {
            reason: format!(
                "source DICOM {} has invalid Lossy Image Compression Method: {err}",
                source_path.display()
            ),
        })?;
    let ratios = object
        .element(tags::LOSSY_IMAGE_COMPRESSION_RATIO)
        .map_err(|_| Error::Metadata {
            reason: format!(
                "source DICOM {} declares lossy pixels without Lossy Image Compression Ratio",
                source_path.display()
            ),
        })?
        .to_multi_float64()
        .map_err(|err| Error::Metadata {
            reason: format!(
                "source DICOM {} has invalid Lossy Image Compression Ratio: {err}",
                source_path.display()
            ),
        })?;
    if methods.len() != ratios.len() || methods.is_empty() {
        return Err(Error::Metadata {
            reason: format!(
                "source DICOM {} has mismatched or empty lossy method and ratio histories",
                source_path.display()
            ),
        });
    }

    let mut history = LossyCompressionHistory::default();
    for (method, ratio) in methods.iter().zip(ratios) {
        history.push_declared(method.trim(), ratio)?;
    }
    Ok(history)
}

fn inferred_transfer_syntax_history(
    object: &DefaultDicomObject,
    source_path: &Path,
    method: &'static str,
) -> Result<LossyCompressionHistory, Error> {
    let rows = required_u64(object, source_path, tags::ROWS, "Rows")?;
    let columns = required_u64(object, source_path, tags::COLUMNS, "Columns")?;
    let samples_per_pixel = required_u64(
        object,
        source_path,
        tags::SAMPLES_PER_PIXEL,
        "Samples per Pixel",
    )?;
    let bits_allocated = u16::try_from(required_u64(
        object,
        source_path,
        tags::BITS_ALLOCATED,
        "Bits Allocated",
    )?)
    .map_err(|_| Error::Metadata {
        reason: format!(
            "source DICOM {} has Bits Allocated outside the u16 range",
            source_path.display()
        ),
    })?;
    let frame_count = match object.element(tags::NUMBER_OF_FRAMES) {
        Ok(element) => element.to_int::<u64>().map_err(|err| Error::Metadata {
            reason: format!(
                "source DICOM {} has invalid Number of Frames: {err}",
                source_path.display()
            ),
        })?,
        Err(_) => 1,
    };
    let uncompressed_bytes =
        uncompressed_pixel_bytes(columns, rows, samples_per_pixel, bits_allocated)?
            .checked_mul(frame_count)
            .ok_or_else(|| Error::Metadata {
                reason: format!(
                    "source DICOM {} lossy uncompressed byte count overflow",
                    source_path.display()
                ),
            })?;
    let pixel_data = object
        .element(tags::PIXEL_DATA)
        .map_err(|_| Error::Metadata {
            reason: format!(
                "source DICOM {} has a lossy transfer syntax but no Pixel Data",
                source_path.display()
            ),
        })?;
    let Value::PixelSequence(sequence) = pixel_data.value() else {
        return Err(Error::Metadata {
            reason: format!(
                "source DICOM {} has a lossy transfer syntax without encapsulated Pixel Data",
                source_path.display()
            ),
        });
    };
    let compressed_bytes = sequence
        .fragments()
        .iter()
        .try_fold(0_u64, |total, fragment| {
            let length = u64::try_from(fragment.len()).map_err(|_| Error::Metadata {
                reason: format!(
                    "source DICOM {} compressed fragment size exceeds u64",
                    source_path.display()
                ),
            })?;
            total.checked_add(length).ok_or_else(|| Error::Metadata {
                reason: format!(
                    "source DICOM {} compressed Pixel Data byte count overflow",
                    source_path.display()
                ),
            })
        })?;
    LossyCompressionHistory::from_byte_counts(method, uncompressed_bytes, compressed_bytes)
}

fn required_u64(
    object: &DefaultDicomObject,
    source_path: &Path,
    tag: dicom_core::Tag,
    name: &str,
) -> Result<u64, Error> {
    object
        .element(tag)
        .map_err(|_| Error::Metadata {
            reason: format!(
                "source DICOM {} has a lossy transfer syntax but no {name}",
                source_path.display()
            ),
        })?
        .to_int::<u64>()
        .map_err(|err| Error::Metadata {
            reason: format!(
                "source DICOM {} has invalid {name}: {err}",
                source_path.display()
            ),
        })
}

#[cfg(test)]
mod tests {
    use dicom_core::value::{PixelFragmentSequence, Value};
    use dicom_core::{DataElement, PrimitiveValue, VR};
    use dicom_dictionary_std::{tags, uids};
    use dicom_object::{FileMetaTableBuilder, InMemDicomObject};

    use super::*;

    #[test]
    fn lossy_transfer_syntax_recovers_history_when_declaration_is_missing() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("lossy-source.dcm");
        let mut object = InMemDicomObject::new_empty();
        for (tag, value) in [
            (
                tags::SOP_CLASS_UID,
                uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE,
            ),
            (tags::SOP_INSTANCE_UID, "1.2.826.0.1.3680043.10.999.801"),
        ] {
            object.put(DataElement::new(tag, VR::UI, value));
        }
        object.put(DataElement::new(
            tags::ROWS,
            VR::US,
            PrimitiveValue::from(2_u16),
        ));
        object.put(DataElement::new(
            tags::COLUMNS,
            VR::US,
            PrimitiveValue::from(2_u16),
        ));
        object.put(DataElement::new(tags::NUMBER_OF_FRAMES, VR::IS, "1"));
        object.put(DataElement::new(
            tags::SAMPLES_PER_PIXEL,
            VR::US,
            PrimitiveValue::from(3_u16),
        ));
        object.put(DataElement::new(
            tags::BITS_ALLOCATED,
            VR::US,
            PrimitiveValue::from(8_u16),
        ));
        object.put(DataElement::<InMemDicomObject>::new(
            tags::PIXEL_DATA,
            VR::OB,
            Value::PixelSequence(PixelFragmentSequence::new_fragments(vec![vec![
                0xFF, 0xD8, 0xFF, 0xD9,
            ]])),
        ));
        object
            .with_meta(
                FileMetaTableBuilder::new()
                    .media_storage_sop_class_uid(uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE)
                    .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.999.801")
                    .transfer_syntax(uids::JPEG_BASELINE8_BIT),
            )
            .unwrap()
            .write_to_file(&path)
            .unwrap();

        let history = declared_source_lossy_history(&path).unwrap();

        assert_eq!(history.stages().len(), 1);
        assert_eq!(
            history.stages()[0].method(),
            crate::lossy::JPEG_BASELINE_METHOD
        );
        assert!(history.stages()[0].ratio() > 0.0);
    }
}
