<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->

# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.7.3] - 2026-08-25

### Fixed

- Enabled the root `--version` flag required by release archive verification.
- Made the packaged self-test unit test independent of host-installed external
  validators; the separate strict release validation continues to run them.
- Used a Basic Offset Table for single-frame instances so older validators can
  read them; multi-frame instances retain Extended Offset Tables.
- Included failed strict self-test records in publish workflow logs.

## [0.7.2] - 2026-08-25

Version `0.7.1` was an unpublished development baseline used for the reviewed
API comparison; its changes are carried forward below.

### Added

- Added first-class scanner calibration through a bounded, portable JSON
  registry. Matching requires exact, case-sensitive governed DICOM
  manufacturer, model, and serial metadata after outer-whitespace trimming;
  verified ICC bytes are retained in memory and local paths never enter DICOM
  output.
- Added `calibration inspect` and `calibration create`. Creation packages an
  existing vendor- or target-generated RGB input-device ICC profile and does
  not claim to derive calibration from an ordinary tissue slide.
- Added per-instance ICC SHA-256, calibration/profile ID, source, and conflict
  decision provenance. Effective profile digests participate in deterministic
  UID identity.
- Added stable `intrinsic-wsi-dicom-2026c-*` validation checks for ICC profiles,
  MONOCHROME2 presentation attributes, lossy declarations, specimen identity,
  dimension ordering, and set-level Specimen UID consistency. They run for VL
  WSI objects independently of external validators.
- Added optional governed Specimen UIDs and structured HL7 identifier issuers
  to `DicomMetadata`. FHIR `Specimen.identifier.system` maps to a universal URI
  issuer.
- Added a shared application workflow for metadata resolution, annotation
  preparation, export, validation, report persistence, progress, and
  stage-aware errors across CLI and GUI frontends.
- Added typed `SlideRouteCoverageRequest` and `CorpusRouteCoverageRequest`
  entry points while retaining `RouteCoverageRequest` as a compatibility
  wrapper.

### Changed

- This is a deliberate pre-1.0 API transition. `IccProfilePolicy` and
  `OmitIfMissing` were removed. Every `ExportRequest` and `Export` builder now
  requires one explicit `ColorManagement`; callers should migrate to
  `RequireSource`, a documented source/fallback choice, a calibration registry,
  or an explicit verified profile.
- Configured/source profile conflicts default to failure before staging or
  output creation. Callers may explicitly prefer the configured or source
  bytes; identical SHA-256 digests are recorded as a match.
- Upgraded the complete `j2k` codec family to 0.10 and `wsi-rs` to 0.6.0 from
  their crates.io releases so export, passthrough, transcode, and optional
  accelerator routes resolve one codec generation. Metal ownership now uses
  the shared `objc2-metal` API across the reader and exporter.
- Color output now requires a valid DICOM input-device ICC profile. Generated
  sRGB and Display P3 fallbacks identify as `scnr` and remain explicit research
  assumptions rather than scanner calibrations.
- Generated Specimen UIDs now include export identity and identifier issuer
  scope. Governed UIDs are preserved, fresh exports remain fresh, and
  deterministic exports remain repeatable.
- Source-aware `Export::run` now opens the source once and reuses the prepared
  slide for transfer-syntax selection and export.
- Export and profiling now consume one bounded route planner with explicit
  rejection and fallback decisions. Raw J2K inspection is shared per planning
  step, and validated options are normalized once into internal semantic,
  resource, execution, and GPU groups.
- CLI and GUI export paths now use the same application workflow; the GUI
  startup, state, mapping, worker, presenter, and view responsibilities are
  separated without changing the report contract.

### Fixed

- MONOCHROME2 WSI output now writes Presentation LUT Shape `IDENTITY`, Rescale
  Intercept `0`, and Rescale Slope `1` without emitting ICC data.
