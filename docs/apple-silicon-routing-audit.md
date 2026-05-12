<!-- SPDX-License-Identifier: Apache-2.0 -->

# Apple Silicon WSI DICOM Routing Audit

Date: 2026-05-05

## Objective

Make real WSI-to-DICOM conversion faster on Apple Silicon by routing
passthrough first, using GPU transcode only where measurements show it pays, and
validating generated DICOM/codec output against external tools.

## Current Routing Policy

- JPEG Baseline output tries compressed-frame passthrough first and uses native
  regular source tile geometry when that preserves legal source frames.
- J2K/HTJ2K output tries compressed J2K-family passthrough first when the source
  codestream, transfer syntax, geometry, color profile, and bit depth are
  compatible.
- `auto` uses CPU source-tile input plus CPU J2K/HTJ2K encode as the
  conservative J2K-family transcode baseline unless a measured route probe
  selects GPU work. Runtime roundtrip codec validation is explicit via
  `codec_validation=RoundTrip` / `--codec-validation round-trip`; production
  conversion defaults to `codec_validation=Disabled`.
- When HTJ2K RPCL output is requested, compressed source device decode is
  explicitly enabled, and the report scope has at least 16 routed frames, `auto`
  probes up to the first four eligible non-passthrough frames through CPU-only,
  CPU-input/device-encode, and resident Metal-input/device-encode routes. It
  keeps a GPU route for the rest of the instance only when the measured route is
  at least 8% faster, or when CPU encode fails and the GPU route succeeds.
- For smaller scopes, unsupported transfer syntaxes, or unmeasured Auto routes,
  the J2K/HTJ2K encoder is demoted to CPU. Explicit `prefer-device` and
  `require-device` remain caller overrides.
- Probe cost and decision data are surfaced as `auto_probe_*` metrics in CLI and
  JSON reports.
- `gpu_dispatch_ms` is reported as aggregate CPU-observed duration from
  GPU-dispatched stages, including command-buffer submission and wait overhead.
- `gpu_encode_hardware_ms` is reported when Metal exposes completed command
  buffer timings for the resident J2K/HTJ2K encode path. It is summed GPU
  execution duration across command buffers and frames, not wall-clock elapsed
  time. `gpu_encode_dispatch_overhead_ms` is the sum of per-frame positive
  remainders of CPU-observed resident encode dispatch time after subtracting
  that frame's hardware duration, so it can be nonzero even when summed GPU
  execution is larger than aggregate elapsed encode time.
- Auto probe decisions are cached in-process by source path, level, tile size,
  transfer syntax, and routed frame count. Bounded coverage probes therefore do
  not seed decisions for full-level conversions, while sustained conversions in
  one process reuse the measured decision after the first iteration.
- Set `WSI_DICOM_AUTO_ROUTE_CACHE=/path/to/cache.json` to persist measured
  route decisions across separate CLI invocations. Cache file errors are
  surfaced as normal I/O or JSON errors instead of being silently ignored.
- Non-JSON `coverage` runs emit per-level start/success progress on stderr.
  Non-JSON `coverage-corpus` runs also emit per-source start, success, failure,
  elapsed time, and route-count progress before the final stdout summary. JSON
  mode keeps stdout machine-readable and suppresses these progress lines.
- `coverage`, `coverage-corpus`, and coverage-mode `sustain` accept
  `--max-level-ms` to cap profiling work per physical level at row/batch
  boundaries. This is a cooperative profiling guard, not thread cancellation;
  an in-flight codec call can still run until it returns.
- Source-generated synthetic downsample levels are skipped for export and route
  coverage. Explicit `profile` on a synthetic level fails fast with an
  unsupported-level error. This keeps WSI-to-DICOM routing focused on
  source-backed frames instead of materializing huge virtual NDPI levels before
  route selection.
- Statumen now has an ROI-aware synthetic NDPI region path for callers that
  explicitly read generated downsample levels. Non-full synthetic ROI reads map
  the request back to the base level, downsample only that source ROI, preserve
  Signinum scaled JPEG semantics for `NdpiFullDecode` bases, zero-fill
  out-of-bounds request areas, and do not populate the full synthetic-level
  caches whose keys omit ROI coordinates.
- `DicomExportOptions::source_device_decode` and the matching CLI
  `--source-device-decode` flag explicitly request Statumen compressed JPEG/JP2K
  device decode without requiring shell env setup. The older
  `STATUMEN_JPEG_DEVICE_DECODE=1` and `STATUMEN_JP2K_DEVICE_DECODE=1` env
  toggles still work. With JP2K device decode enabled, Statumen uses JP2K
  device batch decode by default for compatible RGB tiles and falls back to
  per-tile device decode when batch decode is unsupported. Set
  `STATUMEN_JP2K_DEVICE_BATCH=0` only to force the older per-tile device decode
  behavior during troubleshooting.
- `prefer-device` and `require-device` remain the explicit resident GPU routes.

## Corpus Coverage

Corpus:
local OpenSlide testdata corpus, configured outside the repository.

Bounded HTJ2K RPCL baseline, default non-Metal build, backend `auto`, tile size
512, `max_frames_per_level=4`, `max_levels=3`, `max_level_ms=30000`:

```text
sources_considered=23
sources_profiled=21
failures=2
failure=Leica/Leica-2.scn level 0 JPEG decode unexpected EOI at MCU 608/1024
failure=Leica/Leica-3.scn level 1 JPEG decode unexpected EOI at MCU 928/1024
frames total=229
available_frames=559217
sampled_frames_pct=0.0410
route_cpu_fallback=229 / 100.0%
route_gpu_transcode=0
route_passthrough=0
cpu_input=229
gpu_input_decode=0
gpu_encode=0
rgb_like_frames=229
bits8_frames=229
input_decode_ms=14706.180
compose_ms=911.068
encode_ms=80887.740
elapsed_ms=100163.740
```

