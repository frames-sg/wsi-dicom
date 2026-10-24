<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# wsi-dicom 0.8.0 to 0.9.0 semver report

- Baseline: `0.8.0` at immutable commit `c44ab549caf6c7467854b0667c3dbfae3551fa56`.
- Baseline publication state: tagged as v0.8.0 and published to crates.io.
- Candidate: `0.9.0`.
- Tool: `cargo-semver-checks 0.51.0` with Rust `1.99.0` rustdoc JSON.
- Policy: CI requires the exact reviewed patch-level break set below; an empty set means the public API is compatible.

## Reviewed breaks

```text
  field htj2k_decoder of struct DoctorOptions, previously in file src/validation.rs:122
  field htj2k_decoder of struct ValidationOptions, previously in file src/validation.rs:72
  field max_pixel_frames of struct ValidationOptions, previously in file src/validation.rs:74
  variant ValidationProfile::Core2026c, previously in file src/validation.rs:54
```