- Lossy compression history now survives lossless transcoding and retains
  ordered method/ratio stages independently of the final transfer syntax.
- Dimension Index Sequence and per-frame Dimension Index Values now use row
  then column, matching row-major frame production.
- Outputs produced before these fixes should be regenerated when they contain
  color or MONOCHROME2 pixels, previously lossy source pixels, or specimen
  identifiers whose issuer scope matters.

### Release engineering

- The protected release workflow can rehearse without publication and builds
  CPU-only CLI archives for Linux x86-64 GNU, macOS x86-64, macOS arm64, and
  Windows x86-64 MSVC. Archives contain the CLI, licenses, README, and version
  metadata alongside the exact crate candidate and `SHA256SUMS`.
- Release candidates receive per-artifact SPDX JSON SBOMs plus GitHub/Sigstore
  build-provenance and SBOM attestations from immutable action pins. The
  evidence manifest records the source commit, toolchain, feature set, lockfile
  digest, artifacts, validators, workflow identity, and approval state.
- crates.io OIDC remains confined to the protected publication job. A release
  stays draft until the published registry checksum is compared with the
  attested crate candidate and a human explicitly approves finalization.
- These attestations establish artifact origin; they do not turn the documented
  cargo-vet exemption baseline into audited dependency provenance.
- The standalone manifest now uses immutable HTTPS Git revisions for the
  unpublished compatible `wsi-rs` 0.6.0 and coherent J2K 0.10 family, while
  `wsi-dicom-annotations` remains an exact registry release. Main CI verifies
  locked metadata, standalone topology, and package contents without sibling
  checkouts. The protected publish workflow keeps packaging and extracted-crate
  tests release-blocking until registry sources replace the Git pins.
- Split the GDC benchmark harness into typed discovery, command, execution,
  preflight, validation, aggregation, reporting, and CLI modules while keeping
  the original script as a compatibility entry point.
- Added a CUDA-only require-device regression that performs real device encode
  and CPU round-trip validation. CUDA evidence builds document mandatory
  cuda-oxide PTX generation so a compile-only fallback cannot be mistaken for
  runtime support.

### Clinical limitations

- This release is not a clinical certification or deployment claim. Protected
  real-slide validation, PACS/viewer interoperability, GPU-hardware evidence,
  pathologist color assessment, clinical workflow validation, clean candidate
  CI, independent review, and explicit release approval remain required before
  clinical deployment.

### Security

- Hardened transaction recovery and persistent route-cache reads against
  symlink substitution, rejected duplicate recovery entries, and bounded the
  process-wide route cache.
- Added a workspace-wide RustSec audit gate and updated the vulnerable GUI
  transitive dependencies to patched releases.

### Changes carried forward from the unpublished 0.7.1 development baseline

#### Added

- Added bounded DICOM metadata preflight and write-time accounting. The default
  limits are 256 MiB per instance and 1 GiB per export; Rust callers can set
  `max_instance_metadata_bytes` / `max_total_metadata_bytes`, and the `convert`
  and `sustain-convert` commands expose `--max-instance-metadata-mib` /
  `--max-total-metadata-mib`.
- Added an intrinsic Pixel Data structure check to every validation run,
  independent of external tools and optional pixel decoding. It verifies frame
  count, native-versus-encapsulated representation, nonempty data, and bounded
  Basic/Extended Offset Table frame mapping.
- Added side-effect-free `DicomMetadata::validate_for_export` preflight for
  DICOM text, person-name structure, scalar delimiters and controls, and imaged
  volume depth.

#### Changed

- Per-frame functional groups are streamed as undefined-length sequences and
  items rather than assembled as one in-memory object list. Pixel Data frame
  offsets and lengths use a temporary disk-backed index that is replayed when
  patching the Extended Offset Tables.
- Non-ASCII metadata is emitted as UTF-8 with Specific Character Set
  `ISO_IR 192`; ASCII-only output does not add the declaration.
