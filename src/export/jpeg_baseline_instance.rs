use std::time::Instant;

use wsi_rs::Slide;

use super::icc_profile::{resolve_icc_profile, ResolvedIccProfile};
use super::jpeg_baseline::{jpeg_baseline_route_frame_geometry, JpegBaselineFrameGeometry};
use super::jpeg_baseline_frames::{prepare_jpeg_frames, PreparedJpegFrames};
use super::jpeg_passthrough::{
    try_prepare_direct_jpeg_passthrough, DirectJpegPassthroughFrameWriter,
    DirectJpegPassthroughStart,
};
use super::tile_grid::checked_frame_count_u32;
use super::{
    level_pixel_spacing_mm, require_pixel_spacing_mm, InstanceExportContext,
    DIRECT_JPEG_PASSTHROUGH_WRITE_CHUNK_FRAMES,
};
use crate::error::Error;
use crate::instance_context::{DicomInstanceContext, InstanceDicomObjectParams};
use crate::lossy::{uncompressed_pixel_bytes, JPEG_BASELINE_METHOD};
use crate::report::{ExportMetrics, InstanceReport};
use crate::request::ExportRequest;
use crate::tile::PixelProfile;
use crate::writer::{
    unique_spool_path, write_dicom_object_with_streamed_pixel_data, DeferredLossyCompression,
    FrameGrid, LossyCompressionHistory, StreamedDicomWritePlan, StreamingPixelDataFrameWriter,
};

pub(super) fn export_jpeg_passthrough_instance(
    slide: &Slide,
    request: &ExportRequest,
    export: InstanceExportContext<'_>,
) -> Result<InstanceReport, Error> {
    let geometry = jpeg_baseline_route_frame_geometry(
        slide,
        export.level,
        export.coordinate,
        export.options.semantics.tile_size,
    )?;
    let frame_count = checked_frame_count_u32(geometry.tiles_across, geometry.tiles_down)?;
    let output = JpegInstanceOutput {
        export,
        source_path: &request.source_path,
        source_metadata: std::fs::metadata(&request.source_path).map_err(|source| Error::Io {
            path: request.source_path.clone(),
            source,
        })?,
        frame_count,
        grid: FrameGrid {
            frame_columns: geometry.frame_columns,
            frame_rows: geometry.frame_rows,
            matrix_columns: export.level.dimensions.0,
            matrix_rows: export.level.dimensions.1,
        },
        context: DicomInstanceContext::new(
            export.identity,
            &request.output_dir,
            require_pixel_spacing_mm(level_pixel_spacing_mm(
                slide,
                export.level,
                export.options.semantics.source_pixel_spacing_mm,
            )?)?,
            export.coordinate,
        )?,
    };
    let declared_lossy_compression =
        super::lossy_provenance::declared_source_lossy_history(&request.source_path)?;
    if let Some(start) =
        try_prepare_direct_jpeg_passthrough(slide, export.coordinate, export.level, geometry)?
    {
        if let Some(report) = export_direct(
            slide,
            request,
            &output,
            geometry,
            start,
            declared_lossy_compression.clone(),
        )? {
            return Ok(report);
        }
    }
    let PreparedJpegFrames {
        mut spool,
        profile,
        metrics,
        source_lossy,
        target_lossy,
    } = prepare_jpeg_frames(
        slide,
        export,
        geometry,
        unique_spool_path(&output.context.path),
        frame_count as usize,
    )?;
    let icc = output.resolve_icc(slide, request, profile)?;
    let observed_lossy = source_lossy.into_history()?;
    let mut lossy = if declared_lossy_compression.is_empty() {
        observed_lossy
    } else {
        declared_lossy_compression
    };
    lossy.append(target_lossy.into_history()?);
    output.write(profile, icc, lossy, None, metrics, |writer| {
        spool.stream_frames_to(writer, |index| {
            let raw = super::jpeg_passthrough::read_raw_jpeg_passthrough_tile(
                slide,
                export.coordinate,
                geometry,
                index,
            )?
            .ok_or_else(|| Error::SlideRead {
                message: format!("JPEG source frame {index} is no longer passthrough-eligible"),
            })?;
            if super::pixel_profile_from_raw_jpeg_tile(&raw)? != profile {
                return Err(Error::SlideRead {
                    message: format!("JPEG source frame {index} profile changed after planning"),
                });
            }
            Ok(raw.into_data())
        })
    })
}

