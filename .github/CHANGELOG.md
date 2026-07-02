<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->

# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Export metadata is now explicit. CLI, GUI, `Export`, and `ExportRequest::new`
  require either caller-provided metadata JSON/FHIR input or an explicit
  research-placeholder selection.
- Generated DICOM output refuses existing `.dcm` files by default. Set
  `ExportOptions::overwrite` or pass `--overwrite` to replace existing output.
- Validation and route-corpus walkers are bounded, refuse symlink traversal,
  and cap external validator output capture.
- FHIR Bundle metadata mapping now requires one `DiagnosticReport` anchor and
  maps only its referenced patient, specimen, and service request.

### Added

- Added cargo-vet scaffolding and CI enforcement, SHA-pinned GitHub Actions,
  verified Gitleaks downloads, fuzz smoke targets, and a Metal shader parameter
  layout regression test.

### Fixed

- SOP Instance UID generation now includes the timepoint axis so otherwise
  identical `t` contexts produce distinct UIDs and output paths.

### Removed

- Removed standalone `docs/*.md` files and consolidated public guidance into a
  shorter README.

## [0.4.0] - 2026-06-09

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

- Bumped the crate to `0.4.0`; `0.3.0` was an unpublished in-repository
  development version.
- Relaxed exact `j2k` and `wsi-rs` dependency pins to compatible
  published release requirements while preserving the current lockfile graph.
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

## [0.3.0] - 2026-05-27

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

## [0.1.0] - 2026-05-09

### Added

- Initial public DICOM VL Whole Slide Microscopy export crate and CLI.
- Added research-placeholder metadata support for early conversion workflows.
- Added DICOM object writing, deterministic UID/path construction, metadata
  validation, and JPEG 2000 / HTJ2K frame encoding primitives.
- Added passthrough-first planning for compatible compressed WSI source frames.

[Unreleased]: https://github.com/frames-sg/wsi-dicom/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/frames-sg/wsi-dicom/compare/v0.2.0...v0.4.0
[0.3.0]: https://github.com/frames-sg/wsi-dicom/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/frames-sg/wsi-dicom/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/frames-sg/wsi-dicom/releases/tag/v0.1.0
