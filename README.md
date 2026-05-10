<!-- SPDX-License-Identifier: Apache-2.0 -->

# wsi-dicom

`wsi-dicom` is the DICOM whole-slide export layer for `statumen`.

It is intentionally a sibling crate, not a `statumen` module and not
SlideViewer application code. The dependency direction is:

```text
vendor WSI files -> statumen -> wsi-dicom -> app or CLI integration
```

`statumen` owns WSI format detection, vendor parsing, pyramid geometry, and
tile/region reads. `wsi-dicom` will own DICOM VL Whole Slide Microscopy export:
transfer syntax selection, output layout, metadata validation, and writer
errors. SlideViewer should call into this crate for user-facing export flows
without taking ownership of DICOM writing rules.

## Name

`wsi-dicom` is intentionally literal: it converts whole-slide imaging data into
DICOM VL Whole Slide Microscopy objects. The package imports as `wsi_dicom` in
Rust code.

## Packaging

`wsi-dicom` is the public facade crate and CLI. Users should depend on this
crate directly; JPEG 2000 and WSI reader crates are internal implementation
dependencies.

Default builds are CPU-only:

```toml
[dependencies]
wsi-dicom = "0.1"
```

GPU support is opt-in:

```toml
[dependencies]
wsi-dicom = { version = "0.1", features = ["gpu"] }
```

Feature flags:

| Feature | Effect |
| --- | --- |
| `default` | CPU-only DICOM export. |
| `gpu` | Enables both `cuda` and `metal` backends. |
| `cuda` | Enables CUDA JPEG 2000 encode acceleration when available. |
| `metal` | Enables Metal JPEG 2000 encode acceleration on macOS, Metal codestream validation decode, and statumen Metal tile decode plumbing. |
| `vendored-codecs` | Reserved facade feature for bundled codec builds; currently no extra native codec dependency is required. |

Runtime backend selection is still controlled by the CLI/API backend option:
`auto`, `cpu`, `prefer-device`, or `require-device`. If GPU features are not
compiled in, `prefer-device` falls back to CPU and `require-device` reports a
clear unsupported-device error.

J2K/HTJ2K runtime codec validation is explicit. The default
`codec_validation: Disabled` skips the per-frame roundtrip validation decode in
normal conversion so production exports do not pay that cost. Set
`codec_validation: RoundTrip` in the Rust API, or pass
`--codec-validation round-trip` in the CLI, for QA and benchmark runs that need
an encode-time roundtrip check. External DICOM/reference-codec tests remain the
conformance gate for release evidence.