JPEG Baseline, backend `auto`, tile size 256, one frame per source:

```text
sources_considered=23
sources_profiled=23
failures=0
frames total=23
available_frames=2994845
sampled_frames_pct=0.0008
complete_frame_coverage=false
route_passthrough=11 / 47.8%
route_cpu_fallback=12 / 52.2%
route_gpu_transcode=0
gpu_input_decode=0
rgb_like_frames=11
unknown_pixel_profile_frames=12
bits8_frames=11
elapsed_ms=419.654
rss_mb=37.5
```

Latest first-level JPEG Baseline corpus slice with per-level progress enabled.
JPEG Baseline output, backend `auto`, tile size 256,
`max_frames_per_level=16`, `max_levels=1`:

```text
sources_considered=23
sources_profiled=23
failures=0
frames total=368
available_frames=2994845
sampled_frames_pct=0.0123
route_passthrough=176 / 47.8%
jpeg_passthrough=176
route_cpu_fallback=192 / 52.2%
jpeg_decode_fallback=192
route_gpu_transcode=0
gpu_input_decode=0
gpu_encode=0
route_unclassified=0
elapsed_ms=430.242
rss_mb=42.2
```

Refreshed first-level JPEG Baseline corpus slice on the current workspace.
JPEG Baseline output, backend `auto`, tile size 256,
`max_frames_per_level=16`, `max_levels=1`:

```text
sources_considered=23
sources_profiled=23
failures=0
frames total=368
available_frames=2338259
sampled_frames_pct=0.0157
route_passthrough=224 / 60.9%
jpeg_passthrough=224
route_cpu_fallback=144 / 39.1%
jpeg_decode_fallback=144
route_gpu_transcode=0
gpu_input_decode=0
gpu_encode=0
route_unclassified=0
elapsed_ms=452.207
```

After native regular JPEG passthrough geometry selection, the same first-level
corpus slice with the default CLI tile size preserves source JPEG tiles without
requiring callers to pass `--tile-size 256`:

```text
sources_considered=23
sources_profiled=23
failures=0
frames total=368
route_passthrough=304 / 82.6%
jpeg_passthrough=304
route_cpu_fallback=64 / 17.4%
jpeg_decode_fallback=64
route_gpu_transcode=0
gpu_input_decode=0
gpu_encode=0
rgb_like_frames=288
gray_frames=16
unknown_pixel_profile_frames=64
elapsed_ms=446.530
```

General JPEG 2000 preservation is now exposed as CLI `jpeg2000`
(`1.2.840.10008.1.2.4.91`) and is intentionally passthrough-only. This covers
compatible lossy or lossless source J2K codestreams without decode/encode work.
For this passthrough-only target, wsi-dicom uses native square source tile
geometry automatically when the level exposes one, so callers do not need to
guess the tile size. Incompatible geometry fails or profiles as CPU fallback
instead of invoking GPU or CPU transcode. Real Aperio `JP2K-33003-1.svs`, level
0, backend `require-device`, default CLI tile size, `max_frames=16`:

```text
available_frames=4209
sampled_frames_pct=0.3801
route_passthrough=16 / 100.0%
j2k_passthrough=16
route_gpu_transcode=0
route_cpu_fallback=0
gpu_input_decode=0
gpu_encode=0
input_decode_ms=0.000
compose_ms=0.000
encode_ms=0.000
elapsed_ms=0.917
```

If native square source geometry is unavailable and the requested frame geometry
does not match, the route profiles as CPU fallback classification with no input
decode, no encode, no GPU batches, and no route probe. A full convert of the
same source fails at `row=0 col=60` with the explicit passthrough-only geometry
error because that edge frame is not eligible for byte-preserving
compressed-frame copy. This is expected for the passthrough route: source frame
geometry must match DICOM frame geometry.

Bounded first-level corpus slice for the new general JPEG 2000 target, backend
`auto`, default CLI tile size with native source tile override,
`max_frames_per_level=16`, `max_levels=1`:

```text
sources_considered=23
sources_profiled=23
failures=0
frames total=368
available_frames=585763
sampled_frames_pct=0.0628
route_passthrough=48 / 13.0%
j2k_passthrough=48
route_cpu_fallback=320 / 87.0%
route_gpu_transcode=0
gpu_input_decode=0
gpu_encode=0
route_unclassified=0
elapsed_ms=426.348
```

HTJ2K RPCL, backend `auto`, tile size 512, one frame per source, device decode
env enabled, with measured auto probe and a cold persistent route cache:

```text
sources_considered=23
sources_profiled=23
failures=0
frames total=23
available_frames=447931
sampled_frames_pct=0.0051
complete_frame_coverage=false
route_gpu_transcode=23 / 100.0%
route_resident_gpu_transcode=18
route_partial_gpu_transcode=5
cpu_input=5
gpu_input_decode=18
gpu_encode=23
gpu_validation=23
rgb_like_frames=23
unknown_pixel_profile_frames=0
bits8_frames=23
auto_probe_frames=23
auto_probe_selected_gpu_input=18
auto_probe_cpu_ms=9314.510
auto_probe_gpu_ms=4173.686
elapsed_ms=14455.631
rss_mb=145.4
```

Latest bounded Aperio slice after padded-edge passthrough and DICOM spacing
re-ingest fixes. HTJ2K RPCL, backend `auto`, tile size 512,
`STATUMEN_JPEG_DEVICE_DECODE=1`, `STATUMEN_JP2K_DEVICE_DECODE=1`,
`max_frames_per_level=4`, `max_levels=1`:

```text
sources_considered=7
sources_profiled=7
failures=0
frames total=28
available_frames=39364
sampled_frames_pct=0.0711
route_gpu_transcode=28 / 100.0%
route_resident_gpu_transcode=20
route_partial_gpu_transcode=8
cpu_input=8
gpu_input_decode=20
gpu_encode=28
route_cpu_fallback=0
route_unclassified=0
auto_probe_frames=28
auto_probe_selected_gpu_input=20
auto_probe_cpu_ms=12302.297
auto_probe_gpu_ms=4989.750
elapsed_ms=17581.490
rss_mb=105.3
```

Latest first-level full corpus slice with per-level progress enabled and the
old `STATUMEN_*_DEVICE_DECODE` env toggles explicitly unset. HTJ2K RPCL,
backend `auto`, tile size 512, `--source-device-decode`,
`max_frames_per_level=16`, `max_levels=1`:

```text
sources_considered=23
sources_profiled=22
failures=1
failure=Leica/Leica-2.scn JPEG decode unexpected EOI at MCU 608/1024
frames total=352
available_frames=444004
sampled_frames_pct=0.0793
route_gpu_transcode=288 / 81.8%
route_resident_gpu_transcode=288
route_partial_gpu_transcode=0
cpu_input=64
gpu_input_decode=288
gpu_encode=288
route_cpu_fallback=64
route_unclassified=0
auto_probe_frames=88
auto_probe_selected_gpu_input=72
auto_probe_cpu_ms=36979.581
auto_probe_gpu_ms=5799.134
gpu_input_batches=39
gpu_compose_batches=29
gpu_encode_batches=39
gpu_dispatch_ms=4654.112
gpu_encode_hardware_ms=7354.882
gpu_encode_dispatch_overhead_ms=213.361
input_decode_ms=9258.576
compose_ms=402.647
encode_ms=25213.421
final_byte_write_ms=0.342
elapsed_ms=69598.916
```

The same slice now deliberately avoids partial GPU transcode for RGB-like J2K
routes: Auto either selects resident GPU decode/encode or CPU fallback. Explicit
`prefer-device` and `require-device` still allow device encode overrides.

Persistent route cache smoke on `CMU-1-JP2K-33005.svs` confirms the second run
reuses the resident-GPU decision: `auto_route_probe_frames=0` and elapsed drops
from about 4.93s to 1.34s for the same 16-frame slice. The matching cached
CPU-fallback smoke on `JP2K-33003-1.svs` confirms the second run keeps
`partial_gpu_transcode_frames=0`, keeps `cpu_fallback_frames=16`, and drops from
about 12.37s to 10.12s.

Three-iteration cached coverage sustain on `CMU-1-JP2K-33005.svs` stayed
resident GPU for all 16 sampled frames per iteration. Iteration 1 paid the probe
and took about 4.95s. Iterations 2 and 3 reused the cache with
`auto_route_probe_frames=0` and took about 1.24s and 1.31s. macOS reported no
thermal or performance warning; memory pressure stayed at 91% free.

Two-iteration cached `sustain-convert` on `CMU-1-Small-Region.svs` wrote a
30-frame HTJ2K RPCL DICOM instance. Iteration 1 paid the route probe and took
about 2.08s. Iteration 2 reused the cache with `auto_route_probe_frames=0`, kept
`resident_gpu_transcode_frames=30`, kept `partial_gpu_transcode_frames=0`, and
took about 0.515s. The iteration-2 instance validates with `dciodvfy -new`
status 0, warnings only, and `dcentvfy` status 0.

Latest two-level physical-source corpus slice with per-level progress enabled.
HTJ2K RPCL, backend `auto`, tile size 512,
`STATUMEN_JPEG_DEVICE_DECODE=1`, `STATUMEN_JP2K_DEVICE_DECODE=1`,
`max_frames_per_level=16`, `max_levels=2`:

```text
sources_considered=23
sources_profiled=21
failures=2
failure=Leica/Leica-2.scn level 0 JPEG decode unexpected EOI at MCU 608/1024
failure=Leica/Leica-3.scn level 1 JPEG decode unexpected EOI at MCU 928/1024
frames total=604
available_frames=534039
sampled_frames_pct=0.1131
route_gpu_transcode=604 / 100.0%
route_resident_gpu_transcode=512
route_partial_gpu_transcode=92
cpu_input=92
gpu_input_decode=512
gpu_encode=604
route_cpu_fallback=0
route_unclassified=0
auto_probe_frames=151
auto_probe_selected_gpu_input=128
auto_probe_cpu_ms=52708.673
auto_probe_gpu_ms=10453.303
elapsed_ms=111745.641
rss_mb=220.8
```

Focused Aperio JP2K resident-routing smoke. HTJ2K RPCL, Metal feature build,
backend `auto`, tile size 512, `--source-device-decode`,
`max_frames_per_level=16`, `max_levels=1`:

```text
source=CMU-1-JP2K-33005.svs
level=0
requested_frames=16
available_frames=5850
route_gpu_transcode=16 / 100.0%
route_resident_gpu_transcode=16
route_partial_gpu_transcode=0
cpu_input=0
gpu_input_decode=16
gpu_encode=16
gpu_input_batches=2
gpu_compose_batches=2
gpu_encode_batches=2
auto_probe_frames=4
auto_probe_selected_gpu_input=4
auto_probe_cpu_ms=3277.152
auto_probe_gpu_ms=416.158
input_decode_ms=1204.270
compose_ms=25.550
encode_ms=136.123
gpu_dispatch_ms=1365.943
gpu_encode_hardware_ms=336.337
gpu_encode_dispatch_overhead_ms=11.675
elapsed_ms=4644.685
```

The same source with Metal enabled but without explicit JP2K device decode used
partial GPU transcode: 4/4 frames had CPU input decode and GPU HTJ2K encode,
with `resident_gpu_transcode_frames=0`, `partial_gpu_transcode_frames=4`, and
`elapsed_ms=3391.409`. With `STATUMEN_JP2K_DEVICE_DECODE=1 --backend
prefer-device`, 4/4 frames were resident GPU transcode and elapsed was
`413.801` ms.