- Documented the exact flat-output transaction guarantee. Instances are staged
  together and promoted sequentially under a writer lock. The journal restores
  the prior generation after an ordinary commit failure and supports recovery
  after interruption, but concurrent readers are not given simultaneous
  generation visibility and can observe an in-progress promotion.

#### Fixed

- Corrected `imaged_volume_depth_mm`: Imaged Volume Depth is now an FL value in
  micrometers, while Slice Thickness remains a validated DS value in
  millimeters. Non-finite, non-positive, underflowing, and unrepresentable
  values are rejected before slide access or output staging.
- Outputs made by wsi-dicom 0.7.0 or earlier should be regenerated if they used
  imaged volume depth (including the 0.001 mm default) or non-ASCII metadata;
  those outputs may contain incorrect Imaged Volume Depth units or omit the
  required Specific Character Set declaration.

## [0.7.0] - 2026-07-14

### Changed

- Prepared the first public release after `0.2.0`. Versions `0.3.0` through
  `0.6.0` were unpublished development lines whose changes are consolidated
  into `0.7.0`.
- Generated UIDs are fresh by default; deterministic content/configuration
  identity is now an explicit opt-in.
- Instance filenames now include scene, series, level, Z, channel, and time.
- Export requests now stage the complete generation and use a journaled
  flat-file commit, restoring overwritten files if any commit step fails.

- Export metadata is now explicit. CLI, GUI, `Export`, and `ExportRequest::new`
  require either caller-provided metadata JSON/FHIR input or an explicit
  research-placeholder selection.
- Generated DICOM output refuses existing `.dcm` files by default. Set
  `ExportOptions::overwrite` or pass `--overwrite` to replace existing output.
- Validation and route-corpus walkers are bounded, refuse symlink traversal,
  and cap external validator output capture.
- FHIR Bundle metadata mapping now requires one `DiagnosticReport` anchor and
  maps only its referenced patient, specimen, and service request.
- Metal strip packing and tile composition now retain resident image ownership
  through command completion and reject unverified legacy raw-buffer tiles.

### Added

- Added cargo-vet scaffolding and CI enforcement, SHA-pinned GitHub Actions,
  verified Gitleaks downloads, fuzz smoke targets, and a Metal shader parameter
  layout regression test.

### Fixed

- Updated the exact `j2k` dependency set to `0.7.3`, whose corrected TLM
  descriptors make generated HTJ2K frames acceptable to conforming external
  decoders.
- Fixed Metal tile-composition source and destination addressing beyond 4 GiB
  with checked host-side spans and a 64-bit shader path, while retaining the
  validated 32-bit path for smaller compositions.
- Instance identity and paths now include scene, series, level, Z, channel, and
  time coordinates; synthetic multi-scene/multi-series export coverage prevents
  cross-instance collisions.
- External validation now assembles multi-fragment frames from Basic or Extended
  Offset Tables, verifies bounded decoder output geometry and payload size, and
  terminates timed-out process trees.
- Persistent route-cache and export-lock writes reject symlink destinations and
  use durable same-filesystem replacement.

### Removed

- Removed standalone `docs/*.md` files and consolidated public guidance into a
  shorter README.

### Changes carried forward from the unpublished 0.4 development line

### Added

- Added release-readiness packaging metadata and docs.rs feature coverage.
- Added CI coverage for Metal, GPU feature checks, benchmark internals, Python
  benchmark harness tests, scheduled advisory checks, and typo checks.
- Added pinned benchmark Python requirements for reproducible harness testing.
- Added focused `Export` builder setters for tile size, JPEG quality, ICC
  policy, backend/validation policy, source decode, decomposition levels, and
  GPU tuning knobs.
- Added `ExportPreset::options(tile_size, jpeg_quality)` for library-side
  preset option construction.

### Changed

- Used `0.3.0` through `0.6.0` only as unpublished in-repository development
  versions before consolidating the release at `0.7.0`.
