<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->

# wsi-dicom 0.7.1 to 0.7.2 semver report

- Baseline: `0.7.1` at immutable commit `88c0dc357740cb6d344389449e01b008bb3f2649`.
- Baseline publication state: merged, but not tagged or published to crates.io.
- Candidate: `0.7.2`.
- Tool: `cargo-semver-checks 0.48.0` with Rust `1.96` rustdoc JSON.
- Policy: the patch-level API breaks below are intentional for this pre-1.0 transition; any different break set fails CI.

## Reviewed breaks

```text
  Export::icc_profile_policy, previously in file src/api.rs:117
  enum wsi_dicom::IccProfilePolicy, previously in file src/options.rs:67
  enum wsi_dicom::prelude::IccProfilePolicy, previously in file src/options.rs:67
  field icc_profile_policy of struct ExportOptions, previously in file src/options.rs:272
  variant IccProfileSource::OmittedMissing, previously in file src/report.rs:171
  wsi_dicom::ExportRequest::new takes 4 parameters in src/request.rs:28, but now takes 5 parameters
  wsi_dicom::prelude::ExportRequest::new takes 4 parameters in src/request.rs:28, but now takes 5 parameters
```