The previous `Hamamatsu/CMU-1.ndpi` level-1 stall is resolved for
WSI-to-DICOM route coverage: that level is identified as a synthetic downsample
and skipped. A direct `profile` of that level exits in about 0.37s with
`route profiling skips synthetic downsample level 1; profile a physical source
level instead`. The matching `coverage --max-levels 2 --max-frames-per-level 16`
run profiles physical level 0 in about 1.49s and then skips level 1.
Statumen's explicit synthetic ROI path is covered by unit tests for center
ROIs, negative-origin zero fill, odd ceil edges, cropped synthetic dimensions,
factor-4 repeated-box alignment, and no full synthetic cache population.
Manual real-fixture probe on `Hamamatsu/CMU-1.ndpi` confirms level 1 is
`SyntheticDownsample` (`25600x19072`) and a centered `1024x1024` ROI read on
that level completes without full-level materialization:
`n=30 p50_us=151119 p95_us=153235 max_us=293941 mean_us=155968`. This is a
Statumen explicit-read result; wsi-dicom export/coverage still skips synthetic
levels by policy.

Matching routine H&E JPEG passthrough shape. Aperio `CMU-1.svs`, JPEG Baseline
output, backend `auto`, tile size 256, level 1, `max_frames=16`:

```text
available_frames=1485
sampled_frames_pct=1.0774
frames total=16
route_passthrough=16 / 100.0%
jpeg_passthrough=16
route_gpu_transcode=0
route_cpu_fallback=0
gpu_input_decode=0
gpu_encode=0
elapsed_ms=0.376
rss_mb=13.7
```

These corpus probes intentionally sample one frame from each source. The
`available_frames` denominator is now surfaced so route percentages over the
sample are not confused with whole-corpus frame coverage. JSON coverage reports
carry the same denominator at level, source, and corpus scope. The
`complete_frame_coverage` flag stays false for bounded probes and should be true
before treating route fractions as full report-scope coverage.
Use `--full-frame-coverage` for `coverage`, `coverage-corpus`, or `sustain`
when the run should exhaust all frames in the reported scope instead of sampling
up to `--max-frames-per-level`.
`sustain` and `sustain-convert` report process RSS, macOS memory pressure when
available, and thermal state on each iteration.

## Complete Small-Scope Route Coverage

These runs use the production default `codec_validation=Disabled` and exhaust
every frame in the reported scope, so
`complete_frame_coverage=true` or `sampled_frames_pct=100.0000` can be used as
real route-fraction evidence for that scope.

Small Aperio JPEG Baseline slide to JPEG Baseline DICOM, backend `auto`, tile
size 240:

```text
source=CMU-1-Small-Region.svs
available_frames=130
sampled_frames_pct=100.0000
complete_frame_coverage=true
route_passthrough=130 / 100.0%
jpeg_passthrough=130
route_gpu_transcode=0
route_cpu_fallback=0
gpu_input_decode=0
gpu_encode=0
elapsed_ms=0.694
rss_mb=13.7
```

The same small Aperio JPEG Baseline slide to HTJ2K RPCL DICOM, backend `auto`,
tile size 512, with `STATUMEN_JPEG_DEVICE_DECODE=1`:

```text
source=CMU-1-Small-Region.svs
available_frames=30
sampled_frames_pct=100.0000
complete_frame_coverage=true
route_gpu_transcode=30 / 100.0%
resident_gpu_transcode_frames=30
cpu_fallback_frames=0
gpu_input_decode_frames=30
gpu_encode_frames=30
gpu_validation_frames=0
gpu_input_batches=7
gpu_compose_batches=7
gpu_encode_batches=7
gpu_dispatch_ms=530.753
gpu_encode_hardware_ms=1038.833
gpu_encode_dispatch_overhead_ms=19.229
auto_probe_frames=4
auto_probe_cpu_ms=1201.583
auto_probe_gpu_ms=137.808
final_byte_ms=0.030
input_decode_ms=77.592
compose_ms=25.488
encode_ms=427.673
validation_ms=0.000
elapsed_ms=1736.984
rss_mb=81.0
```

Aperio JP2K level 2 to HTJ2K RPCL DICOM, backend `auto`, tile size 512, with
`STATUMEN_JP2K_DEVICE_DECODE=1`:

```text
source=CMU-1-JP2K-33005.svs
level=2
requested_frames=30
available_frames=30
sampled_frames_pct=100.0000
route_gpu_transcode=30 / 100.0%
resident_gpu_transcode_frames=30
cpu_fallback_frames=0
gpu_input_decode_frames=30
gpu_encode_frames=30
gpu_validation_frames=0
gpu_input_batches=5
gpu_compose_batches=6
gpu_encode_batches=6
gpu_dispatch_ms=1678.169
gpu_encode_hardware_ms=650.866
gpu_encode_dispatch_overhead_ms=15.214
auto_probe_frames=4
auto_probe_cpu_ms=3553.441
auto_probe_gpu_ms=474.542
final_byte_ms=0.030
input_decode_ms=1369.384
compose_ms=32.337
encode_ms=276.448
validation_ms=0.000
elapsed_ms=5234.052
rss_mb=135.0
```

## Sustained Measurements

Real RGB H&E Aperio JPEG slide to JPEG Baseline DICOM, backend `auto`, tile size
256, level 1. This is the routine clinical-pathology routing case: compressed
JPEG passthrough, no decode, no GPU work, and byte-preserving encapsulation.

