use super::*;

use std::collections::HashSet;

use wsi_rs::{
    AxesShape, ChannelInfo, ColorSpace, CpuTile, Dataset, DatasetId, Level, Properties, SampleType,
    Scene, Series, SlideReader, TileLayout, TileRequest, WsiError,
};

struct MultiSceneSource {
    dataset: Dataset,
}

impl SlideReader for MultiSceneSource {
    fn dataset(&self) -> &Dataset {
        &self.dataset
    }

    fn read_tile_cpu(&self, request: &TileRequest) -> Result<CpuTile, WsiError> {
        let value = (request.scene.get() * 40 + request.series.get() * 10) as u8;
        CpuTile::from_u8_interleaved(1, 1, 3, ColorSpace::Rgb, vec![value, 2, 3])
    }

    fn read_associated(&self, name: &str) -> Result<CpuTile, WsiError> {
        Err(WsiError::AssociatedImageNotFound(name.into()))
    }
}

fn one_pixel_series(id: &str) -> Series {
    Series::new(
        id,
        AxesShape::new(1, 1, 1),
        vec![Level::new(
            (1, 1),
            1.0,
            TileLayout::Regular {
                tile_width: 1,
                tile_height: 1,
                tiles_across: 1,
                tiles_down: 1,
            },
        )],
        SampleType::Uint8,
        vec![ChannelInfo::new()],
    )
}

#[test]
fn multi_scene_multi_series_jobs_export_unique_instances() {
    let mut properties = Properties::new();
    properties.insert("openslide.mpp-x", "0.5");
    properties.insert("openslide.mpp-y", "0.5");
    let dataset = Dataset::new(
        DatasetId::new(7),
        vec![
            Scene::new(
                "scene-0",
                vec![one_pixel_series("series-0"), one_pixel_series("series-1")],
            ),
            Scene::new(
                "scene-1",
                vec![one_pixel_series("series-0"), one_pixel_series("series-1")],
            ),
        ],
    )
    .with_properties(properties);
    let slide = Slide::from_source_with_cache_bytes(Box::new(MultiSceneSource { dataset }), 0);
    let output = tempfile::tempdir().unwrap();
    let request = ExportRequest::new(
        PathBuf::from("synthetic-multi-scene"),
        output.path().to_path_buf(),
        ExportOptions {
            tile_size: 1,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            encode_backend: EncodeBackendPreference::CpuOnly,
            ..ExportOptions::default()
        },
        ColorManagement::SourceOrSrgb,
        MetadataSource::ResearchPlaceholder,
    )
    .unwrap();
    let metadata = request.metadata.resolve().unwrap();
    let options = NormalizedExportOptions::from_validated(&request.options);
    let identity = DicomExportIdentity::from_seed("1.2.3".into(), "multi-scene".into());
    let jobs = dicom_export_instance_jobs(&slide, &request).unwrap();

    let reports =
        export_dicom_instance_jobs(&slide, &request, &options, &metadata, &identity, &jobs)
            .unwrap();

    assert_eq!(reports.len(), 4);
    assert_eq!(
        reports
            .iter()
            .map(|report| report.path.clone())
            .collect::<HashSet<_>>()
            .len(),
        reports.len()
    );
    assert_eq!(
        reports
            .iter()
            .map(|report| report.sop_instance_uid.clone())
            .collect::<HashSet<_>>()
            .len(),
        reports.len()
    );
    assert!(reports.iter().all(|report| report.path.is_file()));
    assert_eq!(reports[0].scene, 0);
    assert_eq!(reports[1].series, 1);
    assert_eq!(reports[2].scene, 1);
}

