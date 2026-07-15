<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->

# wsi-dicom

`wsi-dicom` converts whole-slide imaging files that `wsi-rs` can open into
DICOM VL Whole Slide Microscopy instances. It provides a Rust API, a CLI, and an
optional native GUI.

`j2k` supplies JPEG, JPEG 2000, and HTJ2K codec primitives. `wsi-rs`
opens vendor WSI formats such as SVS and NDPI. `wsi-dicom` owns DICOM export,
metadata validation, transfer-syntax routing, reports, and writer errors.

## Install

Install the CLI:

```sh
cargo install wsi-dicom
```

Use the Rust API:

```toml
[dependencies]
wsi-dicom = "0.7.0"
```

GPU support is opt-in:

```toml
[dependencies]
wsi-dicom = { version = "0.7.0", features = ["metal"] } # macOS
# or
wsi-dicom = { version = "0.7.0", features = ["cuda"] } # CUDA-capable Linux/Windows
```

Feature flags:

| Feature | Effect |
| --- | --- |
| `default` | CPU-only DICOM export. |
| `cuda` | Enables CUDA JPEG 2000 encode acceleration when available. wsi-rs CUDA tile decode and direct JPEG-to-HTJ2K CUDA transcode are not exposed by wsi-dicom 0.7.0. |
| `metal` | Enables Metal JPEG 2000 encode acceleration on macOS, Metal codestream validation decode, and wsi-rs Metal tile decode plumbing. |

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
wsi-dicom convert slide.ndpi --out dicom-out --research-placeholder
```

Use `--metadata metadata.json` for real metadata. `--metadata` and
`--research-placeholder` are mutually exclusive. Existing generated `.dcm`
paths are refused by default; pass `--overwrite` only when replacement is
intentional. Each conversion is staged and committed as one generation, so an
ordinary failure does not leave a partial set of final `.dcm` files.

Generated DICOM UIDs are fresh for each conversion. Reproducible pipelines may
opt into full source-content/configuration identity with
`--uid-policy deterministic`; this hashes the complete source and is therefore
more expensive on large slides.

The default conversion preset is `lossless-review`, which emits HTJ2K Lossless
RPCL. For explicit JPEG Baseline output:

```sh
wsi-dicom convert slide.ndpi --out dicom-fast --research-placeholder --preset fast-jpeg
```

Useful operational commands:

```sh
wsi-dicom doctor --strict --json
wsi-dicom self-test --json --out self-test-evidence --keep-output
wsi-dicom validate dicom-out --strict --json
wsi-dicom coverage slide.ndpi --json
```

HTJ2K pixel decode validation auto-detects `grk_decompress` when it is on
`PATH`. You can also provide an explicit absolute decoder command:

```sh
wsi-dicom validate dicom-out \
  --htj2k-decoder "/opt/homebrew/bin/grk_decompress -i {input} -o {output}"
```

Missing external tools are reported as skipped unless `--strict` is set.
Directory validation is bounded by file count, depth, timeout, and child output
capture limits; symlink traversal is refused.

## Rust API

Use the builder API for normal exports:

```rust
use wsi_dicom::{Export, IccProfilePolicy};

let report = Export::from_slide("slide.ndpi")
    .to_directory("out")
    .with_research_placeholder_metadata()
    .tile_size(512)
    .jpeg_quality(90)
    .icc_profile_policy(IccProfilePolicy::FallbackSrgb)
    .run()?;
```

Use request types when an integration needs full control:

```rust
use wsi_dicom::{
    export_dicom, ExportOptions, ExportRequest, IccProfilePolicy,
    JpegDirectHtj2kProfile, MetadataSource, TransferSyntax,
};

let mut options = ExportOptions::lossless_review();
options.transfer_syntax = TransferSyntax::Htj2k;
options.jpeg_direct_htj2k_profile = JpegDirectHtj2kProfile::Lossy97Balanced;
options.icc_profile_policy = IccProfilePolicy::FallbackSrgb;

let request = ExportRequest::new(
    "slide.ndpi".into(),
    "out".into(),
    options,
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

- ICC handling is explicit. Missing source profiles default to synthesized sRGB;
  use `--icc strict`, `--icc fallback-display-p3`, or `--icc omit-if-missing`
  when a different policy is required.
- JPEG Baseline output preserves compatible native JPEG frames. HTJ2K lossless
  output rejects nonconformant color JPEG direct routes and falls back through
  decoded RGB/RCT.
- JPEG 2000 passthrough preserves eligible native source codestreams.
- Route profile and coverage JSON reports expose available frame counts,
  sampled frame percentages, route counters, pixel profiles, and GPU counters.
- Output names encode scene, series, level, Z, channel, and time coordinates;
  consumers must use report paths rather than assuming the pre-0.7 name shape.
- Passing validators is release evidence, not formal DICOM certification.

## Development

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

Before a `1.0 release candidate`, run these gates against published
dependencies and a representative real-slide corpus covering advertised routes,
metadata modes, ICC policies, validator checks, and any GPU route being
advertised.

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
provided.

## License

Dual-licensed under either [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE), at your option.