```text
source=CMU-1.svs
level=1
iterations=3
frames=1485
frames_per_sec=11217.28, 14498.98, 14598.04
route_passthrough=1485 / 100.0%
jpeg_passthrough=1485
route_gpu_transcode=0
route_cpu_fallback=0
gpu_input_decode=0
gpu_encode=0
gpu_validation=0
final_byte_ms=37.665, 37.190, 36.506
input_decode_ms=0.000, 0.000, 0.000
compose_ms=0.000, 0.000, 0.000
encode_ms=0.000, 0.000, 0.000
validation_ms=0.000, 0.000, 0.000
elapsed_ms=132.385, 102.421, 101.726
rss_mb=33.7, 33.8, 33.9
thermal=no warning recorded
memory_pressure=91%, 92%, 92%
dicom_validation=iteration_0003_dciodvfy_exit_0_warnings_only,dcentvfy_exit_0
```

Small full-output Aperio region slide to HTJ2K RPCL, backend `auto`, tile size
512, production default `codec_validation=Disabled`, with
`STATUMEN_JPEG_DEVICE_DECODE=1`:

```text
source=CMU-1-Small-Region.svs
iterations=3
frames=30
frames_per_sec=17.41, 60.24, 54.61
resident_gpu_transcode_frames=30, 30, 30
cpu_fallback_frames=0, 0, 0
gpu_input_decode_frames=30, 30, 30
gpu_encode_frames=30, 30, 30
gpu_validation_frames=0, 0, 0
gpu_input_batches=7, 6, 6
gpu_compose_batches=7, 6, 6
gpu_encode_batches=7, 6, 6
gpu_dispatch_ms=527.027, 444.383, 497.386
gpu_encode_hardware_ms=1024.984, 994.211, 996.106
gpu_encode_dispatch_overhead_ms=20.492, 17.116, 17.422
auto_probe_frames=4, 0, 0
auto_probe_cpu_ms=1119.897, 0.000, 0.000
auto_probe_gpu_ms=99.291, 0.000, 0.000
final_byte_ms=6.405, 6.341, 6.418
input_decode_ms=66.774, 37.609, 31.395
compose_ms=25.441, 20.776, 21.808
encode_ms=434.812, 385.998, 444.183
validation_ms=0.000, 0.000, 0.000
elapsed_ms=1722.792, 497.988, 549.318
rss_mb=80.2, 81.7, 83.0
thermal=no warning recorded
memory_pressure=91%, 91%, 91%
dicom_validation=iteration_0003_dciodvfy_exit_0_warnings_only,dcentvfy_exit_0
```

Aperio JP2K level 2 to HTJ2K RPCL, backend `auto`, tile size 512, production
default `codec_validation=Disabled`, with `STATUMEN_JP2K_DEVICE_DECODE=1`:

```text
source=CMU-1-JP2K-33005.svs
level=2
iterations=3
frames=30
frames_per_sec=5.68, 19.50, 19.48
resident_gpu_transcode_frames=30, 30, 30
cpu_fallback_frames=0, 0, 0
gpu_input_decode_frames=30, 30, 30
gpu_encode_frames=30, 30, 30
gpu_validation_frames=0, 0, 0
gpu_input_batches=5, 4, 4
gpu_compose_batches=6, 5, 5
gpu_encode_batches=6, 5, 5
gpu_dispatch_ms=1717.996, 1509.374, 1511.172
gpu_encode_hardware_ms=711.536, 643.036, 668.927
gpu_encode_dispatch_overhead_ms=22.480, 16.986, 19.167
auto_probe_frames=4, 0, 0
auto_probe_cpu_ms=3512.240, 0.000, 0.000
auto_probe_gpu_ms=483.901, 0.000, 0.000
final_byte_ms=4.365, 4.081, 4.027
input_decode_ms=1377.713, 1233.524, 1222.544
compose_ms=37.172, 30.171, 34.203
encode_ms=303.111, 245.679, 254.425
validation_ms=0.000, 0.000, 0.000
elapsed_ms=5283.346, 1538.113, 1539.858
rss_mb=133.9, 148.2, 148.3
thermal=no warning recorded
memory_pressure=91%, 91%, 91%
dicom_validation=iteration_0003_dciodvfy_exit_0_warnings_only,dcentvfy_exit_0
```

The older sustained measurements below were taken with runtime roundtrip codec
validation enabled or before `codec_validation=Disabled` became the default, so
they intentionally include `gpu_validation_frames` and `validation_ms`.

Small full-output Aperio region slide to HTJ2K RPCL, backend `auto`, tile size
512:

```text
source=CMU-1-Small-Region.svs
iterations=2
frames=30
elapsed=3.566s, 2.091s
frames_per_sec=8.41, 14.35
cpu_input_frames=0
gpu_input_decode_frames=30
gpu_encode_frames=30
gpu_validation_frames=30
resident_gpu_transcode_frames=30
cpu_fallback_frames=0
gpu_input_batches=7, 6
gpu_compose_batches=7, 6
gpu_encode_batches=7, 6
gpu_dispatch_ms=1973.845, 1925.700
auto_probe_frames=4, 0
auto_probe_selected_gpu_input=4, 0
auto_probe_cpu_ms=1347.322, 0.000
auto_probe_gpu_ms=281.304, 0.000
thermal=no warning recorded
memory_pressure=91%, 91%
rss=102.9MB -> 106.1MB
```

Latest single-iteration hardware timing smoke after resident encode timing was
split into CPU-observed dispatch and summed Metal command-buffer execution:

```text
source=CMU-1-Small-Region.svs
iterations=1
frames=30
frames_per_sec=8.34
resident_gpu_transcode_frames=30
gpu_input_batches=7
gpu_compose_batches=7
gpu_encode_batches=7
gpu_dispatch_ms=2189.171
gpu_encode_hardware_ms=1025.331
gpu_encode_dispatch_overhead_ms=80.551
auto_probe_frames=4
auto_probe_cpu_ms=1331.782
auto_probe_gpu_ms=330.792
final_byte_ms=5.393
input_decode_ms=66.394
compose_ms=26.293
encode_ms=461.366
validation_ms=1635.118
elapsed_ms=3597.414
rss_mb=103.1
memory_pressure=91%
dicom_validation=dciodvfy_exit_0_warnings_only,dcentvfy_exit_0
```

