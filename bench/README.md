# GDC WSI Benchmark

This benchmark discovers local `gdc_download*` directories and runs the same
GDC/TCGA slides through real converter commands. It replaces the previous fixed
slide registry and synthetic/manual converter baselines.

Default run:

```sh
./.venv/bin/python bench/gdc_benchmark.py \
  --downloads-root ~/Downloads \
  --probe-slide-metadata \
  --tools wsi-dicom-cpu wsi-dicom-device wsidicomizer \
  --profile htj2k-lossless-rpcl \
  --scope base \
  --runs 1 \
  --system-label macos-metal \
  --device-preflight \
  --validate
```

Use `--dry-run` first to publish or inspect the exact command matrix without
running conversions. Run the same command on macOS for Metal and on the CUDA
Linux host for CUDA, changing the built `wsi-dicom` binary, `--run-label`, and
`--system-label`. Results are written under `bench/results/<run-label>/` as `slides.json`,
`environment.json`, `planned_commands.jsonl`, `results.jsonl`, `results.csv`,
and `summary.md`.

Keep `--device-preflight` enabled for Metal/CUDA smoke and publication runs.
It records bounded CPU and strict-device route profiles before each
`wsi-dicom-device` conversion, then skips the expensive conversion when the
device route is unavailable, fails to use device encode, times out, or is slower
than CPU under the configured threshold. Full preflight evidence is embedded in
`results.jsonl`; summary speedups are computed only from completed conversions.

Use `--merge-results` to combine CPU, Metal, CUDA, and wsidicomizer result
directories. The merged summary preserves accelerator-specific labels instead
of collapsing all `wsi-dicom-device` runs into one column.

For public claims, publish failed and timed-out rows as well as successful rows.
The `htj2k-lossless-rpcl` profile maps wsidicomizer to its HTJ2K option because
that CLI does not expose RPCL-specific control.
