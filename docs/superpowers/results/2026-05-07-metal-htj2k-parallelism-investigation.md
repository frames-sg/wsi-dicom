# Metal HTJ2K Parallelism Investigation - 2026-05-07

## Scope

This note tracks the bounded HTJ2K Metal investigation for low observed
`gpu_encode_effective_parallelism` and the negative-result Tier-1 experiments
that were removed after benchmark review:

- `SIGNINUM_J2K_METAL_HT_CONSTANT_TABLES=1`
- `SIGNINUM_J2K_METAL_HT_MS_COOP=1`

Those two environment variables are historical labels for the benchmark rows
below, not retained runtime knobs.

`gpu_encode_effective_parallelism` is not a direct GPU occupancy metric. It is
reported as summed completed GPU command-buffer duration divided by encode wall
time. Interpret changes against wall time, command-buffer gaps, and stage trace
shape, not as occupancy alone.

## Benchmark Workflow

Use release builds only:

```bash
cargo build --release --features metal
```

Run CMU-1 level 0 with `/tmp` output and remove output through `trash`:

```bash
OUT="/tmp/wsi-dicom-metal-htj2k-default"
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

Run five default iterations and record the median plus full raw JSON metrics.
The removed experiment rows below were captured before cleanup and should be
treated as historical evidence, not current benchmark routes.

## Metal Trace Capture

Capture one representative default run:

```bash
if [ -e /tmp/wsi-dicom-metal-htj2k.trace ]; then trash /tmp/wsi-dicom-metal-htj2k.trace; fi
if [ -e /tmp/wsi-dicom-metal-capture ]; then trash /tmp/wsi-dicom-metal-capture; fi
xcrun xctrace record \
  --template 'Metal System Trace' \
  --output /tmp/wsi-dicom-metal-htj2k.trace \
  --target-stdout - \
  --launch -- target/release/wsi-dicom convert bench/testdata/CMU-1.tiff \
    --out /tmp/wsi-dicom-metal-capture \
    --transfer-syntax htj2k-lossless-rpcl \
    --backend prefer-device \
    --source-device-decode \
    --json \
    --level 0
