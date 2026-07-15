#!/usr/bin/env bash
set -euo pipefail

readonly BASELINE_VERSION="0.2.0"
readonly BASELINE_SHA256="04c38916765f82f0a0d1ed6f74e2607aeb031ece0046967aef3b2625663fc64f"
readonly USER_AGENT="wsi-dicom-semver-check/${BASELINE_VERSION} (+https://github.com/frames-sg/wsi-dicom)"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_dir="$(mktemp -d)"
cleanup() {
  if command -v trash >/dev/null 2>&1; then
    trash "$work_dir" 2>/dev/null || true
  fi
}
trap cleanup EXIT

archive="$work_dir/wsi-dicom-${BASELINE_VERSION}.crate"
curl --fail --silent --show-error --location \
  --user-agent "$USER_AGENT" \
  --retry 3 \
  --connect-timeout 15 \
  --max-time 120 \
  "https://crates.io/api/v1/crates/wsi-dicom/${BASELINE_VERSION}/download" \
  --output "$archive"

if command -v sha256sum >/dev/null 2>&1; then
  actual_sha256="$(sha256sum "$archive" | awk '{print $1}')"
else
  actual_sha256="$(shasum -a 256 "$archive" | awk '{print $1}')"
fi
if [[ "$actual_sha256" != "$BASELINE_SHA256" ]]; then
  echo "baseline archive checksum mismatch: expected ${BASELINE_SHA256}, got ${actual_sha256}" >&2
  exit 1
fi

tar --extract --file "$archive" --directory "$work_dir"
baseline_root="$work_dir/wsi-dicom-${BASELINE_VERSION}"
(
  cd "$baseline_root"
  RUSTDOCFLAGS="-Z unstable-options --output-format json" \
    cargo +nightly rustdoc --lib --locked
)
baseline_rustdoc="$baseline_root/target/doc/wsi_dicom.json"

cd "$repo_root"
current_target="$work_dir/current-target"
RUSTDOCFLAGS="-Z unstable-options --output-format json" \
  CARGO_TARGET_DIR="$current_target" \
  cargo +nightly rustdoc --lib --locked --no-default-features
current_rustdoc="$current_target/doc/wsi_dicom.json"

cargo semver-checks check-release \
  --manifest-path Cargo.toml \
  --current-rustdoc "$current_rustdoc" \
  --baseline-rustdoc "$baseline_rustdoc"

set +e
minor_report="$(cargo semver-checks check-release \
  --manifest-path Cargo.toml \
  --current-rustdoc "$current_rustdoc" \
  --baseline-rustdoc "$baseline_rustdoc" \
  --release-type minor 2>&1)"
minor_status=$?
set -e
printf '%s\n' "$minor_report"
if [[ "$minor_status" -eq 0 ]]; then
  echo "expected the documented 0.2-to-0.7 pre-1.0 migration breaks, but none were reported" >&2
  exit 1
fi

actual_breaks="$work_dir/actual-breaks.txt"
printf '%s\n' "$minor_report" |
  awk '
    /^Failed in:$/ { capture = 1; next }
    capture && /^$/ { capture = 0; next }
    capture && /^  / { print }
  ' |
  sed -E '/, previously in file/! s# in [^ ]+:[0-9]+$##' |
  LC_ALL=C sort -u >"$actual_breaks"

if ! diff -u .github/semver-0.2-to-0.7-allowed-breaks.txt "$actual_breaks"; then
  echo "semver break set differs from the reviewed 0.7 migration allowlist" >&2
  exit 1
fi
