use super::*;

#[cfg(all(feature = "metal", target_os = "macos"))]
const PREFER_DEVICE_HTJ2K_RPCL_GPU_ROW_BATCH_TARGET_TILES: usize = 416;
#[cfg(all(feature = "metal", target_os = "macos"))]
const PREFER_DEVICE_HTJ2K_RPCL_GPU_MEMORY_MIB: u64 = 16_384;
#[cfg(all(feature = "metal", target_os = "macos"))]
const PREFER_DEVICE_HTJ2K_RPCL_CPU_LANE_THREADS: usize = 1;

#[cfg(all(feature = "metal", target_os = "macos"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HybridExportLane {
    Gpu,
    Cpu,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(crate) fn prefer_device_htj2k_rpcl_hybrid_lane(
    options: &ExportOptions,
    frame_count: u64,
) -> Option<HybridExportLane> {
    if options.encode_backend != EncodeBackendPreference::PreferDevice
        || options.transfer_syntax != TransferSyntax::Htj2kLosslessRpcl
    {
        return None;
    }
    match effective_lossless_j2k_encode_backend(options, frame_count) {
        EncodeBackendPreference::CpuOnly => Some(HybridExportLane::Cpu),
        _ => Some(HybridExportLane::Gpu),
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(crate) fn effective_lossless_gpu_row_batch_target_tiles(
    options: &ExportOptions,
    frame_count: u64,
) -> Option<usize> {
    if let Some(configured) = options.gpu_row_batch_target_tiles {
        return Some(configured);
    }
    if prefer_device_htj2k_rpcl_hybrid_lane(options, frame_count) == Some(HybridExportLane::Gpu) {
        return Some(PREFER_DEVICE_HTJ2K_RPCL_GPU_ROW_BATCH_TARGET_TILES);
    }
    effective_gpu_row_batch_target_tiles(options)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(crate) fn effective_lossless_gpu_encode_memory_mib(
    options: &ExportOptions,
    frame_count: u64,
) -> Option<u64> {
    if let Some(configured) = options.gpu_encode_memory_mib {
        return Some(configured);
    }
    if prefer_device_htj2k_rpcl_hybrid_lane(options, frame_count) == Some(HybridExportLane::Gpu) {
        return Some(PREFER_DEVICE_HTJ2K_RPCL_GPU_MEMORY_MIB);
    }
    None
}

#[cfg(not(all(feature = "metal", target_os = "macos")))]
pub(crate) fn effective_lossless_gpu_encode_memory_mib(
    options: &ExportOptions,
    _frame_count: u64,
) -> Option<u64> {
    options.gpu_encode_memory_mib
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(crate) fn prefer_device_htj2k_rpcl_hybrid_export_lanes_enabled(
    request: &ExportRequest,
    jobs: &[DicomExportInstanceJob<'_>],
) -> Result<bool, Error> {
    if request.options.encode_backend != EncodeBackendPreference::PreferDevice
        || request.options.transfer_syntax != TransferSyntax::Htj2kLosslessRpcl
    {
        return Ok(false);
    }

    let mut has_gpu_lane = false;
    let mut has_cpu_lane = false;
    for job in jobs {
        let frame_count = dicom_instance_job_frame_count(&request.options, job)?;
        match prefer_device_htj2k_rpcl_hybrid_lane(&request.options, frame_count) {
            Some(HybridExportLane::Gpu) => has_gpu_lane = true,
            Some(HybridExportLane::Cpu) => has_cpu_lane = true,
            None => return Ok(false),
        }
    }
    Ok(has_gpu_lane && has_cpu_lane)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(crate) fn export_dicom_instance_jobs_prefer_device_htj2k_hybrid_lanes(
    slide: &Slide,
    request: &ExportRequest,
    metadata: &DicomMetadata,
    study_uid: &str,
    jobs: &[DicomExportInstanceJob<'_>],
) -> Result<Vec<InstanceReport>, Error> {
    let mut gpu_jobs = Vec::new();
    let mut cpu_jobs = Vec::new();
    for job in jobs {
        let frame_count = dicom_instance_job_frame_count(&request.options, job)?;
        match prefer_device_htj2k_rpcl_hybrid_lane(&request.options, frame_count) {
            Some(HybridExportLane::Gpu) => gpu_jobs.push(job),
            Some(HybridExportLane::Cpu) => cpu_jobs.push(job),
            None => {
                return export_dicom_instance_jobs_serial(slide, request, metadata, study_uid, jobs)
            }
        }
    }
    if gpu_jobs.is_empty() || cpu_jobs.is_empty() {
        return export_dicom_instance_jobs_serial(slide, request, metadata, study_uid, jobs);
    }

    let mut reports = std::thread::scope(|scope| -> Result<Vec<_>, Error> {
        let (writer_tx, writer_rx) =
            std::sync::mpsc::channel::<(usize, PendingLosslessJ2kInstance)>();
        let writer_handle = scope.spawn(move || {
            let mut writer_reports = Vec::new();
            for (ordinal, pending) in writer_rx {
                writer_reports.push((ordinal, pending.finish()?));
            }
            Ok::<_, Error>(writer_reports)
        });

        let cpu_handle = scope.spawn(|| {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(PREFER_DEVICE_HTJ2K_RPCL_CPU_LANE_THREADS)
                .thread_name(|idx| format!("wsi-dicom-cpu-lane-{idx}"))
                .build()
                .map_err(|err| Error::InvalidOptions {
                    reason: format!("failed to initialize DICOM CPU export lane: {err}"),
                })?;
            pool.install(|| {
                cpu_jobs
                    .iter()
                    .map(|job| {
                        export_dicom_instance_job(slide, request, metadata, study_uid, job)
                            .map(|report| (job.ordinal, report))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
        });

        for job in gpu_jobs {
            let pending = prepare_lossless_j2k_instance(
                slide,
                request,
                metadata,
                study_uid,
                job.instance_number,
                job.scene_idx,
                job.series_idx,
                job.level_idx,
                job.z,
                job.c,
                job.t,
                job.level,
            )?;
            writer_tx
                .send((job.ordinal, pending))
                .map_err(|_| Error::DicomWrite {
                    path: request.output_dir.clone(),
                    message: "DICOM writer lane stopped before receiving GPU instance".into(),
                })?;
        }
        drop(writer_tx);

        let mut lane_reports = writer_handle.join().map_err(|_| Error::DicomWrite {
            path: request.output_dir.clone(),
            message: "DICOM writer lane panicked".into(),
        })??;
        let cpu_reports = cpu_handle.join().map_err(|_| Error::Encode {
            message: "DICOM CPU export lane panicked".into(),
        })??;
        lane_reports.extend(cpu_reports);
        Ok(lane_reports)
    })?;

    reports.sort_by_key(|(ordinal, _)| *ordinal);
    Ok(reports.into_iter().map(|(_, report)| report).collect())
}
