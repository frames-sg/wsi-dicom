use super::*;

mod route_sampling;

use route_sampling::{
    coverage_jpeg_baseline_routes, profile_jpeg_baseline_routes, profile_lossless_j2k_routes,
};

#[derive(Clone, Copy)]
pub(super) struct RouteLevelDeadline {
    pub(super) started: Instant,
    pub(super) max_elapsed: Duration,
}

impl RouteLevelDeadline {
    pub(super) fn new(max_elapsed: Option<Duration>) -> Option<Self> {
        max_elapsed.map(|max_elapsed| Self {
            started: Instant::now(),
            max_elapsed,
        })
    }
}

pub(super) fn validate_max_level_elapsed(
    max_level_elapsed: Option<Duration>,
    context: &str,
) -> Result<(), Error> {
    if max_level_elapsed == Some(Duration::ZERO) {
        return Err(Error::Unsupported {
            reason: format!("{context} requires max_level_elapsed > 0 when provided"),
        });
    }
    Ok(())
}

pub(super) fn check_route_level_deadline(
    deadline: Option<RouteLevelDeadline>,
    level_idx: u32,
) -> Result<(), Error> {
    let Some(deadline) = deadline else {
        return Ok(());
    };
    let elapsed = deadline.started.elapsed();
    if elapsed > deadline.max_elapsed {
        return Err(Error::Unsupported {
            reason: format!(
                "route coverage level {level_idx} timed out after {:.3} ms (max_level_elapsed {:.3} ms)",
                duration_as_reported_micros(elapsed) as f64 / 1000.0,
                duration_as_reported_micros(deadline.max_elapsed) as f64 / 1000.0
            ),
        });
    }
    Ok(())
}

fn route_profile_available_frames(
    slide: &Slide,
    options: &ExportOptions,
    level: &wsi_rs::Level,
    location: JpegBaselineFrameLocation,
) -> Result<u64, Error> {
    if options.transfer_syntax == TransferSyntax::JpegBaseline8Bit {
        let geometry =
            jpeg_baseline_route_frame_geometry(slide, level, location, options.tile_size)?;
        return geometry
            .tiles_across
            .checked_mul(geometry.tiles_down)
            .ok_or_else(|| Error::Unsupported {
                reason: "route profile JPEG frame count overflow".into(),
            });
    }
    let (matrix_columns, matrix_rows) = level.dimensions;
    let tile_size = j2k_route_tile_size(options, level)?;
    matrix_columns
        .div_ceil(u64::from(tile_size))
        .checked_mul(matrix_rows.div_ceil(u64::from(tile_size)))
        .ok_or_else(|| Error::Unsupported {
            reason: "route profile frame count overflow".into(),
        })
}

fn resolve_source_aware_profile_options(
    source_path: &Path,
    mut options: ExportOptions,
    level_filter: Option<u32>,
    max_levels: Option<u32>,
    source_aware_transfer_syntax: bool,
) -> Result<ExportOptions, Error> {
    if source_aware_transfer_syntax {
        let current_default =
            JpegDirectHtj2kProfile::default_for_transfer_syntax(options.transfer_syntax);
        let profile_is_default = options.jpeg_direct_htj2k_profile == current_default;
        let mut request =
            DefaultTransferSyntaxRequest::new(source_path.to_path_buf(), options.tile_size);
        request.level_filter = level_filter;
        request.max_levels = max_levels;
        options.transfer_syntax = default_transfer_syntax_for_source(request)?;
        if profile_is_default {
            options.jpeg_direct_htj2k_profile =
                JpegDirectHtj2kProfile::default_for_transfer_syntax(options.transfer_syntax);
        }
    }
    options.validate()?;
    Ok(options)
}