#[test]
fn export_preserves_anisotropic_pixel_spacing_in_dicom_order() {
    let mut properties = Properties::new();
    properties.insert("openslide.vendor", "philips");
    properties.insert("openslide.mpp-x", "0.25");
    properties.insert("openslide.mpp-y", "0.5");
    let dataset = Dataset::new(
        DatasetId::new(8),
        vec![Scene::new("scene-0", vec![one_pixel_series("series-0")])],
    )
    .with_properties(properties);
    let slide = Slide::from_source_with_cache_bytes(Box::new(MultiSceneSource { dataset }), 0);
    let output = tempfile::tempdir().unwrap();
    let request = ExportRequest::new(
        PathBuf::from("synthetic-anisotropic-spacing"),
        output.path().to_path_buf(),
        ExportOptions {
            tile_size: 1,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            encode_backend: EncodeBackendPreference::CpuOnly,
            ..ExportOptions::default()
        },
        ColorManagement::SourceOrSrgb,
        MetadataSource::ResearchPlaceholder,
    )
    .unwrap();
    let metadata = request.metadata.resolve().unwrap();
    let options = NormalizedExportOptions::from_validated(&request.options);
    let identity = DicomExportIdentity::from_seed("1.2.3".into(), "anisotropic-spacing".into());
    let jobs = dicom_export_instance_jobs(&slide, &request).unwrap();

    let reports =
        export_dicom_instance_jobs(&slide, &request, &options, &metadata, &identity, &jobs)
            .unwrap();

    let object = dicom_object::open_file(&reports[0].path).unwrap();
    let shared = object
        .element(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
        .unwrap()
        .items()
        .unwrap();
    let pixel_measures = shared[0]
        .element(tags::PIXEL_MEASURES_SEQUENCE)
        .unwrap()
        .items()
        .unwrap();
    assert_eq!(
        pixel_measures[0]
            .element(tags::PIXEL_SPACING)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "0.0005\\0.00025"
    );
}

#[test]
fn export_writes_governed_spacing_when_source_calibration_is_missing() {
    let dataset = Dataset::new(
        DatasetId::new(12),
        vec![Scene::new("scene-0", vec![one_pixel_series("series-0")])],
    );
    let slide = Slide::from_source_with_cache_bytes(Box::new(MultiSceneSource { dataset }), 0);
    let output = tempfile::tempdir().unwrap();
    let request = ExportRequest::new(
        PathBuf::from("synthetic-supplied-spacing"),
        output.path().to_path_buf(),
        ExportOptions {
            tile_size: 1,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            encode_backend: EncodeBackendPreference::CpuOnly,
            source_pixel_spacing_mm: Some(SourcePixelSpacingMm::new(0.0006, 0.0004).unwrap()),
            ..ExportOptions::default()
        },
        ColorManagement::SourceOrSrgb,
        MetadataSource::ResearchPlaceholder,
    )
    .unwrap();
    let metadata = request.metadata.resolve().unwrap();
    let options = NormalizedExportOptions::from_validated(&request.options);
    let identity = DicomExportIdentity::from_seed("1.2.3".into(), "supplied-spacing".into());
    let jobs = dicom_export_instance_jobs(&slide, &request).unwrap();

    let reports =
        export_dicom_instance_jobs(&slide, &request, &options, &metadata, &identity, &jobs)
            .unwrap();

    let object = dicom_object::open_file(&reports[0].path).unwrap();
    let shared = object
        .element(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
        .unwrap()
        .items()
        .unwrap();
    let pixel_measures = shared[0]
        .element(tags::PIXEL_MEASURES_SEQUENCE)
        .unwrap()
        .items()
        .unwrap();
    assert_eq!(
        pixel_measures[0]
            .element(tags::PIXEL_SPACING)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        "0.0006\\0.0004"
    );
}

#[test]
fn slide_spacing_uses_the_reader_owned_anisotropic_mpp() {
    let mut properties = Properties::new();
    properties.insert("openslide.vendor", "philips");
    properties.insert("openslide.mpp-x", "0.226907");
    properties.insert("openslide.mpp-y", "0.226891");
    let dataset = Dataset::new(
        DatasetId::new(9),
        vec![Scene::new("scene-0", vec![one_pixel_series("series-0")])],
    )
    .with_properties(properties);
    let slide = Slide::from_source_with_cache_bytes(Box::new(MultiSceneSource { dataset }), 0);
    let level = &slide.dataset().scenes[0].series[0].levels[0];

    assert_eq!(
        level_pixel_spacing_mm(&slide, level, None).unwrap(),
        Some((0.000226891, 0.000226907))
    );
}

#[test]
fn explicit_source_spacing_supplies_missing_metadata_and_scales_with_level() {
    let mut series = one_pixel_series("series-0");
    series.levels[0].downsample = 4.0;
    let dataset = Dataset::new(
        DatasetId::new(10),
        vec![Scene::new("scene-0", vec![series])],
    );
    let slide = Slide::from_source_with_cache_bytes(Box::new(MultiSceneSource { dataset }), 0);
    let level = &slide.dataset().scenes[0].series[0].levels[0];
    let supplied =
        SourcePixelSpacingMm::new(0.00034605325860336383, 0.0003460559834973875).unwrap();

    assert_eq!(
        level_pixel_spacing_mm(&slide, level, Some(supplied)).unwrap(),
        Some((0.0013842130344134553, 0.00138422393398955))
    );
}

#[test]
fn explicit_source_spacing_rejects_a_conflicting_source_value() {
    let mut properties = Properties::new();
    properties.insert("openslide.mpp-x", "0.5");
    properties.insert("openslide.mpp-y", "0.5");
    let dataset = Dataset::new(
        DatasetId::new(11),
        vec![Scene::new("scene-0", vec![one_pixel_series("series-0")])],
    )
    .with_properties(properties);
    let slide = Slide::from_source_with_cache_bytes(Box::new(MultiSceneSource { dataset }), 0);
    let level = &slide.dataset().scenes[0].series[0].levels[0];
    let supplied = SourcePixelSpacingMm::new(0.0006, 0.0006).unwrap();

    let error = level_pixel_spacing_mm(&slide, level, Some(supplied)).unwrap_err();

    assert!(error
        .to_string()
        .contains("conflicts with source pixel spacing"));
}

#[test]
fn preflight_accepts_same_axes_from_different_scenes_and_series() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    write_source_dicom(&source);
    let slide = Slide::open(&source).unwrap();
    let level = &slide.dataset().scenes[0].series[0].levels[0];
    let request = ExportRequest::new(
        source,
        tmp.path().join("out"),
        ExportOptions::default(),
        ColorManagement::SourceOrSrgb,
        MetadataSource::ResearchPlaceholder,
    )
    .unwrap();
    let jobs = [
        DicomExportInstanceJob {
            ordinal: 0,
            instance_number: 1,
            coordinate: InstanceCoordinate::new(0, 0, 0, 0, 0, 0),
            level,
        },
        DicomExportInstanceJob {
            ordinal: 1,
            instance_number: 2,
            coordinate: InstanceCoordinate::new(1, 3, 0, 0, 0, 0),
            level,
        },
    ];

    let options = NormalizedExportOptions::from_validated(&request.options);
    preflight_output_paths(&request, &options, &jobs).unwrap();
    assert_ne!(
        jobs[0].coordinate.output_path(&request.output_dir),
        jobs[1].coordinate.output_path(&request.output_dir)
    );
}

#[test]
fn consistent_pixel_profile_accepts_first_matching_profile_and_rejects_mismatch() {
    let rgb = PixelProfile {
        components: 3,
        bits_allocated: 8,
        photometric_interpretation: "RGB",
    };
    let gray = PixelProfile {
        components: 1,
        bits_allocated: 8,
        photometric_interpretation: "MONOCHROME2",
    };
    let mut existing = None;

    ensure_consistent_pixel_profile(&mut existing, rgb, "profile changed").unwrap();
    ensure_consistent_pixel_profile(&mut existing, rgb, "profile changed").unwrap();

    let err = ensure_consistent_pixel_profile(&mut existing, gray, "profile changed")
        .expect_err("mismatched profile should fail");
    assert!(err.to_string().contains("profile changed"));
}

#[test]
fn read_and_prepare_region_pads_cpu_region_to_requested_output_geometry() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    let pixels = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    write_source_dicom_with_pixels(&source, "1.2.826.0.1.3680043.10.999.81", 2, 2, pixels);
    let slide = Slide::open(&source).unwrap();

    let prepared = read_and_prepare_region(
        &slide,
        CpuRegionReadRequest {
            location: JpegBaselineFrameLocation::first_series_level(0),
            frame: OutputFrameRect::new(0, 0, 2, 2),
            output_width: 3,
            output_height: 3,
            max_prepared_frame_bytes: u64::MAX,
        },
    )
    .unwrap();

    assert_eq!(
        prepared.profile,
        PixelProfile {
            components: 3,
            bits_allocated: 8,
            photometric_interpretation: "RGB",
        }
    );
    assert_eq!(
        prepared.bytes,
        vec![1, 2, 3, 4, 5, 6, 0, 0, 0, 7, 8, 9, 10, 11, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,]
    );
}

