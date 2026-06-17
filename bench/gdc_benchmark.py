#!/usr/bin/env python3
"""Public benchmark harness for GDC/TCGA whole-slide downloads.

The harness intentionally discovers the local GDC download directories at run
time instead of carrying a fixed private slide registry. Results are written as
machine-readable evidence so Metal, CUDA, and CPU-only runs from different
hosts can be published or merged without rewriting the benchmark.
"""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import json
import os
import platform
import shlex
import socket
import subprocess
import sys
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Iterable, Sequence


SUPPORTED_SUFFIXES = {
    ".bif",
    ".czi",
    ".mrxs",
    ".ndpi",
    ".scn",
    ".svs",
    ".tif",
    ".tiff",
    ".vms",
    ".vmu",
}

PROFILE_CHOICES = ("htj2k-lossless-rpcl", "jpeg-baseline")
SCOPE_CHOICES = ("base", "pyramid")
TOOL_CHOICES = ("wsi-dicom-cpu", "wsi-dicom-device", "wsidicomizer")


@dataclass(frozen=True)
class ManifestEntry:
    file_id: str
    filename: str
    md5: str
    size: int | None
    state: str


@dataclass(frozen=True)
class Slide:
    slide_id: str
    display_name: str
    path: Path
    download_dir: Path
    relative_path: str
    gdc_file_id: str | None
    manifest_filename: str | None
    manifest_md5: str | None
    manifest_size: int | None
    manifest_state: str | None
    bytes_on_disk: int
    metadata: dict | None = None


def safe_slug(value: str) -> str:
    slug = []
    for character in value:
        if character.isalnum():
            slug.append(character.lower())
        elif character in {".", "-", "_"}:
            slug.append(character.lower())
        else:
            slug.append("-")
    compacted = "".join(slug).strip("-")
    while "--" in compacted:
        compacted = compacted.replace("--", "-")
    return compacted or "slide"


def split_command(command: str) -> list[str]:
    parts = shlex.split(command)
    if not parts:
        raise ValueError("command cannot be empty")
    return parts