- Pinned the coordinated `j2k` and `wsi-rs` dependency lines exactly so the
  pre-1.0 codec/reader graph cannot drift across incompatible APIs.
- Profile and route coverage metrics no longer report synthetic DICOM write
  duration for code paths that do not write DICOM files.
- Removed dead placeholder facade modules while preserving the public crate
  exports.
- Removed the duplicate `DicomExportConfig` wrapper; `ExportOptions` now
  serves as the serde-friendly export configuration type.
- Removed the duplicate `DicomValidationConfig` wrapper; `ValidationOptions`
  now serializes with `command_timeout_secs`.
- Grouped export metrics into route, direct JPEG-to-HTJ2K, GPU encode, and
  timing sub-structures while preserving the existing 93 flat JSON metric keys.
- Marked report and metric structs non-exhaustive and added defaults for report
  fixture construction.
- Merged route coverage and corpus coverage request configuration into
  `RouteCoverageRequest` with `RouteCoverageTarget` and replaced the
  duplicate progress enums with `RouteProgressSink`.
- Route profile and coverage requests now resolve source-aware transfer syntax
  defaults in the library unless `source_aware_transfer_syntax` is disabled.
- Removed `duration_as_reported_micros` from the public crate facade; it remains
  an internal report timing helper.
- The CLI now derives value parsing directly from the public library enums,
  removing mirror argument enums and surfacing enum documentation in help text.
- Consolidated repeated CLI encode flags behind one shared argument group and
  split command execution into focused subcommand handlers.
- Moved the large inline export test body into `src/export/tests/` while
  preserving the test list, and split the Metal auto-route cache into its own
  module.
- Split lossless JPEG 2000 route policy and planned-frame row construction
  into focused export submodules without changing the public API.
- Split the GUI export/validation option panel into focused rendering helpers.
- Shared the Python benchmark adapter in-process timing/error wrapper and
  removed the persistent highdicom JPEG 2000 temporary directory.
- Renamed stuttered public Rust API types to the shorter 0.4 names below;
  `DicomMetadata` intentionally keeps the domain term.

| Old name | New name |
| --- | --- |
| `WsiDicomError` | `Error` |
| `DicomExport` | `Export` |
| `DicomExportOptions` | `ExportOptions` |
| `DicomExportPreset` | `ExportPreset` |
| `DicomExportRequest` | `ExportRequest` |
| `DicomExportReport` | `ExportReport` |
| `DicomExportMetrics` | `ExportMetrics` |
| `DicomEncodedFrame` | `EncodedFrame` |
| `DicomInstanceReport` | `InstanceReport` |
| `DicomRouteProfileRequest` | `RouteProfileRequest` |
| `DicomRouteCoverageRequest` | `RouteCoverageRequest` |
| `DicomRouteProfileReport` | `RouteProfileReport` |
| `DicomRouteCoverageReport` | `RouteCoverageReport` |
| `DicomRouteCorpusCoverageFailure` | `RouteCorpusCoverageFailure` |
| `DicomRouteCorpusCoverageReport` | `RouteCorpusCoverageReport` |
| `DicomFrameSamples` | `FrameSamples` |
| `DicomJ2kFrameEncodeRequest` | `J2kFrameEncodeRequest` |
| `DicomValidationOptions` | `ValidationOptions` |
| `DicomValidationReport` | `ValidationReport` |
| `DicomValidationCheck` | `ValidationCheck` |
| `DicomValidationStatus` | `ValidationStatus` |
| `DicomDoctorOptions` | `DoctorOptions` |
| `DicomDoctorReport` | `DoctorReport` |
| `DicomDoctorTool` | `DoctorTool` |
| `DicomDoctorStatus` | `DoctorStatus` |
| `DicomSelfTestOptions` | `SelfTestOptions` |
| `DicomSelfTestReport` | `SelfTestReport` |