/// Profile the route selection and encode path for a bounded number of frames.
pub fn profile_dicom_routes(request: RouteProfileRequest) -> Result<RouteProfileReport, Error> {
    #[cfg(all(feature = "metal", target_os = "macos"))]
    load_persistent_auto_metal_input_route_cache_if_requested()?;
    if request.max_frames == 0 {
        return Err(Error::Unsupported {
            reason: "route profiling requires max_frames > 0".into(),
        });
    }
    let options = resolve_source_aware_profile_options(
        &request.source_path,
        request.options,
        Some(request.level),
        None,
        request.source_aware_transfer_syntax,
    )?;
    if options.transfer_syntax != TransferSyntax::JpegBaseline8Bit
        && !options.transfer_syntax.is_j2k_family()
    {
        return Err(Error::Unsupported {
            reason: "bounded route profiling currently supports JPEG Baseline, JPEG 2000, and HTJ2K transfer syntaxes"
                .into(),
        });
    }

    let slide = Slide::open(&request.source_path).map_err(|source| Error::SourceOpen {
        path: request.source_path.clone(),
        message: source.to_string(),
    })?;
    let jobs = dicom_route_profile_jobs(&slide, Some(request.level), None)?;
    if jobs.is_empty() {
        return Err(Error::Unsupported {
            reason: format!("route profiling level {} is not available", request.level),
        });
    }
    let started = Instant::now();
    let transfer_syntax_uid = options.transfer_syntax.uid();
    let mut metrics = ExportMetrics::default();
    let mut available_frames = 0u64;
    let mut remaining = request.max_frames;

    for job in &jobs {
        let location = job.coordinate;
        let job_available_frames =
            route_profile_available_frames(&slide, &options, job.level, location)?;
        available_frames = available_frames.saturating_add(job_available_frames);
        if remaining == 0 || job_available_frames == 0 {
            continue;
        }
        let job_frames = remaining.min(job_available_frames);
        let job_metrics = if options.transfer_syntax == TransferSyntax::JpegBaseline8Bit {
            profile_jpeg_baseline_routes(&slide, options.clone(), job.level, location, job_frames)?
        } else {
            profile_lossless_j2k_routes(
                &slide,
                &request.source_path,
                options.clone(),
                job.level,
                location,
                job_frames,
                None,
            )?
        };
        remaining = remaining.saturating_sub(job_metrics.routes.total_frames);
        metrics.add_assign(job_metrics);
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    flush_persistent_auto_metal_input_route_cache_if_requested()?;

    Ok(RouteProfileReport {
        source_path: request.source_path,
        transfer_syntax_uid,
        level: request.level,
        requested_frames: request.max_frames,
        available_frames,
        metrics,
        elapsed_micros: duration_as_reported_micros(started.elapsed()),
    })
}

/// Profile route coverage across exportable slide planes without writing DICOM.
pub fn profile_dicom_route_coverage(
    request: RouteCoverageRequest,
) -> Result<RouteCoverageReport, Error> {
    #[cfg(all(feature = "metal", target_os = "macos"))]
    load_persistent_auto_metal_input_route_cache_if_requested()?;
    if request.max_frames_per_level == 0 {
        return Err(Error::Unsupported {
            reason: "route coverage profiling requires max_frames_per_level > 0".into(),
        });
    }
    if request.max_levels == Some(0) {
        return Err(Error::Unsupported {
            reason: "route coverage profiling requires max_levels > 0 when provided".into(),
        });
    }
    validate_max_level_elapsed(request.max_level_elapsed, "route coverage profiling")?;
    let source_path = match &request.target {
        RouteCoverageTarget::Source(source_path) => source_path.clone(),
        RouteCoverageTarget::Corpus(_) => {
            return Err(Error::Unsupported {
                reason: "route coverage profiling requires a source target".into(),
            });
        }
    };

    let options = resolve_source_aware_profile_options(
        &source_path,
        request.options,
        None,
        request.max_levels,
        request.source_aware_transfer_syntax,
    )?;

    if options.transfer_syntax != TransferSyntax::JpegBaseline8Bit
        && !options.transfer_syntax.is_j2k_family()
    {
        return Err(Error::Unsupported {
            reason: "route coverage profiling currently supports JPEG Baseline, JPEG 2000, and HTJ2K transfer syntaxes"
                .into(),
        });
    }

    let slide = Slide::open(&source_path).map_err(|source| Error::SourceOpen {
        path: source_path.clone(),
        message: source.to_string(),
    })?;
    let jobs = dicom_route_profile_jobs(&slide, None, request.max_levels)?;
    if jobs.is_empty() {
        return Err(Error::Unsupported {
            reason: "route coverage profiling requires at least one exportable level".into(),
        });
    }

    let started = Instant::now();
    let transfer_syntax_uid = options.transfer_syntax.uid();
    let mut jobs_by_level: BTreeMap<u32, Vec<DicomRouteProfileJob<'_>>> = BTreeMap::new();
    for job in jobs {
        let level_jobs = jobs_by_level.entry(job.coordinate.level_idx).or_default();
        level_jobs.try_reserve(1).map_err(|_| Error::Unsupported {
            reason: "route coverage level plan exceeds available memory".into(),
        })?;
        level_jobs.push(job);
    }
    let level_count = jobs_by_level.len();
    let mut levels = Vec::new();
    levels
        .try_reserve_exact(level_count)
        .map_err(|_| Error::Unsupported {
            reason: "route coverage level report exceeds available memory".into(),
        })?;
    let mut metrics = ExportMetrics::default();
    let mut available_frames = 0u64;

    for (level_ordinal, (level_idx, level_jobs)) in jobs_by_level.into_iter().enumerate() {
        let level_started = Instant::now();
        let mut level_available_frames = 0u64;
        for job in &level_jobs {
            level_available_frames = level_available_frames.saturating_add(
                route_profile_available_frames(&slide, &options, job.level, job.coordinate)?,
            );
        }
        if matches!(request.progress, Some(RouteProgressSink::Stderr)) {
            eprintln!(
                "coverage level {}/{} start {} level={} available_frames={}",
                level_ordinal + 1,
                level_count,
                source_path.display(),
                level_idx,
                level_available_frames
            );
        }
        let level_deadline = RouteLevelDeadline::new(request.max_level_elapsed);
        let mut level_metrics = ExportMetrics::default();
        let mut remaining = request.max_frames_per_level;
        for job in &level_jobs {
            if remaining == 0 {
                break;
            }
            check_route_level_deadline(level_deadline, level_idx)?;
            let location = job.coordinate;
            let job_available_frames =
                route_profile_available_frames(&slide, &options, job.level, location)?;
            if job_available_frames == 0 {
                continue;
            }
            let job_frames = remaining.min(job_available_frames);
            let job_metrics = if options.transfer_syntax == TransferSyntax::JpegBaseline8Bit {
                coverage_jpeg_baseline_routes(
                    &slide,
                    options.clone(),
                    job.level,
                    location,
                    job_frames,
                    level_deadline,
                )?
            } else {
                profile_lossless_j2k_routes(
                    &slide,
                    &source_path,
                    options.clone(),
                    job.level,
                    location,
                    job_frames,
                    level_deadline,
                )?
            };
            remaining = remaining.saturating_sub(job_metrics.routes.total_frames);
            level_metrics.add_assign(job_metrics);
        }
        if matches!(request.progress, Some(RouteProgressSink::Stderr)) {
            eprintln!(
                "coverage level {}/{} ok {} level={} frames={} route_passthrough={} route_gpu_transcode={} route_cpu_fallback={} elapsed_ms={:.3}",
                level_ordinal + 1,
                level_count,
                source_path.display(),
                level_idx,
                level_metrics.routes.total_frames,
                level_metrics.route_passthrough_frames(),
                level_metrics.routes.gpu_transcode_frames,
                level_metrics.routes.cpu_fallback_frames,
                duration_as_reported_micros(level_started.elapsed()) as f64 / 1000.0
            );
        }
        metrics.add_assign(level_metrics);
        available_frames = available_frames.saturating_add(level_available_frames);
        levels.push(RouteProfileReport {
            source_path: source_path.clone(),
            transfer_syntax_uid,
            level: level_idx,
            requested_frames: request.max_frames_per_level,
            available_frames: level_available_frames,
            metrics: level_metrics,
            elapsed_micros: duration_as_reported_micros(level_started.elapsed()),
        });
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    flush_persistent_auto_metal_input_route_cache_if_requested()?;

    Ok(RouteCoverageReport {
        source_path,
        transfer_syntax_uid,
        requested_frames_per_level: request.max_frames_per_level,
        available_frames,
        complete_frame_coverage: metrics.routes.total_frames >= available_frames,
        levels,
        metrics,
        elapsed_micros: duration_as_reported_micros(started.elapsed()),
    })
}

/// Profile route coverage for every WSI-like file under a source root.
pub fn profile_dicom_route_corpus_coverage(
    request: RouteCoverageRequest,
) -> Result<RouteCorpusCoverageReport, Error> {
    if request.max_frames_per_level == 0 {
        return Err(Error::Unsupported {
            reason: "corpus route coverage profiling requires max_frames_per_level > 0".into(),
        });
    }
    if request.max_levels == Some(0) {
        return Err(Error::Unsupported {
            reason: "corpus route coverage profiling requires max_levels > 0 when provided".into(),
        });
    }
    validate_max_level_elapsed(request.max_level_elapsed, "corpus route coverage profiling")?;
    let started = Instant::now();
    let source_root = match &request.target {
        RouteCoverageTarget::Corpus(source_root) => source_root.clone(),
        RouteCoverageTarget::Source(_) => {
            return Err(Error::Unsupported {
                reason: "corpus route coverage profiling requires a corpus target".into(),
            });
        }
    };
    request.options.validate()?;
    if !request.source_aware_transfer_syntax
        && request.options.transfer_syntax != TransferSyntax::JpegBaseline8Bit
        && !request.options.transfer_syntax.is_j2k_family()
    {
        return Err(Error::Unsupported {
            reason: "corpus route coverage profiling currently supports JPEG Baseline, JPEG 2000, and HTJ2K transfer syntaxes"
                .into(),
        });
    }
    let sources =
        collect_wsi_candidate_paths(&source_root, request.max_sources, request.max_depth)?;
    let mut reports = Vec::new();
    reports
        .try_reserve_exact(sources.len())
        .map_err(|_| Error::Unsupported {
            reason: "corpus coverage report plan exceeds available memory".into(),
        })?;
    let mut failures = Vec::new();
    failures
        .try_reserve_exact(sources.len())
        .map_err(|_| Error::Unsupported {
            reason: "corpus coverage failure plan exceeds available memory".into(),
        })?;
    let mut metrics = ExportMetrics::default();
    let mut available_frames = 0u64;

    for (source_idx, source_path) in sources.iter().enumerate() {
        let source_started = Instant::now();
        if matches!(request.progress, Some(RouteProgressSink::Stderr)) {
            eprintln!(
                "coverage-corpus source {}/{} start {}",
                source_idx + 1,
                sources.len(),
                source_path.display()
            );
        }
        match profile_dicom_route_coverage(RouteCoverageRequest {
            target: RouteCoverageTarget::Source(source_path.clone()),
            options: request.options.clone(),
            source_aware_transfer_syntax: request.source_aware_transfer_syntax,
            max_frames_per_level: request.max_frames_per_level,
            max_levels: request.max_levels,
            max_level_elapsed: request.max_level_elapsed,
            progress: request.progress,
            max_sources: request.max_sources,
            max_depth: request.max_depth,
        }) {
            Ok(report) => {
                metrics.add_assign(report.metrics);
                available_frames = available_frames.saturating_add(report.available_frames);
                if matches!(request.progress, Some(RouteProgressSink::Stderr)) {
                    eprintln!(
                        "coverage-corpus source {}/{} ok {} levels={} frames={} route_passthrough={} route_gpu_transcode={} route_cpu_fallback={} elapsed_ms={:.3}",
                        source_idx + 1,
                        sources.len(),
                        source_path.display(),
                        report.levels.len(),
                        report.metrics.routes.total_frames,
                        report.metrics.route_passthrough_frames(),
                        report.metrics.routes.gpu_transcode_frames,
                        report.metrics.routes.cpu_fallback_frames,
                        duration_as_reported_micros(source_started.elapsed()) as f64 / 1000.0
                    );
                }
                reports.push(report);
            }
            Err(err) => {
                if matches!(request.progress, Some(RouteProgressSink::Stderr)) {
                    eprintln!(
                        "coverage-corpus source {}/{} failed {} error={} elapsed_ms={:.3}",
                        source_idx + 1,
                        sources.len(),
                        source_path.display(),
                        err,
                        duration_as_reported_micros(source_started.elapsed()) as f64 / 1000.0
                    );
                }
                failures.push(RouteCorpusCoverageFailure {
                    source_path: source_path.clone(),
                    message: err.to_string(),
                });
            }
        }
    }
    let transfer_syntax_uids = corpus_transfer_syntax_uids(&reports)?;

    Ok(RouteCorpusCoverageReport {
        source_root,
        transfer_syntax_uid: common_corpus_transfer_syntax_uid(&transfer_syntax_uids),
        transfer_syntax_uids,
        requested_frames_per_level: request.max_frames_per_level,
        max_levels: request.max_levels,
        sources_considered: sources.len(),
        available_frames,
        complete_frame_coverage: failures.is_empty()
            && reports.iter().all(|report| report.complete_frame_coverage),
        reports,
        failures,
        metrics,
        elapsed_micros: duration_as_reported_micros(started.elapsed()),
    })
}

fn corpus_transfer_syntax_uids(
    reports: &[RouteCoverageReport],
) -> Result<Vec<&'static str>, Error> {
    let mut transfer_syntax_uids = Vec::new();
    transfer_syntax_uids
        .try_reserve_exact(reports.len())
        .map_err(|_| Error::Unsupported {
            reason: "corpus transfer syntax summary exceeds available memory".into(),
        })?;
    transfer_syntax_uids.extend(reports.iter().map(|report| report.transfer_syntax_uid));
    transfer_syntax_uids.sort_unstable();
    transfer_syntax_uids.dedup();
    Ok(transfer_syntax_uids)
}

fn common_corpus_transfer_syntax_uid(
    transfer_syntax_uids: &[&'static str],
) -> Option<&'static str> {
    let first = transfer_syntax_uids.first().copied()?;
    transfer_syntax_uids
        .iter()
        .all(|uid| *uid == first)
        .then_some(first)
}
