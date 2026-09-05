# Conversion performance survey — 2026-09-05

## Findings

The strongest small-change candidates are bounded buffering of frame-index and
extended-offset-table I/O, followed by enabling the existing external SHA-256
hardware backend on macOS/aarch64. Mixed JPEG passthrough/encode streaming is a
larger opportunity. Production code and dependency configuration were not changed
by this survey. These are measured bottlenecks; speedups require implementation,
regression coverage and matched before/after captures.

| Priority | Candidate | Evidence | Scope and constraints |
| --- | --- | --- | --- |
| 1 | Buffer frame-index records and extended-offset-table patching | Real SVS export: 23,220 frames; main-thread profile has 135/858 samples under `FrameIndexSpool::push` and 66 under offset-table patching. | Preserve record order, append-after-replay behavior, truncation errors, explicit flush/sync errors, offset overflow checks and transaction durability. Use a small temporary standard buffer, not an unbounded frame array. |
| 2 | Hardware SHA-256 for deterministic IDs | Deterministic SVS profile has 310 SHA-256 leaf samples; paired runs add 0.257–0.306 seconds versus fresh IDs. | Preserve every source/configuration/ICC hash input and exact UIDs. Use RustCrypto's existing runtime dispatch/software fallback; no hashing implementation in wsi-dicom. |
| 3 | Avoid spooling passthrough frames in mixed JPEG exports | Of 23,220 SVS frames, 22,912 pass through and 308 boundary frames require decode/encode, but the fallback path spools every frame. | More involved: metadata/profile and lossy-history totals must be known before writing, and clipping, source checks, order, bounded memory and failed-export cleanup must remain intact. |
| Separate integration work | Bring in the recent wsi-rs improvements | This checkout resolves registry wsi-rs 0.6.0 and does not include the sibling checkout's reader optimizations. | Requires a deliberate dependency/API compatibility update and fresh conversion measurements. The previously measured reader speedups cannot be applied directly to converter elapsed times. |

`src/writer/frame_index.rs` currently seeks to EOF and makes three eight-byte
writes for each record. Replay makes one 24-byte read per record.
`src/writer/pixel_data.rs::patch_extended_offset_tables_from_spool` then makes
one eight-byte file write per frame for each of the two tables. In the measured
mixed JPEG path, there are two index spools, three total replay passes and two
table-write passes. That implies roughly **301,860 small index/table I/O calls**
for 23,220 frames, excluding payload I/O and fixed setup. This count is derived
from the executed route and code, not a syscall-tracing counter.

The main DICOM output already uses `BufWriter`. The target is its separate
index files and table-patching handle. Simply wrapping the index in a buffer
while retaining an EOF seek on every push would flush that buffer repeatedly;
the append/replay transitions need a focused implementation and tests. Before
adding abstractions, reuse the existing frame-index owner and persistence helper.

`src/uid.rs::deterministic_generation_seed` already reads in 1 MiB chunks. Its
cost is not tiny file reads: the installed sha2 0.10.9 uses its software
aarch64 path. The previous wsi-rs change demonstrates a possible dependency
configuration, but that change has not been applied or timed in wsi-dicom.

The mixed-route behavior is in `src/export/jpeg_baseline_instance.rs` and
`src/export/jpeg_baseline_frames.rs`. Whole-level direct passthrough is rejected
when a frame fails eligibility; the remaining path spools both unchanged and
encoded frames. Boundary frames must not be forced through passthrough to get a
faster number. Existing fast-JPEG output includes newly encoded boundary pixels.

## Real conversion measurements

Three serial runs per case, using the existing GDC command builder and bounded
process-evidence runner, with `/usr/bin/time -l` for peak RSS:

| Workload | Frames | Median elapsed | Observed elapsed range | Frames/s | Peak RSS range |
| --- | ---: | ---: | ---: | ---: | ---: |
| SVS JPEG, level 0, fresh IDs | 23,220 | 1.486 s | 1.479–1.584 s | 15,627 | 61.67–62.05 MiB |
| Same SVS export, deterministic IDs | 23,220 | 1.792 s | 1.739–1.841 s | 12,956 | 61.83–62.34 MiB |
| NDPI to JPEG, native level 3 | 130 | 1.803 s | 1.753–1.858 s | 72.1 | 181.23–185.41 MiB |
| JPEG 2000 SVS to HTJ2K lossless RPCL, native level 2 | 20 | 0.397 s | 0.390–0.403 s | 50.3 | 57.45–58.73 MiB |

Elapsed time covers the command process, including opening, planning, output
staging and promotion. Throughput is output frame count divided by median
elapsed time; frame dimensions differ between workloads. The fresh-ID SVS
writer's reported median duration is 0.397 seconds. That duration excludes some
earlier pixel/index spooling, so it is not the entire opportunity. Per-frame
decode/encode duration counters can overlap or include nested Rayon work and
must not be summed as mutually exclusive wall-clock phases.

The NDPI profile's main Rayon worker has 1,460/1,537 leaf samples in the
external JPEG entropy encoder. The reduced JP2K conversion also concentrates
work in external codec transforms and HT block encoding. Writer durations are
only 0.016 and 0.012 seconds respectively. These cases do not support a claim
that writer buffering will produce a large whole-conversion improvement.

Profiles are separate from accepted timings. Counts above are unweighted
inclusive stack samples except where explicitly called leaf samples; stacks
can overlap, and threads include waits/work stealing. Some long Rayon stacks
were truncated by the profiler. Counts are hotspot evidence, not exact CPU
percentages or additive elapsed-time measurements.

## Validation and output evidence