The same small-region profile across separate CLI invocations with
`WSI_DICOM_AUTO_ROUTE_CACHE=target/auto-route-cache-smoke.json` measured:

```text
first_invocation_elapsed=3.513s
first_invocation_auto_probe_frames=4
second_invocation_elapsed=2.124s
second_invocation_auto_probe_frames=0
cache_entry_use_gpu_input=true
```

Older Aperio JP2K level 2 to HTJ2K RPCL run, backend `auto`, tile size 512,
measured before Statumen's JP2K batch device decode became the default attempt:

```text
source=CMU-1-JP2K-33005.svs
iterations=2
frames=30
elapsed=25.300s, 19.282s
cpu_input_frames=30
gpu_input_decode_frames=0
gpu_encode_frames=30
gpu_validation_frames=30
partial_gpu_transcode_frames=30
cpu_fallback_frames=0
auto_probe_frames=4, 0
auto_probe_selected_gpu_input=0, 0
auto_probe_cpu_ms=3761.596, 0.000
auto_probe_gpu_ms=6210.115, 0.000
```

Same Aperio JP2K level 2 with explicit resident GPU input decode, measured
before the auto-probe change:

```text
backend=prefer-device
frames=30
gpu_input_decode_frames=30
resident_gpu_transcode_frames=30
elapsed=22.623s
```

This older run is retained as a regression comparison for the current JP2K
batch decode path. The current complete level-2 profile above now shows
resident GPU input selected by `auto` for the same report scope.

Forced GPU JP2K input profile after Statumen changed JP2K device batch decode
to the default attempt once `STATUMEN_JP2K_DEVICE_DECODE=1` is set. This run did
not set `STATUMEN_JP2K_DEVICE_BATCH`; compatible RGB tiles now attempt the
Statumen JP2K batch path before per-tile device fallback.

```text
source=CMU-1-JP2K-33005.svs
level=2
requested_frames=4
available_frames=30
backend=require-device
transfer_syntax=HTJ2K RPCL
route_gpu_transcode=4
resident_gpu_transcode_frames=4
cpu_fallback_frames=0
gpu_input_decode_frames=4
gpu_encode_frames=4
gpu_validation_frames=4
gpu_input_batches=1
gpu_compose_batches=1
gpu_encode_batches=1
gpu_dispatch_ms=687.664
gpu_encode_hardware_ms=105.883
gpu_encode_dispatch_overhead_ms=4.963
input_decode_ms=396.080
compose_ms=8.291
encode_ms=62.619
validation_ms=220.674
elapsed_ms=688.869
rss_mb=77.6
```

## External Validation

Installed/staged local validation tools:

- `grk_decompress` from Homebrew `grokj2k`
- `dciodvfy` and `dcentvfy` from the official dicom3tools macOS snapshot staged
  under `target/dicom3tools-mac`
- `opj_decompress`
- `djpeg`

Passing gates:

```text
cargo test --manifest-path Cargo.toml -- --nocapture
cargo test --manifest-path Cargo.toml --features metal -- --nocapture
cargo clippy --manifest-path Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path Cargo.toml --features metal --all-targets -- -D warnings
```

Generated real DICOM outputs from the sustained HTJ2K and JPEG runs validate
with:

```text
dciodvfy -new: status 0
dcentvfy: status 0
```

Latest dispatch-instrumented HTJ2K RPCL sustained export:
`target/sustain-dispatch-metal-smoke/iteration-0002/level-0000-z0000-c0000-t0000.dcm`.
`dciodvfy -new` exits 0 with the known Plane Position Slide Sequence warnings;
`dcentvfy` exits 0.

The post-cache HTJ2K RPCL export
`target/auto-cache-aperio-jp2k-htj2k/iteration-0002/level-0002-z0000-c0000-t0000.dcm`
also validates with `dciodvfy -new` status 0 and `dcentvfy` status 0.

Known warnings remain from dicom3tools for `PlanePositionSlideSequence` inside
the per-frame functional groups. They are warnings, not errors.
This appears to be a dicom3tools IOD model limitation rather than a writer
conformance defect: current DICOM PS3.3 A.32.8.4 lists the Plane Position
(Slide) Functional Group Macro as "Required if Dimension Organization Type
(0020,9311) is not TILED_FULL; may be present otherwise", and A.32.8.4.1.2 says
each encoded sparse tile frame specifies its position in that macro. The live
HTJ2K RPCL sample at
`target/dciodvfy-htj2k-rpcl-small/level-0000-z0000-c0000-t0000.dcm` validates
with `dciodvfy -new` status 0 and only these Plane Position Slide warnings.

HTJ2K-to-HTJ2K passthrough is covered at the public export path by
`export_htj2k_rpcl_passthrough_does_not_touch_gpu_even_when_device_required`.
That test wraps an HTJ2K RPCL codestream in a tiled source, exports HTJ2K RPCL
DICOM with `RequireDevice`, verifies one J2K-family passthrough frame, zero
CPU/GPU transcode work, the HTJ2K RPCL transfer syntax UID, and byte-identical
pixel-data fragment payload after removing DICOM padding.

General J2K-to-J2K passthrough is covered by
`raw_j2k_ycbcr_tile_can_passthrough_to_general_jpeg2000`,
`export_general_j2k_passthrough_accepts_ycbcr_source_without_gpu_work`, and
`export_general_j2k_passthrough_only_rejects_mismatched_geometry_before_gpu_work`.
Those tests cover YBR source tiles, native source tile geometry selection for
the passthrough-only target, the general JPEG 2000 transfer syntax UID,
byte-identical fragment payload after DICOM padding removal, zero CPU/GPU
transcode metrics, and explicit rejection before device work when source and
DICOM frame geometry cannot be made compatible.
`external_dicom_validators_accept_general_j2k_passthrough_when_available` runs
the generated `.91` passthrough object through `dciodvfy -new` and `dcentvfy`
when those tools are available.

