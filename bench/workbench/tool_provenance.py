"""Provenance beyond a Python validator's executable wrapper."""

from __future__ import annotations

import json
import shlex
from pathlib import Path

from bench.file_digest import sha256_file


def definition_inventory(root: Path, edition: str = "2026c") -> dict:
    directory = root / edition
    required = [directory / "json" / name for name in ("iod_info.json", "module_info.json", "dict_info.json", "uid_info.json", "version")]
    missing = [str(path) for path in required if not path.is_file()]
    if missing:
        raise ValueError(f"missing pinned DICOM definitions: {', '.join(missing)}")
    return {
        "edition": edition,
        "standard_path": str(root),
        "files": [{"path": str(path.relative_to(root)), "sha256": sha256_file(path)} for path in sorted(directory.rglob("*")) if path.is_file() and (path.suffix in {".json", ".xml"} or path in required)],
    }


def python_validator_inventory(wrapper: Path, run_text_command) -> dict:
    with wrapper.open("rb") as stream:
        shebang = stream.readline(4096).decode("utf-8", errors="replace").strip()
    if not shebang.startswith("#!"):
        raise ValueError("validate_iods must expose its Python interpreter for release provenance")
    arguments = shlex.split(shebang[2:])
    if len(arguments) != 1 or not Path(arguments[0]).is_absolute() or "python" not in Path(arguments[0]).name:
        raise ValueError("validate_iods wrapper does not declare one absolute Python interpreter")
    # Execute a fixed inspection program with the wrapper's actual interpreter.
    program = """
import hashlib, importlib.metadata, json, pathlib, sys
import dicom_validator, pydicom
packages = {}
for name, module in [('dicom-validator', dicom_validator), ('pydicom', pydicom)]:
    root = pathlib.Path(module.__file__).parent
    digest = hashlib.sha256()
    for path in sorted(root.rglob('*.py')):
        digest.update(path.relative_to(root).as_posix().encode() + b'\\0')
        digest.update(hashlib.sha256(path.read_bytes()).digest())
    packages[name] = {'version': importlib.metadata.version(name), 'python_sources_sha256': digest.hexdigest()}
print(json.dumps({'python': sys.version, 'packages': packages}))
"""
    result = json.loads(run_text_command([arguments[0], "-c", program], 30, 8192))
    result["interpreter_sha256"] = sha256_file(Path(arguments[0]))
    result["definitions"] = definition_inventory(Path.home() / "dicom-validator")
    return result
