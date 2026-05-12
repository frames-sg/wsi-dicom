# Contributing

Contributions should keep `wsi-dicom` focused on reliable DICOM VL Whole Slide
Microscopy export from `statumen` datasets.

## Development Setup

Use the Rust toolchain declared in `Cargo.toml`.

```sh
cargo fmt --all -- --check
cargo test --no-default-features
cargo clippy --no-default-features --all-targets -- -D warnings
cargo doc --no-default-features --no-deps
```

GPU routes are optional and should be tested on hardware that supports the
requested backend.

## Pull Requests

- Keep changes scoped to one export, routing, metadata, or documentation topic
  when possible.
- Add or update behavior-focused tests for API, routing, data-flow, or DICOM
  writing changes.
- Do not remove passing regression tests as cleanup.
- Avoid hardcoded secrets, credentials, local machine paths, and patient data.
- Surface unsupported inputs and backend failures explicitly; do not add silent
  fallback paths.
- Keep metadata policy explicit. Do not invent patient or clinical identifiers
  in examples outside the documented research-placeholder path.

## Public API Changes

Public export APIs are part of the slide conversion surface. Changes to
`DicomExport`, `DicomExportRequest`, transfer-syntax resolution, route
profiling, coverage reports, or metadata handling should update:

- README quick-start or examples when user-facing behavior changes
- API docs for affected public items
- integration tests covering caller-visible behavior
- `docs/architecture.md` when module ownership changes
