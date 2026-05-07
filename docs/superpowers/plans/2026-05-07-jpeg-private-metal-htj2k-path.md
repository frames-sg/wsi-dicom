# JPEG Private Metal to HTJ2K Handoff Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce JPEG-source input overhead for Metal HTJ2K export without changing public CPU-readable `Surface` semantics.

**Architecture:** Capture a baseline first. Then make only GPU-written JPEG internal planes/intermediates `StorageModePrivate` while keeping public final RGB `Surface` output `StorageModeShared`. If that moves `input_decode_micros`, add an internal private JPEG-to-HTJ2K handoff type rather than exposing Private buffers through the current public `Surface::as_bytes()` API.

**Tech Stack:** Rust, `metal-rs`, `signinum-jpeg-metal`, `statumen`, `wsi-dicom`, `signinum-j2k-metal`.

**Result record:** See `docs/superpowers/results/2026-05-07-metal-htj2k-benchmark-results.md`.

---

### Task 1: Baseline Before Changes

**Files:**
- Modify only benchmark notes if a persistent note file already exists.

- [ ] **Step 1: Build current release binary**

Run from `/Users/user/Bench/wsi-dicom`:

```bash
cargo build --release --features metal
```

Expected: release build exits 0.

- [ ] **Step 2: Run baseline CMU-1 HTJ2K Metal export**

Run:

```bash
OUT="/tmp/wsi-dicom-baseline-jpeg-htj2k-cmu1"
/usr/bin/time -l target/release/wsi-dicom convert bench/testdata/CMU-1.tiff \
  --out "$OUT" \
  --transfer-syntax htj2k-lossless-rpcl \
  --backend prefer-device \
  --source-device-decode \
  --json \
  --level 0
trash "$OUT"
```

Record wall time plus these JSON metrics: `input_decode_micros`, `gpu_encode_wall_micros`, `gpu_encode_hardware_micros`, `write_micros`, `resident_gpu_transcode_frames`, and any wait/dispatch counters already emitted.

- [ ] **Step 3: Record optional real-slide baselines**

If local NDPI/SVS fixtures are available, run the same command shape with each source path and a unique `/tmp` output directory. Record the same metrics. Do not modify `bench/` outputs unless the user explicitly asks to store benchmark artifacts.

### Task 2: Add JPEG Internal Private Allocation Tests

**Files:**
- Modify: `/Users/user/Bench/signinum/crates/signinum-jpeg-metal/src/compute.rs`
- Modify: `/Users/user/Bench/signinum/crates/signinum-jpeg-metal/src/lib.rs`

- [ ] **Step 1: Add test-only allocation counters**

Add counters in `compute.rs` near the existing macOS Metal test instrumentation:

```rust
#[cfg(all(target_os = "macos", test))]
static JPEG_PRIVATE_BUFFER_ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(target_os = "macos", test))]
pub(crate) fn reset_jpeg_private_buffer_allocations_for_test() {
    JPEG_PRIVATE_BUFFER_ALLOCATIONS.store(0, Ordering::Relaxed);
}

#[cfg(all(target_os = "macos", test))]
pub(crate) fn jpeg_private_buffer_allocations_for_test() -> usize {
    JPEG_PRIVATE_BUFFER_ALLOCATIONS.load(Ordering::Relaxed)
}
```

- [ ] **Step 2: Add a focused resident-decode test**

Add a test in `lib.rs` using an existing fast JPEG fixture/helper. The test should:

```rust
compute::reset_jpeg_private_buffer_allocations_for_test();
let surface = decoder
    .decode_to_device_with_session(PixelFormat::Rgb8, &session)
    .expect("resident JPEG Metal decode");
assert_eq!(surface.residency(), SurfaceResidency::MetalResidentDecode);
assert!(
    compute::jpeg_private_buffer_allocations_for_test() > 0,
    "resident JPEG Metal decode should use Private internal planes"
);
let _ = surface.as_bytes();
```

The final `as_bytes()` call is intentional: current public RGB surfaces must remain CPU-readable after Task 2.

- [ ] **Step 3: Verify the test fails before implementation**

Run:

```bash
cargo test -p signinum-jpeg-metal jpeg_device_decode_uses_private_internal_planes -- --nocapture
```

Expected before implementation: failure because the current fast decode planes are `StorageModeShared`.

### Task 3: Use Private Buffers Only for GPU-Written JPEG Internals

**Files:**
- Modify: `/Users/user/Bench/signinum/crates/signinum-jpeg-metal/src/compute.rs`

- [ ] **Step 1: Add allocation helpers**

Add:

```rust
#[cfg(target_os = "macos")]
fn new_shared_buffer(device: &Device, bytes: usize) -> Buffer {
    device.new_buffer(bytes.max(1) as u64, MTLResourceOptions::StorageModeShared)
}

#[cfg(target_os = "macos")]
fn new_private_buffer(device: &Device, bytes: usize) -> Buffer {
    #[cfg(test)]
    JPEG_PRIVATE_BUFFER_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    device.new_buffer(bytes.max(1) as u64, MTLResourceOptions::StorageModePrivate)
}
```

- [ ] **Step 2: Change only fast-path JPEG decode planes**

In fast 4:4:4, 4:2:2, and 4:2:0 decode functions, replace GPU-written Y/Cb/Cr plane allocations with `new_private_buffer`.

Do not change `PlaneStage::new` or `cached_plane_stage`; those paths use CPU row writers and must stay Shared.

- [ ] **Step 3: Keep public final RGB Shared**

Keep `PlaneStage::dispatch_with_runtime` output allocation Shared. The public `Surface` returned by `decode_to_device_with_session` must still support `as_bytes()` and `download_into()` without a Private-buffer blit path.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p signinum-jpeg-metal jpeg_device_decode_uses_private_internal_planes -- --nocapture
cargo test -p signinum-jpeg-metal
cargo clippy -p signinum-jpeg-metal --all-targets -- -D warnings
```

Expected: tests and clippy pass.

### Task 4: Benchmark After Internal Private Allocations

**Files:**
- Modify only benchmark notes if needed.

- [ ] **Step 1: Rebuild release**

Run:

```bash
cargo build --release --features metal
```

- [ ] **Step 2: Re-run the same benchmark commands**

Use the exact sources and flags from Task 1. Record the same metrics.

- [ ] **Step 3: Decision gate**

If `input_decode_micros` and wall time do not move materially, stop this workstream and shift effort to the HTJ2K SIMD Tier-1 plan. If `input_decode_micros` improves and decode/encode synchronization remains visible, proceed to Task 5.

### Task 5: Add Internal Private JPEG-to-HTJ2K Handoff Type

**Files:**
- Modify: `/Users/user/Bench/signinum/crates/signinum-jpeg-metal/src/lib.rs`
- Modify: `/Users/user/Bench/signinum/crates/signinum-jpeg-metal/src/compute.rs`
- Modify: `/Users/user/Bench/statumen/src/decode/jpeg.rs`
- Modify: `/Users/user/Bench/statumen/src/output/metal.rs`
- Modify: `/Users/user/Bench/wsi-dicom/src/lib.rs`

- [ ] **Step 1: Add an internal resident-private JPEG output**

Add an internal type, not a public `Surface` replacement:

```rust
#[cfg(target_os = "macos")]
pub(crate) struct ResidentPrivateJpegTile {
    pub(crate) buffer: Buffer,
    pub(crate) byte_offset: usize,
    pub(crate) dimensions: (u32, u32),
    pub(crate) pixel_format: PixelFormat,
    pub(crate) pitch_bytes: usize,
    pub(crate) status_buffer: Buffer,
    pub(crate) command_buffer: CommandBuffer,
}
```

This type is for internal Metal consumers only. It must not expose `as_bytes()`.

- [ ] **Step 2: Add a private consumer decode method**

Add an internal method that returns `ResidentPrivateJpegTile` for supported fast decode paths. Its RGB pack output may be `StorageModePrivate`. Unsupported JPEG paths should return a clear unsupported error or fall back to the current public Shared `Surface` path when device-private output is not required.

- [ ] **Step 3: Thread through statumen without changing public CPU APIs**

Extend the Metal tile wrapper path so a private JPEG tile can become a `MetalDeviceTile` for GPU consumers. Do not make CPU tile paths read from Private buffers.

- [ ] **Step 4: Use the private handoff only for WSI-DICOM HTJ2K Metal source-device-decode**

In `wsi-dicom`, route this only when the export path is already `--source-device-decode` plus Metal HTJ2K/J2K encode. Other read/display/export paths keep the existing public Shared surface behavior.

- [ ] **Step 5: Verify**

Run:

```bash
cargo test -p signinum-jpeg-metal
cargo test -p statumen jpeg
cargo test --features metal
cargo clippy --all-targets --features metal -- -D warnings
git diff --check
```

Expected: no public `Surface` semantic change, ordered WSI output preserved, and unsupported private paths fail explicitly rather than silently falling back when device-private output is required.

### Task 6: Benchmark and Decide on Decode/Encode Fusion

**Files:**
- Modify only benchmark notes if needed.

- [ ] **Step 1: Re-run release benchmark**

Use the exact benchmark command set from Task 1.

- [ ] **Step 2: Decision gate**

If `input_decode_micros` is now small and wall time is dominated by `gpu_encode_hardware_micros`, stop here and execute the HTJ2K SIMD Tier-1 plan. If wall time still shows decode/encode wait boundaries, create a separate fusion plan for command-buffer/fence dependency plumbing.
