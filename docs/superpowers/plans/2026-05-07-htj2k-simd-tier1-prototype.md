# HTJ2K SIMD-Group Tier-1 Prototype Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce HTJ2K Metal GPU hardware time by replacing the current one-scalar-thread-per-code-block Tier-1 kernel with a narrow SIMD-group cooperative prototype.

**Architecture:** Keep the existing flat mega-buffer batch layout and scalar kernel as the correctness baseline. Add a separate prototype kernel for a constrained HTJ2K code-block shape, prove bit-exact output against the scalar kernel, then expand the cooperative work from max-magnitude reduction to length estimation and prefix placement. Packetization and ICB/argument-buffer work stay out of this first prototype.

**Tech Stack:** Rust, `metal-rs`, Metal Shading Language, `signinum-j2k-metal`, `signinum-j2k-native` parity checks.

---

### Task 1: Baseline and Hotspot Confirmation

**Files:**
- Modify only benchmark notes if needed.

- [ ] **Step 1: Build current release binary**

Run from `/Users/user/Bench/wsi-dicom`:

```bash
cargo build --release --features metal
```

Expected: release build exits 0.

- [ ] **Step 2: Run baseline HTJ2K Metal export**

Run:

```bash
OUT="/tmp/wsi-dicom-baseline-htj2k-simd-cmu1"
/usr/bin/time -l target/release/wsi-dicom convert bench/testdata/CMU-1.tiff \
  --out "$OUT" \
  --transfer-syntax htj2k-lossless-rpcl \
  --backend prefer-device \
  --source-device-decode \
  --json \
  --level 0
trash "$OUT"
```

Record wall time, `gpu_encode_hardware_micros`, `gpu_encode_wall_micros`, `input_decode_micros`, `write_micros`, resident frame count, and any Tier-1 dispatch counters.

- [ ] **Step 3: Confirm the target**

Proceed only if `gpu_encode_hardware_micros` is a meaningful fraction of wall time. If input decode or write dominates, run the JPEG-private plan first.

### Task 2: Add Scalar-vs-Prototype Test Harness

**Files:**
- Modify: `/Users/user/Bench/signinum/crates/signinum-j2k-metal/src/encode_bitstream.metal`
- Modify: `/Users/user/Bench/signinum/crates/signinum-j2k-metal/src/compute.rs`
- Modify: `/Users/user/Bench/signinum/crates/signinum-j2k-metal/src/encode.rs`

- [ ] **Step 1: Add a new prototype pipeline state**

Add a new runtime pipeline entry named `ht_encode_code_blocks_simd_prototype`. It should compile a new kernel without replacing `ht_encode_code_blocks`.

- [ ] **Step 2: Add a constrained test fixture**

Add a test that builds one deterministic 64x64 HT code-block job with fixed coefficients and runs both:

```rust
let scalar = encode_one_ht_block_with_kernel("j2k_encode_ht_code_blocks", &job)?;
let simd = encode_one_ht_block_with_kernel("j2k_encode_ht_code_blocks_simd_prototype", &job)?;
assert_eq!(simd.status, scalar.status);
assert_eq!(simd.bytes, scalar.bytes);
```

- [ ] **Step 3: Verify the test fails before the prototype kernel exists**

Run:

```bash
cargo test -p signinum-j2k-metal ht_simd_prototype_matches_scalar_for_64x64_block -- --nocapture
```

Expected before implementation: failure because the prototype pipeline/kernel is missing.

### Task 3: Prototype SIMD Max-Magnitude Reduction

**Files:**
- Modify: `/Users/user/Bench/signinum/crates/signinum-j2k-metal/src/encode_bitstream.metal`

- [ ] **Step 1: Add the prototype kernel**

Add `j2k_encode_ht_code_blocks_simd_prototype` beside the current `j2k_encode_ht_code_blocks`. For the first version:

```metal
kernel void j2k_encode_ht_code_blocks_simd_prototype(
    device const int *coefficients [[buffer(0)]],
    device uchar *out [[buffer(1)]],
    device const J2kHtEncodeBatchJob *jobs [[buffer(2)]],
    device const ushort *vlc_table0 [[buffer(3)]],
    device const ushort *vlc_table1 [[buffer(4)]],
    device const uchar *uvlc_table [[buffer(5)]],
    device J2kHtEncodeStatus *statuses [[buffer(6)]],
    constant uint &job_count [[buffer(7)]],
    uint tid [[thread_index_in_threadgroup]],
    uint tg [[threadgroup_position_in_grid]]
) {
    if (tg >= job_count) {
        return;
    }
    const J2kHtEncodeBatchJob job = jobs[tg];
    device const int *block = coefficients + job.coefficient_offset;
    uint local_max = 0u;
    for (uint idx = tid; idx < job.width * job.height; idx += 32u) {
        local_max = max(local_max, j2k_classic_magnitude(block[idx]));
    }
    uint block_max = simd_max(local_max);
    if (tid == 0u) {
        J2kHtEncodeParams params;
        params.width = job.width;
        params.height = job.height;
        params.total_bitplanes = job.total_bitplanes;
        params.output_capacity = job.output_capacity;
        j2k_encode_ht_code_block_impl(
            block,
            out + job.output_offset,
            params,
            vlc_table0,
            vlc_table1,
            uvlc_table,
            statuses + tg
        );
    }
}
```

