//! Associated images (label, overview, thumbnail) written as single-frame VL
//! Whole Slide Microscopy instances whose Image Type Value 3 is LABEL,
//! OVERVIEW or THUMBNAIL (PS3.3 C.8.12.4.1.1). Number of Frames is
//! Enumerated as 1 for these flavours, so each image is one frame.

use std::time::Instant;

use wsi_rs::Slide;

use super::icc_profile::{resolve_icc_profile, resolve_label_camera_icc_profile};
use super::jobs::refuse_existing_output;
use super::jpeg_baseline::{encode_jpeg_baseline_cpu_fragment, jpeg_baseline_cpu_restart_interval};
use super::jpeg_baseline_pipeline::{jpeg_baseline_output_profile, jpeg_baseline_samples};
use super::jpeg_lossless_instance::encode_jpeg_lossless_frame;
use super::lossless_j2k_cpu::lossless_j2k_samples;
use super::lossy_provenance::associated_source_lossy_history;
use super::{level_pixel_spacing_mm, require_pixel_spacing_mm};
use crate::coordinate::InstanceCoordinate;
use crate::encode::encode_dicom_lossless;
use crate::error::Error;
use crate::instance_context::{associated_output_path, vl_wsi_file_meta, AssociatedInstanceIds};
use crate::lossy::{uncompressed_pixel_bytes, LossyCompressionHistory, JPEG_BASELINE_METHOD};
use crate::metadata::DicomMetadata;
use crate::options::{
    AssociatedImageKind, ConformanceLevel, NormalizedExportOptions, PixelReencodePolicy,
    TransferSyntax,
};
use crate::report::{ExportMetrics, ImageFlavor, InstanceReport, SkippedAssociatedImage};
use crate::request::ExportRequest;
use crate::routing::j2k_encoded_lossless_profile;
use crate::tile::{prepare_tile_samples_with_limit, PixelProfile};
use crate::uid::DicomExportIdentity;
use crate::writer::{
    build_dicom_object, write_single_frame_dicom_object, DicomObjectIdentifiers, DicomObjectParams,
    FrameGrid,
};

/// One associated image to write, with the decisions that need no pixels.
pub(super) struct AssociatedImageJob {
    pub(super) name: String,
    pub(super) kind: AssociatedImageKind,
    pub(super) source_is_compressed: bool,
    pub(super) transfer_syntax: TransferSyntax,
    pub(super) source_lossy: LossyCompressionHistory,
}

/// Associated images an export writes, and the ones it cannot write.
#[derive(Default)]
pub(super) struct AssociatedImagePlan {
    pub(super) jobs: Vec<AssociatedImageJob>,
    pub(super) skipped: Vec<SkippedAssociatedImage>,
}

/// Plan the source's associated images. A level-filtered export writes only
/// that level, and excluded kinds are left out silently because the caller
/// asked for that. Images without a DICOM flavour, or whose storage the
/// reader does not report (so their lossy history is unknown), are skipped
/// and reported.
pub(super) fn plan_associated_images(
    slide: &Slide,
    request: &ExportRequest,
    options: &NormalizedExportOptions,
) -> Result<AssociatedImagePlan, Error> {
    let mut plan = AssociatedImagePlan::default();
    // They are optional instances of the IOD.
    if request.level_filter.is_some() || options.semantics.conformance == ConformanceLevel::Required
    {
        return Ok(plan);
    }
    let mut names = slide.dataset().associated_images.keys().collect::<Vec<_>>();
    names.sort();
    for name in names {
        let Some(kind) = AssociatedImageKind::from_source_name(name) else {
            plan.skipped.push(skipped(
                name,
                "no LABEL, OVERVIEW or THUMBNAIL flavour corresponds to this associated image",
            ));
            continue;
        };
        if options.semantics.exclude_associated.excludes(kind) {
            continue;
        }
        let encoding = slide
            .associated_encoding(name)
            .map_err(|source| Error::SlideRead {
                message: format!("associated image '{name}': {source}"),
            })?;
        let Some(encoding) = encoding else {
            plan.skipped.push(skipped(
                name,
                "the source reader does not report how this image is stored, so its lossy compression history is unknown",
            ));
            continue;
        };
        if let Some(existing) = plan.jobs.iter().find(|job| job.kind == kind) {
            return Err(Error::Unsupported {
                reason: format!(
                    "associated images '{}' and '{name}' are both {}; exclude that kind to export",
                    existing.name,
                    ImageFlavor::from(kind).as_dicom()
                ),
            });
        }
        let source_is_compressed = crate::lossy::is_image_codec(encoding.compression);
        plan.jobs.push(AssociatedImageJob {
            name: name.clone(),
            kind,
            source_is_compressed,
            transfer_syntax: associated_transfer_syntax(
                options.semantics.transfer_syntax,
                options.semantics.pixel_reencode,
                source_is_compressed,
                name,
            )?,
            source_lossy: associated_source_lossy_history(slide, name, encoding)?,
        });
    }
    Ok(plan)
}

