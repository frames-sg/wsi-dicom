# wsi-dicom Architecture

`wsi-dicom` is one crate with explicit internal boundaries. The public facade in
`src/lib.rs` declares modules and re-exports the supported API surface only.

## Module Map

- `api.rs`: builder-first Rust API, centered on `DicomExport`.
- `request.rs`: public request types for export, profiling, coverage, defaults, and frame encode.
- `report.rs`: public report and metric types returned by API calls.
- `defaults.rs`: source-aware transfer syntax selection entry point.
- `export.rs`: export orchestration and current shared implementation details.
- `profile.rs`: route profiling and coverage entry points.
- `passthrough.rs`: home for JPEG/JPEG 2000 passthrough planning helpers.
- `routing.rs`: home for encode route selection and fallback policy helpers.
- `gpu.rs`: home for Metal/CUDA-gated route cache and session helpers.
- `encode.rs`: DICOM-ready J2K/HTJ2K frame encoding.
- `writer.rs`: DICOM object construction and pixel-data writing.
- `metadata.rs`: metadata sources and DICOM conformance metadata mapping.
- `options.rs`: transfer syntax, backend, validation, and export option types.
- `tile.rs`: tile sample preparation and pixel-profile helpers.
- `uid.rs`: deterministic UID and instance path helpers.
- `error.rs`: crate error type.

## Dependency Direction

Public callers should use `api`, request/report types, and the exported advanced
functions. Internal implementation should flow inward:

```text
lib facade -> api/request/report/defaults/profile/export -> encode/writer/metadata/options/tile/uid/error
```

`statumen` owns source WSI detection, geometry, and tile reads. `wsi-dicom` owns
transfer-syntax selection, routing policy, metadata validation, DICOM output
layout, and writer errors.

## Public API Policy

The primary API is:

```rust
use wsi_dicom::DicomExport;

let report = DicomExport::from_slide("slide.ndpi")
    .to_directory("out")
    .run()?;
```

Advanced API entry points remain public for integrations and tests:

- `export_dicom(DicomExportRequest)`
- `profile_dicom_routes(...)`
- `profile_dicom_route_coverage(...)`
- `profile_dicom_route_corpus_coverage(...)`
- `encode_dicom_j2k_frame(...)`

## Invariant

Do not grow `src/lib.rs`. It should stay facade-sized: module declarations,
crate-level docs, constants, and public re-exports. New behavior belongs in the
owning module.
