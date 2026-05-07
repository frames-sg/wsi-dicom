# Metal HTJ2K Benchmark Results - 2026-05-07

## Scope

These results record the decision gates for:

- `docs/superpowers/plans/2026-05-07-jpeg-private-metal-htj2k-path.md`
- `docs/superpowers/plans/2026-05-07-htj2k-simd-tier1-prototype.md`

All benchmark outputs were written under `/tmp` and removed with `trash` after each run. No files under the repository `bench/` tree were created or modified.

## Environment

- Repository: `/Users/user/Bench/wsi-dicom`
- Source: `bench/testdata/CMU-1.tiff`
- Level: `0`
- Transfer syntax: `htj2k-lossless-rpcl`
- Backend: `prefer-device`
- Input route: `--source-device-decode`
- Build: `cargo build --release --features metal`
- Binary: `target/release/wsi-dicom`

Command shape:

```bash
OUT="/tmp/wsi-dicom-htj2k-simd-off"
if [ -e "$OUT" ]; then trash "$OUT"; fi
/usr/bin/time -l target/release/wsi-dicom convert bench/testdata/CMU-1.tiff \
  --out "$OUT" \
  --transfer-syntax htj2k-lossless-rpcl \
  --backend prefer-device \
  --source-device-decode \
  --json \
  --level 0
STATUS=$?
if [ -e "$OUT" ]; then trash "$OUT"; fi
exit $STATUS
```

For the SIMD prototype run, the same command was run with:

```bash
SIGNINUM_J2K_METAL_HT_SIMD_PROTOTYPE=1
```

## JPEG Private Metal HTJ2K Path

Earlier session observations before this results note was added:

| Step | Wall | `input_decode_micros` | Notes |
| --- | ---: | ---: | --- |
| Baseline before JPEG private internal planes | `9.76s` | `1,321,655` | Captured before the private-plane change. |
| After private internal JPEG planes | `9.66s` | `1,288,804` | Small decode-path improvement; not enough to stop before Task 5. |
| After `ResidentPrivateJpegTile` handoff | `10.05s` | `1,349,252` | `gpu_encode_hardware_micros=63,746,190`; wall still dominated by HTJ2K encode/compose/write rather than JPEG input decode. |

Current final default-path rerun after all commits:

| Metric | Value |
| --- | ---: |
| Wall | `10.27s` |
| `total_frames` | `5,850` |
| `resident_gpu_transcode_frames` | `5,850` |
| `gpu_input_decode_batches` | `65` |
| `gpu_compose_batches` | `65` |
| `gpu_encode_batches` | `65` |
| `input_decode_micros` | `1,328,525` |
| `compose_micros` | `2,130,396` |
| `encode_micros` | `5,422,230` |
| `gpu_dispatch_micros` | `8,881,151` |
| `gpu_encode_wall_micros` | `5,428,144` |
| `gpu_encode_effective_parallelism` | `11.597522099634793` |
| `gpu_encode_hardware_micros` | `62,953,020` |
| `write_micros` | `991,892` |

Decision: the private JPEG handoff is safe and keeps public `Surface::as_bytes()` semantics intact, but CMU-1 input decode is not the dominant wall-time bottleneck. Continue focusing on HTJ2K encode hardware time.

## HTJ2K SIMD Tier-1 Prototype

Current release benchmark after the cooperative compaction prototype:

| Route | Wall | `gpu_encode_wall_micros` | `gpu_encode_hardware_micros` | `gpu_encode_effective_parallelism` |
| --- | ---: | ---: | ---: | ---: |
| Default scalar Tier-1 path | `10.27s` | `5,428,144` | `62,953,020` | `11.597522099634793` |
| `SIGNINUM_J2K_METAL_HT_SIMD_PROTOTYPE=1` | `13.09s` | `8,598,400` | `65,988,540` | `7.674513863044288` |

Decision: keep the SIMD prototype disabled by default. The prototype preserves byte parity but regresses wall time and GPU hardware time. The current prototype parallelizes max-magnitude reduction and final byte compaction, while the actual HT byte-generation loop is still single-lane. A performance-positive version likely requires cooperative HT byte generation, not just cooperative final placement.

## Verification

Fresh verification run after the final code changes:

```bash
cargo test -p signinum-jpeg-metal
cargo test -p signinum-j2k-metal
SIGNINUM_J2K_METAL_HT_SIMD_PROTOTYPE=1 cargo test -p signinum-j2k-metal --tests
cargo clippy -p signinum-jpeg-metal --all-targets -- -D warnings
cargo clippy -p signinum-j2k-metal --all-targets -- -D warnings
```

```bash
cargo test --features metal
cargo clippy --features metal --all-targets -- -D warnings
```

The second command pair was run in both `/Users/user/Bench/statumen` and `/Users/user/Bench/wsi-dicom`.

Also run:

```bash
git diff --check
```

in `/Users/user/Bench/signinum`, `/Users/user/Bench/statumen`, and `/Users/user/Bench/wsi-dicom`.
