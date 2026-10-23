use std::path::{Path, PathBuf};

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::DefaultDicomObject;

use wsi_rs::{Compression, Slide, SourceEncoding};

use crate::coordinate::InstanceCoordinate;
use crate::lossy::{
    lossy_method_for_compression, uncompressed_pixel_bytes, LossyCompressionHistory,
};
use crate::Error;

/// Every attribute read from a source header precedes this tag, so the
/// Per-Frame Functional Groups and Pixel Data are never parsed.
const HEADER_END: Tag = Tag(0x0028, 0x2115);

/// Lossy history the source declares in its DICOM Lossy Image Compression
/// attributes, or else the codec a reader reports for the level (Zeiss JPEG
/// XR subblocks, DICOM lossy transfer syntaxes). JPEG 2000 levels with raw
/// tiles are classified per frame instead.
pub(super) fn source_lossy_history(
    slide: &Slide,
    coordinate: InstanceCoordinate,
) -> Result<LossyCompressionHistory, Error> {
    if let Some(paths) = dicom_source_paths(
        slide,
        &format!("dicom.level.{}.source-files", coordinate.level_idx),
    )? {
        let declared = declared_lossy_history_for_parts(&paths)?;
        if !declared.is_empty() {
            return Ok(declared);
        }
    }
    let encoding = slide
        .level_encoding(
            coordinate.scene_idx,
            coordinate.series_idx,
            coordinate.level_idx,
        )
        .map_err(|source| Error::SlideRead {
            message: format!("level {} encoding: {source}", coordinate.level_idx),
        })?;
    reported_level_history(encoding)
}

/// Lossy history for an associated source image: the declared history of the
/// DICOM instance that supplied it, or else the reader's storage report.
pub(super) fn associated_source_lossy_history(
    slide: &Slide,
    name: &str,
    encoding: SourceEncoding,
) -> Result<LossyCompressionHistory, Error> {
    if let Some(paths) = dicom_source_paths(slide, &format!("dicom.associated.{name}.source-file"))?
    {
        let declared = declared_lossy_history_for_parts(&paths)?;
        if !declared.is_empty() {
            return Ok(declared);
        }
    }
    reported_encoding_history(Some(encoding), &format!("associated image '{name}'"))
}

/// Source instances a DICOM reader lists under `property`: a JSON path array
/// for a level, a single path for an associated image. A DICOM source that
/// does not list them (its paths are not UTF-8) cannot have its declared
/// history read, which is an error rather than a guess.
fn dicom_source_paths(slide: &Slide, property: &str) -> Result<Option<Vec<PathBuf>>, Error> {
    let properties = &slide.dataset().properties;
    let Some(value) = properties.get(property) else {
        if properties
            .get("openslide.vendor")
            .is_some_and(|vendor| vendor == "dicom")
        {
            return Err(Error::Metadata {
                reason: format!(
                    "the source DICOM reader does not list {property}, so the declared lossy compression history of those instances cannot be read"
                ),
            });
        }
        return Ok(None);
    };
    if !property.ends_with("source-files") {
        return Ok(Some(vec![PathBuf::from(value)]));
    }
    let paths: Vec<PathBuf> = serde_json::from_str(value).map_err(|err| Error::Metadata {
        reason: format!("source DICOM property {property} is not a JSON path array: {err}"),
    })?;
    if paths.is_empty() {
        return Err(Error::Metadata {
            reason: format!("source DICOM property {property} contains no source files"),
        });
    }
    Ok(Some(paths))
}