With the `metal` feature on macOS, statumen source tiles are requested as
batched row runs of Metal device tiles and bridged directly into the Metal J2K
encoder when they align to the requested DICOM tile size. WholeLevel virtual
tile grids, including NDPI strip grids, are decoded in source-tile batches and
composed into DICOM frame buffers by a private Metal kernel before encoding.
Edge tiles are padded on Metal before encoding. The export report and CLI output
include frame counts for CPU input, GPU input decode, GPU encode, and GPU
validation decode. `gpu_dispatch_ms` reports aggregate CPU-observed duration
from GPU-dispatched stages, including command-buffer submission and wait
overhead. `gpu_encode_hardware_ms` reports summed Metal command-buffer GPU
execution duration for resident J2K/HTJ2K encode when Metal exposes it;
`gpu_encode_dispatch_overhead_ms` reports the sum of per-frame positive
CPU-observed encode dispatch time left after subtracting that frame's hardware
duration. Because the hardware metric is summed across command buffers and
frames, it is not a wall clock and can exceed the per-stage elapsed encode time.
Statumen's compressed TIFF-family device decode paths remain
opt-in for explicit device-preferred routing: set `STATUMEN_JPEG_DEVICE_DECODE=1`
for JPEG-backed WSI tiles and `STATUMEN_JP2K_DEVICE_DECODE=1` for JPEG 2000-backed
WSI tiles. JP2K device decode batches are enabled by default after JP2K device
decode is requested; set `STATUMEN_JP2K_DEVICE_BATCH=0` only to force the older
per-tile device decode fallback during troubleshooting. `prefer-device` and
`require-device` use the resident path whenever it is available. The `auto`
backend stays conservative: for HTJ2K RPCL output
with statumen device decode explicitly enabled and at least 16 routed frames, it
probes up to the first four eligible non-passthrough frames through both
CPU-input and Metal-input routes, records the probe timings in the
export/profile metrics, and keeps Metal input only when that measured route is
at least 8% faster. When `auto` does not run a route probe, or when the probe
keeps CPU routing, J2K/HTJ2K encode is demoted to CPU so unmeasured small scopes
do not spend GPU work. The measured decision is
cached in-process by source path, level, tile size, transfer syntax, and routed
frame count, so bounded coverage probes cannot seed decisions for full-level
conversions and repeated conversions in sustained runs do not repay the probe. Set
`WSI_DICOM_AUTO_ROUTE_CACHE=/path/to/cache.json` to persist those measured
decisions across separate CLI invocations. Route reports also include
component-count and bit-depth coverage fields, including `rgb_like_frames`,
`gray_frames`, and `unknown_pixel_profile_frames`, so RGB H&E coverage is
visible alongside passthrough/GPU/CPU counts. Profile and coverage summaries
also report `available_frames` and `sampled_frames_pct`, making bounded
one-frame-per-level probes distinguishable from full-level route coverage; the
sampled fraction is printed to four decimals so tiny real-corpus samples do not
round to zero. Coverage reports also include `complete_frame_coverage` so
automation can reject bounded probes when full report-scope coverage is
required. JSON coverage reports carry `available_frames` at the level, source,
and corpus levels for downstream aggregation. JPEG Baseline `auto` does not use
Metal encode unless passthrough is impossible and a device backend is explicitly
requested. JPEG Baseline output uses native regular source tile geometry when
available, so legal JPEG-backed WSI tiles can be encapsulated directly even when
the requested fallback `--tile-size` differs from the source tile size.

Use `--full-frame-coverage` with `coverage`, `coverage-corpus`, or `sustain`
when route fractions must cover every frame in the reported scope. Without that
flag, `--max-frames-per-level` remains a bounded probe control. Sustained
coverage output includes the same coverage denominator and completion flag next
to throughput, process RSS, macOS memory pressure when available, and thermal
state.

Supported compressed export transfer syntaxes:

When `--transfer-syntax` is omitted for a single-source CLI command, the CLI
inspects the selected source scope and prefers native compressed-frame
passthrough: JPEG-backed sources use JPEG Baseline 8-bit when eligible, JPEG
2000-backed sources use general JPEG 2000 when eligible, and other sources fall
back to HTJ2K Lossless RPCL. Pass
`--transfer-syntax htj2k-lossless-rpcl` to explicitly request HTJ2K re-encoding.
The Rust API keeps `DicomExportOptions::default()` source-independent at
HTJ2K Lossless RPCL; integrations that want the CLI default can call
`default_transfer_syntax_for_source(...)` before export.

| CLI value | UID | Description |
| --- | --- | --- |
| `jpeg-baseline8-bit` | `1.2.840.10008.1.2.4.50` | JPEG Baseline 8-bit; preserves compatible native JPEG source frames without decode/encode and re-encodes only frames that cannot be passed through |
| `jpeg2000` | `1.2.840.10008.1.2.4.91` | General JPEG 2000 passthrough-only; uses native square source tile geometry when available and preserves compatible source codestreams without decode/encode |
| `jpeg2000-lossless` | `1.2.840.10008.1.2.4.90` | JPEG 2000 Lossless |
| `htj2k-lossless` | `1.2.840.10008.1.2.4.201` | HTJ2K Lossless |
| `htj2k-lossless-rpcl` | `1.2.840.10008.1.2.4.202` | HTJ2K Lossless RPCL with RPCL progression and TLM markers |

For integrations that already have composed tile samples, the Rust API exposes
`encode_dicom_j2k_frame(DicomJ2kFrameEncodeRequest { ... })`. It returns a
`DicomEncodedFrame` containing finished JPEG 2000 or HTJ2K codestream bytes plus
backend/validation timing flags. The returned bytes are ready to insert as one
encapsulated DICOM Pixel Data fragment; the DICOM writer remains responsible for
fragment padding and offset table bookkeeping.
