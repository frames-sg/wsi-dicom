#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0

"""Compare a crates.io checksum with the exact attested crate candidate."""

import argparse
import hashlib
import hmac
import json
import re
import time
import urllib.error
import urllib.request
from pathlib import Path


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def fetch_registry(version):
    url = f"https://crates.io/api/v1/crates/wsi-dicom/{version}"
    request = urllib.request.Request(
        url,
        headers={"User-Agent": f"wsi-dicom-release-verifier/{version} (+https://github.com/frames-sg/wsi-dicom)"},
    )
    last_error = None
    for attempt in range(6):
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                return json.load(response)
        except (OSError, urllib.error.URLError, json.JSONDecodeError) as error:
            last_error = error
            if attempt != 5:
                time.sleep(5)
    raise SystemExit(f"cannot query crates.io after 6 attempts: {last_error}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--registry-json", type=Path)
    args = parser.parse_args()
    if not re.fullmatch(r"(?:0|[1-9][0-9]*)(?:\.(?:0|[1-9][0-9]*)){2}", args.version):
        raise SystemExit("version must be a three-component numeric semver")
    expected_name = f"wsi-dicom-{args.version}.crate"
    if args.candidate.name != expected_name:
        raise SystemExit(f"candidate must be named {expected_name}")
    document = (
        json.loads(args.registry_json.read_text(encoding="utf-8"))
        if args.registry_json
        else fetch_registry(args.version)
    )
    registry = document.get("version", {})
    if registry.get("num") != args.version:
        raise SystemExit("crates.io response version does not match the candidate")
    registry_checksum = registry.get("checksum")
    if not isinstance(registry_checksum, str) or not re.fullmatch(r"[0-9a-f]{64}", registry_checksum):
        raise SystemExit("crates.io response has no valid SHA-256 checksum")
    candidate_checksum = sha256(args.candidate)
    if not hmac.compare_digest(candidate_checksum, registry_checksum):
        raise SystemExit(
            f"registry checksum mismatch: candidate {candidate_checksum}, registry {registry_checksum}"
        )
    print(f"verified crates.io checksum {candidate_checksum} for {expected_name}")


if __name__ == "__main__":
    main()
