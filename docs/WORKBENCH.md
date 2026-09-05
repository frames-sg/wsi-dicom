# WSI-DICOM validation workbench

The workbench evaluates DICOM VL WSI objects with intrinsic checks, bounded pixel decoding,
and independent validators. The current public challenge contains 69 authored negative cases
and 14 controls across 18 selected rule families. It does not establish exhaustive normative
DICOM coverage or clinical sensitivity.

## Validate an input

```sh
cargo build --release --locked
.venv-bench/bin/python bench/wsi_dicom_bench.py /path/to/dicom \
  --output bench/results/workbench-check \
  --wsi-dicom target/release/wsi-dicom \
  --catalog rules/wsi-dicom-bench-rules-2026c-v4.json \
  --profile rules/wsi-dicom-core-profile-2026c-v2.json
```

The output directory must be new. Required validators are `dciodvfy`, `dcentvfy`, and
`validate_iods`; JPEG, JPEG 2000, and HTJ2K inputs also need their applicable decoders.
`--allow-missing-tools` permits exploratory runs with recorded limitations; such runs cannot
pass the benchmark release gate. `dcmvalidate` is used only with an explicit IOD configuration.

The Rust CLI/API offers `general` and `core-2026c` validation profiles. The general profile is
used with catalog v2 by the current format-coverage runner; core validation uses catalog v4
and profile v2. See the [catalog guide](../rules/README.md) and
[format-coverage commands](../bench/README.md#pinned-source-format-coverage).

## Current challenge inputs

| Input | Source |
| --- | --- |
| Case definitions and control inventory | `bench/negative_bench/manifest-v4.json` |
| Prospective protocol and expectation lock | `bench/negative_bench/protocol-v4.md`, `expected-results-lock-v4.json` |
| Challenge controls | `bench/negative_bench/controls-v4/` |
| Control-generation seeds | `bench/negative_bench/controls-v2/` |

Version suffixes identify inputs bound by checksums. The general catalog v2 and control seeds
v2 are active dependencies of current workflows. Superseded publication files, core-profile
catalogs, challenge manifests and document layouts are not maintained in the source tree.
Sealed result packages retain their own original inputs and executable Python runtime.

Generate, run, analyze and seal a fresh challenge:

```sh
.venv-bench/bin/python bench/negative_bench/generate.py \
  --manifest bench/negative_bench/manifest-v4.json --output bench/results/challenge-run
.venv-bench/bin/python bench/negative_bench/run.py \
  --package bench/results/challenge-run --workbench bench/wsi_dicom_bench.py \
  --wsi-dicom target/release/wsi-dicom
.venv-bench/bin/python bench/negative_bench/analyze.py --package bench/results/challenge-run
.venv-bench/bin/python bench/negative_bench/finalize.py --package bench/results/challenge-run
```

The generator verifies locked inputs and source digests before creating cases. To rebuild control
inventory for a future amendment, `build_core_profile_manifest.py` accepts the current manifest
as its case specification and writes a new output. It does not change expectations or overwrite
the current lock. Rebuilding controls uses the seed directory with `build_core_profile_controls.py`.

## Evidence and scoring

Workbench report v2 records per-instance findings, catalog/profile identities and hashes, input
hashes, commands, tool provenance, process outcomes, raw stdout/stderr, and domain summaries.
Timeouts, launch/configuration failures, missing required tools, output truncation, unmapped
checks and incomplete per-instance check sets are execution errors and cannot count as detection.
A negative case must fail every expected rule and its declared domain. Extra intrinsic failures
require explicit adjudication. Controls must pass every required validator.

`validate_iods` receives `--edition 2026c`. Its supported Python launcher exposes one absolute
interpreter in its shebang. Provenance includes that interpreter, dicom-validator and pydicom
versions/source hashes, and the local 2026c JSON/XML definition hashes. The finalizer requires
complete matching evidence before creating an exclusive checksum seal. Analysis refuses to
write into sealed packages; `summarize_challenge` recomputes metrics without writes.

The retained public challenge detected and localized 69/69 defects and accepted 14/14 controls.
The independent detection counts were 43/69, 3/69 and 29/69 for dciodvfy, dcentvfy and validate_iods.
The separate historical TCGA cohort remains part of the current publication and is not pooled
with the public denominator or claimed to have been rerun under the current core profile.
Publication sources and retained study results are maintained separately from this
software repository. Verify their sealed evidence before making publication claims.

## Bounds and limitations

`wsi-dicom validate --max-input-bytes` applies an encoded-input limit before parsing (default
1 GiB); intrinsic and decoder checks reuse the parsed object. This is not a hard heap limit.
External set-level validators operate in 512-file chunks; intrinsic corpus checks span the
entire discovered set. Core scope is VOLUME, TILED_FULL, one optical path and focal plane,
without extended depth of field or concatenations. Optional implicit TILED_FULL representations
are supported. Grouped profile rows do not enumerate every normative attribute or condition.

Source fidelity, calibrated color accuracy, sparse tiling, all compressed 16-bit combinations,
DICOMweb behavior and clinical utility remain outside the challenge claims. Publication availability,
authorship, ethics and redistribution decisions require author confirmation; no public deposit
is established by these local artifacts. Durable design decisions are in the
[architecture records](architecture/README.md).
