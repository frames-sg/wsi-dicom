#!/usr/bin/env bash
set -euo pipefail

readonly BASELINE_VERSION="0.7.1"
readonly BASELINE_COMMIT="88c0dc357740cb6d344389449e01b008bb3f2649"
readonly CANDIDATE_VERSION="0.7.2"
readonly SEMVER_CHECKS_VERSION="cargo-semver-checks 0.48.0"
readonly ALLOWLIST=".github/semver-0.7.1-to-0.7.2-allowed-breaks.txt"
readonly ARCHIVED_REPORT=".github/semver-0.7.1-to-0.7.2-report.md"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_dir="$(mktemp -d)"
cleanup() {
  if command -v trash >/dev/null 2>&1; then
    trash "$work_dir" 2>/dev/null || true
  else
    echo "temporary semver workspace retained because trash is unavailable: ${work_dir}" >&2
  fi
}
trap cleanup EXIT

cd "$repo_root"
if [[ "$(cargo semver-checks --version)" != "$SEMVER_CHECKS_VERSION" ]]; then
  echo "semver report requires ${SEMVER_CHECKS_VERSION}" >&2
  exit 1
fi
if ! git cat-file -e "${BASELINE_COMMIT}^{commit}"; then
  echo "baseline commit ${BASELINE_COMMIT} is unavailable; fetch full history" >&2
  exit 1
fi

baseline_root="$work_dir/wsi-dicom-${BASELINE_VERSION}"
mkdir "$baseline_root"
git archive --format=tar "$BASELINE_COMMIT" | tar --extract --file - --directory "$baseline_root"
baseline_manifest_version="$(
  cargo metadata \
    --manifest-path "$baseline_root/Cargo.toml" \
    --locked \
    --no-deps \
    --format-version 1 |
    python3 -c 'import json, sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "wsi-dicom"))'
)"
candidate_manifest_version="$(
  cargo metadata --locked --no-deps --format-version 1 |
    python3 -c 'import json, sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "wsi-dicom"))'
)"
if [[ "$baseline_manifest_version" != "$BASELINE_VERSION" ]]; then
  echo "baseline commit declares ${baseline_manifest_version}, expected ${BASELINE_VERSION}" >&2
  exit 1
fi
if [[ "$candidate_manifest_version" != "$CANDIDATE_VERSION" ]]; then
  echo "candidate declares ${candidate_manifest_version}, expected ${CANDIDATE_VERSION}" >&2
  exit 1
fi

baseline_target="$work_dir/baseline-target"
(
  cd "$baseline_root"
  RUSTC_BOOTSTRAP=1 RUSTDOCFLAGS="-Z unstable-options --output-format json" \
    CARGO_TARGET_DIR="$baseline_target" \
    cargo +1.96 rustdoc --lib --locked --no-default-features
)
baseline_rustdoc="$baseline_target/doc/wsi_dicom.json"

current_target="$work_dir/current-target"
RUSTC_BOOTSTRAP=1 RUSTDOCFLAGS="-Z unstable-options --output-format json" \
  CARGO_TARGET_DIR="$current_target" \
  cargo +1.96 rustdoc --lib --locked --no-default-features
current_rustdoc="$current_target/doc/wsi_dicom.json"

set +e
patch_report="$(cargo semver-checks check-release \
  --manifest-path Cargo.toml \
  --color never \
  --current-rustdoc "$current_rustdoc" \
  --baseline-rustdoc "$baseline_rustdoc" \
  --release-type patch 2>&1)"
patch_status=$?
set -e
printf '%s\n' "$patch_report"
if [[ "$patch_status" -eq 0 ]]; then
  echo "expected the reviewed 0.7.2 API transition, but no patch-level breaks were reported" >&2
  exit 1
fi

actual_breaks="$work_dir/actual-breaks.txt"
printf '%s\n' "$patch_report" |
  awk '
    /^Failed in:$/ { capture = 1; next }
    capture && /^$/ { capture = 0; next }
    capture && /^  / { print }
  ' |
  sed -E '/, previously in file/! s# in [^ ]+:[0-9]+$##' |
  LC_ALL=C sort -u >"$actual_breaks"
if [[ ! -s "$actual_breaks" ]]; then
  echo "cargo-semver-checks failed without a parseable break set" >&2
  exit 1
fi
if ! diff -u "$ALLOWLIST" "$actual_breaks"; then
  echo "semver break set differs from the reviewed 0.7.1-to-0.7.2 allowlist" >&2
  exit 1
fi

generated_report="$work_dir/semver-report.md"
REPORT_BASELINE_COMMIT="$BASELINE_COMMIT" \
REPORT_BASELINE_VERSION="$BASELINE_VERSION" \
REPORT_CANDIDATE_VERSION="$CANDIDATE_VERSION" \
REPORT_SEMVER_CHECKS_VERSION="$SEMVER_CHECKS_VERSION" \
ACTUAL_BREAKS="$actual_breaks" \
python3 - "$generated_report" <<'PY'
import os
from pathlib import Path
import sys

breaks = Path(os.environ["ACTUAL_BREAKS"]).read_text(encoding="utf-8").splitlines()
report = [
    "<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->",
    "",
    "# wsi-dicom 0.7.1 to 0.7.2 semver report",
    "",
    f"- Baseline: `{os.environ['REPORT_BASELINE_VERSION']}` at immutable commit `{os.environ['REPORT_BASELINE_COMMIT']}`.",
    "- Baseline publication state: merged, but not tagged or published to crates.io.",
    f"- Candidate: `{os.environ['REPORT_CANDIDATE_VERSION']}`.",
    f"- Tool: `{os.environ['REPORT_SEMVER_CHECKS_VERSION']}` with Rust `1.96` rustdoc JSON.",
    "- Policy: the patch-level API breaks below are intentional for this pre-1.0 transition; any different break set fails CI.",
    "",
    "## Reviewed breaks",
    "",
    "```text",
    *breaks,
    "```",
    "",
]
Path(sys.argv[1]).write_text("\n".join(report), encoding="utf-8")
PY
if ! diff -u "$ARCHIVED_REPORT" "$generated_report"; then
  echo "archived semver report is stale" >&2
  exit 1
fi

# A major release classification should accept the same rustdoc pair. This
# distinguishes expected API incompatibility from malformed analysis inputs.
cargo semver-checks check-release \
  --manifest-path Cargo.toml \
  --color never \
  --current-rustdoc "$current_rustdoc" \
  --baseline-rustdoc "$baseline_rustdoc" \
  --release-type major