This first step intentionally computes `block_max` but still delegates encoding to the scalar implementation on lane 0. The purpose is to validate dispatch shape, threadgroup size, and parity before moving byte placement.

- [ ] **Step 2: Dispatch one 32-lane threadgroup per code block**

In Rust, dispatch the prototype with grid width equal to `job_count` threadgroups and threadgroup width `32`.

- [ ] **Step 3: Verify parity**

Run:

```bash
cargo test -p signinum-j2k-metal ht_simd_prototype_matches_scalar_for_64x64_block -- --nocapture
```

Expected: pass.

### Task 4: Move Length Estimation into SIMD Lanes

**Files:**
- Modify: `/Users/user/Bench/signinum/crates/signinum-j2k-metal/src/encode_bitstream.metal`

- [ ] **Step 1: Add per-lane length counters**

Compute per-lane estimated byte counts for the HT cleanup/MEL/VLC/MS regions without writing final bytes. Use SIMD reductions/prefix sums to produce block-level segment lengths.

- [ ] **Step 2: Keep scalar byte writer active**

Do not replace byte output yet. Store estimated lengths in a debug/status-only buffer and assert they match scalar-produced lengths in tests.

- [ ] **Step 3: Verify**

Run:

```bash
cargo test -p signinum-j2k-metal ht_simd_prototype_length_estimate_matches_scalar -- --nocapture
cargo test -p signinum-j2k-metal metal_ht -- --nocapture
```

Expected: prototype length estimates match scalar output lengths for constrained fixtures.

### Task 5: Move Prefix Placement for One Constrained HT Path

**Files:**
- Modify: `/Users/user/Bench/signinum/crates/signinum-j2k-metal/src/encode_bitstream.metal`

- [ ] **Step 1: Choose one constrained path**

Limit the first byte-writing path to one supported case:

```text
64x64, 8-bit or 16-bit lossless, no exotic block style, HT cleanup only, one coding pass
```

- [ ] **Step 2: Add prefix placement**

Use SIMD prefix sums to assign byte offsets for lane-produced output fragments. Write directly to the existing per-code-block output slice.

- [ ] **Step 3: Fall back outside the constrained path**

For unsupported block shapes, call the scalar implementation from lane 0 so correctness is unchanged.

- [ ] **Step 4: Verify bit-exactness**

Run:

```bash
cargo test -p signinum-j2k-metal ht_simd_prototype_matches_scalar_for_64x64_block -- --nocapture
cargo test -p signinum-j2k-metal
cargo clippy -p signinum-j2k-metal --all-targets -- -D warnings
```

Expected: exact byte parity for supported constrained blocks; scalar fallback parity for unsupported blocks.

### Task 6: Benchmark Prototype Routing

**Files:**
- Modify: `/Users/user/Bench/signinum/crates/signinum-j2k-metal/src/compute.rs`
- Modify: `/Users/user/Bench/signinum/crates/signinum-j2k-metal/src/encode.rs`

- [ ] **Step 1: Add internal opt-in routing**

Gate prototype use behind an environment variable:

```rust
const SIGNINUM_J2K_METAL_HT_SIMD_PROTOTYPE: &str = "SIGNINUM_J2K_METAL_HT_SIMD_PROTOTYPE";
```

Use the prototype kernel only when the env var is enabled. Default remains the existing scalar-compatible kernel.

- [ ] **Step 2: Benchmark with prototype off and on**

Run baseline:

```bash
OUT="/tmp/wsi-dicom-htj2k-simd-off"
/usr/bin/time -l target/release/wsi-dicom convert bench/testdata/CMU-1.tiff \
  --out "$OUT" \
  --transfer-syntax htj2k-lossless-rpcl \
  --backend prefer-device \
  --source-device-decode \
  --json \
  --level 0
trash "$OUT"
```

Run prototype:

```bash
OUT="/tmp/wsi-dicom-htj2k-simd-on"
SIGNINUM_J2K_METAL_HT_SIMD_PROTOTYPE=1 \
/usr/bin/time -l target/release/wsi-dicom convert bench/testdata/CMU-1.tiff \
  --out "$OUT" \
  --transfer-syntax htj2k-lossless-rpcl \
  --backend prefer-device \
  --source-device-decode \
  --json \
  --level 0
trash "$OUT"
```

Compare wall time and `gpu_encode_hardware_micros`.

- [ ] **Step 3: Decision gate**

If the prototype does not reduce `gpu_encode_hardware_micros`, keep it disabled and move to packetization profiling. If it reduces hardware time, expand supported block shapes and remove the env gate after full parity coverage.

