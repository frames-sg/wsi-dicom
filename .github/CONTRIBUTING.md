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
`Export`, `ExportRequest`, transfer-syntax resolution, route
profiling, coverage reports, or metadata handling should update:

- README quick-start or examples when user-facing behavior changes
- API docs for affected public items
- integration tests covering caller-visible behavior
- release or validation docs only when the change affects published behavior

## Compatibility Policy

The MSRV is the `rust-version` declared in `Cargo.toml` and
`rust-toolchain.toml`. Raising the MSRV is a deliberate release note item and
should not be hidden inside unrelated changes.

The crate follows Semantic Versioning. Before 1.0, breaking API changes are
allowed only when they improve the stable contract that will be carried into
1.0. After 1.0, public Rust API changes must pass `cargo semver-checks
check-release` or be released as a major version.

JSON report fields emitted by CLI `--json` modes and serialized public report
types are treated as integration surfaces. Additive fields are preferred;
renames, removals, or semantic changes require changelog entries and migration
notes.
