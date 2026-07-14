#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: scripts/publish-crate.sh {--dry-run|--publish}" >&2
}

if [[ "$#" -ne 1 ]]; then
  usage
  exit 2
fi

if [[ -n "${CRATES_IO_API_TOKEN:-}" ]]; then
  echo "CRATES_IO_API_TOKEN is unsupported; use crates.io trusted publishing" >&2
  exit 2
fi

case "$1" in
  --dry-run)
    if [[ -n "${CARGO_REGISTRY_TOKEN:-}" ]]; then
      echo "dry-run mode refuses registry credentials" >&2
      exit 2
    fi
    exec cargo publish \
      --package wsi-dicom \
      --registry crates-io \
      --locked \
      --dry-run
    ;;
  --publish)
    if [[ -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
      echo "CARGO_REGISTRY_TOKEN is required for trusted publication" >&2
      exit 2
    fi
    exec cargo publish \
      --package wsi-dicom \
      --registry crates-io \
      --locked \
      --no-verify
    ;;
  *)
    usage
    exit 2
    ;;
esac
