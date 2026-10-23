use std::path::Path;

use dicom_core::value::Value;
use dicom_dictionary_std::tags;
use dicom_object::DefaultDicomObject;

use wsi_rs::{Compression, Slide, SourceEncoding};

use crate::coordinate::InstanceCoordinate;
use crate::lossy::{
    lossy_method_for_compression, method_for_lossy_transfer_syntax, uncompressed_pixel_bytes,
    LossyCompressionHistory,
};
use crate::Error;

/// Lossy history the source declares: DICOM Lossy Image Compression
/// attributes, or else the codec a reader reports for a level without raw
/// compressed tiles (Zeiss JPEG XR subblocks). Levels with raw tiles are
/// classified per frame instead.
pub(super) fn source_lossy_history(
    slide: &Slide,
    source_path: &Path,
    coordinate: InstanceCoordinate,
) -> Result<LossyCompressionHistory, Error> {
    let declared = declared_source_lossy_history(source_path)?;
    if !declared.is_empty() {
        return Ok(declared);
    }
    reported_level_history(slide.level_encoding(
        coordinate.scene_idx,
        coordinate.series_idx,
        coordinate.level_idx,
    ))
}

/// JPEG 2000 levels are left to the per-frame codestream inspection, which
/// tells reversible from irreversible coding; the reported codec cannot.
fn reported_level_history(
    encoding: Option<SourceEncoding>,
) -> Result<LossyCompressionHistory, Error> {
    match encoding {
        Some(encoding)
            if matches!(
                encoding.compression,
                Compression::Jp2kRgb | Compression::Jp2kYcbcr
            ) =>
        {
            Ok(LossyCompressionHistory::default())
        }
        encoding => reported_encoding_history(encoding, "source level"),
    }
}

/// One lossy stage for a source stored with a lossy codec, measured from the
/// reported decoded and stored sizes; empty for lossless or unreported storage.
pub(super) fn reported_encoding_history(
    encoding: Option<SourceEncoding>,
    what: &str,
) -> Result<LossyCompressionHistory, Error> {
    let Some(encoding) = encoding else {
        return Ok(LossyCompressionHistory::default());
    };
    let Some(method) = lossy_method_for_compression(encoding.compression)? else {
        return Ok(LossyCompressionHistory::default());
    };
    match (encoding.decoded_bytes, encoding.encoded_bytes) {
        (Some(decoded), Some(encoded)) => {
            LossyCompressionHistory::from_byte_counts(method, decoded, encoded)
        }
        _ => Err(Error::Metadata {
            reason: format!(
                "{what} is stored with lossy {method} compression of unknown size, so Lossy Image Compression Ratio cannot be recorded"
            ),
        }),
    }
}

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

    #[test]
    fn reported_lossy_codecs_record_one_measured_stage() {
        let encoding = SourceEncoding::new(wsi_rs::Compression::JpegXr).with_sizes(2_800, 100);
        let history = reported_encoding_history(Some(encoding), "level").unwrap();
        assert_eq!(history.stages().len(), 1);
        assert_eq!(history.stages()[0].method(), crate::lossy::JPEG_XR_METHOD);
        assert_eq!(history.stages()[0].ratio(), 28.0);

        let jpeg = SourceEncoding::new(wsi_rs::Compression::Jpeg).with_sizes(900, 90);
        let history = reported_encoding_history(Some(jpeg), "label").unwrap();
        assert_eq!(
            history.stages()[0].method(),
            crate::lossy::JPEG_BASELINE_METHOD
        );
    }

    #[test]
    fn reported_lossless_or_absent_encodings_record_no_stage() {
        for compression in [
            wsi_rs::Compression::None,
            wsi_rs::Compression::Lzw,
            wsi_rs::Compression::Deflate,
            wsi_rs::Compression::Zstd,
        ] {
            let encoding = SourceEncoding::new(compression).with_sizes(10, 5);
            assert!(reported_encoding_history(Some(encoding), "level")
                .unwrap()
                .is_empty());
        }
        assert!(reported_encoding_history(None, "level").unwrap().is_empty());
    }

    #[test]
    fn reported_jpeg_2000_levels_defer_to_codestream_inspection() {
        for compression in [wsi_rs::Compression::Jp2kRgb, wsi_rs::Compression::Jp2kYcbcr] {
            let encoding = SourceEncoding::new(compression).with_sizes(1_000, 100);
            assert!(reported_level_history(Some(encoding)).unwrap().is_empty());
        }
        let jpeg_xr = SourceEncoding::new(wsi_rs::Compression::JpegXr).with_sizes(1_000, 100);
        assert_eq!(
            reported_level_history(Some(jpeg_xr)).unwrap().stages()[0].method(),
            crate::lossy::JPEG_XR_METHOD
        );
    }

    #[test]
    fn reported_lossy_codec_without_sizes_or_unknown_codec_is_an_error() {
        let unsized_jpeg_xr = SourceEncoding::new(wsi_rs::Compression::JpegXr);
        let error = reported_encoding_history(Some(unsized_jpeg_xr), "source level")
            .unwrap_err()
            .to_string();
        assert!(error.contains("Lossy Image Compression Ratio"), "{error}");

        let unknown = SourceEncoding::new(wsi_rs::Compression::Other(34_712)).with_sizes(10, 5);
        let error = reported_encoding_history(Some(unknown), "label")
            .unwrap_err()
            .to_string();
        assert!(error.contains("cannot be classified"), "{error}");
    }
}
