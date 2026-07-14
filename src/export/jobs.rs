use std::collections::HashSet;

use super::*;

const WSI_DICOM_EXPORT_INSTANCE_WORKERS_ENV: &str = "WSI_DICOM_EXPORT_INSTANCE_WORKERS";

#[derive(Clone, Copy)]
pub(super) struct DicomExportInstanceJob<'a> {
    pub(super) ordinal: usize,
    pub(super) instance_number: u32,
    pub(super) coordinate: InstanceCoordinate,
    pub(super) level: &'a wsi_rs::Level,
}

#[derive(Clone, Copy)]
pub(super) struct DicomRouteProfileJob<'a> {
    pub(super) coordinate: InstanceCoordinate,
    pub(super) level: &'a wsi_rs::Level,
}

pub(super) fn dicom_export_instance_jobs<'a>(
    slide: &'a Slide,
    request: &ExportRequest,
) -> Result<Vec<DicomExportInstanceJob<'a>>, Error> {
    let mut jobs = Vec::new();
    for (scene_idx, scene) in slide.dataset().scenes.iter().enumerate() {
        for (series_idx, series) in scene.series.iter().enumerate() {
            for (level_idx, level) in series.levels.iter().enumerate() {
                let level_idx = u32::try_from(level_idx).map_err(|_| Error::Unsupported {
                    reason: "export level index exceeds u32".into(),
                })?;
                if request
                    .level_filter
                    .is_some_and(|requested_level| requested_level != level_idx)
                {
                    continue;
                }
                for z in 0..series.axes.z {
                    for t in 0..series.axes.t {
                        for c in optical_path_groups(series.axes.c) {
                            let instance_number =
                                u32::try_from(jobs.len() + 1).map_err(|_| Error::Unsupported {
                                    reason: "DICOM instance count exceeds u32".into(),
                                })?;
                            jobs.push(DicomExportInstanceJob {
                                ordinal: jobs.len(),
                                instance_number,
                                coordinate: InstanceCoordinate::new(
                                    scene_idx, series_idx, level_idx, z, c, t,
                                ),
                                level,
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(jobs)
}

pub(super) fn dicom_route_profile_jobs(
    slide: &Slide,
    level_filter: Option<u32>,
    max_levels: Option<u32>,
) -> Result<Vec<DicomRouteProfileJob<'_>>, Error> {
    let max_levels =
        max_levels
            .map(usize::try_from)
            .transpose()
            .map_err(|_| Error::Unsupported {
                reason: "route profiling max_levels exceeds platform addressable memory".into(),
            })?;
    let mut jobs = Vec::new();
    for (scene_idx, scene) in slide.dataset().scenes.iter().enumerate() {
        for (series_idx, series) in scene.series.iter().enumerate() {
            let level_limit = max_levels
                .unwrap_or(series.levels.len())
                .min(series.levels.len());
            for (level_idx, level) in series.levels.iter().take(level_limit).enumerate() {
                let level_idx = u32::try_from(level_idx).map_err(|_| Error::Unsupported {
                    reason: "route profiling level index exceeds u32".into(),
                })?;
                if level_filter.is_some_and(|requested_level| requested_level != level_idx) {
                    continue;
                }
                for z in 0..series.axes.z {
                    for t in 0..series.axes.t {
                        for c in optical_path_groups(series.axes.c) {
                            jobs.push(DicomRouteProfileJob {
                                coordinate: InstanceCoordinate::new(
                                    scene_idx, series_idx, level_idx, z, c, t,
                                ),
                                level,
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(jobs)
}

pub(super) fn preflight_output_paths(
    request: &ExportRequest,
    jobs: &[DicomExportInstanceJob<'_>],
) -> Result<(), Error> {
    let mut paths = HashSet::with_capacity(jobs.len());
    for job in jobs {
        let path = job.coordinate.output_path(&request.output_dir);
        if !paths.insert(path.clone()) {
            return Err(Error::InvalidOptions {
                reason: format!("multiple export instances would write {}", path.display()),
            });
        }
        if !request.options.overwrite && path.exists() {
            return Err(Error::Io {
                path,
                source: std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "output file exists; enable overwrite to replace it",
                ),
            });
        }
    }
    Ok(())
}

pub(super) fn export_dicom_instance_jobs(
    slide: &Slide,
    request: &ExportRequest,
    metadata: &DicomMetadata,
    identity: &DicomExportIdentity,
    jobs: &[DicomExportInstanceJob<'_>],
) -> Result<Vec<InstanceReport>, Error> {
    if jobs.len() <= 1 {
        return export_dicom_instance_jobs_serial(slide, request, metadata, identity, jobs);
    }

    if let Some(configured) = configured_export_instance_worker_count()? {
        let workers = configured.max(1).min(jobs.len());
        if workers <= 1 {
            return export_dicom_instance_jobs_serial(slide, request, metadata, identity, jobs);
        }
        return export_dicom_instance_jobs_parallel(
            slide, request, metadata, identity, jobs, workers,
        );
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    if hybrid_lane::prefer_device_htj2k_rpcl_hybrid_export_lanes_enabled(request, jobs)? {
        return hybrid_lane::export_dicom_instance_jobs_prefer_device_htj2k_hybrid_lanes(
            slide, request, metadata, identity, jobs,
        );
    }

    let default_workers = default_export_instance_worker_count(
        &request.options,
        jobs.len(),
        rayon::current_num_threads(),
    );
    if default_workers > 1 {
        return export_dicom_instance_jobs_parallel(
            slide,
            request,
            metadata,
            identity,
            jobs,
            default_workers,
        );
    }

    export_dicom_instance_jobs_serial(slide, request, metadata, identity, jobs)
}

pub(super) fn export_dicom_instance_jobs_serial(
    slide: &Slide,
    request: &ExportRequest,
    metadata: &DicomMetadata,
    identity: &DicomExportIdentity,
    jobs: &[DicomExportInstanceJob<'_>],
) -> Result<Vec<InstanceReport>, Error> {
    jobs.iter()
        .map(|job| export_dicom_instance_job(slide, request, metadata, identity, job))
        .collect()
}

fn export_dicom_instance_jobs_parallel(
    slide: &Slide,
    request: &ExportRequest,
    metadata: &DicomMetadata,
    identity: &DicomExportIdentity,
    jobs: &[DicomExportInstanceJob<'_>],
    workers: usize,
) -> Result<Vec<InstanceReport>, Error> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .thread_name(|idx| format!("wsi-dicom-export-{idx}"))
        .build()
        .map_err(|err| Error::InvalidOptions {
            reason: format!("failed to initialize DICOM export worker pool: {err}"),
        })?;
    let mut reports = pool.install(|| {
        jobs.par_iter()
            .map(|job| {
                export_dicom_instance_job(slide, request, metadata, identity, job)
                    .map(|report| (job.ordinal, report))
            })
            .collect::<Result<Vec<_>, _>>()
    })?;
    reports.sort_by_key(|(ordinal, _)| *ordinal);
    Ok(reports.into_iter().map(|(_, report)| report).collect())
}

#[cfg_attr(not(all(feature = "metal", target_os = "macos")), allow(dead_code))]
pub(super) fn dicom_instance_job_frame_count(
    options: &ExportOptions,
    job: &DicomExportInstanceJob<'_>,
) -> Result<u64, Error> {
    let tile_size = j2k_route_tile_size(options, job.level)?;
    let (matrix_columns, matrix_rows) = job.level.dimensions;
    TileGrid::square(matrix_columns, matrix_rows, tile_size)?.frame_count_u64()
}

pub(super) fn export_dicom_instance_job(
    slide: &Slide,
    request: &ExportRequest,
    metadata: &DicomMetadata,
    identity: &DicomExportIdentity,
    job: &DicomExportInstanceJob<'_>,
) -> Result<InstanceReport, Error> {
    if request.options.transfer_syntax == TransferSyntax::JpegBaseline8Bit {
        export_jpeg_passthrough_instance(
            slide,
            request,
            metadata,
            identity,
            job.instance_number,
            job.coordinate,
            job.level,
        )
    } else {
        export_instance(
            slide,
            request,
            metadata,
            identity,
            job.instance_number,
            job.coordinate,
            job.level,
        )
    }
}

fn configured_export_instance_worker_count() -> Result<Option<usize>, Error> {
    let value = match std::env::var(WSI_DICOM_EXPORT_INSTANCE_WORKERS_ENV) {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => return Ok(None),
        Err(err) => {
            return Err(Error::InvalidOptions {
                reason: format!(
                    "{WSI_DICOM_EXPORT_INSTANCE_WORKERS_ENV} is not valid UTF-8: {err}"
                ),
            });
        }
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let workers = trimmed
        .parse::<usize>()
        .map_err(|_| Error::InvalidOptions {
            reason: format!("{WSI_DICOM_EXPORT_INSTANCE_WORKERS_ENV} must be a positive integer"),
        })?;
    if workers == 0 {
        return Err(Error::InvalidOptions {
            reason: format!("{WSI_DICOM_EXPORT_INSTANCE_WORKERS_ENV} must be greater than zero"),
        });
    }
    Ok(Some(workers))
}

pub(super) fn default_export_instance_worker_count(
    options: &ExportOptions,
    job_count: usize,
    rayon_threads: usize,
) -> usize {
    if job_count <= 1 {
        return 1;
    }
    if !options.encode_backend.cpu_batch_safe() {
        return 1;
    }
    job_count.min(rayon_threads.saturating_sub(1).max(1)).max(1)
}
