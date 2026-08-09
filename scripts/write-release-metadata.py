#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0

"""Write portable metadata for one release artifact."""

import argparse
import json
import re
from pathlib import Path


HEX_40 = re.compile(r"[0-9a-f]{40}\Z")
HEX_64 = re.compile(r"[0-9a-f]{64}\Z")
VERSION = re.compile(r"(?:0|[1-9][0-9]*)(?:\.(?:0|[1-9][0-9]*)){2}\Z")


def arguments():
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--artifact", required=True)
    parser.add_argument("--feature", action="append", default=[])
    parser.add_argument("--rustc", required=True)
    parser.add_argument("--cargo", required=True)
    parser.add_argument("--lock-sha256", required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args()


def main():
    args = arguments()
    if not VERSION.fullmatch(args.version):
        raise SystemExit("version must be a three-component numeric semver")
    if not HEX_40.fullmatch(args.commit):
        raise SystemExit("commit must contain 40 lowercase hexadecimal characters")
    if not HEX_64.fullmatch(args.lock_sha256):
        raise SystemExit("lock digest must contain 64 lowercase hexadecimal characters")
    if not args.target.strip():
        raise SystemExit("target must not be empty")
    artifact = Path(args.artifact)
    if artifact.name != args.artifact or args.artifact in {"", ".", ".."}:
        raise SystemExit("artifact must be a basename")
    if any(not feature.strip() for feature in args.feature):
        raise SystemExit("features must not be empty")

    record = {
        "schema_version": 1,
        "version": args.version,
        "commit": args.commit,
        "artifact": args.artifact,
        "target": args.target,
        "features": sorted(set(args.feature)),
        "toolchain": {"rustc": args.rustc, "cargo": args.cargo},
        "dependency_lock_sha256": args.lock_sha256,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
