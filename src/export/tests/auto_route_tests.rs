use super::*;

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn auto_metal_input_routing_ignores_device_decode_env_until_explicitly_preferred() {
    let _guard = DEVICE_DECODE_ENV_MUTEX.lock().unwrap();
    let old_jpeg = std::env::var_os(STATUMEN_JPEG_DEVICE_DECODE_ENV);
    let old_jp2k = std::env::var_os(STATUMEN_JP2K_DEVICE_DECODE_ENV);
    std::env::remove_var(STATUMEN_JPEG_DEVICE_DECODE_ENV);
    std::env::remove_var(STATUMEN_JP2K_DEVICE_DECODE_ENV);

    assert!(!statumen_device_decode_opted_in());
    assert!(!MetalInputTileReader::new(EncodeBackendPreference::Auto, false).enabled());
    assert!(!lossless_j2k_auto_allows_metal_input(
        EncodeBackendPreference::Auto,
        TransferSyntax::Htj2kLosslessRpcl,
        1,
        true
    ));
    assert!(!lossless_j2k_auto_allows_metal_input(
        EncodeBackendPreference::Auto,
        TransferSyntax::Htj2kLosslessRpcl,
        15,
        true
    ));
    assert!(lossless_j2k_auto_allows_metal_input(
        EncodeBackendPreference::Auto,
        TransferSyntax::Htj2kLosslessRpcl,
        16,
        true
    ));
    assert!(lossless_j2k_auto_allows_metal_input(
        EncodeBackendPreference::Auto,
        TransferSyntax::Htj2kLosslessRpcl,
        16,
        false
    ));
    assert!(lossless_j2k_auto_allows_metal_input(
        EncodeBackendPreference::Auto,
        TransferSyntax::Htj2kLossless,
        16,
        true
    ));
    assert!(auto_metal_input_route_cache_key(
        &PathBuf::from("slide.svs"),
        ExportOptions {
            transfer_syntax: TransferSyntax::Htj2kLossless,
            encode_backend: EncodeBackendPreference::Auto,
            ..ExportOptions::default()
        },
        0,
        16
    )
    .is_some());
    assert!(lossless_j2k_auto_should_start_cpu_only(
        EncodeBackendPreference::Auto,
        TransferSyntax::Htj2kLosslessRpcl,
        1,
        true
    ));
    assert!(!lossless_j2k_auto_should_start_cpu_only(
        EncodeBackendPreference::Auto,
        TransferSyntax::Htj2kLosslessRpcl,
        16,
        true
    ));
    assert!(lossless_j2k_auto_should_start_cpu_only(
        EncodeBackendPreference::Auto,
        TransferSyntax::Jpeg2000Lossless,
        64,
        true
    ));
    assert!(!lossless_j2k_auto_should_start_cpu_only(
        EncodeBackendPreference::PreferDevice,
        TransferSyntax::Htj2kLosslessRpcl,
        1,
        true
    ));
    assert!(!jpeg_baseline_auto_allows_metal_batch(
        EncodeBackendPreference::Auto,
        512,
        512,
        4,
        false
    ));

    std::env::set_var(STATUMEN_JP2K_DEVICE_DECODE_ENV, "1");
    assert!(statumen_device_decode_opted_in());
    assert!(!MetalInputTileReader::new(EncodeBackendPreference::Auto, false).enabled());
    assert!(!lossless_j2k_auto_allows_metal_input(
        EncodeBackendPreference::Auto,
        TransferSyntax::Htj2kLosslessRpcl,
        1,
        false
    ));
    assert!(lossless_j2k_auto_allows_metal_input(
        EncodeBackendPreference::Auto,
        TransferSyntax::Htj2kLosslessRpcl,
        30,
        false
    ));
    assert!(!lossless_j2k_auto_allows_metal_input(
        EncodeBackendPreference::Auto,
        TransferSyntax::Jpeg2000Lossless,
        1,
        false
    ));
    assert!(!jpeg_baseline_auto_allows_metal_batch(
        EncodeBackendPreference::Auto,
        512,
        512,
        1,
        true
    ));
    assert!(!jpeg_baseline_auto_allows_metal_batch(
        EncodeBackendPreference::Auto,
        256,
        512,
        4,
        true
    ));
    assert!(!jpeg_baseline_auto_allows_metal_batch(
        EncodeBackendPreference::Auto,
        512,
        512,
        2,
        true
    ));
    assert!(!jpeg_baseline_auto_allows_metal_batch(
        EncodeBackendPreference::Auto,
        512,
        512,
        4,
        false
    ));
    assert!(jpeg_baseline_auto_allows_metal_batch(
        EncodeBackendPreference::Auto,
        512,
        512,
        4,
        true
    ));
    assert!(jpeg_baseline_auto_allows_metal_batch(
        EncodeBackendPreference::PreferDevice,
        64,
        64,
        1,
        false
    ));
    assert!(!jpeg_baseline_auto_allows_metal_batch(
        EncodeBackendPreference::CpuOnly,
        1024,
        1024,
        8,
        true
    ));

    match old_jpeg {
        Some(value) => std::env::set_var(STATUMEN_JPEG_DEVICE_DECODE_ENV, value),
        None => std::env::remove_var(STATUMEN_JPEG_DEVICE_DECODE_ENV),
    }
    match old_jp2k {
        Some(value) => std::env::set_var(STATUMEN_JP2K_DEVICE_DECODE_ENV, value),
        None => std::env::remove_var(STATUMEN_JP2K_DEVICE_DECODE_ENV),
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn auto_lossless_j2k_probe_covers_minimum_decision_scope() {
    assert!(LOSSLESS_J2K_AUTO_ROUTE_PROBE_MAX_FRAMES as u64 >= LOSSLESS_J2K_AUTO_ROUTE_MIN_FRAMES);
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn generated_jpeg_direct_5_3_remains_allowed_with_active_metal_route() {
    let auto_reader = MetalInputTileReader::new_for_lossless_j2k(
        EncodeBackendPreference::Auto,
        true,
        None,
        false,
    );
    assert!(auto_reader.enabled());
    assert!(generated_jpeg_direct_htj2k_allowed_for_route(
        TransferSyntax::Htj2kLosslessRpcl,
        &auto_reader,
    ));

    let cpu_reader = MetalInputTileReader::new_for_lossless_j2k(
        EncodeBackendPreference::CpuOnly,
        false,
        None,
        false,
    );
    assert!(generated_jpeg_direct_htj2k_allowed_for_route(
        TransferSyntax::Htj2kLosslessRpcl,
        &cpu_reader,
    ));
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn lossless_j2k_source_device_decode_enables_private_jpeg_handoff() {
    let reader = MetalInputTileReader::new_for_lossless_j2k(
        EncodeBackendPreference::PreferDevice,
        true,
        None,
        true,
    );
    assert!(reader.private_jpeg_decode);

    let reader = MetalInputTileReader::new_for_lossless_j2k(
        EncodeBackendPreference::Auto,
        true,
        None,
        false,
    );
    assert!(reader.private_jpeg_decode);

    let jpeg_baseline_reader =
        MetalInputTileReader::new(EncodeBackendPreference::PreferDevice, true);
    assert!(!jpeg_baseline_reader.private_jpeg_decode);
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn auto_lossless_j2k_probe_requires_material_speedup() {
    assert_eq!(
        select_auto_lossless_j2k_probe_route(
            auto_route_candidate(true, 1_000),
            auto_route_candidate(true, 920),
            auto_route_candidate(false, 1),
        ),
        AutoLosslessJ2kRouteDecision::CpuOnly
    );
    assert_eq!(
        select_auto_lossless_j2k_probe_route(
            auto_route_candidate(true, 1_000),
            auto_route_candidate(true, 910),
            auto_route_candidate(false, 1),
        ),
        AutoLosslessJ2kRouteDecision::CpuInputDeviceEncode
    );
    assert_eq!(
        select_auto_lossless_j2k_probe_route(
            auto_route_candidate(true, 1_000),
            auto_route_candidate(true, 780),
            auto_route_candidate(true, 700),
        ),
        AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode
    );
    assert_eq!(
        select_auto_lossless_j2k_probe_route(
            auto_route_candidate(false, 1_000),
            auto_route_candidate(true, 900),
            auto_route_candidate(true, 800),
        ),
        AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode
    );
    assert_eq!(
        select_auto_lossless_j2k_probe_route(
            auto_route_candidate(true, 1_000),
            auto_route_candidate(false, 1),
            auto_route_candidate(false, 1),
        ),
        AutoLosslessJ2kRouteDecision::CpuOnly
    );
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn auto_cpu_input_device_encode_allows_gray_and_rgb_profiles() {
    let gray_run = CpuEncodedTileRun {
        tiles: vec![(
            Err(Error::Unsupported {
                reason: "not encoded in this selector test".into(),
            }),
            PixelProfile {
                components: 1,
                bits_allocated: 8,
                photometric_interpretation: "MONOCHROME2",
            },
        )],
        input_decode_duration: Duration::ZERO,
        compose_duration: Duration::ZERO,
    };
    let rgb_run = CpuEncodedTileRun {
        tiles: vec![(
            Err(Error::Unsupported {
                reason: "not encoded in this selector test".into(),
            }),
            PixelProfile {
                components: 3,
                bits_allocated: 8,
                photometric_interpretation: "RGB",
            },
        )],
        input_decode_duration: Duration::ZERO,
        compose_duration: Duration::ZERO,
    };
    let cmyk_run = CpuEncodedTileRun {
        tiles: vec![(
            Err(Error::Unsupported {
                reason: "not encoded in this selector test".into(),
            }),
            PixelProfile {
                components: 4,
                bits_allocated: 8,
                photometric_interpretation: "CMYK",
            },
        )],
        input_decode_duration: Duration::ZERO,
        compose_duration: Duration::ZERO,
    };

    assert!(cpu_input_device_encode_auto_allowed(&gray_run));
    assert!(cpu_input_device_encode_auto_allowed(&rgb_run));
    assert!(!cpu_input_device_encode_auto_allowed(&cmyk_run));
    assert!(!cpu_input_device_encode_auto_probe_allowed(
        &rgb_run,
        LOSSLESS_J2K_AUTO_PARTIAL_GPU_MIN_FRAMES - 1
    ));
    assert!(cpu_input_device_encode_auto_probe_allowed(
        &rgb_run,
        LOSSLESS_J2K_AUTO_PARTIAL_GPU_MIN_FRAMES
    ));
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn auto_metal_input_route_cache_reuses_probe_decision() {
    let _guard = DEVICE_DECODE_ENV_MUTEX.lock().unwrap();
    clear_auto_metal_input_route_cache_for_tests();
    clear_auto_metal_input_route_cache_state_for_tests();
    let key = AutoMetalInputRouteCacheKey {
        source_path: PathBuf::from("slide.svs"),
        level: 2,
        tile_size: 512,
        transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
        route_scope_frames: 1,
    };
    let full_key = AutoMetalInputRouteCacheKey {
        source_path: PathBuf::from("slide.svs"),
        level: 2,
        tile_size: 512,
        transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
        route_scope_frames: 128,
    };
    let partial_key = AutoMetalInputRouteCacheKey {
        source_path: PathBuf::from("partial.svs"),
        level: 0,
        tile_size: 512,
        transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
        route_scope_frames: 16,
    };

    let mut reader = MetalInputTileReader::new_with_auto_device_decode_and_cache_key(
        EncodeBackendPreference::Auto,
        true,
        Some(key.clone()),
        false,
    );
    assert!(reader.enabled());
    assert!(reader.auto_input_probe_pending());
    reader.record_auto_route_probe_decision(AutoLosslessJ2kRouteDecision::CpuOnly);

    let cached_cpu_reader = MetalInputTileReader::new_with_auto_device_decode_and_cache_key(
        EncodeBackendPreference::Auto,
        true,
        Some(key.clone()),
        false,
    );
    assert!(!cached_cpu_reader.enabled());
    assert!(!cached_cpu_reader.auto_input_probe_pending());
    assert_eq!(
        cached_cpu_reader.auto_route_decision(),
        AutoLosslessJ2kRouteDecision::CpuOnly
    );

    let uncached_full_reader = MetalInputTileReader::new_with_auto_device_decode_and_cache_key(
        EncodeBackendPreference::Auto,
        true,
        Some(full_key),
        false,
    );
    assert!(uncached_full_reader.enabled());
    assert!(uncached_full_reader.auto_input_probe_pending());

    store_cached_auto_metal_input_decision(
        &key,
        AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode,
    );
    let cached_gpu_reader = MetalInputTileReader::new_with_auto_device_decode_and_cache_key(
        EncodeBackendPreference::Auto,
        true,
        Some(key),
        false,
    );
    assert!(cached_gpu_reader.enabled());
    assert!(!cached_gpu_reader.auto_input_probe_pending());
    assert_eq!(
        cached_gpu_reader.auto_route_decision(),
        AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode
    );

    store_cached_auto_metal_input_decision(
        &partial_key,
        AutoLosslessJ2kRouteDecision::CpuInputDeviceEncode,
    );
    let cached_partial_reader = MetalInputTileReader::new_with_auto_device_decode_and_cache_key(
        EncodeBackendPreference::Auto,
        true,
        Some(partial_key),
        false,
    );
    assert!(!cached_partial_reader.enabled());
    assert!(!cached_partial_reader.auto_input_probe_pending());
    assert_eq!(
        cached_partial_reader.auto_route_decision(),
        AutoLosslessJ2kRouteDecision::CpuInputDeviceEncode
    );

    clear_auto_metal_input_route_cache_for_tests();
    clear_auto_metal_input_route_cache_state_for_tests();
}

#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn auto_metal_input_route_cache_can_persist_when_env_path_is_set() {
    let _guard = DEVICE_DECODE_ENV_MUTEX.lock().unwrap();
    clear_auto_metal_input_route_cache_for_tests();
    clear_auto_metal_input_route_cache_state_for_tests();
    let old_cache = std::env::var_os(WSI_DICOM_AUTO_ROUTE_CACHE_ENV);
    let tmp = tempfile::tempdir().unwrap();
    let cache_path = tmp.path().join("auto-route-cache.json");
    std::env::set_var(WSI_DICOM_AUTO_ROUTE_CACHE_ENV, &cache_path);

    let key = AutoMetalInputRouteCacheKey {
        source_path: PathBuf::from("slide.svs"),
        level: 2,
        tile_size: 512,
        transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
        route_scope_frames: 128,
    };
    store_cached_auto_metal_input_decision(
        &key,
        AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode,
    );
    flush_persistent_auto_metal_input_route_cache_if_requested().unwrap();

    clear_auto_metal_input_route_cache_for_tests();
    clear_auto_metal_input_route_cache_state_for_tests();
    load_persistent_auto_metal_input_route_cache_if_requested().unwrap();

    assert_eq!(
        cached_auto_metal_input_decision(&key),
        Some(AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode)
    );

    match old_cache {
        Some(value) => std::env::set_var(WSI_DICOM_AUTO_ROUTE_CACHE_ENV, value),
        None => std::env::remove_var(WSI_DICOM_AUTO_ROUTE_CACHE_ENV),
    }
    clear_auto_metal_input_route_cache_for_tests();
    clear_auto_metal_input_route_cache_state_for_tests();
}