STATUS=$?
if [ -e /tmp/wsi-dicom-metal-capture ]; then trash /tmp/wsi-dicom-metal-capture; fi
exit $STATUS
```

If the trace needs clearer stage names, rerun with:

```bash
SIGNINUM_J2K_METAL_PROFILE_STAGES=1
```

The profiling flag adds Metal command-buffer/encoder labels for input
deinterleave, coefficient prep, HTJ2K Tier-1, packet block prep, packetization,
codestream assembly, and WSI tile composition. Normal JSON output is unchanged.

## Results

Release build: `cargo build --release --features metal`.

Input: `bench/testdata/CMU-1.tiff`, level 0, `/tmp` output directories removed
with `trash` after each run.

| Route | Median wall | Median `gpu_encode_wall_micros` | Median `gpu_encode_hardware_micros` | Median `gpu_encode_effective_parallelism` | Decision |
| --- | ---: | ---: | ---: | ---: | --- |
| Default scalar | 9.53 s | 5,123,233 | 62,164,710 | 12.133883 | Baseline |
| Constant tables | 9.57 s | 5,134,297 | 62,147,430 | 12.096917 | Removed. No repeatable wall-time win; hardware time is neutral and within run noise. |
| MS-coop | 13.24 s | 8,926,563 | 66,341,520 | 7.432017 | Removed. Encode wall time regresses materially. |
| Constant tables + MS-coop | Not run | Not run | Not run | Not run | Skipped because neither experiment won independently. |

## Raw Metrics

MS-coop rows were rerun after fixing the prototype to keep its temporary MS
scratch capacity out of downstream packet/codestream capacity estimates.

| Route | Iter | Wall s | `gpu_encode_wall_micros` | `gpu_encode_hardware_micros` | `gpu_encode_effective_parallelism` | `input_decode_micros` | `compose_micros` | `encode_micros` | `write_micros` | `gpu_encode_batches` | `resident_frames` |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Default | 1 | 9.49 | 5,100,509 | 62,004,870 | 12.156604 | 1,306,196 | 2,090,553 | 5,095,530 | 854,753 | 65 | 5,850 |
| Default | 2 | 9.54 | 5,141,570 | 62,290,350 | 12.115045 | 1,294,209 | 2,100,597 | 5,136,030 | 868,824 | 65 | 5,850 |
| Default | 3 | 9.53 | 5,123,233 | 62,164,710 | 12.133883 | 1,295,058 | 2,111,912 | 5,117,400 | 869,365 | 65 | 5,850 |
| Default | 4 | 9.55 | 5,123,115 | 62,210,610 | 12.143122 | 1,317,583 | 2,118,890 | 5,117,490 | 853,305 | 65 | 5,850 |
| Default | 5 | 9.52 | 5,133,132 | 62,030,610 | 12.084359 | 1,296,391 | 2,132,068 | 5,127,300 | 821,772 | 65 | 5,850 |
| Constant tables | 1 | 9.87 | 5,482,444 | 62,187,750 | 11.343071 | 1,294,775 | 2,127,874 | 5,477,130 | 831,550 | 65 | 5,850 |
| Constant tables | 2 | 9.57 | 5,136,411 | 62,134,740 | 12.096917 | 1,306,074 | 2,152,936 | 5,130,450 | 838,746 | 65 | 5,850 |
| Constant tables | 3 | 9.58 | 5,133,070 | 62,025,930 | 12.083593 | 1,293,530 | 2,164,959 | 5,127,840 | 849,622 | 65 | 5,850 |
| Constant tables | 4 | 9.57 | 5,134,297 | 62,147,430 | 12.104370 | 1,297,174 | 2,146,148 | 5,128,740 | 853,539 | 65 | 5,850 |
| Constant tables | 5 | 9.57 | 5,133,846 | 62,250,930 | 12.125594 | 1,300,296 | 2,152,683 | 5,128,290 | 850,519 | 65 | 5,850 |
| MS-coop | 1 | 13.53 | 8,932,128 | 66,383,730 | 7.432017 | 1,309,050 | 2,059,322 | 8,926,740 | 831,831 | 65 | 5,850 |
| MS-coop | 2 | 13.18 | 8,926,563 | 66,370,320 | 7.435148 | 1,261,597 | 2,086,834 | 8,920,980 | 777,299 | 65 | 5,850 |
| MS-coop | 3 | 13.19 | 8,928,066 | 66,218,580 | 7.416901 | 1,276,705 | 2,084,320 | 8,922,420 | 777,747 | 65 | 5,850 |
| MS-coop | 4 | 13.25 | 8,925,710 | 66,341,520 | 7.432632 | 1,260,653 | 2,135,834 | 8,919,990 | 799,604 | 65 | 5,850 |
| MS-coop | 5 | 13.24 | 8,922,178 | 66,276,180 | 7.428251 | 1,273,905 | 2,124,627 | 8,916,300 | 789,781 | 65 | 5,850 |

## Capture Triage

Default trace artifact:
`/tmp/wsi-dicom-metal-htj2k.trace`.

The captured run completed successfully with:

- `input_decode_micros`: 1,412,251
- `compose_micros`: 3,524,335
- `encode_micros`: 5,366,610
- `gpu_dispatch_micros`: 10,303,196
- `gpu_encode_wall_micros`: 5,372,091
- `gpu_encode_hardware_micros`: 62,077,230
- `gpu_encode_effective_parallelism`: 11.555506
- `write_micros`: 1,208,440
- `gpu_encode_batches`: 65
- `resident_frames`: 5,850

`xcrun xctrace export --toc` verified the Metal System Trace capture and exposed
Metal command-buffer submission/completion tables. The CLI export did not expose
enough labeled stage timing to distinguish queue gaps, retained prepare/decode
buffers, register pressure, table-load stalls, or packetization/assembly stalls
with confidence. The trace should be opened in Instruments for that
classification before planning a full cooperative HT Tier-1 rewrite.

The JSON timings still show encode wall time as the largest measured stage.
However, the MS-only cooperative prototype regressed encode wall time, so the
current evidence does not justify extending cooperative MEL/VLC work.

## Implementation Notes

- Retained: `SIGNINUM_J2K_METAL_PROFILE_STAGES=1` adds Metal
  command-buffer/encoder labels without changing normal JSON output.
- Removed: the constant-table scalar-equivalent kernel variant and MS-only
  cooperative Tier-1 prototype, along with their experimental tests.
- The shallow `SIGNINUM_J2K_METAL_HT_SIMD_PROTOTYPE` path remains disabled by
  default and separate from this benchmark. If explicitly requested on a device
  where its pipeline is unavailable, initialization now surfaces that as an
  error instead of silently falling back.