### Fixed

- Reworked the advanced request README example so it uses the public
  `ExportRequest::new` constructor instead of a non-exhaustive struct
  literal.
- Made source-checkout infrastructure tests skip cleanly from packaged
  tarballs where repository-only files are intentionally absent.
- Rejected VL WSI dimensions that exceed DICOM attribute ranges instead of
  silently narrowing them.
- Patched streamed extended offset tables using recorded element offsets
  instead of scanning serialized bytes.
- Drained validator child output while waiting for process exit to avoid pipe
  buffer deadlocks.
- Deduplicated transfer syntax UID routing and tightened JPEG error
  classification regression coverage.
- Propagated JPEG passthrough write-time source read errors instead of
  reclassifying them as route ineligibility.
- Made DICOM doctor probes execute lightweight tool commands and gated staged
  dicom3tools lookup behind debug builds or explicit opt-in.

### Changes carried forward from the unpublished 0.3 development line

### Added

- Added explicit ICC profile policy controls to the Rust API and `convert` /
  `sustain-convert` CLI commands: `strict`, `fallback-srgb`,
  `fallback-display-p3`, and `omit-if-missing`.
- Added per-instance ICC provenance reporting so manifests distinguish source
  metadata, embedded JPEG ICC, synthesized sRGB, synthesized Display P3, and
  omitted missing profiles.
- Added strict pre-1.0 release gates for missing docs, package-scoped coverage,
  public API compatibility checks, and CUDA/Metal feature builds.
- Added crate-owned `FrameSamples` for advanced per-frame encoding so the
  public API is not coupled to j2k sample enums.

### Changed

- Missing source ICC profiles now default to an explicitly reported synthesized
  sRGB ICC profile instead of an implicit writer fallback. Display P3 fallback
  is available only by opt-in policy.
- JPEG-backed HTJ2K export now rejects direct routes that would emit
  nonconformant VL WSI `YBR_FULL` output and falls back through decoded RGB/RCT
  for lossless HTJ2K.
- Release validation now uses the published crates.io dependency graph instead
  of local `[patch.crates-io]` overrides.
- Public option, metadata, validation, diagnostics, request, and error types
  are documented and marked non-exhaustive where future extension is expected.

## [0.2.0] - 2026-05-15

### Added

- Added the public `Export` builder for converting wsi-rs-readable
  whole-slide images into DICOM VL Whole Slide Microscopy output directories.
- Added source-aware default transfer syntax selection so eligible JPEG and
  JPEG 2000 source frames can be preserved without decode/re-encode.
- Added JPEG Baseline, JPEG 2000 Lossless, HTJ2K Lossless, and HTJ2K Lossless
  RPCL export paths, including per-frame route metrics and JSON-capable reports.
- Added profile, coverage, corpus coverage, and sustained-run CLI commands for
  route validation and throughput evidence.
- Added optional Metal/CUDA feature plumbing for JPEG 2000 encode acceleration
  and wsi-rs Metal tile decode integration on supported hosts.

### Changed

- Moved export behavior behind the `frames-sg/wsi-dicom` public repository and
  aligned dependencies with `wsi-rs` 0.3 and `j2k` 0.4.

## 0.1.0 - 2026-05-09

### Added

- Initial public DICOM VL Whole Slide Microscopy export crate and CLI.
- Added research-placeholder metadata support for early conversion workflows.
- Added DICOM object writing, deterministic UID/path construction, metadata
  validation, and JPEG 2000 / HTJ2K frame encoding primitives.
- Added passthrough-first planning for compatible compressed WSI source frames.

[Unreleased]: https://github.com/frames-sg/wsi-dicom/compare/v0.7.0...HEAD
[0.7.0]: https://github.com/frames-sg/wsi-dicom/compare/v0.2.0...v0.7.0
[0.2.0]: https://github.com/frames-sg/wsi-dicom/releases/tag/v0.2.0
