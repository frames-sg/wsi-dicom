//! Codec-neutral completion of a prepared lossless DICOM instance.

use std::time::Instant;

use super::icc_profile::ResolvedIccProfile;
use super::InstanceExportContext;
use crate::error::Error;
use crate::instance_context::{DicomInstanceContext, InstanceDicomObjectParams};
use crate::lossy::LossyCompressionHistory;
use crate::report::{ExportMetrics, IccProfileReport, InstanceReport};
use crate::tile::PixelProfile;
use crate::writer::{
    write_dicom_object_with_streamed_pixel_data, BufferedPixelDataSink, FrameGrid, PixelDataSink,
    StreamedDicomWritePlan,
};

/// An encoded lossless instance waiting to be written.
pub(super) struct PendingLosslessInstance {
    context: DicomInstanceContext,
    metadata: crate::metadata::DicomMetadata,
    study_uid: String,
    specimen_uid: String,
    instance_number: u32,
    tile_size: u32,
    matrix_columns: u64,
    matrix_rows: u64,
    frame_count: u32,
    profile: PixelProfile,
    pixel_data: BufferedPixelDataSink,
    icc_profile: Option<Vec<u8>>,
    icc_profile_report: IccProfileReport,
    lossy_compression: LossyCompressionHistory,
    metrics: ExportMetrics,
    transfer_syntax: crate::options::TransferSyntax,
    overwrite: bool,
    max_instance_metadata_bytes: u64,
    per_frame_plan: Option<crate::writer::PerFrameFunctionalGroupsPlan>,
}

impl PendingLosslessInstance {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        export: InstanceExportContext<'_>,
        context: DicomInstanceContext,
        tile_size: u32,
        matrix_columns: u64,
        matrix_rows: u64,
        frame_count: u32,
        profile: PixelProfile,
        pixel_data: BufferedPixelDataSink,
        icc_profile: ResolvedIccProfile,
        lossy_compression: LossyCompressionHistory,
        metrics: ExportMetrics,
    ) -> Self {
        Self {
            context,
            metadata: export.metadata.clone(),
            study_uid: export.identity.study_uid().to_string(),
            specimen_uid: export.identity.specimen_uid().to_string(),
            instance_number: export.instance_number,
            tile_size,
            matrix_columns,
            matrix_rows,
            frame_count,
            profile,
            pixel_data,
            icc_profile: icc_profile.bytes,
            icc_profile_report: icc_profile.report,
            lossy_compression,
            metrics,
            transfer_syntax: export.options.semantics.transfer_syntax,
            overwrite: export.options.semantics.overwrite,
            max_instance_metadata_bytes: export.options.resources.max_instance_metadata_bytes,
            per_frame_plan: export.per_frame_plan.cloned(),
        }
    }

    pub(super) fn finish(mut self) -> Result<InstanceReport, Error> {
        let frame_grid = FrameGrid {
            frame_columns: self.tile_size,
            frame_rows: self.tile_size,
            matrix_columns: self.matrix_columns,
            matrix_rows: self.matrix_rows,
        };
        let per_frame_plan =
            self.context
                .per_frame_plan(self.frame_count, frame_grid, self.per_frame_plan)?;
        let object = self.context.build_dicom_object(InstanceDicomObjectParams {
            metadata: &self.metadata,
            study_uid: &self.study_uid,
            specimen_uid: &self.specimen_uid,
            instance_number: self.instance_number,
            frame_grid,
            frame_count: self.frame_count,
            profile: self.profile,
            icc_profile: self.icc_profile.as_deref(),
            lossy_compression: self.lossy_compression,
            sparse: per_frame_plan.is_sparse(),
        })?;
        let write_started = Instant::now();
        let streamed = write_dicom_object_with_streamed_pixel_data(
            &self.context.path,
            StreamedDicomWritePlan {
                object,
                meta: self.context.file_meta(self.transfer_syntax.uid()),
                overwrite: self.overwrite,
                per_frame_plan,
                max_instance_metadata_bytes: self.max_instance_metadata_bytes,
                frame_count: self.frame_count as usize,
                deferred_lossy_compression: None,
            },
            |writer| self.pixel_data.stream_frames_to(writer),
        )?;
        self.metrics
            .record_streaming_write_duration(streamed.streaming_write_duration);
        self.metrics
            .record_pixel_data_patch_duration(streamed.pixel_data_patch_duration);
        self.metrics.record_write_duration(write_started.elapsed());

        Ok(self.context.report(
            self.transfer_syntax.uid(),
            self.frame_count,
            self.icc_profile_report,
            self.metrics,
        ))
    }
}
