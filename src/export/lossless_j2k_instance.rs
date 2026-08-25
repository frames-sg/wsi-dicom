use std::time::Instant;

use wsi_rs::Slide;

use super::frame_region::FrameRectGrid;
use super::icc_profile::resolve_icc_profile;
#[cfg(not(all(feature = "metal", target_os = "macos")))]
use super::j2k_policy::lossless_j2k_cpu_row_batch_count;
use super::j2k_policy::{lossless_j2k_use_direct_pixel_data, reject_lossy_j2k_lossless_fallback};
use super::lossless_j2k_direct_routes::{
    encode_direct_lossless_j2k_routes, try_write_existing_lossless_j2k_frame,
    ExistingLosslessJ2kFrameContext,
};
use super::lossless_j2k_pipeline::{
    encode_lossless_j2k_cpu_fallback_after_routes, record_resolved_lossless_j2k_fallback_frame,
    resolve_lossless_j2k_fallback_frame, LosslessJ2kBatchContext, LosslessJ2kRoutePipeline,
};
use super::lossless_j2k_plan::{plan_lossless_j2k_frames, LosslessJ2kPlanRequest};
use super::route_plan::RouteExecutionContext;
use super::tile_grid::TileGrid;
use super::{level_pixel_spacing_mm, require_pixel_spacing_mm, InstanceExportContext};
use crate::error::Error;
use crate::instance_context::{DicomInstanceContext, InstanceDicomObjectParams};
use crate::lossy::{uncompressed_pixel_bytes, LossyCompressionAccumulator, HTJ2K_METHOD};
use crate::metadata::DicomMetadata;
use crate::options::TransferSyntax;
use crate::report::{ExportMetrics, IccProfileReport, InstanceReport};
use crate::request::ExportRequest;
use crate::routing::{
    j2k_family_passthrough_probe_allowed, j2k_route_tile_size, unsupported_j2k_route_error,
};
use crate::tile::PixelProfile;
use crate::writer::{
    unique_spool_path, write_dicom_object_with_streamed_pixel_data, BufferedPixelDataSink,
    FrameGrid, LossyCompressionHistory, PixelDataSink, StreamedDicomWritePlan,
};

#[cfg(all(feature = "metal", target_os = "macos"))]
use super::lossless_j2k_pipeline::route_lossless_j2k_metal_input_runs;

pub(super) fn export_instance(
    slide: &Slide,
    request: &ExportRequest,
    export: InstanceExportContext<'_>,
) -> Result<InstanceReport, Error> {
    prepare_lossless_j2k_instance(slide, request, export)?.finish()
}

pub(super) struct PendingLosslessJ2kInstance {
    context: DicomInstanceContext,
    metadata: DicomMetadata,
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
    transfer_syntax: TransferSyntax,
    overwrite: bool,
    max_instance_metadata_bytes: u64,
}

impl PendingLosslessJ2kInstance {
    pub(super) fn finish(mut self) -> Result<InstanceReport, Error> {
        let frame_grid = FrameGrid {
            frame_columns: self.tile_size,
            frame_rows: self.tile_size,
            matrix_columns: self.matrix_columns,
            matrix_rows: self.matrix_rows,
        };
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
        })?;
        let per_frame_plan = self.context.per_frame_plan(self.frame_count, frame_grid)?;
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

