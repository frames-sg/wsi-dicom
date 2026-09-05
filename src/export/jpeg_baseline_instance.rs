use std::time::Instant;

use wsi_rs::Slide;

use super::icc_profile::{resolve_icc_profile, ResolvedIccProfile};
use super::jpeg_baseline::{jpeg_baseline_route_frame_geometry, JpegBaselineFrameGeometry};
use super::jpeg_baseline_frames::{prepare_jpeg_frames, PreparedJpegFrames};
use super::jpeg_passthrough::{
    try_plan_direct_jpeg_passthrough_frames, DirectJpegPassthroughFrameWriter,
    DirectJpegPassthroughPlan,
};
use super::tile_grid::checked_frame_count_u32;
use super::{
    level_pixel_spacing_mm, require_pixel_spacing_mm, InstanceExportContext,
    DIRECT_JPEG_PASSTHROUGH_WRITE_CHUNK_FRAMES,
};
use crate::error::Error;
use crate::instance_context::{DicomInstanceContext, InstanceDicomObjectParams};
use crate::lossy::JPEG_BASELINE_METHOD;
use crate::report::{ExportMetrics, InstanceReport};
use crate::request::ExportRequest;
use crate::tile::PixelProfile;
use crate::writer::{
    unique_spool_path, write_dicom_object_with_streamed_pixel_data, FrameGrid,
    LossyCompressionHistory, StreamedDicomWritePlan, StreamingPixelDataFrameWriter,
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
    if let Some(plan) =
        try_plan_direct_jpeg_passthrough_frames(slide, export.coordinate, export.level, geometry)?
    {
        return export_direct(
            slide,
            request,
            &output,
            geometry,
            plan,
            declared_lossy_compression,
        );
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
    output.write(profile, icc, lossy, metrics, |writer| {
        spool.stream_frames_to(writer)
    })
}

fn export_direct(
    slide: &Slide,
    request: &ExportRequest,
    output: &JpegInstanceOutput<'_>,
    geometry: JpegBaselineFrameGeometry,
    plan: DirectJpegPassthroughPlan,
    declared_lossy: LossyCompressionHistory,
) -> Result<InstanceReport, Error> {
    let icc = output.resolve_icc(slide, request, plan.profile)?;
    let mut metrics = ExportMetrics::default();
    for _ in 0..plan.frame_count {
        metrics.record_passthrough_frame();
        metrics.record_pixel_profile(plan.profile);
    }
    let lossy = if declared_lossy.is_empty() {
        LossyCompressionHistory::from_byte_counts(
            JPEG_BASELINE_METHOD,
            plan.uncompressed_bytes,
            plan.compressed_bytes,
        )?
    } else {
        declared_lossy
    };
    let mut source = DirectJpegPassthroughFrameWriter::new(
        slide,
        output.export.coordinate,
        geometry,
        plan.frame_count,
        DIRECT_JPEG_PASSTHROUGH_WRITE_CHUNK_FRAMES,
    );
    output.write(plan.profile, icc, lossy, metrics, |writer| {
        for index in 0..plan.frame_count {
            let bytes = source.frame_len(index).map_err(|source| Error::Io {
                path: output.context.path.clone(),
                source,
            })?;
            writer.push_frame_with(bytes, |destination| source.write_frame(index, destination))?;
        }
        Ok(())
    })
}

struct JpegInstanceOutput<'a> {
    export: InstanceExportContext<'a>,
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
        let per_frame_plan = self.context.per_frame_plan(self.frame_count, self.grid)?;
        let write_started = Instant::now();
        write_dicom_object_with_streamed_pixel_data(
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
            },
            frames,
        )?;
        metrics.record_write_duration(write_started.elapsed());
        Ok(self.context.report(
            self.export.options.semantics.transfer_syntax.uid(),
            self.frame_count,
            icc.report,
            metrics,
        ))
    }
}