#[test]
fn lossless_j2k_cpu_tile_preparation_returns_named_prepared_region() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    let pixels = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    write_source_dicom_with_pixels(&source, "1.2.826.0.1.3680043.10.999.82", 2, 2, pixels);
    let slide = Slide::open(&source).unwrap();

    let prepared: PreparedCpuRegion = prepare_cpu_input_lossless_j2k_tile(
        &slide,
        InstanceCoordinate::first_series_level(0),
        OutputFrameRect::new(0, 0, 2, 2),
        3,
        u64::MAX,
    )
    .unwrap();

    assert_eq!(
        prepared.profile,
        PixelProfile {
            components: 3,
            bits_allocated: 8,
            photometric_interpretation: "RGB",
        }
    );
    assert_eq!(prepared.bytes.len(), 27);
    assert!(prepared.input_decode_duration > Duration::ZERO);
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn metal_row_batch_target_default_is_tuned_and_not_scaled_by_pipeline_depth() {
    let options = ExportOptions::default();
    assert_eq!(
        effective_gpu_row_batch_target_tiles(&NormalizedExportOptions::from_validated(&options)),
        Some(384)
    );

    let depth_override = ExportOptions {
        gpu_pipeline_depth: Some(3),
        ..ExportOptions::default()
    };
    assert_eq!(
        effective_gpu_row_batch_target_tiles(&NormalizedExportOptions::from_validated(
            &depth_override,
        )),
        Some(384)
    );

    let explicit_target = ExportOptions {
        gpu_pipeline_depth: Some(3),
        gpu_row_batch_target_tiles: Some(96),
        ..ExportOptions::default()
    };
    assert_eq!(
        effective_gpu_row_batch_target_tiles(&NormalizedExportOptions::from_validated(
            &explicit_target,
        )),
        Some(96)
    );
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn prefer_device_htj2k_rpcl_jobs_are_split_into_gpu_and_cpu_lanes() {
    let options = ExportOptions {
        encode_backend: EncodeBackendPreference::PreferDevice,
        transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
        ..ExportOptions::default()
    };
    let normalized = |options: &ExportOptions| NormalizedExportOptions::from_validated(options);

    assert_eq!(
        hybrid_lane::prefer_device_htj2k_rpcl_hybrid_lane(&normalized(&options), 19_008),
        Some(hybrid_lane::HybridExportLane::Gpu)
    );
    assert_eq!(
        hybrid_lane::effective_lossless_gpu_row_batch_target_tiles(&normalized(&options), 19_008),
        Some(416)
    );
    assert_eq!(
        hybrid_lane::effective_lossless_gpu_encode_memory_mib(&normalized(&options), 19_008),
        Some(16_384)
    );
    assert_eq!(
        hybrid_lane::prefer_device_htj2k_rpcl_hybrid_lane(&normalized(&options), 391),
        Some(hybrid_lane::HybridExportLane::Gpu)
    );
    assert_eq!(
        hybrid_lane::prefer_device_htj2k_rpcl_hybrid_lane(&normalized(&options), 1_188),
        Some(hybrid_lane::HybridExportLane::Gpu)
    );
    assert_eq!(
        hybrid_lane::prefer_device_htj2k_rpcl_hybrid_lane(&normalized(&options), 128),
        Some(hybrid_lane::HybridExportLane::Cpu)
    );
    assert_eq!(
        hybrid_lane::effective_lossless_gpu_row_batch_target_tiles(&normalized(&options), 128),
        Some(384)
    );
    assert_eq!(
        hybrid_lane::effective_lossless_gpu_encode_memory_mib(&normalized(&options), 128),
        None
    );

    let require_device = ExportOptions {
        encode_backend: EncodeBackendPreference::RequireDevice,
        ..ExportOptions::default()
    };
    assert_eq!(
        hybrid_lane::prefer_device_htj2k_rpcl_hybrid_lane(&normalized(&require_device), 1_188,),
        None
    );

    let explicit_target = ExportOptions {
        gpu_row_batch_target_tiles: Some(320),
        ..options
    };
    assert_eq!(
        hybrid_lane::effective_lossless_gpu_row_batch_target_tiles(
            &normalized(&explicit_target),
            19_008,
        ),
        Some(320)
    );

    let explicit_memory = ExportOptions {
        gpu_encode_memory_mib: Some(8_192),
        ..options
    };
    assert_eq!(
        hybrid_lane::effective_lossless_gpu_encode_memory_mib(
            &normalized(&explicit_memory),
            19_008,
        ),
        Some(8_192)
    );
}

#[test]
fn lossless_j2k_prefer_device_backend_routing_uses_measured_cpu_cutoffs() {
    use EncodeBackendPreference::{CpuOnly, PreferDevice, RequireDevice};
    use TransferSyntax::{Htj2kLosslessRpcl, Jpeg2000, Jpeg2000Lossless};

    let backend = |encode_backend, transfer_syntax, frame_count| {
        let options = ExportOptions {
            encode_backend,
            transfer_syntax,
            ..ExportOptions::default()
        };
        effective_lossless_j2k_encode_backend(
            &NormalizedExportOptions::from_validated(&options),
            frame_count,
        )
    };
    for (encode_backend, transfer_syntax, frame_count, expected) in [
        (PreferDevice, Htj2kLosslessRpcl, 128, CpuOnly),
        (PreferDevice, Htj2kLosslessRpcl, 129, PreferDevice),
        (PreferDevice, Htj2kLosslessRpcl, 391, PreferDevice),
        (RequireDevice, Htj2kLosslessRpcl, 1_188, RequireDevice),
        (PreferDevice, Jpeg2000Lossless, 5_850, CpuOnly),
        (RequireDevice, Jpeg2000Lossless, 5_850, RequireDevice),
        (PreferDevice, Jpeg2000, 5_850, CpuOnly),
    ] {
        assert_eq!(
            backend(encode_backend, transfer_syntax, frame_count),
            expected
        );
    }
}

#[test]
fn lossless_j2k_cpu_row_batch_count_groups_rows_by_target_tiles() {
    assert_eq!(lossless_j2k_cpu_row_batch_count(8, 64), 32);
    assert_eq!(lossless_j2k_cpu_row_batch_count(384, 64), 1);
    assert_eq!(lossless_j2k_cpu_row_batch_count(0, 64), 1);
    assert_eq!(lossless_j2k_cpu_row_batch_count(8, 3), 3);
}

#[test]
fn lossless_j2k_cpu_fallback_indices_skip_ineligible_and_already_encoded_frames() {
    let mut planned = (0..5)
        .map(test_lossless_j2k_planned_frame)
        .collect::<Vec<_>>();
    planned[0].passthrough = Some(J2kPassthroughFrame {
        codestream: vec![1, 2, 3],
        profile: PixelProfile {
            components: 3,
            bits_allocated: 8,
            photometric_interpretation: "RGB",
        },
        transfer_syntax: CompressedTransferSyntax::Jpeg2000Lossless,
    });
    planned[4].width = 3;
    planned[4].source_j2k_dimensions = Some((3, 4));
    planned[4].source_j2k_syntax = Some(CompressedTransferSyntax::Jpeg2000Lossless);
    planned[3].source_j2k_dimensions = Some((4, 4));
    planned[3].source_j2k_syntax = Some(CompressedTransferSyntax::Jpeg2000Lossless);
    let already_encoded = [false, true, false, false, false];

    assert_eq!(
        lossless_j2k_cpu_fallback_indices(&planned, TransferSyntax::Htj2kLosslessRpcl, |idx| {
            already_encoded[idx]
        },),
        vec![2, 3, 4]
    );
    assert_eq!(
        lossless_j2k_cpu_fallback_indices(&planned, TransferSyntax::Jpeg2000, |_| false),
        vec![3, 4]
    );
    assert!(
        lossless_j2k_cpu_fallback_indices(&planned, TransferSyntax::Htj2k, |_| false).is_empty()
    );
}

#[test]
fn lossy_htj2k_rejection_does_not_advertise_an_intermediate_jpeg_fallback() {
    let error = unsupported_j2k_route_error(TransferSyntax::Htj2k, 3, 7);
    let Error::Unsupported { reason } = error else {
        panic!("lossy HTJ2K route rejection should be unsupported");
    };

    assert_eq!(
        reason,
        "HTJ2K 9/7 export requires direct source JPEG-to-HTJ2K transcoding; frame row=3 col=7 was not eligible"
    );
}

#[test]
fn missing_metal_frame_indices_selects_only_unencoded_slots() {
    assert_eq!(
        missing_metal_frame_indices(&[Some("metal-0"), None, Some("metal-2"), None]),
        vec![1, 3]
    );
    assert!(missing_metal_frame_indices::<&str>(&[]).is_empty());
}

#[test]
fn jpeg_baseline_fallback_run_collects_contiguous_fallback_frames() {
    let planned = vec![
        JpegBaselinePlannedFrame::Fallback {
            frame: test_jpeg_baseline_fallback_frame(0),
            source_lossy_compression: None,
        },
        JpegBaselinePlannedFrame::Fallback {
            frame: test_jpeg_baseline_fallback_frame(1),
            source_lossy_compression: None,
        },
        JpegBaselinePlannedFrame::Blank {
            profile: test_rgb8_pixel_profile(),
            uncompressed_bytes: 1,
        },
        JpegBaselinePlannedFrame::Fallback {
            frame: test_jpeg_baseline_fallback_frame(2),
            source_lossy_compression: None,
        },
    ];

    let (next_index, fallback_frames) = jpeg_baseline_fallback_run(&planned, 0);
    assert_eq!(next_index, 2);
    assert_eq!(
        fallback_frames
            .iter()
            .map(|frame| (frame.x, frame.y, frame.width, frame.height))
            .collect::<Vec<_>>(),
        vec![(0, 0, 4, 4), (4, 0, 4, 4)]
    );

    let (next_index, fallback_frames) = jpeg_baseline_fallback_run(&planned, 3);
    assert_eq!(next_index, 4);
    assert_eq!(
        fallback_frames
            .iter()
            .map(|frame| frame.x)
            .collect::<Vec<_>>(),
        vec![8]
    );
}

#[test]
fn shared_route_planner_uses_codec_specific_precedence() {
    use crate::export::route_plan::{
        FrameRouteSource, PlannedFrameRoute, RouteExecutionContext, RoutePlanner,
    };

    let jpeg = RoutePlanner::new(RouteExecutionContext::new(
        TransferSyntax::JpegBaseline8Bit,
        EncodeBackendPreference::CpuOnly,
    ));
    assert_eq!(
        jpeg.decide(FrameRouteSource::Jpeg {
            passthrough: true,
            retile: true,
            blank: false,
        })
        .route,
        PlannedFrameRoute::JpegPassthrough
    );
    assert_eq!(
        jpeg.decide(FrameRouteSource::Jpeg {
            passthrough: false,
            retile: true,
            blank: false,
        })
        .route,
        PlannedFrameRoute::JpegRetile
    );

    let j2k = RoutePlanner::new(RouteExecutionContext::new(
        TransferSyntax::Htj2kLossless,
        EncodeBackendPreference::PreferDevice,
    ));
    assert_eq!(
        j2k.decide(FrameRouteSource::J2k {
            passthrough: false,
            direct_j2k: true,
            direct_jpeg: true,
            j2k_reencode: true,
        })
        .route,
        PlannedFrameRoute::DirectJ2kToHtj2k
    );
}

#[test]
fn shared_route_planner_rejects_non_passthrough_jpeg2000_frames() {
    use crate::export::route_plan::{
        FrameRouteSource, PlannedFrameRoute, RouteExecutionContext, RoutePlanner, RouteRejection,
    };

    let planner = RoutePlanner::new(RouteExecutionContext::new(
        TransferSyntax::Jpeg2000,
        EncodeBackendPreference::PreferDevice,
    ));
    let decision = planner.decide(FrameRouteSource::J2k {
        passthrough: false,
        direct_j2k: false,
        direct_jpeg: false,
        j2k_reencode: false,
    });

    assert_eq!(
        decision.route,
        PlannedFrameRoute::Unsupported(RouteRejection::PassthroughRequired)
    );
    assert_eq!(
        decision.rejections.passthrough,
        Some(RouteRejection::SourceRouteUnavailable)
    );
}

#[test]
fn shared_route_planner_marks_device_fallback_as_candidate_only() {
    use crate::export::route_plan::{
        FrameRouteSource, PlannedFrameRoute, RouteExecutionContext, RoutePlanner,
    };

    for backend in [
        EncodeBackendPreference::Auto,
        EncodeBackendPreference::PreferDevice,
        EncodeBackendPreference::RequireDevice,
    ] {
        let decision = RoutePlanner::new(RouteExecutionContext::new(
            TransferSyntax::Htj2kLosslessRpcl,
            backend,
        ))
        .decide(FrameRouteSource::J2k {
            passthrough: false,
            direct_j2k: false,
            direct_jpeg: false,
            j2k_reencode: false,
        });
        assert_eq!(decision.route, PlannedFrameRoute::J2kDeviceEncodeCandidate);
    }

    let cpu = RoutePlanner::new(RouteExecutionContext::new(
        TransferSyntax::Htj2kLosslessRpcl,
        EncodeBackendPreference::CpuOnly,
    ))
    .decide(FrameRouteSource::J2k {
        passthrough: false,
        direct_j2k: false,
        direct_jpeg: false,
        j2k_reencode: false,
    });
    assert_eq!(cpu.route, PlannedFrameRoute::J2kCpuEncode);
}

#[test]
fn jpeg_baseline_fallback_frame_clips_edge_frames() {
    let frame = jpeg_baseline_fallback_frame(
        2,
        1,
        FrameRectGrid {
            matrix_columns: 10,
            matrix_rows: 7,
            frame_columns: 4,
            frame_rows: 4,
        },
    )
    .expect("edge frame should fit inside matrix");

    assert_eq!((frame.x, frame.y, frame.width, frame.height), (8, 4, 2, 3));
}

#[test]
fn codec_fallback_frame_types_share_output_frame_rect() {
    let rect = OutputFrameRect::new(3, 5, 7, 11);
    let jpeg_frame: JpegBaselineFallbackFrame = rect;
    let j2k_frame: LosslessJ2kCpuBatchFrame = rect;

    assert_eq!(
        (
            jpeg_frame.x,
            jpeg_frame.y,
            jpeg_frame.width,
            jpeg_frame.height
        ),
        (3, 5, 7, 11)
    );
    assert_eq!(
        (j2k_frame.x, j2k_frame.y, j2k_frame.width, j2k_frame.height),
        (3, 5, 7, 11)
    );
}

#[test]
fn scatter_indexed_results_places_values_by_original_index() {
    let mut slots = vec![None, None, None, None];
    scatter_indexed_results(&mut slots, [(2, "two"), (0, "zero")]).expect("indices are in range");

    assert_eq!(slots, vec![Some("zero"), None, Some("two"), None]);
    assert!(scatter_indexed_results(&mut slots, [(4, "bad")]).is_err());

    let mut duplicate_slots = vec![None; 2];
    let duplicate =
        scatter_indexed_results(&mut duplicate_slots, [(1, "first"), (1, "second")]).unwrap_err();
    assert!(duplicate.to_string().contains("duplicate"));
}

#[test]
fn lossless_j2k_planned_frame_exposes_shared_output_rect() {
    let planned = test_lossless_j2k_planned_frame(2);

    assert_eq!(planned.rect(), OutputFrameRect::new(8, 0, 4, 4));
}