/// Refuse to replace existing associated image files unless overwrite is set.
pub(super) fn preflight_associated_output_paths(
    request: &ExportRequest,
    options: &NormalizedExportOptions,
    jobs: &[AssociatedImageJob],
) -> Result<(), Error> {
    jobs.iter().try_for_each(|job| {
        refuse_existing_output(
            associated_output_path(&request.output_dir, job.kind),
            options.semantics.overwrite,
        )
    })
}

fn skipped(name: &str, reason: &str) -> SkippedAssociatedImage {
    SkippedAssociatedImage {
        name: name.to_owned(),
        reason: reason.to_owned(),
    }
}

/// Inputs shared by the associated images of one export.
#[derive(Clone, Copy)]
pub(super) struct AssociatedExportContext<'a> {
    pub(super) slide: &'a Slide,
    pub(super) request: &'a ExportRequest,
    pub(super) options: &'a NormalizedExportOptions,
    pub(super) metadata: &'a DicomMetadata,
    pub(super) identity: &'a DicomExportIdentity,
}

/// Write each planned associated image, numbering instances after the
/// `pyramid_instance_count` pyramid instances.
pub(super) fn export_associated_images(
    context: AssociatedExportContext<'_>,
    jobs: &[AssociatedImageJob],
    pyramid_instance_count: usize,
) -> Result<Vec<InstanceReport>, Error> {
    jobs.iter()
        .enumerate()
        .map(|(offset, job)| {
            let instance_number = pyramid_instance_count
                .checked_add(offset + 1)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| Error::Unsupported {
                    reason: "DICOM instance count exceeds u32".into(),
                })?;
            export_associated_image(context, job, instance_number)
        })
        .collect()
}

fn export_associated_image(
    context: AssociatedExportContext<'_>,
    job: &AssociatedImageJob,
    instance_number: u32,
) -> Result<InstanceReport, Error> {
    let AssociatedExportContext {
        slide,
        request,
        options,
        metadata,
        identity,
    } = context;
    let flavor = ImageFlavor::from(job.kind);
    let mut metrics = ExportMetrics::default();
    let transfer_syntax = job.transfer_syntax;
    let mut lossy_compression = job.source_lossy.clone();

    let decode_started = Instant::now();
    let tile = slide
        .read_associated(&job.name)
        .map_err(|source| Error::SlideRead {
            message: format!("associated image '{}': {source}", job.name),
        })?;
    let (width, height) = (tile.width(), tile.height());
    if u16::try_from(width).is_err() || u16::try_from(height).is_err() {
        return Err(Error::Unsupported {
            reason: format!(
                "associated image '{}' is {width}x{height}; a single-frame {} instance holds at most 65535 rows and columns",
                job.name,
                flavor.as_dicom()
            ),
        });
    }
    let max_prepared_bytes =
        usize::try_from(options.resources.max_prepared_frame_bytes).unwrap_or(usize::MAX);
    let prepared = prepare_tile_samples_with_limit(&tile, width, height, max_prepared_bytes)?;
    metrics.timings.input_decode_micros = decode_started.elapsed().as_micros();

    if job.source_is_compressed {
        metrics.record_compressed_source_reencode();
    }
    let ids = AssociatedInstanceIds::new(identity, &request.output_dir, job.kind)?;
    let (pixel_spacing_mm, icc, derivation_source) = match job.kind {
        AssociatedImageKind::Thumbnail => {
            let level_zero = level_zero(slide)?;
            let spacing = thumbnail_pixel_spacing_mm(slide, level_zero, options, width, height)?;
            let icc = resolve_icc_profile(
                slide,
                request,
                metadata,
                InstanceCoordinate::new(0, 0, 0, 0, 0, 0),
                level_zero,
                prepared.profile,
            )?;
            (
                Some(spacing),
                icc,
                Some(ids.level_zero_sop_instance_uid.as_str()),
            )
        }
        AssociatedImageKind::Label | AssociatedImageKind::Overview => {
            let embedded = slide
                .dataset()
                .associated_images
                .get(&job.name)
                .and_then(|image| image.icc_profile());
            let icc = resolve_label_camera_icc_profile(
                &request.color_management,
                embedded,
                prepared.profile,
                &job.name,
            )?;
            (None, icc, None)
        }
    };

    let encode_started = Instant::now();
    let encoded = encode_frame(
        &prepared.bytes,
        prepared.profile,
        width,
        height,
        transfer_syntax,
        options,
    )?;
    metrics.timings.encode_micros = encode_started.elapsed().as_micros();
    lossy_compression.append(encoded.lossy_compression);

    let object = build_dicom_object(DicomObjectParams {
        metadata,
        identifiers: DicomObjectIdentifiers {
            study_uid: identity.study_uid(),
            specimen_uid: identity.specimen_uid(),
            series_uid: &ids.series_uid,
            sop_instance_uid: &ids.sop_instance_uid,
            frame_of_reference_uid: &ids.frame_of_reference_uid,
            pyramid: None,
            dimension_organization_uid: &ids.dimension_organization_uid,
            derivation_source_sop_instance_uid: derivation_source,
        },
        series_number: ids.series_number,
        instance_number,
        flavor,
        level_idx: 0,
        frame_grid: FrameGrid {
            frame_columns: width,
            frame_rows: height,
            matrix_columns: u64::from(width),
            matrix_rows: u64::from(height),
        },
        frame_count: 1,
        profile: encoded.profile,
        pixel_spacing_mm,
        icc_profile: icc.bytes.as_deref(),
        objective_lens_power: slide.dataset().properties.objective_power(),
        lossy_compression,
        conformance: options.semantics.conformance,
    })?;
    let write_started = Instant::now();
    write_single_frame_dicom_object(
        &ids.path,
        object,
        vl_wsi_file_meta(&ids.sop_instance_uid, transfer_syntax.uid()),
        options.semantics.overwrite,
        &encoded.data,
    )?;
    metrics.timings.write_micros = write_started.elapsed().as_micros();
    metrics.record_pixel_profile(encoded.profile);

    Ok(InstanceReport {
        path: ids.path,
        sop_instance_uid: ids.sop_instance_uid,
        series_instance_uid: ids.series_uid,
        transfer_syntax_uid: transfer_syntax.uid(),
        icc_profile_source: icc.report.source,
        icc_profile_sha256: icc.report.sha256,
        icc_calibration_id: icc.report.calibration_id,
        icc_conflict_decision: icc.report.conflict_decision,
        frame_count: 1,
        metrics,
        flavor,
        associated_image: Some(job.name.clone()),
        ..InstanceReport::default()
    })
}