All 12 real conversions succeeded. Pydicom verified repeated output Pixel Data
hashes, extended offset/length table hashes, transfer syntaxes, frame/matrix
dimensions and per-frame group counts. Route counters and ICC digests are stable.
The fresh/deterministic SVS outputs have identical pixel payloads and geometry;
deterministic study/series/SOP UIDs are stable across repetitions. This establishes
repeatability of the starting implementation, not source-pixel identity for
lossy JPEG encoding or before/after equivalence for an unimplemented change.

Pixel Data SHA-256 values:

- SVS JPEG, both ID policies:
  `d96e201b2831d1eb1a16422fd60c3dea5627e9a4dc7623fe210b3824860b24ba`
- NDPI JPEG level 3:
  `f4d26783303d8902ffceff531cfc5166254cd37518e7fd8b26876c17a66c2636`
- JP2K-to-HTJ2K level 2:
  `41b59cf19457dcfaec9b629c74283da4190d53b02c2bd6dd9c2675b37c949475`

A profiled validation of the generated SVS output, with `--max-pixel-frames 0`,
completed in 11.16 seconds. All intrinsic checks and the three available
external validators passed. Recorded external durations were 2.436 seconds
for dciodvfy, 2.372 for dcentvfy and 5.728 for validate_iods. This is primarily
external-tool time. The installed dicom-object reader already wraps
`from_reader` in a `BufReader`; another wrapper is not a justified optimization.
Pixel decoding was disabled for this diagnostic, so it is not exhaustive pixel
conformance validation.

| Executed check | Result |
| --- | --- |
| Release build, locked dependencies | Passed. |
| `cargo test --locked --lib writer::` | 34 passed, including spool truncation, large odd/even frame tables, streamed/spooled output equivalence, frame-length enforcement and exact metadata budgets. |
| `cargo test --locked --lib uid::` | Five passed, including source/configuration identity and specimen issuer changes. |
| `cargo fmt --all -- --check` | Passed. |
| Repeated real conversions/output checks | 12 conversions passed; all four workload groups are repeatable. |

No production behavior changed, so a full feature/coverage/fuzz matrix was not
rerun for this survey. It remains required as appropriate for an implementation.

## Reproduction and environment

- Existing checkout `wsi-dicom checkout`, HEAD
  `929e0ae6b2ff344aab7e50ea4bb308eacced49fd`, with all unfinished work preserved.
  Starting tracked patch SHA-256:
  `c1df7abb44d9a2d72181fc1ff6b72bd0fa33c8aaed0fefe8abb7fa03a5bc249f`.
- Apple M4 Pro, 12 logical CPUs, 48 GiB RAM, macOS 26.5.2 (25F84),
  Rust 1.96.0, default CPU features, release LTO/codegen settings unchanged.
  Line-table debugging and retained symbols were enabled for profile resolution.
- wsi-dicom 0.7.5 dirty source; registry wsi-rs 0.6.0; j2k/j2k-jpeg/
  j2k-transcode 0.10.0; sha2 0.10.9; dicom-object 0.9.1.
- `RAYON_NUM_THREADS=1`, `WSI_DICOM_EXPORT_INSTANCE_WORKERS=1`.
  **The wsi-rs reader has a separate default 12-thread decode pool**, confirmed
  in the profiles. This is not a one-thread process. No GPU was requested.
- Existing reader cache policy: 64 MiB shared tiles, 32 MiB display tiles,
  16 MiB private-cache budget. No cache overrides were set. Fresh processes
  have cold reader caches; filesystem caches were warm/uncontrolled.
- No builds or task-owned language server ran during measured captures. The
  one task-created language server was stopped after navigation. No worktree,
  commit, dependency update, push or publication was performed.
- Evidence is in `target/speed-survey-c8y7lr3f/`: starting patch/status,
  preserved baseline binary, `environment.json`, per-file source hashes,
  commands, raw reports/logs, sampled profiles, symbol mappings, output DICOMs,
  `verified-baselines.json` and narrow-test logs. Preserved binary SHA-256:
  `2db12b0dcbf60f28b609d9ea165dcd30cea56ce2444027b71a6af37d8c9aed32`.

```sh
CARGO_PROFILE_RELEASE_DEBUG=line-tables-only CARGO_PROFILE_RELEASE_STRIP=none \
  cargo build --release --locked --bin wsi-dicom

RAYON_NUM_THREADS=1 WSI_DICOM_EXPORT_INSTANCE_WORKERS=1 \
/usr/bin/time -l target/release/wsi-dicom convert \
  /path/to/parity-corpus/svs-001.svs \
  --out target/svs-writer-measurement --research-placeholder \
  --preset fast-jpeg --jpeg-quality 90 --tile-size 512 --backend cpu \
  --level 0 --uid-policy fresh --json
```

Use a fresh output directory each time. Repeat with deterministic IDs. For NDPI,
use `ndpi-001.ndpi` and `--level 3`. For JP2K, use `svs-jp2k-001.svs`,
`--level 2`, and replace the JPEG preset/quality flags with
`--transfer-syntax htj2k-lossless-rpcl`. The retained `run_baselines.py` uses
`bench.gdc.commands.build_wsi_dicom_command` and `bench.gdc.runner.run_command`;
it has already produced its uniquely named outputs and should not overwrite them.
`verify_baselines.py` can be rerun using `.venv/bin/python`. Profile separately
with `samply record --save-only -o <profile.json.gz>` around the conversion.

Only one real sample per source variant and three repetitions were used. There
is no p95/p99 claim, cold-filesystem/network-storage measurement or device
comparison. All cases ran in the same order and showed some upward timing drift.
The SHA observation is supported by the paired timing differences and profile,
but exact savings are not established. Large numbers of tiny frames should be
included in the writer implementation's benchmarks, together with codec-bound
controls and failure/cleanup tests.