fn export_direct(
    slide: &Slide,
    request: &ExportRequest,
    output: &JpegInstanceOutput<'_>,
    geometry: JpegBaselineFrameGeometry,
    start: DirectJpegPassthroughStart,
    declared_lossy: LossyCompressionHistory,
) -> Result<Option<InstanceReport>, Error> {
    let icc = output.resolve_icc(slide, request, start.profile)?;
    let mut metrics = ExportMetrics::default();
    let frame_count = output.frame_count as usize;
    for _ in 0..frame_count {
        metrics.record_passthrough_frame();
        metrics.record_pixel_profile(start.profile);
    }
    let (lossy, deferred_lossy_compression) = if declared_lossy.is_empty() {
        let uncompressed_bytes = uncompressed_pixel_bytes(
            u64::from(geometry.frame_columns),
            u64::from(geometry.frame_rows),
            u64::from(start.profile.components),
            start.profile.bits_allocated,
        )?
        .checked_mul(u64::from(output.frame_count))
        .ok_or_else(|| Error::Metadata {
            reason: "direct JPEG passthrough uncompressed byte count overflow".into(),
        })?;
        (
            LossyCompressionHistory::default(),
            Some(DeferredLossyCompression {
                method: JPEG_BASELINE_METHOD,
                uncompressed_bytes,
            }),
        )
    } else {
        (declared_lossy, None)
    };
    let chunk_byte_budget =
        usize::try_from(output.export.options.resources.max_prepared_frame_bytes)
            .unwrap_or(usize::MAX);
    let mut source = DirectJpegPassthroughFrameWriter::new(
        slide,
        output.export.coordinate,
        geometry,
        frame_count,
        start.profile,
        start.first_frame,
        DIRECT_JPEG_PASSTHROUGH_WRITE_CHUNK_FRAMES,
        chunk_byte_budget,
    );
    let result = output.write(
        start.profile,
        icc,
        lossy,
        deferred_lossy_compression,
        metrics,
        |writer| {
            for index in 0..frame_count {
                let bytes = source.frame_len(index).map_err(|source| Error::Io {
                    path: output.context.path.clone(),
                    source,
                })?;
                writer
                    .push_frame_with(bytes, |destination| source.write_frame(index, destination))?;
            }
            Ok(())
        },
    );
    match result {
        Ok(report) => Ok(Some(report)),
        Err(_) if source.became_ineligible() => Ok(None),
        Err(error) => Err(error),
    }
}

struct JpegInstanceOutput<'a> {
    export: InstanceExportContext<'a>,
    source_path: &'a std::path::Path,
    source_metadata: std::fs::Metadata,
    context: DicomInstanceContext,
    grid: FrameGrid,
    frame_count: u32,
}

impl JpegInstanceOutput<'_> {
    fn resolve_icc(
        &self,
        slide: &Slide,
        request: &ExportRequest,
        profile: PixelProfile,
    ) -> Result<ResolvedIccProfile, Error> {
        resolve_icc_profile(
            slide,
            request,
            self.export.metadata,
            self.export.coordinate,
            self.export.level,
            profile,
        )
    }

    fn write(
        &self,
        profile: PixelProfile,
        icc: ResolvedIccProfile,
        lossy: LossyCompressionHistory,
        deferred_lossy_compression: Option<DeferredLossyCompression>,
        mut metrics: ExportMetrics,
        frames: impl FnOnce(&mut StreamingPixelDataFrameWriter<'_>) -> Result<(), Error>,
    ) -> Result<InstanceReport, Error> {
        let object = self.context.build_dicom_object(InstanceDicomObjectParams {
            metadata: self.export.metadata,
            study_uid: self.export.identity.study_uid(),
            specimen_uid: self.export.identity.specimen_uid(),
            instance_number: self.export.instance_number,
            frame_grid: self.grid,
            frame_count: self.frame_count,
            profile,
            icc_profile: icc.bytes.as_deref(),
            lossy_compression: lossy,
        })?;
        let per_frame_plan =
            self.context
                .per_frame_plan(self.frame_count, self.grid, self.export.per_frame_plan)?;
        let write_started = Instant::now();
        let streamed = write_dicom_object_with_streamed_pixel_data(
            &self.context.path,
            StreamedDicomWritePlan {
                object,
                meta: self
                    .context
                    .file_meta(self.export.options.semantics.transfer_syntax.uid()),
                overwrite: self.export.options.semantics.overwrite,
                per_frame_plan,
                max_instance_metadata_bytes: self
                    .export
                    .options
                    .resources
                    .max_instance_metadata_bytes,
                frame_count: self.frame_count as usize,
                deferred_lossy_compression,
            },
            |writer| {
                self.check_source_unchanged()?;
                frames(writer)?;
                self.check_source_unchanged()
            },
        )?;
        metrics.record_streaming_write_duration(streamed.streaming_write_duration);
        metrics.record_pixel_data_patch_duration(streamed.pixel_data_patch_duration);
        metrics.record_write_duration(write_started.elapsed());
        Ok(self.context.report(
            self.export.options.semantics.transfer_syntax.uid(),
            self.frame_count,
            icc.report,
            metrics,
        ))
    }

    fn check_source_unchanged(&self) -> Result<(), Error> {
        let current = std::fs::metadata(self.source_path).map_err(|source| Error::Io {
            path: self.source_path.into(),
            source,
        })?;
        let unchanged = current.len() == self.source_metadata.len()
            && current.modified().map_err(|source| Error::Io {
                path: self.source_path.into(),
                source,
            })? == self
                .source_metadata
                .modified()
                .map_err(|source| Error::Io {
                    path: self.source_path.into(),
                    source,
                })?;
        #[cfg(unix)]
        let unchanged = {
            use std::os::unix::fs::MetadataExt;
            unchanged
                && current.dev() == self.source_metadata.dev()
                && current.ino() == self.source_metadata.ino()
        };
        if !unchanged {
            return Err(Error::SlideRead {
                message: format!(
                    "JPEG source {} changed during export",
                    self.source_path.display()
                ),
            });
        }
        Ok(())
    }
}