pub(super) fn prepare_lossless_j2k_instance(
    slide: &Slide,
    request: &ExportRequest,
    export: InstanceExportContext<'_>,
) -> Result<PendingLosslessJ2kInstance, Error> {
    let InstanceExportContext {
        options,
        metadata,
        identity,
        instance_number,
        coordinate,
        level,
    } = export;
    let tile_size = j2k_route_tile_size(
        options.semantics.tile_size,
        options.semantics.transfer_syntax,
        level,
    )?;
    let (matrix_columns, matrix_rows) = level.dimensions;
    let grid = TileGrid::square(matrix_columns, matrix_rows, tile_size)?;
    let tiles_across = grid.tiles_across;
    let tiles_down = grid.tiles_down;
    let frame_count = grid.frame_count_u32()?;
    let context = DicomInstanceContext::new(
        identity,
        &request.output_dir,
        require_pixel_spacing_mm(level_pixel_spacing_mm(slide, level))?,
        coordinate,
    )?;
    let declared_lossy_compression =
        super::lossy_provenance::declared_source_lossy_history(&request.source_path)?;
    let location = coordinate;
    let spool_path = unique_spool_path(&context.path);
    let mut pixel_data = BufferedPixelDataSink::create(
        spool_path,
        frame_count as usize,
        lossless_j2k_use_direct_pixel_data(frame_count, tile_size, rayon::current_num_threads()),
    )?;
    let LosslessJ2kRoutePipeline {
        encoder: mut j2k_encoder,
        #[cfg(all(feature = "metal", target_os = "macos"))]
        mut metal_input,
        mut metrics,
        mut pixel_profile,
        mut jpeg_direct_encoder,
    } = LosslessJ2kRoutePipeline::new(
        &request.source_path,
        options,
        location,
        u64::from(frame_count),
    )?;
    let mut source_lossy_compression = LossyCompressionAccumulator::default();
    let mut target_lossy_compression = LossyCompressionAccumulator::default();
    let allow_passthrough_probe = j2k_family_passthrough_probe_allowed(
        &request.source_path,
        options.semantics.transfer_syntax,
    );
    let route_context = RouteExecutionContext::new(
        options.semantics.transfer_syntax,
        options.execution.encode_backend,
    );

    let mut row = 0;
    while row < tiles_down {
        #[cfg(all(feature = "metal", target_os = "macos"))]
        let planned_row_count = 1;
        #[cfg(not(all(feature = "metal", target_os = "macos")))]
        let planned_row_count = lossless_j2k_cpu_row_batch_count(tiles_across, tiles_down - row);
        let planned = plan_lossless_j2k_frames(
            slide,
            LosslessJ2kPlanRequest {
                location: coordinate,
                start_row: row,
                row_count: planned_row_count,
                start_col: 0,
                tile_count: tiles_across,
                grid: FrameRectGrid {
                    matrix_columns,
                    matrix_rows,
                    frame_columns: tile_size,
                    frame_rows: tile_size,
                },
                transfer_syntax: options.semantics.transfer_syntax,
                allow_passthrough_probe,
            },
        )?;
        for source in planned
            .iter()
            .filter_map(|frame| frame.source_lossy_compression.as_ref())
        {
            source_lossy_compression.observe(source)?;
        }
        let batch_context = LosslessJ2kBatchContext {
            slide,
            level,
            planned: &planned,
            options,
            location,
            tile_size,
        };
        let mut direct_routes =
            encode_direct_lossless_j2k_routes(batch_context, &mut jpeg_direct_encoder)?;
        #[cfg(all(feature = "metal", target_os = "macos"))]
        let mut routed_tiles = route_lossless_j2k_metal_input_runs(
            batch_context,
            &mut metal_input,
            &mut j2k_encoder,
            row,
            &direct_routes,
            frame_count as usize,
            &mut metrics,
        )?;
        let mut cpu_batch_results = encode_lossless_j2k_cpu_fallback_after_routes(
            batch_context,
            &j2k_encoder,
            &direct_routes,
            |idx| {
                #[cfg(all(feature = "metal", target_os = "macos"))]
                {
                    routed_tiles[idx].is_some()
                }
                #[cfg(not(all(feature = "metal", target_os = "macos")))]
                {
                    let _ = idx;
                    false
                }
            },
        )?;
        for (idx, planned_frame) in planned.iter().enumerate() {
            let decision = planned_frame.route_decision(route_context);
            let compressed_bytes_before = pixel_data.total_raw_bytes();
            if try_write_existing_lossless_j2k_frame(
                ExistingLosslessJ2kFrameContext {
                    idx,
                    planned_frame,
                    direct_routes: &mut direct_routes,
                    options,
                    metrics: &mut metrics,
                    pixel_profile: &mut pixel_profile,
                },
                &mut pixel_data,
            )? {
                if options.semantics.transfer_syntax == TransferSyntax::Htj2k
                    && planned_frame.passthrough.is_none()
                {
                    let compressed_bytes = pixel_data
                        .total_raw_bytes()
                        .checked_sub(compressed_bytes_before)
                        .ok_or_else(|| Error::Metadata {
                            reason: "encoded HTJ2K byte count decreased unexpectedly".into(),
                        })?;
                    let profile = pixel_profile.ok_or_else(|| Error::Metadata {
                        reason: "encoded HTJ2K frame did not establish a pixel profile".into(),
                    })?;
                    target_lossy_compression.observe_bytes(
                        HTJ2K_METHOD,
                        uncompressed_pixel_bytes(
                            u64::from(tile_size),
                            u64::from(tile_size),
                            u64::from(profile.components),
                            profile.bits_allocated,
                        )?,
                        compressed_bytes,
                    )?;
                }
                continue;
            }
            if !decision.allows_j2k_encode_fallback() {
                return Err(unsupported_j2k_route_error(
                    options.semantics.transfer_syntax,
                    planned_frame.row,
                    planned_frame.col,
                ));
            }
            reject_lossy_j2k_lossless_fallback(
                planned_frame,
                options.semantics.transfer_syntax,
                planned_frame.row,
            )?;
            let resolved = resolve_lossless_j2k_fallback_frame(
                batch_context,
                &mut j2k_encoder,
                planned_frame,
                &mut cpu_batch_results[idx],
                #[cfg(all(feature = "metal", target_os = "macos"))]
                routed_tiles[idx].take(),
            )?;
            let encoded = record_resolved_lossless_j2k_fallback_frame(
                &mut metrics,
                &mut pixel_profile,
                resolved,
                options.semantics.transfer_syntax,
                "pixel profile changed across frames",
                |err| match err {
                    Error::Encode { message } => Error::FrameEncode {
                        level: coordinate.level_idx,
                        row: planned_frame.row,
                        col: planned_frame.col,
                        message,
                    },
                    other => other,
                },
            )?;
            let codestream = encoded.into_codestream()?;
            if options.semantics.transfer_syntax == TransferSyntax::Htj2k {
                let profile = pixel_profile.ok_or_else(|| Error::Metadata {
                    reason: "encoded HTJ2K frame did not establish a pixel profile".into(),
                })?;
                target_lossy_compression.observe_encoded_frame(
                    HTJ2K_METHOD,
                    uncompressed_pixel_bytes(
                        u64::from(tile_size),
                        u64::from(tile_size),
                        u64::from(profile.components),
                        profile.bits_allocated,
                    )?,
                    &codestream,
                )?;
            }
            let byte_started = Instant::now();
            pixel_data.push_owned_frame(codestream)?;
            metrics.record_write_duration(byte_started.elapsed());
        }
        row = row
            .checked_add(planned_row_count)
            .ok_or_else(|| Error::Unsupported {
                reason: "lossless J2K row batch overflow".into(),
            })?;
    }

    let profile = pixel_profile.ok_or_else(|| Error::Unsupported {
        reason: "slide level produced no frames".into(),
    })?;
    let icc_profile = resolve_icc_profile(slide, request, metadata, coordinate, level, profile)?;
    let observed_lossy_compression = source_lossy_compression.into_history()?;
    let mut lossy_compression = if declared_lossy_compression.is_empty() {
        observed_lossy_compression
    } else {
        declared_lossy_compression
    };
    lossy_compression.append(target_lossy_compression.into_history()?);

    Ok(PendingLosslessJ2kInstance {
        context,
        metadata: metadata.clone(),
        study_uid: identity.study_uid().to_string(),
        specimen_uid: identity.specimen_uid().to_string(),
        instance_number,
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
        transfer_syntax: options.semantics.transfer_syntax,
        overwrite: options.semantics.overwrite,
        max_instance_metadata_bytes: options.resources.max_instance_metadata_bytes,
    })
}
