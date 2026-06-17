<!-- SPDX-License-Identifier: Apache-2.0 -->

# Contributing to statumen

Thanks for taking the time to contribute. This document describes the
expectations for patches, tests, and review on this crate.

## Code of Conduct

Participation in this project is governed by the
[Code of Conduct](CODE_OF_CONDUCT.md). By participating you agree to abide
by its terms.

## Development setup

```bash
# Install the toolchain pinned by the MSRV declared in `Cargo.toml`.
rustup show

# Run the same gates that CI runs.
cargo xtask validate
```

`cargo xtask validate` runs `fmt`, `clippy`, `bench-check`, `nextest`, and
`doc`. `cargo xtask bench-check` compiles the Rust benchmark targets without
running timings; use `cargo xtask bench` for the synthetic local Criterion
benchmarks. `cargo xtask ci` runs `validate` plus `package`.

## Branching and commits

- The default branch is `main`. Direct commits to `main` are accepted for
  small, focused changes; larger work goes through a PR.
- Commit messages should be imperative and short. Use prefixes when useful
  (`feat:`, `fix:`, `chore:`, `ci:`, `docs:`).
- Keep each commit self-contained: building / formatting / linting / tests
  should pass at every commit, not only at the tip.

## Refactor boundaries

- Keep the main `statumen` library at the repository root.
- Do not add workspace crates or move the library under `crates/*` for
  maintainability-only work.
- Prefer focused module directories inside the existing crate when a file grows
  too large to review comfortably.
- Preserve deliberate public re-exports from `src/lib.rs`; internal module
  splits should not accidentally expand the public API.

## Tests

- Unit tests live next to the code they cover.
- Integration tests live under `tests/`.
- Behavior-focused tests are preferred over implementation-coupled ones.
- Aim for ≥ 80% changed-path coverage. CI enforces this for changed
  non-test library and shim Rust source paths that have instrumented LCOV records with
  `cargo xtask coverage-changed` after generating `lcov.info`. Full-repo
  coverage is still uploaded as an artifact for review context. If something
  is genuinely hard to cover, document the gap in the PR description.

## Performance changes

Capture benchmark evidence before performance-oriented work and compare again
after each workstream:

```sh
cargo xtask perf-capture baseline
cargo xtask perf-capture after-change
cargo xtask perf-compare bench/results/local-regression/baseline.json bench/results/local-regression/after-change.json
cargo xtask perf-capture-openslide openslide-baseline path/to/slide.svs
cargo xtask perf-compare bench/results/local-regression/openslide-baseline.json bench/results/local-regression/after-change.json
```

`perf-capture` defaults to the public JP2K fixture and writes ignored local
JSON under `bench/results/local-regression/`. Pass slide paths explicitly or
set `STATUMEN_PERF_SLIDES` to a platform path-list for private WSI/DICOM
coverage. The comparison gate checks p50, p95, p99, mean, and peak RSS when
available over at least 3 runs and flags regressions when after/before is
greater than 1.05 in at least 2 comparable runs. Maintainability-only changes
need a clean comparison, not a speedup. Metal timing is optional and should be
reported only when the local macOS hardware/session supports it.

`perf-capture-openslide` records the same workload schema through
`openslide_bench` for OpenSlide competitor evidence. It requires explicit real
WSI slide paths or `STATUMEN_PERF_SLIDES`; compare it as `before` against a
Statumen capture as `after`, so the printed ratio is Statumen/OpenSlide.

For non-trivial hot-path changes, include a profiler artifact or written
summary. Use `cargo xtask perf-profile path/to/slide.svs [workload]` to print
the preferred `samply` and `xcrun xctrace` recipes. Treat flamegraphs as
optional diagnostic evidence, not as the benchmark gate.

Every performance workstream must document:

- Time and space complexity before and after the change.
- Allocation behavior, including avoided `Vec` / `CpuTile` clones and whether
  remaining clones are cheap shared-handle clones such as `Arc::clone`.
- Cache behavior: expected hit rate, byte budget impact, RSS effect, and why a
  larger cache is or is not justified.
- Data structure choice: contiguous arrays/slices for dense tile grids and maps
  only for genuinely sparse layouts.
- Rayon usage: independent CPU-bound work, explicit pools or chunk thresholds
  when needed, and no unbounded blocking I/O inside compute-saturated workers.
- Safety impact: preserve safe Rust in the main crate, checked arithmetic for
  byte/pixel ranges, and explicit errors instead of silent fallbacks.

## Reporting issues

Please include:

1. statumen version (`cargo pkgid`).
2. Rust toolchain (`rustc --version`).
3. Operating system and architecture.
4. The smallest reproducer you can share, including the WSI container
   format if relevant.

## Security

If you believe you have found a security vulnerability, please **do not**
open a public issue. Email the maintainers privately and we will
coordinate a fix and disclosure.

## License

By contributing, you agree that your contributions will be licensed under
the Apache License, Version 2.0, as declared in `LICENSE`.
