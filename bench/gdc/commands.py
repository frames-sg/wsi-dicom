"""Pure command construction for benchmarked tools."""

from __future__ import annotations

from pathlib import Path
from typing import Sequence


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
