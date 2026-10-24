<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# wsi-dicom

[![CI](https://github.com/frames-sg/wsi-dicom/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/frames-sg/wsi-dicom/actions/workflows/ci.yml?query=branch%3Amain)

[Project website](https://frames-sg.com/dicom-whole-slide-conversion/) ·
[API documentation](https://docs.rs/wsi-dicom)

`wsi-dicom` converts whole-slide imaging files that `wsi-rs` can open into
DICOM VL Whole Slide Microscopy instances. It provides a Rust API and a CLI.

[J2K, the pure-Rust JPEG 2000 codec](https://frames-sg.com/j2k/rust-jpeg2000-codec/),
supplies JPEG, JPEG 2000, and HTJ2K codec primitives. `wsi-rs`
opens vendor WSI formats such as SVS and NDPI. `wsi-dicom` owns DICOM export,
metadata validation, transfer-syntax routing, reports, and writer errors.
`wsi-dicom-annotations` owns the UI-independent QuPath/GeoJSON terminology and
DICOM ANN, SEG, and SR conversion; this package orchestrates it alongside WSI
conversion.

DICOM parsing and serialization use [dicom-rs](https://github.com/Enet4/dicom-rs).
ICC validation uses `moxcms`; LZW, DEFLATE, and Zstd use `j2k-tilecodec`. Native
CZI reading belongs to `wsi-rs`. The reader uses the `image` crate for PNG/BMP
decoding, public RGB/RGBA display buffers, and image saving.

## Validation benchmark

The validation challenge is maintained in
[`wsi-dicom-bench`](https://github.com/frames-sg/wsi-dicom-bench). Its v7 suite
contains 66 controls and 861 file, JSON, HTTP and advisory cases, with witnesses
for all 886 cataloged checked statements. Seven requirements remain unimplemented
across 16 IOD scope pairs, so the full-coverage acceptance policy fails even when
the implemented checks satisfy every case. Earlier sealed studies remain
historical evidence; this challenge does not claim certification or exhaustive
normative DICOM coverage.

## Install

Install the [latest published release](https://crates.io/crates/wsi-dicom) with:

```sh
cargo install wsi-dicom
```

Use the Rust API:

```toml
[dependencies]
wsi-dicom = "0.8.0"
```

GPU support is opt-in:

```toml
[dependencies]
wsi-dicom = { version = "0.8.0", features = ["metal"] } # macOS
# or
wsi-dicom = { version = "0.8.0", features = ["cuda"] } # CUDA-capable Linux
```

This source tree requires Rust 1.99.0 and targets J2K 0.12.0, `wsi-rs` 0.8.0,
and `wsi-dicom-annotations` 0.2.1. Release builds resolve these dependencies
from crates.io:

```sh
cargo build --release --locked
```

The commands and APIs below describe 0.8.0; the 0.7.5 API has
[versioned documentation](https://docs.rs/wsi-dicom/0.7.5).
Frames that source coverage metadata identifies as absent are omitted, and the
instance is written as `TILED_SPARSE` with explicit frame positions. Acquired
black tiles are retained. Gaps within a partially covered frame remain black
padding; other non-opaque pixels require an explicit composite policy.
dicom3tools `dciodvfy` currently reports a false Number of Frames error for
`TILED_SPARSE` output (it applies the `TILED_FULL` formula); validation reports
that error as `dciodvfy` emits it.

Feature flags:

| Feature | Effect |
| --- | --- |
| `default` | CPU-only DICOM export. |
| `cuda` | Enables CUDA JPEG 2000 encode acceleration and propagates the wsi-rs CUDA feature. Direct JPEG-to-HTJ2K CUDA transcode is not exposed. |
| `metal` | Enables Metal JPEG 2000 encode acceleration on macOS, Metal codestream validation decode, and wsi-rs Metal tile decode plumbing. |

CUDA release and hardware-evidence builds should require cuda-oxide PTX
generation instead of accepting the dependency's compile-time fallback. Install
`libclang`, select the GPU architecture, and make a missing PTX build fatal; for
example, an RTX 4070-class device uses `sm_89`:

```sh
J2K_CUDA_OXIDE_ARCH=sm_89 J2K_REQUIRE_CUDA_OXIDE_BUILD=1 \
  cargo test --features cuda --lib --locked
```

Set `LIBCLANG_PATH` when `libclang` is not discoverable through the platform's
normal library paths. `--backend require-device` remains the runtime fail-closed
check: it does not silently substitute CPU encoding when CUDA is unavailable.

For local maximum CPU throughput:

```sh
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

## Quickstart

Always provide metadata JSON/FHIR input or explicitly select research
placeholder metadata.

```sh
wsi-dicom convert slide.ndpi --out dicom-out --research-placeholder \
  --icc source-or-srgb
```

If a proprietary reader can decode pixels but cannot surface physical calibration, provide a
governed level-zero spacing explicitly in DICOM row/column order. The converter rejects non-positive
values and rejects a supplied value that conflicts with calibration already present in the source:

```sh
wsi-dicom convert slide.vsi --out dicom-out --research-placeholder \
  --source-pixel-spacing-mm 0.00034605325860336383,0.0003460559834973875
```

To convert a QuPath-annotated slide in the same operation, export the QuPath
objects as GeoJSON and provide an explicit mapping from every QuPath class name
to DICOM coded concepts:

```sh
wsi-dicom convert slide.ndpi --out dicom-out --research-placeholder \
  --qupath-annotations slide.geojson \
  --annotation-mapping examples/qupath-neoplasm-mapping-v1.json \
  --annotation-target ann --annotation-target seg
```

The mapping is required: `wsi-dicom` never guesses terminology or adds QuPath
classes to a global profile. Copy the example and add entries keyed by the
exact QuPath classification names used in the GeoJSON. The default
`level0-pixels` coordinate space matches QuPath's full-resolution image
coordinates. `source-pixels` is for coordinates already expressed on the
generated DICOM image grid, and `slide-mm` is for physical slide coordinates.

Annotation output is written under `dicom-out/annotations/` with a manifest and
one verified sidecar per requested target. Use ANN for directly representable
points and simple polygons, SEG for rasterized regions, and SR for mapped
measurements. Lossy omission or conversion is rejected unless
`--allow-lossy-annotations` is supplied explicitly. The WSI instances are
published first; if annotation conversion then fails, the error identifies the
failure and the already completed WSI export remains valid.

Use `--metadata metadata.json` for real metadata. `--metadata` and
`--research-placeholder` are mutually exclusive. Existing generated `.dcm`
paths are refused by default; pass `--overwrite` only when replacement is
intentional. Each conversion writes its complete set to a sibling staging
directory, then promotes the flat `.dcm` files sequentially under an
output-directory writer lock. The journaled commit is failure-atomic for
ordinary reported errors: newly promoted files are removed and overwritten
files are restored if commit fails. It is not visibility-atomic for concurrent
readers, which may observe a mixed set or a temporarily absent overwritten file
during promotion. An interrupted transaction is recovered before the next
export; a failed rollback returns a recovery-required error and retains its
journal and backups.

Metadata is validated before slide access and output staging. Non-ASCII text is
encoded as UTF-8 and declares DICOM Specific Character Set `ISO_IR 192`; scalar
text rejects DICOM value delimiters and control characters. The
`imaged_volume_depth_mm` input is always millimeters. It is converted to
micrometers for Imaged Volume Depth (FL) and remains millimeters for Slice
Thickness (DS).

> [!IMPORTANT]
> Regenerate output produced by wsi-dicom 0.7.0 or earlier if it used imaged
> volume depth (including the 0.001 mm default) or non-ASCII metadata. Those
> versions could write the depth with the wrong Imaged Volume Depth units or
> omit the required Specific Character Set declaration.

Per-frame functional-group and extended-offset-table metadata is bounded by
default to 256 MiB per instance and 1 GiB across one export. Trusted large
workloads can override these limits for `convert` and `sustain-convert`:

```sh
wsi-dicom convert slide.ndpi --out dicom-out --metadata metadata.json \
  --max-instance-metadata-mib 512 --max-total-metadata-mib 2048
```

Library callers can set `ExportOptions::max_instance_metadata_bytes` and
`ExportOptions::max_total_metadata_bytes`, or use the corresponding `Export`
builder methods. The writer preflights both limits and enforces the per-instance
limit while streaming metadata.

Generated DICOM UIDs are fresh for each conversion. Reproducible pipelines may
opt into full source-content/configuration identity with
`--uid-policy deterministic`; this hashes the complete source and is therefore
more expensive on large slides. The deterministic identity also includes the
SHA-256 digest of the effective ICC profile for each generated instance.

`DicomMetadata::specimen_uid` accepts a governed Specimen UID and preserves it
verbatim across exports. When it is absent, the exporter derives the Specimen
UID from the export identity, specimen identifier, and structured identifier
issuer. Fresh exports therefore receive fresh fallback Specimen UIDs,
deterministic exports repeat them, and issuer namespaces keep otherwise equal
local identifiers separate. FHIR `Specimen.identifier.system` is emitted as a
universal issuer of type `URI`.

## Scanner calibration

Every export requires an explicit color-management choice. The CLI defaults to
`--icc source-or-srgb` for research use; calibrated workflows should select
`--icc require-source`, a governed calibration registry, or an explicit
profile. Registry and explicit-profile modes are mutually exclusive:

```sh
wsi-dicom convert slide.ndpi --out dicom-out --metadata metadata.json \
  --icc-calibration-registry calibration/registry.json \
  --icc-conflict fail

wsi-dicom convert slide.ndpi --out dicom-out --metadata metadata.json \
  --icc-profile vendor-profile.icc --icc-profile-id lab-at2-2026q3 \
  --icc-conflict prefer-configured
```

Portable registries use schema version 1:

```json
{
  "schema_version": 1,
  "calibrations": [
    {
      "id": "lab-at2-sn123-2026q3",
      "scanner": {
        "manufacturer": "Leica Biosystems",
        "model_name": "Aperio AT2",
        "device_serial_number": "SN123"
      },
      "icc_profile": "profiles/profile.icc",
      "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    }
  ]
}
```

Scanner matching uses the governed DICOM manufacturer, model name, and device
serial number. All three values are required; outer whitespace is trimmed and
the remaining text is compared exactly and case-sensitively. Filenames and raw
vendor properties are not matching inputs. Registries reject unknown fields,
duplicate IDs or scanner tuples, malformed hashes, unsafe paths, missing or
checksum-mismatched files, and profiles that are not valid RGB input-device ICC
profiles. Registry JSON is limited to 1 MiB and 256 entries; each ICC file is
limited to 16 MiB. Relative profile paths resolve from the registry directory,
and only verified bytes—not local paths—enter conversion state or DICOM output.

When configured and source ICC digests differ, `--icc-conflict fail` stops the
export before staging or output creation. `prefer-configured` embeds the
configured bytes; `prefer-source` retains the source bytes. Equal digests are
accepted as a match. Each instance report records the effective SHA-256 digest,
profile or calibration ID, source, and conflict decision. MONOCHROME2 output
does not write ICC data and reports calibration as not applicable.

Inspect an existing profile or package it into a local portable bundle:

```sh
wsi-dicom calibration inspect --icc vendor-profile.icc
wsi-dicom calibration create --icc vendor-profile.icc \
  --id lab-at2-sn123-2026q3 \
  --manufacturer "Leica Biosystems" --model "Aperio AT2" --serial SN123 \
  --out calibration
```

`calibration create` validates and packages an existing vendor- or
target-generated profile. It does not derive scanner calibration from an
ordinary tissue slide. Calibration selection is available through the CLI and
Rust API.

The default conversion preset is `lossless-review`, which emits HTJ2K Lossless
RPCL. For explicit JPEG Baseline output:

```sh
wsi-dicom convert slide.ndpi --out dicom-fast --research-placeholder \
  --icc source-or-srgb --preset fast-jpeg
```

Useful operational commands:

```sh
wsi-dicom doctor --strict --json
wsi-dicom self-test --json --out self-test-evidence --keep-output
wsi-dicom validate dicom-out --strict --json
wsi-dicom coverage slide.ndpi --json
wsi-dicom requirements --json
```

For the cataloged DICOM checks and one per-slide evidence bundle, install
the independently versioned [WSI-DICOM Bench](https://github.com/frames-sg/wsi-dicom-bench) package:

```sh
python -m pip wheel \
  "git+https://github.com/frames-sg/wsi-dicom-bench@08f848d217849dbf5013eb429e7a6a129fc14eda" \
  --wheel-dir .benchmark-wheel
python -m pip install .benchmark-wheel/wsi_dicom_bench-*.whl
catalog="$(python -c 'from importlib.resources import files; print(files("wsi_dicom_bench").joinpath("rules/wsi-dicom-bench-rules-v7.json"))')"
wsi-dicom-bench-workbench dicom-out \
  --output evidence/slide-id \
  --wsi-dicom target/release/wsi-dicom \
  --catalog "${catalog}"
```

To run the locked v7 challenge from this checkout, select its manifest explicitly:

```sh
suite="$(python -c 'from importlib.resources import files; print(files("wsi_dicom_bench").joinpath("negative_bench/manifest-v7.json"))')"
cargo xtask benchmark --suite "${suite}" --output evidence/challenge-v7
```

The command builds the candidate with DICOMweb support. The current v7 completeness
policy returns exit code 1 for the seven documented unchecked requirements, even when
all implemented checks meet the locked expectations.

Validation never runs an external codec. Every validation run performs
intrinsic checks before the optional external DICOM conformance validators
(dicom3tools `dciodvfy`/`dcentvfy`, `validate_iods`, and `dcmvalidate` when an
IOD file is given). The Pixel Data structure check verifies Number of Frames,
native versus encapsulated representation as each Transfer Syntax requires,
nonempty data, the Native length, and bounded frame mapping through Basic or
Extended Offset Tables. The `encoding` check re-reads every file's Data Set
from its bytes: element order and uniqueness, VR and reserved bytes, Value
Lengths and trailing bytes, the encapsulated Pixel Data VR, Fragment Item
lengths and delimiters, Group Lengths, private blocks and Private Creators,
Specific Character Set and the repertoire of text Values, Person Name
component groups and deflated Data Sets. The `digital-signatures` check
recomputes each Digital Signature's MAC and verifies its RSA signature and
Certified Timestamp; whether its certificate is trusted is not checked. VL
Whole Slide Microscopy Image objects additionally run stable
`intrinsic-wsi-dicom-*` checks for ICC profiles, monochrome
presentation attributes, lossy declarations, specimen identity, dimension
indices and ordering, the WSI image constraints the tables cannot express
(`wsi-image`: four-valued Image and Frame Type, Specimen Label in Image, Tiles
Overlap, Imaged Volume Depth, slide-corner Frame of Reference, optical path
identifiers, lossy derivation, pixel spacing and the regular tiling), pixel
description content (`pixel-attributes`: Pixel Padding, Pixel Aspect Ratio,
Icon Images and palette tables), the codestream headers of each encapsulated
Frame against the Image Pixel attributes and the Transfer Syntax (`codestream`:
JPEG, JPEG-LS, JPEG 2000, HTJ2K, JPEG XL and RLE; headers are read, nothing is
decoded) and the constraints between Attributes in every Data Set and Item
(`attribute-relations`: Selector Attribute macro, Context Identifiers,
Referenced Frame Numbers, identification Items against names, calibration
dates, patient orientation and laterality, specimen preparation order, Color
Space against the ICC Profile primaries, signed element lists, Original
Attributes, measured values and reference SOP Classes). A validated directory
also runs set-level checks: Specimen UID consistency and `series-set` (one
Frame of Reference per Series and per slide, UID roles, Instance Numbers,
Concatenation agreement and numbering, Dimension Organization UIDs, localizer
Frames of Reference, Specimen Identifiers within a Study, Frame Extraction
history and patient group arrangements).

They also run the VL WSI IOD requirements generated from the pinned DICOM
edition's tables (PS3.3 A.32.8 and the module and macro tables it includes, PS3.6
data dictionary, PS3.10 file meta): `iod-modules` (module and functional group
usage, Per-Frame item count), `iod-attribute-presence` (Type 1, 1C, 2 and 2C,
recursively in Sequence Items, with each condition evaluated), `iod-enumerated-values`,
`iod-vr-vm`, `iod-value-format` (PS3.5 6.2), `iod-sequence-items` (including
Type 3 Sequences present with zero Items), `iod-value-sets` and `file-meta`
(including Presentation Addresses). `iod-value-sets` checks coded entries
against the PS3.16 Context Groups and Templates bound in the tables: a code
outside a Defined non-extensible group, or a Content Item tree that departs
from a Defined Template, fails; codes outside a Defined extensible group (Context
Group Extension Flag not `Y`) or a Baseline group, and departures from a
Baseline Template, are advisories. Conditions that depend on facts an object
cannot carry are counted as undeterminable rather than failed. With
`--optional` (the default), the JSON report's `optional` lists, for each
object, the optional modules, functional groups and attributes it encodes
(`present`) and lacks (`absent`), and the suggested codes and templates it
does not follow (`advisories`); none of them are failures. Most absent ones
need facts a slide file does not hold (clinical trial, physician or procedure
details). `--required` skips the list.

The IOD requirements apply to the pathology objects, chosen by SOP Class UID
as PS3.4 Table B.5-1 assigns it: VL Whole Slide Microscopy Image,
Segmentation (including Label Map Segmentation), Parametric Map, Microscopy
Bulk Simple Annotations, the Key Object Selection Document, and Basic Text,
Enhanced, Comprehensive and Comprehensive 3D SR; a DICOMDIR is held to the
Basic Directory IOD. An object of another SOP Class is reported as one to
which no IOD rules apply.

`wsi-dicom requirements` prints the catalog of the normative statements of
PS3.3, PS3.5, PS3.10, PS3.15 Annex E and PS3.18 for those objects, the
File-set and DICOMweb, each with its strength (`shall`, or `should` for a
recommendation, which is only ever an advisory), the IODs it applies to and its
disposition for each: enforced by the generated tables, the intrinsic check
that enforces it, the check family it awaits, or why an encoded object cannot
be held to it (another IOD or encoding, application behavior, or a fact the
bytes cannot show). `--json`
emits the `wsi-dicom-requirements-v1` schema;
`wsi_dicom::requirement_catalog()` returns the same entries.
Single-component MPEG2 can represent a monochrome WSI or Segmentation.
Its declared pixel attributes and encapsulation are checked, but MPEG2 stream
headers, container contents and audio restrictions are not yet validated.
The catalog marks those five requirements as unchecked. Byte-order validation
of network JPIP Data Sets also remains unchecked: JPIP cannot be stored in a
Part 10 file, and JSON metadata carries no Data Set byte encoding. The
completeness gate remains blocked until these requirements are implemented.
`scripts/generate-iod-tables.py --check` verifies the generated tables,
dictionary, requirement catalog and value sets against the pinned DocBook
sources. These checks do not decode pixel
values or infer unknowable compression history predating the object. Missing
external tools are reported as skipped unless `--strict` is set. Directory
validation is bounded by file count, depth, timeout, and child
output capture limits; symlink traversal is refused.

Diagnostics withhold identifying Attribute Values, including Study, Series
and Instance UIDs, using PS3.15 Table E.1-1. SOP Class and Transfer Syntax
UIDs remain visible. External-validator lines that name an identifying
Attribute and their continuations are withheld; DICOMweb reports also
withhold server-provided URI references and Warning text. This filtering
does not de-identify input paths, command arguments or unrecognized external
text.

`wsi-dicom validate-json metadata.json` holds a DICOM JSON document (PS3.18
Annex F), such as a saved Retrieve Metadata response, to the JSON Model and
each Data Set to the same checks as a file; checks that need Pixel Data, the
PS3.5 encoding or the File Meta Information report that they cannot run, a
check that depends on the Transfer Syntax the Metadata does not name runs with
each one of PS3.18 Table 8.7.3-2 and is undecidable when they disagree
(`--transfer-syntax` names it), and `--fetch-bulkdata` retrieves BulkDataURI
Values. With a JPIP Referenced Transfer Syntax, the Provider URL and exclusion
of inline Pixel Data are checked directly from the metadata.

`wsi-dicom dicomweb https://host/dicomweb [--study UID]` checks a
DICOMweb origin server for the transactions a slide viewer uses: Retrieve
Capabilities, Study, Series and Instance Search with matching, includefield
and paging, Series and Instance Metadata (JSON and XML), Frames in their own
and in uncompressed media types, Bulk Data, Rendered Frames, Thumbnails and
Instance retrieval, with content negotiation, character sets, Transfer Syntax
defaults, the Rendering Query Parameters, the matching parameters and
attributes, the Mandatory Search resources, Range, and the response message
and header fields of every response; each check cites PS3.18, missing optional
capabilities are `unsupported`, a recommendation not followed is an advisory,
never a failure, and the Metadata, Frames and Instance retrieved are validated
as files are. Store runs only with `--allow-store <FILE>`, and the stored copy
fails only checks the file stored passes. The two requests that ask for a 401
and a 405, one with an invalid bearer token and a TRACE, run only with
`--probe-error-responses`, since either can raise alerts on a server. Requests have
per-request timeouts, response-size limits and a request budget, follow
redirects only to the same origin, and send the bearer token held by the
environment variable `--bearer-token-env` names to the base URL's origin only.
`dicomweb` and `--fetch-bulkdata` need the network stack of the `dicomweb`
feature (`cargo install wsi-dicom --features dicomweb`), which the release
archives include; without it they report that the build has no DICOMweb
transport.

## Convention profiles

DICOM conformance is one question. Whether objects meet the conventions a
partner requires, and whether common viewers and archives will read them, are
two more. `validate --conventions` answers them separately. Select profiles
comma-separated or by repeating the flag:

```sh
wsi-dicom validate slide-dir --conventions wg26,ihe-dpia --conventions interop --json
```

| Profile | What it checks | Source |
|---|---|---|
| `wg26` | the nine checks `dciodvfy -profile WG26SP2025` adds (TILED_FULL, one focal plane, no Concatenation UID, Specimen Type, Specimen Preparation and Primary Anatomic Structure present, 2D annotations; missing Acquisition UID and Pyramid UID are warnings, as in dciodvfy), and the connectathon requirements: one Series per scan, a LABEL or OVERVIEW image with Barcode Value, true color brightfield, baseline JPEG or JPEG 2000 (the top layer, LABEL and OVERVIEW may be uncompressed), a pyramid, TILED_FULL without Per-Frame Functional Groups, one Z-plane, no Concatenations and one instance per layer, the ICC Profile in the single Optical Path Item, coded fixation, embedding and staining, SNOMED CT Specimen Type, and annotations sharing the images' patient | DICOM WG-26 Digital Pathology Connectathon Technical Requirements 2026 (2026/07/31); dicom3tools 1.00.snapshot.20260927113651 `libsrc/standard/module/vl.tpl:700-719, 1614-1616` |
| `ihe-dpia` | Appendix B: Request Attributes Accession Number, Issuer and Study Instance UID equal the top level; collection, fixation, embedding and staining steps with their codes, the Specimen Identifier and Issuer in every step, in chronological order; a coded Specimen Type; Patient's Sex from PID-8; one Accession Number per Study | IHE PaLM DPIA Rev. 1.4 (Trial Implementation, 2026-03-06) |
| `ihe-dpia-draft` | True Color Tiled Full Single Z Plane Pyramid Option (true color, pyramid, TILED_FULL, one pyramid and Z-plane, one Acquisition UID per Series, shared Frame of Reference and Pyramid UID, no Concatenations, a Basic or Extended Offset Table) and the Acquisition Complete Option's KOS (new Series, whole images, the images' identity, every image of the scan) | IHE PaLM CP-LAB-279 and CP-LAB-281, draft (CP under ballot) |
| `ihe-dpia-draft-jpeg` | JPEG Storage Option: baseline JPEG pyramid layers | IHE PaLM CP-LAB-279, draft (CP under ballot) |
| `interop` | advisories only: what dicom-microscopy-viewer 0.49.5, OpenSlide 4.0.1, Slim 0.46.10, the Orthanc WSI plugin 4.0, wsidicom 0.36.1, highdicom 0.28.2, the IDC index, the Google Cloud Healthcare API and dciodvfy's base checks reject or warn about | each advisory names the tool, version and source lines |

Convention results never enter `checks` and do not make validation fail
unless `--fail-on-conventions` asks them to. The JSON report adds a
`conventions` object (absent when no profile is selected) with the selected
`profiles`, one `checks` entry per check and object or Series it applies to
(`passed`, `failed`, or `skipped` with the reason when the object lacks what
the check reads), and `advisories`, listed only where they apply. Each entry
is named `convention-<profile>-<topic>` and carries its `source`; messages of
the draft profiles say "draft (CP under ballot)". The text summary prints the
counts, each check that did not pass and each advisory. `wsi-dicom
requirements --json` lists every convention, with its kind, source and what
it requires, under `conventions`, apart from the DICOM `requirements`;
`wsi_dicom::convention_catalog()` returns the same entries, and
`ValidationOptions::conventions` selects profiles from the Rust API.
`validate-json` and `dicomweb` (on the Series Metadata) take `--conventions`
and `--fail-on-conventions` too, and report skipped the checks that need the
Transfer Syntax or encoded Pixel Data the Metadata lacks;
`--fail-on-conventions` (`ValidationOptions::fail_on_conventions`) makes a
failed convention check, never an advisory, fail validation. The
connectathon's
Patient's Name scheme (`26CN^...`) is event logistics and is not checked; its
DIMSE, DICOMweb and HL7 requirements need a transaction, not an object.

### Conflicts between sources

DICOM normative text is followed over informative text and conventions, and
each conflict is reported as an advisory of the profile it affects:

| Conflict | Position | Advisory |
|---|---|---|
| DPIA maps order numbers into the Request Attributes Sequence, which the Request Attributes Macro does not define | the DICOM checks decide conformance; `ihe-dpia` does not require the order numbers | `convention-ihe-dpia-order-numbers-standard-extended` |
| CP-LAB-281 wants Specimen information in the completion KOS; the KOS IOD has no Specimen Module until CP-2656 | Patient and Study identity are required; Specimen Attributes are compared only where present | `convention-ihe-dpia-draft-completion-specimen-standard-extended` |
| DPIA's SPM-4 uses HL7 Table 0487; the Specimen Type Code Sequence's Baseline CID 8103 and the connectathon use SNOMED CT | `wg26` requires SCT; `ihe-dpia` accepts any coded type and notes other schemes | `convention-ihe-dpia-specimen-type-coding` |
| PS3.17 NN.6.2 describes preparation steps newest first; PS3.3 C.7.6.22.1.3 requires ascending chronological order | PS3.3: steps out of order fail `convention-ihe-dpia-step-order` | `convention-ihe-dpia-preparation-order-conflict` |
| The PS3.3 A.1.2.30 Note puts LABEL and OVERVIEW images in a pyramid's Series only if they share its Frame of Reference; Table A.32.8-1 (CP-2406) does not require one for them | Table A.32.8-1: no DICOM or convention failure | `convention-interop-associated-frame-of-reference` |
| PS3.15 Table E.3.4-1 (Clean Structured Content) removes the step Specimen Identifier and Issuer that TID 8001 and DPIA put in every step | for an object declaring Patient Identity Removed YES with (113104, DCM), the step identifier check is skipped | `convention-ihe-dpia-deidentified-step-identifiers` |
| Philips writes Photometric Interpretation XYB for JPEG XL; C.8.12.4.1.5 does not list it for VL WSI | the Enumerated Values: DICOM validation fails it | `convention-interop-xyb-photometric` |

### Changes after the pinned edition

These are known and not yet normative, so validation of the pinned edition
(2026d) does not apply them:

- Letter Ballot (2026-09-06): CP-2540 (Total Number of Frames in
  Concatenation, In-concatenation Total Number above one), CP-2541 (EFO coding
  scheme, "single cell identifier" in CID 8136), CP-2542 (Image Region
  concept), CP-2588 (Image Storage SOP Class UID types), CP-2610 (Institution
  Code Sequence in the General Equipment Module), CP-2620 and CP-2621 (STOW-RS,
  Storage Commitment and C-MOVE/C-GET response contents), CP-2559 (VM
  corrections).
- November 2026 voting packet: CP-2628 (WSI scan quality KOS titles), CP-2633
  (more WSI QC segmentation properties), CP-2625 (de-identification of overlay
  attributes), CP-2632 (de-identification of retrieval URIs).
- Assigned: CP-2656 (Specimen Module in SR and KOS IODs).
- Public Comment: Supplement 255 (Microscopy Bulk Enhanced Annotations) and
  Supplement 248 (DICOMweb Send).
- IHE PaLM: CP-LAB-279 and CP-LAB-281 (selectable as `ihe-dpia-draft` and
  `ihe-dpia-draft-jpeg`) and CP-LAB-283, under ballot.

The weekly edition check fails when a newer edition is published.

## Rust API

Use the builder API for normal exports:

```rust
use wsi_dicom::{ColorManagement, Export};

let report = Export::from_slide("slide.ndpi")
    .to_directory("out")
    .with_research_placeholder_metadata()
    .tile_size(512)
    .jpeg_quality(90)
    .color_management(ColorManagement::SourceOrSrgb)
    .run()?;
```

Applications that also need metadata selection, annotation
preparation, optional validation, report persistence, and progress events can
use `run_export_workflow` with `ExportWorkflowRequest`. Route-analysis callers
can use the target-specific `SlideRouteCoverageRequest` and
`CorpusRouteCoverageRequest` APIs; the older `RouteCoverageRequest` remains a
compatibility wrapper.

Use request types when an integration needs full control:

```rust
use wsi_dicom::{
    export_dicom, ColorManagement, ExportOptions, ExportRequest,
    JpegDirectHtj2kProfile, MetadataSource, TransferSyntax,
};

let mut options = ExportOptions::lossless_review();
options.transfer_syntax = TransferSyntax::Htj2k;
options.jpeg_direct_htj2k_profile = JpegDirectHtj2kProfile::Lossy97Balanced;
let request = ExportRequest::new(
    "slide.ndpi".into(),
    "out".into(),
    options,
    ColorManagement::SourceOrSrgb,
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

- Every color optical path carries a validated DICOM input-device ICC profile.
  Source profiles are preserved by the source/fallback modes. A synthesized
  sRGB or Display P3 input profile is an explicit color-space assumption, not
  scanner calibration. Configured/source conflicts are resolved only through
  the selected `IccConflictPolicy`.
- ICC is not applicable to MONOCHROME2 output, which instead carries
  Presentation LUT Shape `IDENTITY`, Rescale Intercept `0`, and Rescale Slope
  `1`.
- Lossy compression history is independent of the final transfer syntax. A
  lossless transcode does not erase prior lossy JPEG/JPEG 2000/HTJ2K stages.
  A DICOM source's history comes from the SOP Instances that supply each level
  or associated image. When a level spans several Instances, they must declare
  the same methods; each ratio is combined over them, weighted by decoded size.
  An Instance that declares nothing takes the codec and sizes its transfer
  syntax implies.
  For non-DICOM sources the stage is measured from the source tiles: JPEG tiles
  are lossy, and JPEG 2000 tiles are lossy when their codestream is
  irreversible. Levels without readable compressed tiles (Zeiss CZI) use the
  codec and stored size the reader reports, with the ratio of decoded to stored
  bytes. JPEG XR and JPEG-LS are recorded as lossy, because no reader reports
  whether a lossless mode was used. JPEG XR uses the method `ISO_29199_2`, which extends
  the Defined Terms. A lossy codec without stored sizes, or a codec that cannot
  be classified, fails the export.
- Resampled levels reference the level-zero instance of their plane, when it is
  part of the same export, through Source Image Sequence, Referenced Series
  Sequence and a shared Derivation Image functional group (DCM 113085 "Spatial
  resampling").
- JPEG Baseline output copies compatible native JPEG frames or retiles them
  without pixel re-encoding. Padded TIFF edge tiles and DICOM edge frames are
  copied whole into full-size edge frames. A tile cut from a larger stored
  image, or starting inside its encoded image (a JPEG crop from an earlier MCU
  boundary), is decoded instead, as is a MONOCHROME1 frame. A JPEG whose
  container declares RGB but whose bitstream identifies RGB only by component
  identifiers 0, 1, 2 (Aperio) gains an Adobe APP14 transform-0 marker, so
  bitstream-only decoders read the same RGB samples; its entropy-coded data is
  unchanged.
- JPEG 2000 and HTJ2K passthrough preserve compatible source codestreams,
  including ICT/9/7, regardless of the source container extension. Incompatible
  edges and mixed color profiles fail before the instance is published.
- Direct J2K-to-HTJ2K conversion requires coefficient-preserving recoding.
  Compressed-domain routes are always tried first. `pixel_reencode`
  (`--pixel-reencode`) governs frames they reject: `lossless` (the default)
  decodes and re-encodes them only into lossless transfer syntaxes, which adds
  no loss; `any` also permits a lossy JPEG Baseline re-encode, which adds a
  lossy generation; `never` fails instead. Re-encoded frames keep the source's
  lossy history and are counted as `source_reencoded` in the summary
  (`compressed_source_reencoded_frames` in reports). Lossy JPEG 2000 and HTJ2K
  output have no pixel re-encoding route.
- `--optional` (the default) writes the optional instances, encodings and
  attributes of the VL WSI IOD that the source supports: the associated images
  below, `TILED_SPARSE` for absent frames, and attributes it can fill
  truthfully (Tiles Overlap `NONE`, Quality Control Image `NO`, Recognizable
  Visual Features `YES` for label and overview and `NO` for pyramid levels,
  Series Date and Time, Color Space `SRGB` with the synthesized sRGB profile,
  Objective Lens Power from the source's `openslide.objective-power` on pyramid
  levels and thumbnails, a black Recommended Absent Pixel CIELab Value for
  `TILED_SPARSE`). `--required`
  (`Export::conformance(ConformanceLevel::Required)`) writes only the pyramid
  levels, `TILED_FULL` with absent frames padded and without a Per-Frame
  Functional Groups Sequence, and leaves out the optional descriptive
  attributes (Multi-Resolution Pyramid module, Acquisition Date and Time,
  Representative Frame Number, Dimension Index Sequence, shared Optical Path
  Identification, Illumination Wave Length, specimen and optical path
  descriptions, General Reference links). It keeps what the encoding depends
  on: Dimension Organization Type, offset tables and the Derivation Image
  functional group of resampled levels.
- Associated images are written by default as single-frame instances in the
  Series of the first plane: the label (`associated-label.dcm`,
  ORIGINAL\PRIMARY\LABEL\NONE, with an empty Slide Label module), the overview
  or macro (`associated-overview.dcm`, OVERVIEW) and the thumbnail
  (`associated-thumbnail.dcm`, DERIVED\PRIMARY\THUMBNAIL\RESAMPLED, which
  references level zero and has level-zero spacing scaled to its size). Label
  and overview declare Specimen Label in Image and Burned In Annotation YES and
  carry no Frame of Reference or Pixel Spacing. None join the
  Multi-Resolution Pyramid. The label and the overview, which shows at least
  part of the label, can identify the patient: leave them out with
  `--exclude-associated label,overview` (`all` leaves out every associated
  image) or `Export::exclude_associated`. A level-filtered or `--required`
  export writes no associated images.
- Associated images have no compressed-domain route, so their pixels are always
  encoded: in the requested transfer syntax when it is lossless, otherwise in
  its lossless sibling (JPEG Lossless SV1, JPEG 2000 Lossless, HTJ2K Lossless).
  `pixel_reencode=any` keeps a JPEG Baseline request lossy; `never` refuses
  associated images stored with an image codec (JPEG, JPEG 2000, JPEG XR,
  JPEG-LS), not those stored uncompressed or with LZW, Deflate, Zstandard or
  RLE. Their lossy history
  comes from their own DICOM Instance when applicable, or from the codec, mode,
  and size the reader reports. Labels and overviews take only their
  own embedded ICC profile, because the slide profile and scanner calibrations
  describe the scan; without one they get the policy's synthesized profile, and
  `RequireSource` fails. Thumbnails resolve ICC like level zero. Associated
  images with no matching flavour, or whose reader does not report how they are
  stored, are skipped and listed in `skipped_associated_images` and on stderr.
- CPU encoding remains available as the primary route for pixel inputs. Auto
  may choose a backend before execution; failures after device submission are
  returned rather than retried on the CPU. A selected device encode must complete
  its required stages on the device.
- Mixed JPEG exports retain only encoded/retiled exceptions in the payload
  spool and reread unchanged source frames when writing. Keep source datasets
  unchanged during conversion. Length, geometry, profile and file metadata
  checks detect ordinary changes; they do not provide a source snapshot.
- Route profile and coverage JSON reports expose available frame counts,
  sampled frame percentages, route counters, pixel profiles, and GPU counters.
  Profile and automatic-probe lookahead stay within the requested frame budget.
  Automatic route probes continue across short rows within the frame budget.
  CPU-input candidates share a warm-up batch; when enough later frames are
  available, selection uses their steady batch time and retains the warmed
  encoder. Cached decisions include execution settings, CPU pool size, Metal
  device identity and the probe revision. Short probes can still be noisy.
- Per-frame functional groups use streamed undefined-length sequences and items
  instead of an in-memory item list. Frame offsets and lengths are kept in a
  temporary disk-backed index and replayed when patching Extended Offset Tables.
- Output names encode scene, series, level, Z, channel, and time coordinates;
  consumers must use report paths rather than assuming the pre-0.7 name shape.
- Passing validators is release evidence, not formal DICOM certification.

WholeLevel JPEG route preflight inspects native source headers only for the
current output batch or bounded profile sample. It does not scan the entire
slide to authorize a pixel-reconstruction fallback.

CPU preparation runs in bounded batches and shares unchanged U8 storage. GPU
source caching accounts for unique retained allocations, with a 128 MiB cache
limit, and the instance writer accepts at most one queued prepared instance. The
pixel-data spool is written on a background thread through a 32-frame queue.
These are local bounds: source-reader caches, codec scratch and in-flight GPU
work also contribute to process memory. The reader owns a separate JP2K CPU
pool, so `RAYON_NUM_THREADS` is not a process-wide thread limit. See the
[execution ownership decision](docs/architecture/0007-performance-ownership.md).

> [!IMPORTANT]
> Regenerate color, MONOCHROME2, previously lossy, or multi-institution
> specimen output produced by version 0.7.1 or earlier. Older objects
> can contain a display-class fallback ICC profile, omit required monochrome
> presentation attributes, erase lossy history after lossless transcoding, or
> derive the same Specimen UID for identifiers governed by different issuers.

## Development

Durable repository contracts and the bounded architecture backlog are indexed in
[the architecture decision records](docs/architecture/README.md). Release history
remains in the changelog; manifests, lockfiles, tests, and CI are the current build
source of truth.

Core checks:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --no-default-features --all-targets --locked -- -D warnings
cargo test --workspace --no-default-features --all-targets --locked
```

The ignored fixture checks are explicit real-slide runs. Set
`WSI_DICOM_NDPI_FIXTURE` to the HE.ndpi fixture with a synthetic overview,
a full JPEG passthrough level, and a physical WholeLevel pyramid level with
partial edge tiles, at least three tile rows and columns, and at most 256
512-pixel frames. Set `WSI_DICOM_APERIO_JP2K_FIXTURE` to the
Aperio JPEG 2000 fixture with a 256-pixel source grid, the problem region at
level-zero `(12288, 6144)`, and an irreversible level 2. Missing fixture files,
incompatible layouts, and missing required Metal hardware fail the tests.
These runs keep raw inputs unchanged and use temporary output unless an
explicit output directory is supplied.

Run the three CPU fixture checks after changes to real-slide export or
reconstruction:

```sh
WSI_DICOM_NDPI_FIXTURE=/absolute/path/HE.ndpi \
WSI_DICOM_APERIO_JP2K_FIXTURE=/absolute/path/aperio-jp2k.svs \
  cargo test -p wsi-dicom --lib --no-default-features --locked \
  export::tests::fixture_tests -- --ignored --nocapture
```

Run all nine retained fixture checks on a Mac with Metal after changes to
source reconstruction, device encoding, or composition:

```sh
WSI_DICOM_NDPI_FIXTURE=/absolute/path/HE.ndpi \
WSI_DICOM_APERIO_JP2K_FIXTURE=/absolute/path/aperio-jp2k.svs \
  cargo test -p wsi-dicom --lib --features metal --locked \
  export::tests::fixture_tests -- --ignored --nocapture
```

| Explicit fixture check | Backend |
| --- | --- |
| `export_dicom_can_export_source_synthetic_downsample_level` | CPU or Metal build, CPU encode |
| `export_dicom_requires_device_encode_for_synthetic_level_with_cpu_source_input` | Metal |
| `ndpi_default_htj2k_export_rejects_implicit_color_reencoding` | CPU reconstruction, Metal encode |
| `ndpi_edge_tile_device_encode_round_trips_all_lossless_j2k_profiles` | Metal |
| `ndpi_fixture_exports_full_jpeg_baseline_passthrough_instance` | CPU or Metal build, passthrough |
| `aperio_jp2k_device_encoded_256_htj2k_rpcl_tile_matches_cpu` | Metal encode, policy-selected input |
| `lossy_jp2k_input_preserves_canonical_cpu_pixels_for_lossless_export` | CPU reconstruction, Metal encode |
| `aperio_jp2k_device_encoded_composed_512_htj2k_rpcl_tile_matches_cpu` | Metal encode, policy-selected input |
| `real_aperio_jp2k_problem_tile_round_trips` | CPU or Metal build, CPU encode |

The generated NDPI route and reversible JPEG 2000 Metal-input regressions run
in the normal CPU and Metal suites respectively. The Metal address-width
performance gate remains an explicit release-mode run. Run it on an otherwise
idle Mac with Metal after changes to the composition kernels; it compares
three rotated samples against the reference kernel and enforces the 5% limit:

```sh
cargo test -p wsi-dicom --lib --features metal --release --locked \
  metal_compose_selected_u32_stays_within_five_percent_of_reference \
  -- --ignored --nocapture
```

Pre-1.0 release gates:

```sh
cargo xtask docs-strict
cargo xtask coverage
cargo xtask semver
cargo publish --dry-run
```

Before any release candidate that changes or advertises export routes,
performance, conformance, or accelerator behavior, run these gates against
published dependencies and a representative real-slide corpus covering the
affected routes, metadata modes, color-management policies, validator checks,
and GPU backends.

Speed evidence and source-format coverage come from the GDC benchmark and
format-coverage harnesses in
[`wsi-dicom-bench`](https://github.com/frames-sg/wsi-dicom-bench#converter-speed-and-format-coverage).
Install that package and run its `wsi-dicom-bench-gdc` or
`wsi-dicom-bench-format-coverage` command from this checkout, so results record
this source revision and use its `target/release/wsi-dicom`.

## Stability

`wsi-dicom` is pre-1.0. The builder API is the preferred integration surface.
Lower-level request, report, validation, and profiling types are public, but
callers should prefer constructors and defaults over struct literals where
provided. The 0.7.4 release deliberately broke the pre-1.0
color-management API: `IccProfilePolicy` is removed, `ExportRequest::new` requires a
`ColorManagement`, and `Export` requires `.color_management(...)`.

## License

The 0.9.0 development release is available under the
[GNU Affero General Public License version 3 only](LICENSE-AGPL)
(`AGPL-3.0-only`), or under a separate [commercial license](COMMERCIAL-LICENSE.md)
from Frames. Commercial use is permitted under the AGPL without purchasing a
commercial license when its terms are followed.

Published releases through 0.8.0 retain their original MIT/Apache-2.0 licenses.
Existing permissive grants and separately licensed material are preserved; see
[LICENSE](LICENSE). J2K and wsi-rs retain their own licenses.
