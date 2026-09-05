# WSI-DICOM Negative Bench: September 4, 2026 amendment

This prospective amendment preserves manifest v3, its expected-results lock, controls-v3,
the v2 public evidence seal, and the v2 publication freeze. It corrects the derived pyramid
control and validator contracts found in the subsequent audit. It is not an independent
blinded replication: cases and expected failures are authored with knowledge of the rules.

The primary cohort has 69 authored negative cases and 14 controls (15 objects), exercising
18 selected rule families in five reporting domains. Profile v2 contains 51 grouped challenge
rows. Link completeness measures only those declared rows; it does not establish exhaustive
coverage of the DICOM standard, all attributes in a module, or every conditional branch.

Scope is DICOM 2026c VL WSI VOLUME, TILED_FULL, one optical path, one focal plane, no extended
depth of field, and no concatenations. Positive controls include implicit frame positions,
MONOCHROME2 at 8 and 16 bits in Explicit VR Little Endian, and the previously specified color
transfer syntaxes. The 16-bit control does not cover every compressed syntax/bit-depth pair.
The derived level is a deterministic nearest-neighbor reduction of its referenced source,
with the required coded Derivation Image functional group. External validators do not replace
intrinsic checks of that conditional requirement.

Expectations, source hashes, catalog v4, and profile v2 are bound in expected-results-lock-v4.json
before evaluation object generation and execution. Existing five pixel-assembly cascades are
listed by exact rule ID in the manifest. Any additional intrinsic failure blocks release until
its cause is understood and an explicit adjudication is recorded in a subsequent amendment;
expected failures must not be changed merely to obtain a passing gate.

Run the generator and workbench using the commands recorded in the manifest and package README.
Use explicit `core-2026c` selection, required external validators dciodvfy, dcentvfy and
validate_iods (edition 2026c), and bounded decoder checks. Preserve tool stdout/stderr, return
codes, elapsed time, executable digests, Python validator runtime provenance, IOD definition
hashes, and input digests. A timeout, launch error, unavailable required tool, output truncation,
unmapped check, or incomplete check set is an execution error and cannot count as detection.

Controls must pass all required validators. Every negative must fail all expected intrinsic
rules and its declared domain. Finalization runs the complete evidence gate before creating
an exclusive checksum seal. Sealed packages are read-only: reanalysis uses summarize_challenge;
a corrected execution requires a separate package. Object and static-input bytes are
deterministic; runtime measurements and environment evidence are execution-specific and are
sealed separately, not claimed byte-identical between runs.

Report observed counts and fractions on the authored challenge. These are not clinical
sensitivity, prevalence, or estimates of error rates in deployed converters. Severity labels
are expert assessments, not measured patient outcomes. Sparse tiling, multiple paths/planes,
source fidelity, calibrated color accuracy, DICOMweb behavior, and compound defects remain
outside this evaluation. Earlier TCGA-derived evidence is retained as a historical cohort;
this amendment does not claim it was rerun under the corrected core profile.

Normative sources: DICOM PS3.3 2026c [WSI functional groups](https://dicom.nema.org/medical/dicom/current/output/chtml/part03/sect_A.32.8.4.html),
[WSI Image Module](https://dicom.nema.org/medical/dicom/current/output/chtml/part03/sect_C.8.12.4.html),
and [implicit frame order](https://dicom.nema.org/medical/dicom/current/output/chtml/part03/sect_C.7.6.17.3.html).