/// Transfer syntax for an associated image. There is no compressed-domain
/// route for associated images, so their pixels are always encoded. A
/// lossless request is kept. A lossy request becomes its lossless sibling,
/// unless it is JPEG Baseline and `pixel_reencode=any` admits a lossy
/// re-encode; lossy JPEG 2000 and HTJ2K have no pixel encode route.
fn associated_transfer_syntax(
    requested: TransferSyntax,
    pixel_reencode: PixelReencodePolicy,
    source_is_compressed: bool,
    name: &str,
) -> Result<TransferSyntax, Error> {
    let target = match requested {
        TransferSyntax::JpegBaseline8Bit if pixel_reencode == PixelReencodePolicy::Any => {
            TransferSyntax::JpegBaseline8Bit
        }
        TransferSyntax::JpegBaseline8Bit => TransferSyntax::JpegLosslessSv1,
        TransferSyntax::Jpeg2000 => TransferSyntax::Jpeg2000Lossless,
        TransferSyntax::Htj2k => TransferSyntax::Htj2kLossless,
        lossless => lossless,
    };
    if source_is_compressed && !pixel_reencode.permits(target) {
        return Err(Error::Unsupported {
            reason: format!(
                "associated image '{name}' is stored compressed and has no compressed-domain route; {}; or exclude it",
                pixel_reencode.refusal(target)
            ),
        });
    }
    Ok(target)
}

struct EncodedAssociatedFrame {
    data: Vec<u8>,
    profile: PixelProfile,
    lossy_compression: LossyCompressionHistory,
}