/// A DICOM level can be assembled from several SOP Instances. Their declared
/// histories describe parallel pieces of one output image, not successive
/// stages, so they must name the same methods; each stage's ratio is combined
/// over the parts, weighted by their decoded size.
fn declared_lossy_history_for_parts(paths: &[PathBuf]) -> Result<LossyCompressionHistory, Error> {
    let parts = paths
        .iter()
        .map(|path| {
            let header = read_dicom_header(path)?;
            let history = declared_lossy_history(&header, path)?;
            Ok((path.as_path(), header, history))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let Some((first_path, _, first)) = parts.first() else {
        return Err(Error::Metadata {
            reason: "source DICOM level contains no source files".into(),
        });
    };
    let methods = |history: &LossyCompressionHistory| {
        history
            .stages()
            .iter()
            .map(|stage| stage.method().to_owned())
            .collect::<Vec<_>>()
    };
    if let Some((path, _, _)) = parts
        .iter()
        .find(|(_, _, history)| methods(history) != methods(first))
    {
        return Err(Error::Metadata {
            reason: format!(
                "source DICOM level parts {} and {} declare different lossy compression methods",
                first_path.display(),
                path.display()
            ),
        });
    }
    if parts.iter().all(|(_, _, history)| history == first) {
        return Ok(first.clone());
    }
    let weights = parts
        .iter()
        .map(|(path, header, _)| decoded_bytes(header, path).map(|bytes| bytes as f64))
        .collect::<Result<Vec<_>, Error>>()?;
    let total: f64 = weights.iter().sum();
    let mut combined = LossyCompressionHistory::default();
    for (index, stage) in first.stages().iter().enumerate() {
        let compressed: f64 = parts
            .iter()
            .zip(&weights)
            .map(|((_, _, history), weight)| weight / history.stages()[index].ratio())
            .sum();
        combined.push_declared(stage.method(), total / compressed)?;
    }
    Ok(combined)
}

/// JPEG 2000 levels are left to per-frame codestream inspection so exact
/// copied-frame sizes, rather than a level-wide estimate, determine history.
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
    if encoding.lossy == Some(false) {
        return Ok(LossyCompressionHistory::default());
    }
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

fn read_dicom_header(path: &Path) -> Result<DefaultDicomObject, Error> {
    dicom_object::OpenFileOptions::new()
        .read_until(HEADER_END)
        .open_file(path)
        .map_err(|err| Error::Metadata {
            reason: format!(
                "cannot read source DICOM {} while preserving lossy compression history: {err}",
                path.display()
            ),
        })
}

/// The stages a header declares with Lossy Image Compression "01"; empty for
/// "00" or an absent flag, whose transfer syntax the reader reports instead.
fn declared_lossy_history(
    object: &DefaultDicomObject,
    source_path: &Path,
) -> Result<LossyCompressionHistory, Error> {
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
        Some("01") => {}
        Some("00") | None => return Ok(LossyCompressionHistory::default()),
        Some(value) => {
            return Err(Error::Metadata {
                reason: format!(
                    "source DICOM {} has invalid Lossy Image Compression value {value:?}",
                    source_path.display()
                ),
            })
        }
    }
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

fn decoded_bytes(object: &DefaultDicomObject, source_path: &Path) -> Result<u64, Error> {
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
    uncompressed_pixel_bytes(columns, rows, samples_per_pixel, bits_allocated)?
        .checked_mul(frame_count)
        .ok_or_else(|| Error::Metadata {
            reason: format!(
                "source DICOM {} decoded byte count overflow",
                source_path.display()
            ),
        })
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
                "source DICOM {} declares lossy compression but has no {name}",
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

    fn write_declared_history_source(path: &Path, ratios: &[f64]) {
        write_declared_history_part(path, &[crate::lossy::JPEG_BASELINE_METHOD], ratios, 1);
    }

    /// A 2x2 RGB header of `frames` frames declaring `methods` with `ratios`.
    fn write_declared_history_part(path: &Path, methods: &[&str], ratios: &[f64], frames: u32) {
        let mut object = InMemDicomObject::new_empty();
        for (tag, value) in [
            (tags::ROWS, 2_u16),
            (tags::COLUMNS, 2),
            (tags::SAMPLES_PER_PIXEL, 3),
            (tags::BITS_ALLOCATED, 8),
        ] {
            object.put(DataElement::new(tag, VR::US, PrimitiveValue::from(value)));
        }
        object.put(DataElement::new(
            tags::NUMBER_OF_FRAMES,
            VR::IS,
            frames.to_string(),
        ));
        object.put(DataElement::new(
            tags::SOP_CLASS_UID,
            VR::UI,
            uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE,
        ));
        object.put(DataElement::new(
            tags::SOP_INSTANCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.802",
        ));
        object.put(DataElement::new(
            tags::LOSSY_IMAGE_COMPRESSION,
            VR::CS,
            if ratios.is_empty() { "00" } else { "01" },
        ));
        if !ratios.is_empty() {
            object.put(DataElement::new(
                tags::LOSSY_IMAGE_COMPRESSION_METHOD,
                VR::CS,
                methods
                    .iter()
                    .cycle()
                    .take(ratios.len())
                    .copied()
                    .collect::<Vec<_>>()
                    .join("\\"),
            ));
            object.put(DataElement::new(
                tags::LOSSY_IMAGE_COMPRESSION_RATIO,
                VR::DS,
                ratios
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\\"),
            ));
        }
        object
            .with_meta(
                FileMetaTableBuilder::new()
                    .media_storage_sop_class_uid(uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE)
                    .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.999.802")
                    .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN),
            )
            .unwrap()
            .write_to_file(path)
            .unwrap();
    }

    #[test]
    fn merged_dicom_parts_record_one_matching_history() {
        let temporary = tempfile::tempdir().unwrap();
        let first = temporary.path().join("part-1.dcm");
        let second = temporary.path().join("part-2.dcm");
        write_declared_history_source(&first, &[8.5, 3.25]);
        write_declared_history_source(&second, &[8.5, 3.25]);

        let history = declared_lossy_history_for_parts(&[first, second]).unwrap();

        assert_eq!(history.stages().len(), 2);
        assert_eq!(history.stages()[0].ratio(), 8.5);
        assert_eq!(history.stages()[1].ratio(), 3.25);
    }

    #[test]
    fn merged_dicom_parts_combine_ratios_weighted_by_decoded_size() {
        let temporary = tempfile::tempdir().unwrap();
        let first = temporary.path().join("part-1.dcm");
        let second = temporary.path().join("part-2.dcm");
        let method = crate::lossy::JPEG_BASELINE_METHOD;
        // 3 frames at 10:1 and 1 frame at 5:1 store 3/10 + 1/5 of 4 frames.
        write_declared_history_part(&first, &[method], &[10.0], 3);
        write_declared_history_part(&second, &[method], &[5.0], 1);

        let history = declared_lossy_history_for_parts(&[first, second]).unwrap();

        assert_eq!(history.stages().len(), 1);
        assert_eq!(history.stages()[0].method(), method);
        assert!((history.stages()[0].ratio() - 8.0).abs() < 1e-9);
    }

    #[test]
    fn merged_dicom_parts_reject_different_methods_or_lossiness() {
        let temporary = tempfile::tempdir().unwrap();
        let jpeg = temporary.path().join("part-1.dcm");
        let jpeg_2000 = temporary.path().join("part-2.dcm");
        let lossless = temporary.path().join("part-3.dcm");
        write_declared_history_part(&jpeg, &[crate::lossy::JPEG_BASELINE_METHOD], &[8.5], 1);
        write_declared_history_part(&jpeg_2000, &[crate::lossy::JPEG_2000_METHOD], &[8.5], 1);
        write_declared_history_part(&lossless, &[], &[], 1);

        for other in [jpeg_2000, lossless] {
            let error = declared_lossy_history_for_parts(&[jpeg.clone(), other])
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("different lossy compression methods"),
                "{error}"
            );
            assert!(error.contains("part-1.dcm"), "{error}");
        }
    }

    #[test]
    fn declared_history_does_not_read_pixel_data() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("truncated-pixels.dcm");
        write_declared_history_source(&path, &[8.5]);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        std::io::Write::write_all(
            &mut file,
            &[0xE0, 0x7F, 0x10, 0x00, b'O', b'B', 0, 0, 4, 0, 0, 0],
        )
        .unwrap();

        assert!(dicom_object::open_file(&path).is_err());
        let history = declared_lossy_history_for_parts(&[path]).unwrap();
        assert_eq!(history.stages().len(), 1);
        assert_eq!(history.stages()[0].ratio(), 8.5);
    }

    #[test]
    fn recognized_dicom_source_path_errors_are_not_ignored() {
        let temporary = tempfile::tempdir().unwrap();
        let missing = temporary.path().join("missing.dcm");

        let error = declared_lossy_history_for_parts(&[missing])
            .unwrap_err()
            .to_string();

        assert!(error.contains("cannot read source DICOM"), "{error}");
    }

    #[test]
    fn undeclared_parts_leave_lossy_transfer_syntaxes_to_the_reader() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("lossy-source.dcm");
        let mut object = InMemDicomObject::new_empty();
        object.put(DataElement::new(
            tags::SOP_INSTANCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.801",
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

        // Without a declaration the reader's report of the stored codec and
        // sizes supplies the stage; JPEG 2000 is left to codestream inspection.
        assert!(declared_lossy_history_for_parts(&[path])
            .unwrap()
            .is_empty());
        let reported = SourceEncoding::new(Compression::Jpeg)
            .with_sizes(12, 4)
            .with_lossy(true);
        let history = reported_level_history(Some(reported)).unwrap();
        assert_eq!(history.stages().len(), 1);
        assert_eq!(
            history.stages()[0].method(),
            crate::lossy::JPEG_BASELINE_METHOD
        );
        assert_eq!(history.stages()[0].ratio(), 3.0);
    }

    #[test]
    fn reported_lossy_codecs_record_one_measured_stage() {
        let encoding = SourceEncoding::new(Compression::JpegXr).with_sizes(2_800, 100);
        let history = reported_encoding_history(Some(encoding), "level").unwrap();
        assert_eq!(history.stages().len(), 1);
        assert_eq!(history.stages()[0].method(), crate::lossy::JPEG_XR_METHOD);
        assert_eq!(history.stages()[0].ratio(), 28.0);

        let jpeg = SourceEncoding::new(Compression::Jpeg).with_sizes(900, 90);
        let history = reported_encoding_history(Some(jpeg), "label").unwrap();
        assert_eq!(
            history.stages()[0].method(),
            crate::lossy::JPEG_BASELINE_METHOD
        );
    }

    #[test]
    fn reported_lossless_or_absent_encodings_record_no_stage() {
        for compression in [
            Compression::None,
            Compression::Lzw,
            Compression::Deflate,
            Compression::Zstd,
        ] {
            let encoding = SourceEncoding::new(compression).with_sizes(10, 5);
            assert!(reported_encoding_history(Some(encoding), "level")
                .unwrap()
                .is_empty());
        }
        assert!(reported_encoding_history(None, "level").unwrap().is_empty());
    }

    #[test]
    fn authoritative_lossless_mode_avoids_conservative_jpeg_xr_and_jpeg_ls_marking() {
        for compression in [Compression::JpegXr, Compression::JpegLs] {
            let lossless = SourceEncoding::new(compression)
                .with_sizes(1_000, 100)
                .with_lossy(false);
            assert!(reported_encoding_history(Some(lossless), "level")
                .unwrap()
                .is_empty());

            let lossy = SourceEncoding::new(compression)
                .with_sizes(1_000, 100)
                .with_lossy(true);
            assert_eq!(
                reported_encoding_history(Some(lossy), "level")
                    .unwrap()
                    .stages()
                    .len(),
                1
            );
        }
    }

    #[test]
    fn reported_jpeg_2000_levels_defer_to_codestream_inspection() {
        for compression in [Compression::Jp2kRgb, Compression::Jp2kYcbcr] {
            let encoding = SourceEncoding::new(compression).with_sizes(1_000, 100);
            assert!(reported_level_history(Some(encoding)).unwrap().is_empty());
        }
        let jpeg_xr = SourceEncoding::new(Compression::JpegXr).with_sizes(1_000, 100);
        assert_eq!(
            reported_level_history(Some(jpeg_xr)).unwrap().stages()[0].method(),
            crate::lossy::JPEG_XR_METHOD
        );
    }

    #[test]
    fn reported_lossy_codec_without_sizes_or_unknown_codec_is_an_error() {
        let unsized_jpeg_xr = SourceEncoding::new(Compression::JpegXr);
        let error = reported_encoding_history(Some(unsized_jpeg_xr), "source level")
            .unwrap_err()
            .to_string();
        assert!(error.contains("Lossy Image Compression Ratio"), "{error}");

        let unknown = SourceEncoding::new(Compression::Other(34_712)).with_sizes(10, 5);
        let error = reported_encoding_history(Some(unknown), "label")
            .unwrap_err()
            .to_string();
        assert!(error.contains("cannot be classified"), "{error}");
    }
}
