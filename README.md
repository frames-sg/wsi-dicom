<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->

The current WSI validation challenge is maintained in
[`wsi-dicom-bench`](https://github.com/frames-sg/wsi-dicom-bench):
69 authored defects and 14 controls covering 18 selected rule families. Earlier sealed studies
remain historical evidence; this challenge does not claim exhaustive normative DICOM coverage.

# wsi-dicom

`wsi-dicom` converts whole-slide imaging files that `wsi-rs` can open into
DICOM VL Whole Slide Microscopy instances. It provides a Rust API, a CLI, and an
optional native GUI.

[J2K, the pure-Rust JPEG 2000 codec](https://frames-sg.github.io/j2k/rust-jpeg2000-codec/),
supplies JPEG, JPEG 2000, and HTJ2K codec primitives. `wsi-rs`
opens vendor WSI formats such as SVS and NDPI. `wsi-dicom` owns DICOM export,
metadata validation, transfer-syntax routing, reports, and writer errors.
`wsi-dicom-annotations` owns the UI-independent QuPath/GeoJSON terminology and
DICOM ANN, SEG, and SR conversion; this package orchestrates it alongside WSI
conversion.

## Install

The latest published release is `0.7.4`:

```sh
cargo install wsi-dicom
```

Use the Rust API:

```toml
[dependencies]
wsi-dicom = "0.7.5"
```

GPU support is opt-in:

```toml
[dependencies]
wsi-dicom = { version = "0.7.5", features = ["metal"] } # macOS
# or
wsi-dicom = { version = "0.7.5", features = ["cuda"] } # CUDA-capable Linux/Windows
```

This source tree prepares the `0.7.5` release and requires Rust 1.96.
The manifest and lockfile resolve registry dependencies, including `wsi-rs`
0.6.0 and `wsi-dicom-annotations` 0.1.1. No sibling source checkout or local
Cargo patch is required for the default CPU build:

```sh
cargo build --release --locked
```

The commands and APIs below describe the 0.7.5 interface.

Feature flags:

| Feature | Effect |
| --- | --- |
| `default` | CPU-only DICOM export. |
| `cuda` | Enables CUDA JPEG 2000 encode acceleration when available. wsi-rs CUDA tile decode and direct JPEG-to-HTJ2K CUDA transcode are not exposed by the 0.7.5 release. |
| `metal` | Enables Metal JPEG 2000 encode acceleration on macOS, Metal codestream validation decode, and wsi-rs Metal tile decode plumbing. |

CUDA release and hardware-evidence builds should require cuda-oxide PTX
generation instead of accepting the dependency's compile-time fallback. Install
`libclang`, select the GPU architecture, and make a missing PTX build fatal; for
example, an RTX 4070-class device uses `sm_89`:

```sh
J2K_CUDA_OXIDE_ARCH=sm_89 J2K_REQUIRE_CUDA_OXIDE_BUILD=1 \
  cargo test --features cuda --lib --locked
```

Set `LIBCLANG_PATH` when `libclang` is not discoverable through the platform's
normal library paths. `--backend require-device` remains the runtime fail-closed
check: it does not silently substitute CPU encoding when CUDA is unavailable.

For local maximum CPU throughput:

```sh
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

The optional GUI lives in `apps/wsi-dicom-gui`:

```sh
cargo run -p wsi-dicom-gui
```

## Quickstart

Always provide metadata JSON/FHIR input or explicitly select research
placeholder metadata.

```sh
wsi-dicom convert slide.ndpi --out dicom-out --research-placeholder \
  --icc source-or-srgb
```

If a proprietary reader can decode pixels but cannot surface physical calibration, provide a
governed level-zero spacing explicitly in DICOM row/column order. The converter rejects non-positive
values and rejects a supplied value that conflicts with calibration already present in the source:

```sh
wsi-dicom convert slide.vsi --out dicom-out --research-placeholder \
  --source-pixel-spacing-mm 0.00034605325860336383,0.0003460559834973875
```

To convert a QuPath-annotated slide in the same operation, export the QuPath
objects as GeoJSON and provide an explicit mapping from every QuPath class name
to DICOM coded concepts:

```sh
wsi-dicom convert slide.ndpi --out dicom-out --research-placeholder \
  --qupath-annotations slide.geojson \
  --annotation-mapping examples/qupath-neoplasm-mapping-v1.json \
  --annotation-target ann --annotation-target seg
```

The mapping is required: `wsi-dicom` never guesses terminology or adds QuPath
classes to a global profile. Copy the example and add entries keyed by the
exact QuPath classification names used in the GeoJSON. The default
`level0-pixels` coordinate space matches QuPath's full-resolution image
coordinates. `source-pixels` is for coordinates already expressed on the
generated DICOM image grid, and `slide-mm` is for physical slide coordinates.

Annotation output is written under `dicom-out/annotations/` with a manifest and
one verified sidecar per requested target. Use ANN for directly representable
points and simple polygons, SEG for rasterized regions, and SR for mapped
measurements. Lossy omission or conversion is rejected unless
`--allow-lossy-annotations` is supplied explicitly. The WSI instances are
published first; if annotation conversion then fails, the error identifies the
failure and the already completed WSI export remains valid.

Use `--metadata metadata.json` for real metadata. `--metadata` and
`--research-placeholder` are mutually exclusive. Existing generated `.dcm`
paths are refused by default; pass `--overwrite` only when replacement is
intentional. Each conversion writes its complete set to a sibling staging
directory, then promotes the flat `.dcm` files sequentially under an
output-directory writer lock. The journaled commit is failure-atomic for
ordinary reported errors: newly promoted files are removed and overwritten
files are restored if commit fails. It is not visibility-atomic for concurrent
readers, which may observe a mixed set or a temporarily absent overwritten file
during promotion. An interrupted transaction is recovered before the next
export; a failed rollback returns a recovery-required error and retains its
journal and backups.

Metadata is validated before slide access and output staging. Non-ASCII text is
encoded as UTF-8 and declares DICOM Specific Character Set `ISO_IR 192`; scalar
text rejects DICOM value delimiters and control characters. The
`imaged_volume_depth_mm` input is always millimeters. It is converted to
micrometers for Imaged Volume Depth (FL) and remains millimeters for Slice
Thickness (DS).

> [!IMPORTANT]
> Regenerate output produced by wsi-dicom 0.7.0 or earlier if it used imaged
> volume depth (including the 0.001 mm default) or non-ASCII metadata. Those
> versions could write the depth with the wrong Imaged Volume Depth units or
> omit the required Specific Character Set declaration.

Per-frame functional-group and extended-offset-table metadata is bounded by
default to 256 MiB per instance and 1 GiB across one export. Trusted large
workloads can override these limits for `convert` and `sustain-convert`:

```sh
wsi-dicom convert slide.ndpi --out dicom-out --metadata metadata.json \
  --max-instance-metadata-mib 512 --max-total-metadata-mib 2048
```

Library callers can set `ExportOptions::max_instance_metadata_bytes` and
`ExportOptions::max_total_metadata_bytes`, or use the corresponding `Export`
builder methods. The writer preflights both limits and enforces the per-instance
limit while streaming metadata.

Generated DICOM UIDs are fresh for each conversion. Reproducible pipelines may
opt into full source-content/configuration identity with
`--uid-policy deterministic`; this hashes the complete source and is therefore
more expensive on large slides. The deterministic identity also includes the
SHA-256 digest of the effective ICC profile for each generated instance.

`DicomMetadata::specimen_uid` accepts a governed Specimen UID and preserves it
verbatim across exports. When it is absent, the exporter derives the Specimen
UID from the export identity, specimen identifier, and structured identifier
issuer. Fresh exports therefore receive fresh fallback Specimen UIDs,
deterministic exports repeat them, and issuer namespaces keep otherwise equal
local identifiers separate. FHIR `Specimen.identifier.system` is emitted as a
universal issuer of type `URI`.

## Scanner calibration

Every export requires an explicit color-management choice. The CLI defaults to
`--icc source-or-srgb` for research use; calibrated workflows should select
`--icc require-source`, a governed calibration registry, or an explicit
profile. Registry and explicit-profile modes are mutually exclusive:

```sh
wsi-dicom convert slide.ndpi --out dicom-out --metadata metadata.json \
  --icc-calibration-registry calibration/registry.json \
  --icc-conflict fail

wsi-dicom convert slide.ndpi --out dicom-out --metadata metadata.json \
  --icc-profile vendor-profile.icc --icc-profile-id lab-at2-2026q3 \
  --icc-conflict prefer-configured
```

Portable registries use schema version 1:

```json
{
  "schema_version": 1,
  "calibrations": [
    {
      "id": "lab-at2-sn123-2026q3",
      "scanner": {
        "manufacturer": "Leica Biosystems",
        "model_name": "Aperio AT2",
        "device_serial_number": "SN123"
      },
      "icc_profile": "profiles/profile.icc",
      "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    }
  ]
}
```

Scanner matching uses the governed DICOM manufacturer, model name, and device
serial number. All three values are required; outer whitespace is trimmed and
the remaining text is compared exactly and case-sensitively. Filenames and raw
vendor properties are not matching inputs. Registries reject unknown fields,
duplicate IDs or scanner tuples, malformed hashes, unsafe paths, missing or
checksum-mismatched files, and profiles that are not valid RGB input-device ICC
profiles. Registry JSON is limited to 1 MiB and 256 entries; each ICC file is
limited to 16 MiB. Relative profile paths resolve from the registry directory,
and only verified bytes—not local paths—enter conversion state or DICOM output.

When configured and source ICC digests differ, `--icc-conflict fail` stops the
export before staging or output creation. `prefer-configured` embeds the
configured bytes; `prefer-source` retains the source bytes. Equal digests are
accepted as a match. Each instance report records the effective SHA-256 digest,
profile or calibration ID, source, and conflict decision. MONOCHROME2 output
does not write ICC data and reports calibration as not applicable.

Inspect an existing profile or package it into a local portable bundle:

```sh
wsi-dicom calibration inspect --icc vendor-profile.icc
wsi-dicom calibration create --icc vendor-profile.icc \
  --id lab-at2-sn123-2026q3 \
  --manufacturer "Leica Biosystems" --model "Aperio AT2" --serial SN123 \
  --out calibration
```

`calibration create` validates and packages an existing vendor- or
target-generated profile. It does not derive scanner calibration from an
ordinary tissue slide. Calibration selection is CLI/API-only in 0.7.5; the GUI
offers source-required, sRGB fallback, and Display P3 fallback choices.

The default conversion preset is `lossless-review`, which emits HTJ2K Lossless
RPCL. For explicit JPEG Baseline output:

```sh
wsi-dicom convert slide.ndpi --out dicom-fast --research-placeholder \
  --icc source-or-srgb --preset fast-jpeg
```

Useful operational commands:

```sh
wsi-dicom doctor --strict --json
wsi-dicom self-test --json --out self-test-evidence --keep-output
wsi-dicom validate dicom-out --strict --json
wsi-dicom coverage slide.ndpi --json
```

For the complete cataloged DICOM-side evaluation and one unified per-slide evidence bundle, install
the independently versioned [WSI-DICOM Bench](https://github.com/frames-sg/wsi-dicom-bench) package:

```sh
python -m pip wheel \
  "git+https://github.com/frames-sg/wsi-dicom-bench@caafcd9bc660fa86a7fc6db09a08bd8538181400" \
  --wheel-dir .benchmark-wheel
python -m pip install .benchmark-wheel/wsi_dicom_bench-*.whl
wsi-dicom-bench-workbench dicom-out \
  --output evidence/slide-id \
  --wsi-dicom target/release/wsi-dicom
```

HTJ2K pixel decode validation auto-detects `grk_decompress` when it is on
`PATH`. You can also provide an explicit absolute decoder command:

```sh
wsi-dicom validate dicom-out \
  --htj2k-decoder "/opt/homebrew/bin/grk_decompress -i {input} -o {output}"
```

Every validation run performs intrinsic checks before optional external
validation. The Pixel Data structure check verifies Number of Frames, native
versus encapsulated representation, nonempty data, and bounded frame mapping
through Basic or Extended Offset Tables; it still runs when
`--max-pixel-frames 0`. VL Whole Slide Microscopy Image objects additionally
run stable `intrinsic-wsi-dicom-2026c-*` checks for ICC profiles, monochrome
presentation attributes, lossy declarations, specimen identity, and dimension
ordering, plus a set-level Specimen UID consistency check. These checks do not
decode pixel values or infer unknowable compression history predating the
object. Missing external tools are reported as skipped unless `--strict` is
set. Directory validation is bounded by file count, depth, timeout, and child
output capture limits; symlink traversal is refused.

## Rust API

Use the builder API for normal exports:

```rust
use wsi_dicom::{ColorManagement, Export};

let report = Export::from_slide("slide.ndpi")
    .to_directory("out")
    .with_research_placeholder_metadata()
    .tile_size(512)
    .jpeg_quality(90)
    .color_management(ColorManagement::SourceOrSrgb)
    .run()?;
```

CLI and GUI-style integrations that also need metadata selection, annotation
preparation, optional validation, report persistence, and progress events can
use `run_export_workflow` with `ExportWorkflowRequest`. Route-analysis callers
can use the target-specific `SlideRouteCoverageRequest` and
`CorpusRouteCoverageRequest` APIs; the older `RouteCoverageRequest` remains a
compatibility wrapper.

Use request types when an integration needs full control:

```rust
use wsi_dicom::{
    export_dicom, ColorManagement, ExportOptions, ExportRequest,
    JpegDirectHtj2kProfile, MetadataSource, TransferSyntax,
};

let mut options = ExportOptions::lossless_review();
options.transfer_syntax = TransferSyntax::Htj2k;
options.jpeg_direct_htj2k_profile = JpegDirectHtj2kProfile::Lossy97Balanced;
let request = ExportRequest::new(
    "slide.ndpi".into(),
    "out".into(),
    options,
    ColorManagement::SourceOrSrgb,
    MetadataSource::ResearchPlaceholder,
)?;

let report = export_dicom(request)?;
```

For composed tile samples:

```rust
use wsi_dicom::{
    encode_dicom_j2k_frame, CodecValidation, EncodeBackendPreference,
    FrameSamples, J2kFrameEncodeRequest, TransferSyntax,
};

let pixels = vec![0_u8; 512 * 512 * 3];
let samples = FrameSamples::new(&pixels, 512, 512, 3, 8, false)?;
let frame = encode_dicom_j2k_frame(J2kFrameEncodeRequest::new(
    samples,
    TransferSyntax::Htj2kLosslessRpcl,
    EncodeBackendPreference::CpuOnly,
    CodecValidation::RoundTrip,
))?;
```

## Behavior Notes

- Every color optical path carries a validated DICOM input-device ICC profile.
  Source profiles are preserved by the source/fallback modes. A synthesized
  sRGB or Display P3 input profile is an explicit color-space assumption, not
  scanner calibration. Configured/source conflicts are resolved only through
  the selected `IccConflictPolicy`.
- ICC is not applicable to MONOCHROME2 output, which instead carries
  Presentation LUT Shape `IDENTITY`, Rescale Intercept `0`, and Rescale Slope
  `1`.
- Lossy compression history is independent of the final transfer syntax. A
  lossless transcode does not erase prior lossy JPEG/JPEG 2000/HTJ2K stages.
- JPEG Baseline output preserves compatible native JPEG frames. HTJ2K lossless
  output rejects nonconformant color JPEG direct routes and falls back through
  decoded RGB/RCT.
- JPEG 2000 passthrough preserves eligible native source codestreams.
- Route profile and coverage JSON reports expose available frame counts,
  sampled frame percentages, route counters, pixel profiles, and GPU counters.
- Per-frame functional groups use streamed undefined-length sequences and items
  instead of an in-memory item list. Frame offsets and lengths are kept in a
  temporary disk-backed index and replayed when patching Extended Offset Tables.
- Output names encode scene, series, level, Z, channel, and time coordinates;
  consumers must use report paths rather than assuming the pre-0.7 name shape.
- Passing validators is release evidence, not formal DICOM certification.

> [!IMPORTANT]
> Regenerate color, MONOCHROME2, previously lossy, or multi-institution
> specimen output produced by version 0.7.1 or earlier. Older objects
> can contain a display-class fallback ICC profile, omit required monochrome
> presentation attributes, erase lossy history after lossless transcoding, or
> derive the same Specimen UID for identifiers governed by different issuers.

## Development

Durable repository contracts and the bounded architecture backlog are indexed in
[the architecture decision records](docs/architecture/README.md). Release history
remains in the changelog; manifests, lockfiles, tests, and CI are the current build
source of truth.

Core checks:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --no-default-features --all-targets --locked -- -D warnings
cargo test --workspace --no-default-features --all-targets --locked
cargo check -p wsi-dicom-gui --locked
```

Pre-1.0 release gates:

```sh
cargo xtask docs-strict
cargo xtask coverage
cargo xtask semver
cargo publish --dry-run
```

Before any release candidate that changes or advertises export routes,
performance, conformance, or accelerator behavior, run these gates against
published dependencies and a representative real-slide corpus covering the
affected routes, metadata modes, color-management policies, validator checks,
and GPU backends.

Use the GDC benchmark harness only when publishing speed evidence:

```sh
./.venv/bin/python bench/gdc_benchmark.py \
  --downloads-root ~/Downloads \
  --probe-slide-metadata \
  --tools wsi-dicom-cpu wsi-dicom-device wsidicomizer \
  --profile htj2k-lossless-rpcl \
  --scope base \
  --runs 1 \
  --system-label macos-metal \
  --validate
```

Run the same command on the Metal and CUDA hosts with host-specific release
binaries and `--system-label` values. Merge result directories with
`--merge-results`, then publish failures, unsupported slides, transfer syntax,
frame geometry, tool versions, host details, and machine-readable results with
any performance claim.

## Stability

`wsi-dicom` is pre-1.0. The builder API is the preferred integration surface.
Lower-level request, report, validation, and profiling types are public, but
callers should prefer constructors and defaults over struct literals where
provided. The 0.7.4 release deliberately broke the pre-1.0
color-management API: `IccProfilePolicy` is removed, `ExportRequest::new` requires a
`ColorManagement`, and `Export` requires `.color_management(...)`.

## License

Dual-licensed under either [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE), at your option.
