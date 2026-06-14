use super::*;

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
        JpegBaselineFrameLocation::first_series_level(0),
        0,
        0,
        2,
        2,
        3,
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

    let prepared: PreparedCpuRegion =
        prepare_cpu_input_lossless_j2k_tile(&slide, 0, 0, 0, 0, 0, 0, 0, 0, 2, 2, 3, u64::MAX)
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
    assert_eq!(effective_gpu_row_batch_target_tiles(&options), Some(384));

    let depth_override = ExportOptions {
        gpu_pipeline_depth: Some(3),
        ..ExportOptions::default()
    };
    assert_eq!(
        effective_gpu_row_batch_target_tiles(&depth_override),
        Some(384)
    );

    let explicit_target = ExportOptions {
        gpu_pipeline_depth: Some(3),
        gpu_row_batch_target_tiles: Some(96),
        ..ExportOptions::default()
    };
    assert_eq!(
        effective_gpu_row_batch_target_tiles(&explicit_target),
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

    assert_eq!(
        hybrid_lane::prefer_device_htj2k_rpcl_hybrid_lane(&options, 19_008),
        Some(hybrid_lane::HybridExportLane::Gpu)
    );
    assert_eq!(
        hybrid_lane::effective_lossless_gpu_row_batch_target_tiles(&options, 19_008),
        Some(416)
    );
    assert_eq!(
        hybrid_lane::effective_lossless_gpu_encode_memory_mib(&options, 19_008),
        Some(16_384)
    );
    assert_eq!(
        hybrid_lane::prefer_device_htj2k_rpcl_hybrid_lane(&options, 391),
        Some(hybrid_lane::HybridExportLane::Gpu)
    );
    assert_eq!(
        hybrid_lane::prefer_device_htj2k_rpcl_hybrid_lane(&options, 1_188),
        Some(hybrid_lane::HybridExportLane::Gpu)
    );
    assert_eq!(
        hybrid_lane::prefer_device_htj2k_rpcl_hybrid_lane(&options, 128),
        Some(hybrid_lane::HybridExportLane::Cpu)
    );
    assert_eq!(
        hybrid_lane::effective_lossless_gpu_row_batch_target_tiles(&options, 128),
        Some(384)
    );
    assert_eq!(
        hybrid_lane::effective_lossless_gpu_encode_memory_mib(&options, 128),
        None
    );

    let require_device = ExportOptions {
        encode_backend: EncodeBackendPreference::RequireDevice,
        ..ExportOptions::default()
    };
    assert_eq!(
        hybrid_lane::prefer_device_htj2k_rpcl_hybrid_lane(&require_device, 1_188),
        None
    );

    let explicit_target = ExportOptions {
        gpu_row_batch_target_tiles: Some(320),
        ..options
    };
    assert_eq!(
        hybrid_lane::effective_lossless_gpu_row_batch_target_tiles(&explicit_target, 19_008),
        Some(320)
    );

    let explicit_memory = ExportOptions {
        gpu_encode_memory_mib: Some(8_192),
        ..options
    };
    assert_eq!(
        hybrid_lane::effective_lossless_gpu_encode_memory_mib(&explicit_memory, 19_008),
        Some(8_192)
    );
}

#[test]
fn lossless_j2k_prefer_device_backend_routing_uses_measured_cpu_cutoffs() {
    use EncodeBackendPreference::{CpuOnly, PreferDevice, RequireDevice};
    use TransferSyntax::{Htj2kLosslessRpcl, Jpeg2000, Jpeg2000Lossless};

    let backend = |encode_backend, transfer_syntax, frame_count| {
        effective_lossless_j2k_encode_backend(
            &ExportOptions {
                encode_backend,
                transfer_syntax,
                ..ExportOptions::default()
            },
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
    let already_encoded = [false, true, false, true, false];

    assert_eq!(
        lossless_j2k_cpu_fallback_indices(&planned, TransferSyntax::Htj2kLosslessRpcl, 4, |idx| {
            already_encoded[idx]
        },),
        vec![2, 4]
    );
    assert_eq!(
        lossless_j2k_cpu_fallback_indices(&planned, TransferSyntax::Jpeg2000, 4, |_| false),
        vec![4]
    );
    assert!(
        lossless_j2k_cpu_fallback_indices(&planned, TransferSyntax::Htj2k, 4, |_| false).is_empty()
    );
}

#[test]
fn generated_jpeg_direct_htj2k_indices_centralize_candidate_selection() {
    let mut planned = vec![test_lossless_j2k_planned_frame(0)];
    planned[0].source_jpeg_direct_rejected = true;

    assert!(generated_jpeg_direct_htj2k_indices(
        &planned,
        TransferSyntax::Htj2kLosslessRpcl,
        |_| false,
    )
    .is_empty());
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
        JpegBaselinePlannedFrame::Fallback(test_jpeg_baseline_fallback_frame(0)),
        JpegBaselinePlannedFrame::Fallback(test_jpeg_baseline_fallback_frame(1)),
        JpegBaselinePlannedFrame::Blank {
            data: vec![255],
            profile: test_rgb8_pixel_profile(),
            uncompressed_bytes: 1,
            encode_duration: Duration::ZERO,
        },
        JpegBaselinePlannedFrame::Fallback(test_jpeg_baseline_fallback_frame(2)),
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
fn jpeg_baseline_fallback_frame_clips_edge_frames() {
    let frame =
        jpeg_baseline_fallback_frame(2, 1, 10, 7, 4, 4, "test x overflow", "test y overflow")
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
}

#[test]
fn lossless_j2k_planned_frame_exposes_shared_output_rect() {
    let planned = test_lossless_j2k_planned_frame(2);

    assert_eq!(planned.rect(), OutputFrameRect::new(8, 0, 4, 4));
}