The padded-edge DICOM re-ingest case is covered by
`export_htj2k_rpcl_dicom_edge_passthrough_keeps_padded_source_frame`. The full
runtime re-export for
`target/sustain-prod-default-small-htj2k/iteration-0003/level-0000-z0000-c0000-t0000.dcm`
with HTJ2K RPCL, `RequireDevice`, tile size 512, and level 0 now reports
30/30 J2K-family passthrough frames, zero GPU input decode, zero GPU encode,
zero CPU fallback, and zero unclassified frames. The re-exported instance
validates with `dciodvfy -new` status 0 and `dcentvfy` status 0, with the known
Plane Position Slide Sequence warnings only.

Small-scope Auto routing is now CPU-first. On 2026-05-05,
`profile Aperio/JP2K-33003-1.svs --transfer-syntax htj2k-lossless-rpcl --backend auto --source-device-decode --level 0 --max-frames 1 --json`
reported `auto_probe_frames=0`, `gpu_input_decode_frames=0`,
`gpu_encode_frames=0`, `cpu_fallback_frames=1`, and `elapsed_micros=618598`.
The same source with `require-device --source-device-decode --max-frames 80`
reported `gpu_input_decode_frames=80`, `gpu_encode_frames=80`, zero CPU
fallback, and `elapsed_micros=46001718`; CPU-only for the same 80-frame scope
reported `cpu_fallback_frames=80` and `elapsed_micros=49404226`. That is a
measured small win for explicit resident GPU routing, but not enough to justify
unconditional Auto GPU routing for small or weakly batched scopes.

Statumen now has a JP2K Metal batch-decode regression for YCbCr source tiles:
`fixture_ycbcr_device_batch_returns_rgb_metal_tiles`. Re-running the same
80-frame Aperio JP2K resident profile after that change still reported
`input_decode_micros=44555221` and `elapsed_micros=46011517`, effectively
unchanged from the prior `input_decode_micros=44510245` /
`elapsed_micros=46001718` run. That means the current Aperio bottleneck is not
the YCbCr batch eligibility guard; it is deeper in JP2K Metal decode submission,
waiting, or the decode kernels themselves. Disabling Statumen JP2K device
batching for the same 80-frame profile with `STATUMEN_JP2K_DEVICE_BATCH=0`
also stayed flat at `input_decode_micros=44589093` and
`elapsed_micros=46036161`.

The signinum J2K Metal surface type now distinguishes true resident Metal decode
from CPU-staged Metal upload with `SurfaceResidency`. Statumen rejects
`CpuStagedMetalUpload` for JP2K device decode, so `gpu_input_decode_frames` no
longer counts CPU decode followed by upload as resident GPU decode. The
low-level signinum test `explicit_metal_request_does_not_stage_cpu_pixels`
enforces that `BackendRequest::Metal` cannot be satisfied by CPU-staged upload;
separate `decode_*_cpu_staged_metal_surface_with_session` APIs cover the rare
case where callers explicitly want CPU decode followed by upload. The real
Aperio JP2K 16-frame `require-device` profile after the strict API split reported
`gpu_input_decode_frames=16`, `gpu_encode_frames=16`,
`resident_gpu_transcode_frames=16`, `partial_gpu_transcode_frames=0`,
`cpu_fallback_frames=0`, `gpu_input_decode_batches=1`, `gpu_encode_batches=1`,
and `elapsed_micros=9049203`.

Statumen now caches the JP2K YCbCr-to-RGB8 Metal conversion shader, pipeline,
and command queue on the caller-owned `MetalBackendSessions` instead of
recompiling that tiny converter per tile. JP2K YCbCr batch decode also converts
all YCbCr Metal tiles through one cached converter submission instead of one
command buffer per tile. The regressions
`ycbcr_to_rgb8_converter_is_cached_per_backend_sessions` and
`ycbcr_to_rgb8_tiles_converts_batch_with_one_cached_converter` verify reuse and
batched output, and the JP2K YCbCr single/batch fixtures still return RGB Metal
tiles. The same Aperio JP2K 16-frame `require-device` profile after this cache
and batch-conversion change reports `gpu_input_decode_frames=16`,
`gpu_encode_frames=16`, `resident_gpu_transcode_frames=16`,
`partial_gpu_transcode_frames=0`, `cpu_fallback_frames=0`, and
`elapsed_micros=9056084`.

Two-iteration bounded `sustain` on the same Aperio JP2K source, level 0 only,
`max_frames_per_level=16`, `backend=require-device`, and
`--source-device-decode` stayed resident and stable after the batch-conversion
change:

```text
iteration=1 elapsed_ms=9044.019 resident_gpu_transcode_frames=16 partial_gpu_transcode_frames=0 cpu_fallback_frames=0 gpu_input_batches=1 gpu_encode_batches=1 rss_mb=49.2 memory_pressure=91% thermal=no warning recorded
iteration=2 elapsed_ms=8994.206 resident_gpu_transcode_frames=16 partial_gpu_transcode_frames=0 cpu_fallback_frames=0 gpu_input_batches=1 gpu_encode_batches=1 rss_mb=51.6 memory_pressure=91% thermal=no warning recorded
```

