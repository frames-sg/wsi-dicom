#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0

"""Extract one dated release section from the controlled changelog."""

import argparse
import datetime
import re
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--changelog", type=Path, required=True)
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    lines = args.changelog.read_text(encoding="utf-8").splitlines()
    heading = re.compile(rf"^## \[{re.escape(args.version)}\] - ([0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}})$")
    matches = [(index, heading.fullmatch(line)) for index, line in enumerate(lines)]
    matches = [(index, match) for index, match in matches if match]
    if len(matches) != 1:
        raise SystemExit(f"expected exactly one dated changelog section for {args.version}")
    start, match = matches[0]
    datetime.date.fromisoformat(match.group(1))
    end = next(
        (index for index in range(start + 1, len(lines)) if lines[index].startswith("## [")),
        len(lines),
    )
    body = "\n".join(lines[start + 1 : end]).strip()
    if not body:
        raise SystemExit(f"changelog section for {args.version} is empty")
    print(body)


if __name__ == "__main__":
    main()