def parse_manifest(path: Path) -> list[ManifestEntry]:
    if not path.exists():
        return []
    lines = [line for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
    if not lines:
        return []
    header = lines[0].split("\t")
    entries: list[ManifestEntry] = []
    for line in lines[1:]:
        fields = dict(zip(header, line.split("\t")))
        size_text = fields.get("size")
        try:
            size = int(size_text) if size_text else None
        except ValueError:
            size = None
        entries.append(
            ManifestEntry(
                file_id=fields.get("id", ""),
                filename=fields.get("filename", ""),
                md5=fields.get("md5", ""),
                size=size,
                state=fields.get("state", ""),
            )
        )
    return entries


def manifest_entry_for_slide(
    slide_path: Path,
    download_dir: Path,
    entries: Sequence[ManifestEntry],
) -> ManifestEntry | None:
    relative_path = slide_path.relative_to(download_dir).as_posix()
    for entry in entries:
        if entry.filename == relative_path:
            return entry
    for entry in entries:
        if entry.file_id and slide_path.parent.name == entry.file_id:
            return entry
    for entry in entries:
        if entry.filename and Path(entry.filename).name.lower() == slide_path.name.lower():
            return entry
    return None


def read_slide_metadata(path: Path) -> dict:
    try:
        import openslide
    except Exception as exc:  # noqa: BLE001 - metadata is optional evidence.
        return {"error": f"openslide unavailable: {type(exc).__name__}: {exc}"}

    try:
        slide = openslide.OpenSlide(str(path))
        try:
            return {
                "vendor": slide.properties.get("openslide.vendor"),
                "dimensions": list(slide.dimensions),
                "level_count": slide.level_count,
                "level_dimensions": [list(dim) for dim in slide.level_dimensions],
                "level_downsamples": list(slide.level_downsamples),
            }
        finally:
            slide.close()
    except Exception as exc:  # noqa: BLE001 - keep the slide in the benchmark.
        return {"error": f"{type(exc).__name__}: {exc}"}


def discover_gdc_slides(
    downloads_root: Path,
    *,
    gdc_glob: str = "gdc_download*",
    probe_metadata: bool = False,
) -> list[Slide]:
    downloads_root = downloads_root.expanduser().resolve()
    slides: list[Slide] = []
    for download_dir in sorted(path for path in downloads_root.glob(gdc_glob) if path.is_dir()):
        entries = parse_manifest(download_dir / "MANIFEST.txt")
        for path in sorted(download_dir.rglob("*")):
            if not path.is_file() or path.suffix.lower() not in SUPPORTED_SUFFIXES:
                continue
            entry = manifest_entry_for_slide(path, download_dir, entries)
            manifest_name = Path(entry.filename).name if entry and entry.filename else None
            display_name = manifest_name or path.name
            file_id = entry.file_id if entry else None
            stem = Path(display_name).stem
            id_part = file_id[:8] if file_id else download_dir.name.replace("gdc_download_", "")
            slide_id = safe_slug(f"{stem}-{id_part}")
            metadata = read_slide_metadata(path) if probe_metadata else None
            slides.append(
                Slide(
                    slide_id=slide_id,
                    display_name=display_name,
                    path=path,
                    download_dir=download_dir,
                    relative_path=path.relative_to(download_dir).as_posix(),
                    gdc_file_id=file_id,
                    manifest_filename=entry.filename if entry else None,
                    manifest_md5=entry.md5 if entry else None,
                    manifest_size=entry.size if entry else None,
                    manifest_state=entry.state if entry else None,
                    bytes_on_disk=path.stat().st_size,
                    metadata=metadata,
                )
            )
    return sorted(slides, key=lambda slide: (slide.display_name.lower(), slide.slide_id))


def slide_to_json(slide: Slide) -> dict:
    record = asdict(slide)
    record["path"] = str(slide.path)
    record["download_dir"] = str(slide.download_dir)
    return record


def select_slides(
    slides: Sequence[Slide],
    *,
    only_filters: Sequence[str],
    max_slides: int | None,
) -> list[Slide]:
    selected = list(slides)
    for only in only_filters:
        needle = only.lower()
        selected = [
            slide
            for slide in selected
            if needle in slide.slide_id.lower()
            or needle in slide.display_name.lower()
            or needle in slide.relative_path.lower()
            or (slide.gdc_file_id and needle in slide.gdc_file_id.lower())
        ]
    if max_slides is not None:
        selected = selected[:max_slides]
    return selected


def build_wsi_dicom_command(
    base_command: Sequence[str],
    source: Path,
    output_dir: Path,
    *,
    profile: str,
    scope: str,
    tile_size: int,
    jpeg_quality: int,
    backend: str,
    source_device_decode: bool,
) -> list[str]:
    command = [
        *base_command,
        "convert",
        str(source),
        "--out",
        str(output_dir),
        "--research-placeholder",
        "--tile-size",
        str(tile_size),
        "--backend",
        backend,
        "--json",
    ]
    if scope == "base":
        command.extend(["--level", "0"])
    elif scope != "pyramid":
        raise ValueError(f"unsupported scope: {scope}")

    if profile == "jpeg-baseline":
        command.extend(["--preset", "fast-jpeg", "--jpeg-quality", str(jpeg_quality)])
    elif profile == "htj2k-lossless-rpcl":
        command.extend(["--transfer-syntax", "htj2k-lossless-rpcl"])
    else:
        raise ValueError(f"unsupported profile: {profile}")

    if source_device_decode:
        command.append("--source-device-decode")
    return command


def build_wsi_dicom_profile_command(
    base_command: Sequence[str],
    source: Path,
    *,
    profile: str,
    scope: str,
    tile_size: int,
    jpeg_quality: int,
    backend: str,
    source_device_decode: bool,
    max_frames: int,
) -> list[str]:
    command = [
        *base_command,
        "profile",
        str(source),
        "--backend",
        backend,
        "--tile-size",
        str(tile_size),
        "--jpeg-quality",
        str(jpeg_quality),
        "--max-frames",
        str(max_frames),
        "--json",
    ]
    if scope in {"base", "pyramid"}:
        command.extend(["--level", "0"])
    else:
        raise ValueError(f"unsupported scope: {scope}")

    if profile == "htj2k-lossless-rpcl":
        command.extend(["--transfer-syntax", "htj2k-lossless-rpcl"])
    else:
        raise ValueError(f"device preflight does not support profile: {profile}")

    if source_device_decode:
        command.append("--source-device-decode")
    return command


def build_wsidicomizer_command(
    base_command: Sequence[str],
    source: Path,
    output_dir: Path,
    *,
    profile: str,
    scope: str,
    tile_size: int,
    jpeg_quality: int,
    workers: int,
    offset_table: str,
) -> list[str]:
    command = [
        *base_command,
        "--input",
        str(source),
        "--output",
        str(output_dir),
        "--tile-size",
        str(tile_size),
        "--workers",
        str(workers),
        "--offset-table",
        offset_table,
        "--no-confidential",
    ]
    if scope == "base":
        command.extend(["--levels", "0"])
    elif scope != "pyramid":
        raise ValueError(f"unsupported scope: {scope}")

    if profile == "jpeg-baseline":
        command.extend(["--format", "jpeg", "--quality", str(jpeg_quality)])
    elif profile == "htj2k-lossless-rpcl":
        command.extend(["--format", "htjpeg2000"])
    else:
        raise ValueError(f"unsupported profile: {profile}")
    return command


def command_for_tool(
    tool: str,
    *,
    wsi_dicom_command: Sequence[str],
    wsidicomizer_command: Sequence[str],
    source: Path,
    output_dir: Path,
    profile: str,
    scope: str,
    tile_size: int,
    jpeg_quality: int,
    workers: int,
    offset_table: str,
    device_source_decode: bool,
) -> list[str]:
    if tool == "wsi-dicom-cpu":
        return build_wsi_dicom_command(
            wsi_dicom_command,
            source,
            output_dir,
            profile=profile,
            scope=scope,
            tile_size=tile_size,
            jpeg_quality=jpeg_quality,
            backend="cpu",
            source_device_decode=False,
        )
    if tool == "wsi-dicom-device":
        return build_wsi_dicom_command(
            wsi_dicom_command,
            source,
            output_dir,
            profile=profile,
            scope=scope,
            tile_size=tile_size,
            jpeg_quality=jpeg_quality,
            backend="require-device",
            source_device_decode=device_source_decode,
        )
    if tool == "wsidicomizer":
        return build_wsidicomizer_command(
            wsidicomizer_command,
            source,
            output_dir,
            profile=profile,
            scope=scope,
            tile_size=tile_size,
            jpeg_quality=jpeg_quality,
            workers=workers,
            offset_table=offset_table,
        )
    raise ValueError(f"unsupported tool: {tool}")


def command_output(command: Sequence[str], *, cwd: Path, timeout_secs: int = 30) -> str | None:
    try:
        completed = subprocess.run(
            list(command),
            cwd=cwd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            timeout=timeout_secs,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if completed.returncode != 0:
        return None
    output = completed.stdout.strip()
    return output or None


def python_package_version(python_command: Sequence[str], package: str, *, cwd: Path) -> str | None:
    return command_output(
        [
            *python_command,
            "-c",
            "import importlib.metadata as m; print(m.version(%r))" % package,
        ],
        cwd=cwd,
    )


def cargo_package_version(*, cwd: Path) -> str | None:
    output = command_output(["cargo", "pkgid", "-p", "wsi-dicom"], cwd=cwd)
    if not output:
        return None
    package = output.rsplit("#", maxsplit=1)[-1]
    return package.rsplit("@", maxsplit=1)[-1] if "@" in package else package


def host_accelerator_info() -> dict:
    info: dict[str, object] = {}
    if platform.system() == "Darwin":
        output = command_output(
            ["system_profiler", "SPDisplaysDataType"],
            cwd=Path.cwd(),
            timeout_secs=15,
        )
        if output:
            info["system_profiler_displays"] = output
    if platform.system() == "Linux":
        output = command_output(
            [
                "nvidia-smi",
                "--query-gpu=name,driver_version,cuda_version,memory.total",
                "--format=csv,noheader",
            ],
            cwd=Path.cwd(),
            timeout_secs=15,
        )
        if output:
            info["nvidia_smi"] = output
    return info


def collect_environment(
    *,
    cwd: Path,
    wsi_dicom_command: Sequence[str],
    python_command: Sequence[str],
) -> dict:
    return {
        "created_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "hostname": socket.gethostname(),
        "platform": platform.platform(),
        "system": platform.system(),
        "machine": platform.machine(),
        "processor": platform.processor(),
        "python": sys.version.replace("\n", " "),
        "cwd": str(cwd),
        "git_commit": command_output(["git", "rev-parse", "HEAD"], cwd=cwd),
        "git_status": command_output(["git", "status", "--short"], cwd=cwd),
        "wsi_dicom_help": command_output([*wsi_dicom_command, "--help"], cwd=cwd),
        "wsi_dicom_package_version": cargo_package_version(cwd=cwd),
        "wsidicomizer_version": python_package_version(
            python_command, "wsidicomizer", cwd=cwd
        ),
        "wsidicom_version": python_package_version(python_command, "wsidicom", cwd=cwd),
        "openslide_python_version": python_package_version(
            python_command, "openslide-python", cwd=cwd
        ),
        "accelerator": host_accelerator_info(),
    }


def run_command(
    command: Sequence[str],
    *,
    cwd: Path,
    stdout_path: Path,
    stderr_path: Path,
    timeout_secs: int,
) -> dict:
    started = time.perf_counter()
    stdout_path.parent.mkdir(parents=True, exist_ok=True)
    with stdout_path.open("w", encoding="utf-8") as stdout_file, stderr_path.open(
        "w", encoding="utf-8"
    ) as stderr_file:
        try:
            completed = subprocess.run(
                list(command),
                cwd=cwd,
                stdout=stdout_file,
                stderr=stderr_file,
                text=True,
                timeout=timeout_secs,
                check=False,
            )
            returncode = completed.returncode
            status = "passed" if returncode == 0 else "failed"
        except subprocess.TimeoutExpired:
            returncode = None
            status = "timeout"
        except OSError as exc:
            returncode = None
            status = "failed"
            stderr_file.write(f"{type(exc).__name__}: {exc}\n")
    return {
        "status": status,
        "returncode": returncode,
        "elapsed_secs": time.perf_counter() - started,
        "stdout_path": str(stdout_path),
        "stderr_path": str(stderr_path),
    }


def count_output_files(output_dir: Path) -> tuple[int, int]:
    if not output_dir.exists():
        return 0, 0
    produced_files = 0
    output_bytes = 0
    for path in output_dir.rglob("*"):
        if path.is_file():
            produced_files += 1
            output_bytes += path.stat().st_size
    return produced_files, output_bytes


def dicom_metadata_from_dataset(path: Path, dataset) -> dict:
    return {
        "file": path.name,
        "transfer_syntax_uid": str(getattr(dataset.file_meta, "TransferSyntaxUID", "")),
        "rows": int(getattr(dataset, "Rows", 0) or 0),
        "columns": int(getattr(dataset, "Columns", 0) or 0),
        "number_of_frames": int(getattr(dataset, "NumberOfFrames", 0) or 0),
        "total_pixel_matrix_columns": int(
            getattr(dataset, "TotalPixelMatrixColumns", 0) or 0
        ),
        "total_pixel_matrix_rows": int(getattr(dataset, "TotalPixelMatrixRows", 0) or 0),
    }


def collect_dicom_outputs(output_dir: Path) -> tuple[list[dict], str | None]:
    if not output_dir.exists():
        return [], None
    try:
        import pydicom
    except ImportError as exc:
        return [], f"pydicom unavailable: {exc}"

    outputs = []
    for path in sorted(output_dir.rglob("*.dcm")):
        try:
            dataset = pydicom.dcmread(str(path), stop_before_pixels=True)
        except Exception as exc:  # noqa: BLE001 - evidence should report failures.
            return outputs, f"failed to read DICOM metadata from {path}: {exc}"
        outputs.append(dicom_metadata_from_dataset(path, dataset))
    return outputs, None


def validate_output(
    *,
    wsi_dicom_command: Sequence[str],
    output_dir: Path,
    artifact_dir: Path,
    cwd: Path,
    timeout_secs: int,
) -> dict:
    command = [
        *wsi_dicom_command,
        "validate",
        str(output_dir),
        "--strict",
        "--json",
        "--command-timeout-secs",
        str(max(1, min(timeout_secs, 300))),
    ]
    result = run_command(
        command,
        cwd=cwd,
        stdout_path=artifact_dir / "validate.stdout.json",
        stderr_path=artifact_dir / "validate.stderr.txt",
        timeout_secs=timeout_secs,
    )
    return {
        "command": command,
        "status": result["status"],
        "returncode": result["returncode"],
        "elapsed_secs": result["elapsed_secs"],
        "stdout_path": result["stdout_path"],
        "stderr_path": result["stderr_path"],
    }


def read_profile_report(stdout_path: Path) -> tuple[dict | None, str | None]:
    try:
        text = stdout_path.read_text(encoding="utf-8").strip()
    except OSError as exc:
        return None, f"failed to read profile stdout: {exc}"
    if not text:
        return None, "profile stdout was empty"
    try:
        return json.loads(text), None
    except json.JSONDecodeError as exc:
        return None, f"failed to parse profile JSON: {exc}"


def first_text_line(path: Path) -> str | None:
    try:
        for line in path.read_text(encoding="utf-8").splitlines():
            if line.strip():
                return line.strip()
    except OSError:
        return None
    return None


def validation_failure_message(validation: dict) -> str:
    stderr_path = validation.get("stderr_path")
    if stderr_path:
        detail = first_text_line(Path(stderr_path))
        if detail:
            return f"validation failed: {detail}"
    status = validation.get("status") or "failed"
    return f"validation {status}"


def profile_metric(report: dict | None, name: str, default: int = 0) -> int:
    if not report:
        return default
    try:
        return int(report.get("metrics", {}).get(name, default) or default)
    except (TypeError, ValueError):
        return default


def evaluate_device_preflight(
    *,
    cpu_result: dict,
    device_result: dict,
    min_speedup: float,
    min_device_frame_pct: float,
) -> dict:
    cpu_report, cpu_error = read_profile_report(Path(cpu_result["stdout_path"]))
    device_report, device_error = read_profile_report(Path(device_result["stdout_path"]))
    status = "passed"
    reason = None
    if cpu_result["status"] != "passed":
        status = "failed"
        reason = "CPU preflight profile did not pass"
        detail = first_text_line(Path(cpu_result["stderr_path"]))
        if detail:
            reason = f"{reason}: {detail}"
    elif device_result["status"] != "passed":
        status = device_result["status"]
        reason = "device preflight profile did not pass"
        detail = first_text_line(Path(device_result["stderr_path"]))
        if detail:
            reason = f"{reason}: {detail}"
    elif cpu_error:
        status = "failed"
        reason = cpu_error
    elif device_error:
        status = "failed"
        reason = device_error
    else:
        total_frames = profile_metric(device_report, "total_frames")
        gpu_encode_frames = profile_metric(device_report, "gpu_encode_frames")
        device_frame_pct = (
            100.0 * gpu_encode_frames / total_frames if total_frames > 0 else 0.0
        )
        speedup = (
            float(cpu_result["elapsed_secs"]) / float(device_result["elapsed_secs"])
            if float(device_result["elapsed_secs"]) > 0
            else 0.0
        )
        if total_frames == 0:
            status = "failed"
            reason = "device preflight reported zero frames"
        elif device_frame_pct < min_device_frame_pct:
            status = "failed"
            reason = (
                "device preflight used device encode for "
                f"{device_frame_pct:.1f}% of frames; required {min_device_frame_pct:.1f}%"
            )
        elif speedup < min_speedup:
            status = "failed"
            reason = (
                f"device preflight speedup was {speedup:.3f}x; "
                f"required at least {min_speedup:.3f}x"
            )

    return {
        "status": status,
        "reason": reason,
        "cpu": {
            **cpu_result,
            "report": cpu_report,
            "report_error": cpu_error,
        },
        "device": {
            **device_result,
            "report": device_report,
            "report_error": device_error,
        },
        "cpu_elapsed_secs": cpu_result["elapsed_secs"],
        "device_elapsed_secs": device_result["elapsed_secs"],
        "speedup_vs_cpu": (
            float(cpu_result["elapsed_secs"]) / float(device_result["elapsed_secs"])
            if float(device_result["elapsed_secs"]) > 0
            else None
        ),
        "device_frame_pct": (
            100.0
            * profile_metric(device_report, "gpu_encode_frames")
            / profile_metric(device_report, "total_frames")
            if profile_metric(device_report, "total_frames") > 0
            else None
        ),
    }


def run_device_preflight(
    *,
    wsi_dicom_command: Sequence[str],
    slide: Slide,
    artifact_dir: Path,
    cwd: Path,
    profile: str,
    scope: str,
    tile_size: int,
    jpeg_quality: int,
    source_device_decode: bool,
    max_frames: int,
    timeout_secs: int,
    min_speedup: float,
    min_device_frame_pct: float,
) -> dict:
    if profile != "htj2k-lossless-rpcl":
        return {
            "status": "skipped",
            "reason": f"device preflight does not support profile: {profile}",
        }

    preflight_dir = artifact_dir / "device-preflight"
    cpu_command = build_wsi_dicom_profile_command(
        wsi_dicom_command,
        slide.path,
        profile=profile,
        scope=scope,
        tile_size=tile_size,
        jpeg_quality=jpeg_quality,
        backend="cpu",
        source_device_decode=False,
        max_frames=max_frames,
    )
    device_command = build_wsi_dicom_profile_command(
        wsi_dicom_command,
        slide.path,
        profile=profile,
        scope=scope,
        tile_size=tile_size,
        jpeg_quality=jpeg_quality,
        backend="require-device",
        source_device_decode=source_device_decode,
        max_frames=max_frames,
    )
    cpu_result = run_command(
        cpu_command,
        cwd=cwd,
        stdout_path=preflight_dir / "cpu.stdout.json",
        stderr_path=preflight_dir / "cpu.stderr.txt",
        timeout_secs=timeout_secs,
    )
    device_result = run_command(
        device_command,
        cwd=cwd,
        stdout_path=preflight_dir / "device.stdout.json",
        stderr_path=preflight_dir / "device.stderr.txt",
        timeout_secs=timeout_secs,
    )
    preflight = evaluate_device_preflight(
        cpu_result={**cpu_result, "command": cpu_command},
        device_result={**device_result, "command": device_command},
        min_speedup=min_speedup,
        min_device_frame_pct=min_device_frame_pct,
    )
    preflight["max_frames"] = max_frames
    preflight["timeout_secs"] = timeout_secs
    preflight["min_speedup"] = min_speedup
    preflight["min_device_frame_pct"] = min_device_frame_pct
    return preflight


def preflight_failure_row(
    *,
    slide: Slide,
    tool: str,
    command: Sequence[str],
    output_dir: Path,
    profile: str,
    scope: str,
    run_index: int,
    preflight: dict,
    system_label: str | None,
) -> dict:
    status = "preflight-failed"
    if preflight.get("status") == "timeout":
        status = "preflight-timeout"
    return attach_result_context(
        {
            "slide": slide.slide_id,
            "display_name": slide.display_name,
            "gdc_file_id": slide.gdc_file_id,
            "source_path": str(slide.path),
            "tool": tool,
            "profile": profile,
            "scope": scope,
            "run_index": run_index,
            "status": status,
            "returncode": preflight.get("device", {}).get("returncode"),
            "elapsed_secs": preflight.get("device_elapsed_secs", 0.0) or 0.0,
            "output_dir": str(output_dir),
            "produced_files": 0,
            "output_bytes": 0,
            "command": list(command),
            "stdout_path": preflight.get("device", {}).get("stdout_path"),
            "stderr_path": preflight.get("device", {}).get("stderr_path"),
            "preflight": preflight,
            "error": preflight.get("reason") or "device preflight failed",
        },
        system_label=system_label,
    )


def benchmark_trial(
    *,
    slide: Slide,
    tool: str,
    command: Sequence[str],
    output_dir: Path,
    artifact_dir: Path,
    cwd: Path,
    timeout_secs: int,
    run_index: int,
    profile: str,
    scope: str,
    validate: bool,
    wsi_dicom_command: Sequence[str],
    system_label: str | None = None,
    preflight: dict | None = None,
) -> dict:
    if output_dir.exists():
        return attach_result_context(
            {
                "slide": slide.slide_id,
                "display_name": slide.display_name,
                "gdc_file_id": slide.gdc_file_id,
                "source_path": str(slide.path),
                "tool": tool,
                "profile": profile,
                "scope": scope,
                "run_index": run_index,
                "status": "failed",
                "returncode": None,
                "elapsed_secs": 0.0,
                "output_dir": str(output_dir),
                "produced_files": 0,
                "output_bytes": 0,
                "command": list(command),
                "error": "output directory already exists; use --resume to skip completed trials or a new --run-label",
                "preflight": preflight,
            },
            system_label=system_label,
        )
    output_dir.parent.mkdir(parents=True, exist_ok=True)
    result = run_command(
        command,
        cwd=cwd,
        stdout_path=artifact_dir / "stdout.txt",
        stderr_path=artifact_dir / "stderr.txt",
        timeout_secs=timeout_secs,
    )
    produced_files, output_bytes = count_output_files(output_dir)
    dicom_outputs, dicom_metadata_error = collect_dicom_outputs(output_dir)
    row = {
        "slide": slide.slide_id,
        "display_name": slide.display_name,
        "gdc_file_id": slide.gdc_file_id,
        "source_path": str(slide.path),
        "tool": tool,
        "profile": profile,
        "scope": scope,
        "run_index": run_index,
        "status": result["status"],
        "returncode": result["returncode"],
        "elapsed_secs": result["elapsed_secs"],
        "output_dir": str(output_dir),
        "produced_files": produced_files,
        "output_bytes": output_bytes,
        "command": list(command),
        "stdout_path": result["stdout_path"],
        "stderr_path": result["stderr_path"],
        "dicom_outputs": dicom_outputs,
    }
    if dicom_metadata_error:
        row["dicom_metadata_error"] = dicom_metadata_error
    if validate and result["status"] == "passed":
        validation = validate_output(
            wsi_dicom_command=wsi_dicom_command,
            output_dir=output_dir,
            artifact_dir=artifact_dir,
            cwd=cwd,
            timeout_secs=timeout_secs,
        )
        row["validation"] = validation
        if validation["status"] != "passed":
            row["status"] = "failed"
            row["error"] = validation_failure_message(validation)
    if preflight:
        row["preflight"] = preflight
    return attach_result_context(row, system_label=system_label)


def read_jsonl(path: Path) -> list[dict]:
    if not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            rows.append(json.loads(line))
    return rows


def append_jsonl(path: Path, row: dict) -> None:
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(row, sort_keys=True))
        handle.write("\n")


def completed_result_keys(rows: Iterable[dict]) -> set[tuple[str, str, str, str, int]]:
    return {
        (
            row["slide"],
            row["tool"],
            row.get("profile", ""),
            row.get("scope", ""),
            int(row.get("run_index", 1)),
        )
        for row in rows
        if "slide" in row and "tool" in row
    }


def infer_result_label(row: dict, *, result_set: str | None = None) -> str:
    existing = row.get("result_label")
    if existing:
        return str(existing)

    tool = str(row.get("tool", ""))
    if tool == "wsi-dicom-cpu":
        return "wsi-dicom CPU"
    if tool == "wsidicomizer":
        return "wsidicomizer"
    if tool == "wsi-dicom-device":
        hints = [
            row.get("system_label"),
            result_set,
            row.get("result_set"),
            row.get("output_dir"),
            row.get("stdout_path"),
            row.get("stderr_path"),
            row.get("source_path"),
        ]
        hint = " ".join(str(value) for value in hints if value).lower()
        if "cuda" in hint or "nvidia" in hint:
            return "wsi-dicom CUDA"
        if "metal" in hint or "darwin" in hint:
            return "wsi-dicom Metal"
        system_label = row.get("system_label")
        if system_label:
            return f"wsi-dicom Device ({system_label})"
        return "wsi-dicom Device"
    return tool or "unknown"


def attach_result_context(
    row: dict,
    *,
    system_label: str | None = None,
    result_set: str | None = None,
) -> dict:
    if system_label:
        row.setdefault("system_label", system_label)
    if result_set:
        row.setdefault("result_set", result_set)
    row.setdefault("result_label", infer_result_label(row, result_set=result_set))
    return row


def write_json(path: Path, data: object) -> None:
    path.write_text(json.dumps(data, indent=2, sort_keys=True), encoding="utf-8")


def write_csv(path: Path, rows: Sequence[dict]) -> None:
    fields = [
        "slide",
        "display_name",
        "gdc_file_id",
        "tool",
        "result_label",
        "system_label",
        "result_set",
        "profile",
        "scope",
        "run_index",
        "status",
        "returncode",
        "elapsed_secs",
        "produced_files",
        "output_bytes",
        "output_dir",
        "stdout_path",
        "stderr_path",
        "error",
        "preflight_status",
        "preflight_reason",
        "preflight_speedup_vs_cpu",
        "preflight_device_frame_pct",
        "dicom_metadata_error",
    ]
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for row in rows:
            csv_row = {field: row.get(field) for field in fields}
            preflight = row.get("preflight") or {}
            csv_row["preflight_status"] = preflight.get("status")
            csv_row["preflight_reason"] = preflight.get("reason")
            csv_row["preflight_speedup_vs_cpu"] = preflight.get("speedup_vs_cpu")
            csv_row["preflight_device_frame_pct"] = preflight.get("device_frame_pct")
            writer.writerow(csv_row)


def average_passed_seconds(
    rows: Sequence[dict],
    *,
    slide: str,
    result_label: str,
    profile: str,
    scope: str,
) -> float | None:
    passed = [
        row
        for row in rows
        if row.get("slide") == slide
        and infer_result_label(row) == result_label
        and row.get("profile") == profile
        and row.get("scope") == scope
        and row.get("status") == "passed"
    ]
    if not passed:
        return None
    return sum(float(row["elapsed_secs"]) for row in passed) / len(passed)


def status_summary(
    rows: Sequence[dict],
    *,
    slide: str,
    result_label: str,
    profile: str,
    scope: str,
) -> str:
    selected = [
        row
        for row in rows
        if row.get("slide") == slide
        and infer_result_label(row) == result_label
        and row.get("profile") == profile
        and row.get("scope") == scope
    ]
    if not selected:
        return "missing"
    passed = sum(1 for row in selected if row.get("status") == "passed")
    if passed == len(selected):
        return "passed"
    if passed:
        return f"partial {passed}/{len(selected)}"
    return str(selected[-1].get("status", "failed"))


def has_rows_for_cell(
    rows: Sequence[dict],
    *,
    slide: str,
    profile: str,
    scope: str,
) -> bool:
    return any(
        row.get("slide") == slide
        and row.get("profile") == profile
        and row.get("scope") == scope
        for row in rows
    )


def format_seconds(value: float | None) -> str:
    return "" if value is None else f"{value:.3f}"


def format_speedup(numerator: float | None, denominator: float | None) -> str:
    if numerator is None or denominator is None or denominator <= 0:
        return ""
    return f"{numerator / denominator:.2f}x"


def result_label_sort_key(label: str) -> tuple[int, str]:
    order = {
        "wsi-dicom CPU": 0,
        "wsi-dicom Metal": 1,
        "wsi-dicom CUDA": 2,
        "wsi-dicom Device": 3,
        "wsidicomizer": 4,
    }
    return order.get(label, 50), label


def render_markdown_summary(rows: Sequence[dict], *, title: str) -> str:
    slides = sorted({row["slide"] for row in rows})
    profiles = sorted({row.get("profile", "") for row in rows})
    scopes = sorted({row.get("scope", "") for row in rows})
    labels = sorted({infer_result_label(row) for row in rows}, key=result_label_sort_key)
    device_labels = [
        label
        for label in labels
        if any(
            row.get("tool") == "wsi-dicom-device" and infer_result_label(row) == label
            for row in rows
        )
    ]
    cpu_label = "wsi-dicom CPU" if "wsi-dicom CPU" in labels else None
    dicomizer_label = "wsidicomizer" if "wsidicomizer" in labels else None

    header = ["Slide", "Profile", "Scope"]
    separator = ["---", "---", "---"]
    for label in labels:
        header.extend([f"{label} status", f"{label} seconds"])
        separator.extend(["---", "---:"])
    for label in device_labels:
        if cpu_label:
            header.append(f"{label} vs CPU")
            separator.append("---:")
        if dicomizer_label:
            header.append(f"{label} vs wsidicomizer")
            separator.append("---:")

    lines = [
        f"# {title}",
        "",
        "| " + " | ".join(header) + " |",
        "| " + " | ".join(separator) + " |",
    ]
    for slide in slides:
        for profile in profiles:
            for scope in scopes:
                if not has_rows_for_cell(rows, slide=slide, profile=profile, scope=scope):
                    continue
                seconds_by_label = {
                    label: average_passed_seconds(
                        rows,
                        slide=slide,
                        result_label=label,
                        profile=profile,
                        scope=scope,
                    )
                    for label in labels
                }
                cells = [slide, profile, scope]
                for label in labels:
                    cells.extend(
                        [
                            status_summary(
                                rows,
                                slide=slide,
                                result_label=label,
                                profile=profile,
                                scope=scope,
                            ),
                            format_seconds(seconds_by_label[label]),
                        ]
                    )
                for label in device_labels:
                    if cpu_label:
                        cells.append(
                            format_speedup(seconds_by_label[cpu_label], seconds_by_label[label])
                        )
                    if dicomizer_label:
                        cells.append(
                            format_speedup(
                                seconds_by_label[dicomizer_label],
                                seconds_by_label[label],
                            )
                        )
                lines.append(
                    "| " + " | ".join(cells) + " |"
                )
    lines.extend(
        [
            "",
            "Speedups above 1.00x mean the device-labeled `wsi-dicom` run was faster than the comparison tool.",
            "Failed, timed-out, and unsupported runs remain in `results.jsonl` and are not used for speedup ratios.",
            "`preflight-failed` rows mean the bounded device route profile did not meet the configured publication threshold, so the full conversion was skipped.",
            "The `htj2k-lossless-rpcl` profile maps wsidicomizer to its HTJ2K setting because RPCL-specific control is not exposed by that CLI.",
        ]
    )
    return "\n".join(lines) + "\n"


def write_planned_commands(path: Path, commands: Sequence[dict]) -> None:
    with path.open("w", encoding="utf-8") as handle:
        for command in commands:
            handle.write(json.dumps(command, sort_keys=True))
            handle.write("\n")


def rows_from_result_dirs(result_dirs: Sequence[Path], *, annotate: bool = False) -> list[dict]:
    rows: list[dict] = []
    for result_dir in result_dirs:
        path = result_dir / "results.jsonl"
        if not path.exists():
            raise FileNotFoundError(f"missing results.jsonl in {result_dir}")
        for row in read_jsonl(path):
            if annotate:
                row = attach_result_context(dict(row), result_set=result_dir.name)
            rows.append(row)
    return rows


def default_python_command() -> str:
    venv_python = Path("./.venv/bin/python")
    return str(venv_python) if venv_python.exists() else sys.executable


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Benchmark wsi-dicom and comparable converters on local GDC downloads."
    )
    parser.add_argument("--downloads-root", type=Path, default=Path.home() / "Downloads")
    parser.add_argument("--gdc-glob", default="gdc_download*")
    parser.add_argument("--out", type=Path, default=Path("bench/results"))
    parser.add_argument("--run-label")
    parser.add_argument(
        "--merge-results",
        nargs="+",
        type=Path,
        help="Merge existing benchmark result directories instead of running conversions.",
    )
    parser.add_argument("--profile", action="append", choices=PROFILE_CHOICES)
    parser.add_argument("--scope", action="append", choices=SCOPE_CHOICES)
    parser.add_argument("--tools", nargs="+", choices=TOOL_CHOICES, default=list(TOOL_CHOICES))
    parser.add_argument("--runs", type=int, default=1)
    parser.add_argument("--tile-size", type=int, default=512)
    parser.add_argument("--jpeg-quality", type=int, default=80)
    parser.add_argument("--workers", type=int, default=min(os.cpu_count() or 1, 8))
    parser.add_argument(
        "--offset-table", choices=("basic", "extended", "empty"), default="extended"
    )
    parser.add_argument("--timeout-secs", type=int, default=7200)
    parser.add_argument("--max-slides", type=int)
    parser.add_argument(
        "--only",
        action="append",
        default=[],
        help="Case-insensitive substring filter against slide id, display name, path, or GDC id.",
    )
    parser.add_argument("--wsi-dicom-command", default="target/release/wsi-dicom")
    parser.add_argument("--wsidicomizer-command", default="./.venv/bin/wsidicomizer")
    parser.add_argument("--python-command", default=default_python_command())
    parser.add_argument(
        "--system-label",
        help="Human-readable host/backend label to preserve in result rows, e.g. macos-metal or cuda-rtx4070.",
    )
    parser.add_argument(
        "--device-source-decode",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Add --source-device-decode to wsi-dicom-device runs.",
    )
    parser.add_argument(
        "--device-preflight",
        action=argparse.BooleanOptionalAction,
        default=False,
        help="Run bounded CPU/device route profiles before wsi-dicom-device conversions.",
    )
    parser.add_argument("--device-preflight-frames", type=int, default=64)
    parser.add_argument("--device-preflight-timeout-secs", type=int, default=120)
    parser.add_argument(
        "--device-preflight-min-speedup",
        type=float,
        default=1.0,
        help="Require the device preflight wall time to be at least this fast versus CPU.",
    )
    parser.add_argument(
        "--device-preflight-min-device-frame-pct",
        type=float,
        default=100.0,
        help="Require at least this percent of preflight frames to use device encode.",
    )
    parser.add_argument("--probe-slide-metadata", action="store_true")
    parser.add_argument("--validate", action="store_true")
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--list-slides", action="store_true")
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    if args.merge_results:
        run_label = args.run_label or dt.datetime.now().strftime("merged-%Y%m%d-%H%M%S")
        run_dir = args.out / run_label
        if run_dir.exists() and not args.resume:
            print(f"Output run directory already exists: {run_dir}", file=sys.stderr)
            return 2
        run_dir.mkdir(parents=True, exist_ok=True)
        rows = rows_from_result_dirs(args.merge_results, annotate=True)
        for row in rows:
            append_jsonl(run_dir / "results.jsonl", row)
        write_csv(run_dir / "results.csv", rows)
        write_json(
            run_dir / "merged-runs.json",
            {"inputs": [str(path) for path in args.merge_results], "rows": len(rows)},
        )
        (run_dir / "summary.md").write_text(
            render_markdown_summary(rows, title=f"GDC WSI benchmark {run_label}"),
            encoding="utf-8",
        )
        print(f"Merged {len(rows)} rows into {run_dir}")
        return 0

    if args.runs < 1:
        print("--runs must be at least 1", file=sys.stderr)
        return 2
    if args.device_preflight_frames < 1:
        print("--device-preflight-frames must be at least 1", file=sys.stderr)
        return 2
    if args.device_preflight_timeout_secs < 1:
        print("--device-preflight-timeout-secs must be at least 1", file=sys.stderr)
        return 2
    if args.device_preflight_min_speedup < 0:
        print("--device-preflight-min-speedup must be non-negative", file=sys.stderr)
        return 2
    if not 0 <= args.device_preflight_min_device_frame_pct <= 100:
        print(
            "--device-preflight-min-device-frame-pct must be between 0 and 100",
            file=sys.stderr,
        )
        return 2

    repo_root = Path.cwd()
    profiles = args.profile or ["htj2k-lossless-rpcl"]
    scopes = args.scope or ["base"]
    slides = select_slides(
        discover_gdc_slides(
            args.downloads_root,
            gdc_glob=args.gdc_glob,
            probe_metadata=args.probe_slide_metadata,
        ),
        only_filters=args.only,
        max_slides=args.max_slides,
    )
    if args.list_slides:
        for slide in slides:
            print(
                json.dumps(
                    {
                        "slide_id": slide.slide_id,
                        "display_name": slide.display_name,
                        "path": str(slide.path),
                        "gdc_file_id": slide.gdc_file_id,
                        "bytes_on_disk": slide.bytes_on_disk,
                    },
                    sort_keys=True,
                )
            )
        return 0
    if not slides:
        print("No supported GDC slide files found for the selected filters.", file=sys.stderr)
        return 2

    run_label = args.run_label or dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    run_dir = args.out / run_label
    if run_dir.exists() and not args.resume:
        print(f"Output run directory already exists: {run_dir}", file=sys.stderr)
        return 2
    run_dir.mkdir(parents=True, exist_ok=True)

    wsi_dicom_command = split_command(args.wsi_dicom_command)
    wsidicomizer_command = split_command(args.wsidicomizer_command)
    python_command = split_command(args.python_command)

    write_json(run_dir / "slides.json", [slide_to_json(slide) for slide in slides])
    write_json(
        run_dir / "benchmark-run.json",
        {
            "profiles": profiles,
            "scopes": scopes,
            "tools": args.tools,
            "runs": args.runs,
            "tile_size": args.tile_size,
            "jpeg_quality": args.jpeg_quality,
            "workers": args.workers,
            "offset_table": args.offset_table,
            "timeout_secs": args.timeout_secs,
            "device_source_decode": args.device_source_decode,
            "device_preflight": args.device_preflight,
            "device_preflight_frames": args.device_preflight_frames,
            "device_preflight_timeout_secs": args.device_preflight_timeout_secs,
            "device_preflight_min_speedup": args.device_preflight_min_speedup,
            "device_preflight_min_device_frame_pct": (
                args.device_preflight_min_device_frame_pct
            ),
            "system_label": args.system_label,
            "validate": args.validate,
            "downloads_root": str(args.downloads_root.expanduser()),
            "gdc_glob": args.gdc_glob,
        },
    )
    write_json(
        run_dir / "environment.json",
        collect_environment(
            cwd=repo_root,
            wsi_dicom_command=wsi_dicom_command,
            python_command=python_command,
        ),
    )

    results_path = run_dir / "results.jsonl"
    rows = read_jsonl(results_path) if args.resume else []
    completed = completed_result_keys(rows)
    planned_commands = []

    for slide in slides:
        for profile in profiles:
            for scope in scopes:
                for tool in args.tools:
                    for run_index in range(1, args.runs + 1):
                        output_dir = (
                            run_dir
                            / "outputs"
                            / slide.slide_id
                            / profile
                            / scope
                            / tool
                            / f"run-{run_index}"
                        )
                        artifact_dir = (
                            run_dir
                            / "artifacts"
                            / slide.slide_id
                            / profile
                            / scope
                            / tool
                            / f"run-{run_index}"
                        )
                        command = command_for_tool(
                            tool,
                            wsi_dicom_command=wsi_dicom_command,
                            wsidicomizer_command=wsidicomizer_command,
                            source=slide.path,
                            output_dir=output_dir,
                            profile=profile,
                            scope=scope,
                            tile_size=args.tile_size,
                            jpeg_quality=args.jpeg_quality,
                            workers=args.workers,
                            offset_table=args.offset_table,
                            device_source_decode=args.device_source_decode,
                        )
                        key = (slide.slide_id, tool, profile, scope, run_index)
                        planned = {
                            "slide": slide.slide_id,
                            "display_name": slide.display_name,
                            "tool": tool,
                            "profile": profile,
                            "scope": scope,
                            "run_index": run_index,
                            "command": command,
                            "output_dir": str(output_dir),
                            "system_label": args.system_label,
                            "result_label": infer_result_label(
                                {
                                    "tool": tool,
                                    "system_label": args.system_label,
                                    "output_dir": str(output_dir),
                                },
                                result_set=run_label,
                            ),
                        }
                        if args.device_preflight and tool == "wsi-dicom-device":
                            planned["device_preflight"] = {
                                "frames": args.device_preflight_frames,
                                "timeout_secs": args.device_preflight_timeout_secs,
                                "min_speedup": args.device_preflight_min_speedup,
                                "min_device_frame_pct": (
                                    args.device_preflight_min_device_frame_pct
                                ),
                            }
                        planned_commands.append(planned)
                        if args.dry_run or key in completed:
                            continue
                        print(
                            f"{slide.slide_id} {profile} {scope} {tool} run {run_index}",
                            flush=True,
                        )
                        preflight = None
                        if args.device_preflight and tool == "wsi-dicom-device":
                            preflight = run_device_preflight(
                                wsi_dicom_command=wsi_dicom_command,
                                slide=slide,
                                artifact_dir=artifact_dir,
                                cwd=repo_root,
                                profile=profile,
                                scope=scope,
                                tile_size=args.tile_size,
                                jpeg_quality=args.jpeg_quality,
                                source_device_decode=args.device_source_decode,
                                max_frames=args.device_preflight_frames,
                                timeout_secs=args.device_preflight_timeout_secs,
                                min_speedup=args.device_preflight_min_speedup,
                                min_device_frame_pct=(
                                    args.device_preflight_min_device_frame_pct
                                ),
                            )
                            if preflight.get("status") not in {"passed", "skipped"}:
                                row = preflight_failure_row(
                                    slide=slide,
                                    tool=tool,
                                    command=command,
                                    output_dir=output_dir,
                                    profile=profile,
                                    scope=scope,
                                    run_index=run_index,
                                    preflight=preflight,
                                    system_label=args.system_label,
                                )
                                rows.append(row)
                                append_jsonl(results_path, row)
                                continue
                        row = benchmark_trial(
                            slide=slide,
                            tool=tool,
                            command=command,
                            output_dir=output_dir,
                            artifact_dir=artifact_dir,
                            cwd=repo_root,
                            timeout_secs=args.timeout_secs,
                            run_index=run_index,
                            profile=profile,
                            scope=scope,
                            validate=args.validate,
                            wsi_dicom_command=wsi_dicom_command,
                            system_label=args.system_label,
                            preflight=preflight,
                        )
                        rows.append(row)
                        append_jsonl(results_path, row)

    write_planned_commands(run_dir / "planned_commands.jsonl", planned_commands)
    if rows:
        write_csv(run_dir / "results.csv", rows)
        (run_dir / "summary.md").write_text(
            render_markdown_summary(rows, title=f"GDC WSI benchmark {run_label}"),
            encoding="utf-8",
        )
    if args.dry_run:
        print(f"Dry run wrote {len(planned_commands)} planned commands to {run_dir}")
    else:
        print(f"Wrote benchmark results to {run_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