The same residency split now applies to `signinum-jpeg-metal`. `Surface` reports
`SurfaceResidency`, direct Metal decode returns `MetalResidentDecode`, and
CPU-populated Metal buffers are marked `CpuStagedMetalUpload`. Statumen rejects
CPU-staged JPEG Metal surfaces for device decode. This exposed that the prior
CMU-1 JPEG-to-HTJ2K "GPU input" profile was not a real resident decode path:
after the fix, `Aperio/CMU-1.svs` with `--transfer-syntax
htj2k-lossless-rpcl --backend auto --source-device-decode --max-frames 16`
routes to CPU with `gpu_input_decode_frames=0`,
`resident_gpu_transcode_frames=0`, `cpu_fallback_frames=16`, and
`elapsed_micros=5875225`. The JPEG Baseline target now uses native regular
source tile geometry before considering the fallback requested tile size. On
the same default CLI request against CMU-1, the Aperio 256x256 JPEG source tiles
are legal DICOM frames and profile as pure passthrough:
`jpeg_passthrough_frames=16`, `jpeg_decode_fallback_frames=0`,
`jpeg_cpu_encode_frames=0`, and `elapsed_micros=1035`.

RGB CPU-input/device-encode is now eligible as a partial GPU route, but only
for route scopes of at least 32 frames so the four-frame probe can amortize.
On `Aperio/CMU-1.svs`, 16 HTJ2K RPCL frames still route CPU:
`gpu_encode_frames=0`, `partial_gpu_transcode_frames=0`,
`cpu_fallback_frames=16`, `elapsed_micros=5875225`. Sequential 32-frame CMU-1
checks measured `elapsed_micros=11435445` CPU-only versus
`elapsed_micros=9305661` with explicit CPU-input/GPU-encode, so the partial
auto gate was lowered from 64 to 32 frames. At 64 frames the same command
selects CPU-input/GPU-encode: `gpu_encode_frames=64`,
`partial_gpu_transcode_frames=64`, `cpu_fallback_frames=0`,
`elapsed_micros=19900259`, compared with CPU-only `elapsed_micros=22740017` on
the same 64-frame scope. With the 32-frame gate, `auto --source-device-decode`
now selects partial GPU for 32 CMU-1 frames: `partial_gpu_transcode_frames=32`,
`cpu_fallback_frames=0`, and `elapsed_micros=10630683` including probe cost.

The measured route gate was lowered from 15% to 8% after sequential Aperio JP2K
checks showed that the old threshold rejected useful resident batches. On
`Aperio/JP2K-33003-1.svs`, 64 HTJ2K RPCL frames measured
`elapsed_micros=40874428` CPU-only and `elapsed_micros=36621074` with explicit
resident GPU. With the 8% gate, `auto --source-device-decode` selects resident
GPU for the same 64-frame scope: `resident_gpu_transcode_frames=64`,
`cpu_fallback_frames=0`, and `elapsed_micros=39165374` including probe cost.

Bounded `coverage-corpus` over the local OpenSlide testdata corpus,
`max_levels=1`,
`max_frames_per_level=16`, `max_level_ms=30000`, HTJ2K RPCL, `backend=auto`,
and `--source-device-decode` after the JPEG residency split considered 23
sources and sampled 352 frames. After the 8% gate, the honest resident count is
128/352 frames: `gpu_input_decode_frames=128`,
`resident_gpu_transcode_frames=128`, `cpu_fallback_frames=224`,
`gpu_input_decode_batches=16`, `gpu_encode_batches=16`, and
`elapsed_micros=130657275`. One broken Leica JPEG fixture still fails with an
unexpected EOI. The resident frames came from Generic TIFF, Leica tiled,
Philips TIFF, and the two Aperio JP2K fixtures. Aperio JPEG, NDPI, Leica
fluorescence, and one Philips TIFF sample still route CPU under `auto`.

## Fixture Gates Run Manually

These ignored tests were run with real OpenSlide fixtures and passed:

- `real_aperio_jp2k_problem_tile_round_trips`
- `aperio_jp2k_aligned_metal_input_256_htj2k_rpcl_tile_matches_cpu`
- `aperio_jp2k_regular_tiled_metal_input_composes_512_htj2k_rpcl_tile_matches_cpu`
- `fixture_first_mappable_tiles_use_batched_statumen_metal_input_decode_and_metal_encode`
- `ndpi_fixture_exports_full_jpeg_baseline_passthrough_instance`
- `ndpi_fixture_exports_jpeg_baseline_passthrough_pyramid_subset_for_qupath`
- `ndpi_fixture_exports_all_lossless_j2k_transfer_syntaxes_and_tile_sizes`
- `ndpi_whole_level_metal_rows_do_not_turn_black_after_reused_encoder_state`

## Remaining Gaps

- Auto-routing now has a bounded first-instance runtime probe, route-scope-aware
  in-process decision cache, and opt-in persistent JSON route cache. It still
  does not have a learned per-source-family policy.
- Full-size whole-slide sustained conversion remains only partially measured;
  the largest attempted full JP2K export was interrupted because it was
  CPU-bound and not useful for the current routing decision.
- Multi-slide corpus coverage now reports per-source progress, skips
  source-generated synthetic levels, and has a cooperative per-level elapsed
  guard through `--max-level-ms`. It still cannot interrupt a single in-flight
  codec decode or encode call.
- Explicit synthetic NDPI ROI reads now avoid synthetic whole-level cache
  materialization in Statumen tests and the `Hamamatsu/CMU-1.ndpi` real-fixture
  probe. wsi-dicom route coverage intentionally skips synthetic levels.
- General JPEG 2000 preservation is passthrough-only. It now avoids accidental
  GPU/CPU transcode work on incompatible frames, but whole-level export still
  needs source/DICOM frame geometry compatibility, including edge frames.
- RGB H&E clinical relevance remains gated by direct GPU decode coverage and
  broader real-slide measurements.
- Remaining DICOM warning tracking is limited to dicom3tools model drift for
  allowed Plane Position Slide metadata; dciodvfy currently exits 0.