fn encode_frame(
    bytes: &[u8],
    profile: PixelProfile,
    width: u32,
    height: u32,
    transfer_syntax: TransferSyntax,
    options: &NormalizedExportOptions,
) -> Result<EncodedAssociatedFrame, Error> {
    match transfer_syntax {
        TransferSyntax::JpegLosslessSv1 => {
            let (data, _, _) = encode_jpeg_lossless_frame(
                bytes,
                profile,
                width,
                height,
                options.execution.codec_validation,
            )?;
            Ok(EncodedAssociatedFrame {
                data,
                profile,
                lossy_compression: LossyCompressionHistory::default(),
            })
        }
        TransferSyntax::JpegBaseline8Bit => {
            let (output_profile, subsampling) = jpeg_baseline_output_profile(profile)?;
            let encoded = encode_jpeg_baseline_cpu_fragment(
                jpeg_baseline_samples(bytes, profile, width, height)?,
                options.semantics.jpeg_quality,
                options.semantics.jpeg_optimize_huffman,
                subsampling,
                jpeg_baseline_cpu_restart_interval(width, height, subsampling),
            )?;
            let uncompressed = uncompressed_pixel_bytes(
                u64::from(width),
                u64::from(height),
                u64::from(profile.components),
                profile.bits_allocated,
            )?;
            let compressed = u64::try_from(encoded.data.len()).unwrap_or(u64::MAX);
            Ok(EncodedAssociatedFrame {
                lossy_compression: LossyCompressionHistory::from_byte_counts(
                    JPEG_BASELINE_METHOD,
                    uncompressed,
                    compressed,
                )?,
                data: encoded.data,
                profile: output_profile,
            })
        }
        lossless_j2k => {
            let data = encode_dicom_lossless(
                lossless_j2k_samples(bytes, profile, width, height)?,
                lossless_j2k,
                options.execution.encode_backend,
                options.execution.codec_validation,
            )?;
            Ok(EncodedAssociatedFrame {
                data,
                profile: j2k_encoded_lossless_profile(profile, lossless_j2k),
                lossy_compression: LossyCompressionHistory::default(),
            })
        }
    }
}

fn level_zero(slide: &Slide) -> Result<&wsi_rs::Level, Error> {
    slide
        .dataset()
        .scenes
        .first()
        .and_then(|scene| scene.series.first())
        .and_then(|series| series.levels.first())
        .ok_or_else(|| Error::Unsupported {
            reason: "slide has no level zero for its thumbnail to reference".into(),
        })
}

/// A thumbnail shows the whole scanned area, so each axis takes the level
/// zero spacing scaled by that axis's size ratio.
fn thumbnail_pixel_spacing_mm(
    slide: &Slide,
    level_zero: &wsi_rs::Level,
    options: &NormalizedExportOptions,
    width: u32,
    height: u32,
) -> Result<(f64, f64), Error> {
    let (row_spacing_mm, column_spacing_mm) = require_pixel_spacing_mm(level_pixel_spacing_mm(
        slide,
        level_zero,
        options.semantics.source_pixel_spacing_mm,
    )?)?;
    let (level_columns, level_rows) = level_zero.dimensions;
    Ok((
        row_spacing_mm * level_rows as f64 / f64::from(height),
        column_spacing_mm * level_columns as f64 / f64::from(width),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossy_requests_become_their_lossless_sibling_unless_any_admits_jpeg() {
        let lossless = PixelReencodePolicy::Lossless;
        for (requested, expected) in [
            (
                TransferSyntax::JpegBaseline8Bit,
                TransferSyntax::JpegLosslessSv1,
            ),
            (TransferSyntax::Jpeg2000, TransferSyntax::Jpeg2000Lossless),
            (TransferSyntax::Htj2k, TransferSyntax::Htj2kLossless),
            (
                TransferSyntax::Htj2kLosslessRpcl,
                TransferSyntax::Htj2kLosslessRpcl,
            ),
            (
                TransferSyntax::JpegLosslessSv1,
                TransferSyntax::JpegLosslessSv1,
            ),
        ] {
            assert_eq!(
                associated_transfer_syntax(requested, lossless, true, "label").unwrap(),
                expected
            );
        }
        assert_eq!(
            associated_transfer_syntax(
                TransferSyntax::JpegBaseline8Bit,
                PixelReencodePolicy::Any,
                true,
                "macro"
            )
            .unwrap(),
            TransferSyntax::JpegBaseline8Bit
        );
        assert_eq!(
            associated_transfer_syntax(
                TransferSyntax::Htj2k,
                PixelReencodePolicy::Any,
                true,
                "macro"
            )
            .unwrap(),
            TransferSyntax::Htj2kLossless
        );
    }

    #[test]
    fn never_refuses_compressed_associated_images_but_encodes_uncompressed_ones() {
        let error = associated_transfer_syntax(
            TransferSyntax::Htj2kLosslessRpcl,
            PixelReencodePolicy::Never,
            true,
            "macro",
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("associated image 'macro'"), "{error}");
        assert!(error.contains("pixel_reencode=never"), "{error}");
        assert!(error.contains("exclude it"), "{error}");

        assert_eq!(
            associated_transfer_syntax(
                TransferSyntax::Htj2kLosslessRpcl,
                PixelReencodePolicy::Never,
                false,
                "label"
            )
            .unwrap(),
            TransferSyntax::Htj2kLosslessRpcl
        );
    }
}
