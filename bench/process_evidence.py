"""One bounded subprocess owner for benchmark and workbench evidence."""

from __future__ import annotations

import os
import platform
import re
import signal
import subprocess
import threading
import time
from pathlib import Path
from typing import BinaryIO, Sequence


DEFAULT_MAX_OUTPUT_BYTES = 1024 * 1024
READ_CHUNK_BYTES = 64 * 1024


class ProcessEvidenceError(RuntimeError):
    """A subprocess could not be launched under the requested evidence policy."""


def read_bounded_text(path: Path, limit: int = DEFAULT_MAX_OUTPUT_BYTES) -> str:
    with path.open("rb") as stream:
        data = stream.read(limit + 1)
    suffix = "\n[truncated]" if len(data) > limit else ""
    return data[:limit].decode("utf-8", errors="replace") + suffix


def parse_bsd_time_metrics(text: str) -> dict | None:
    """Parse the stable fields emitted by macOS `/usr/bin/time -lp`."""
    patterns = {
        "wall_seconds": r"(?m)^real\s+([0-9]+(?:\.[0-9]+)?)\s*$",
        "user_seconds": r"(?m)^user\s+([0-9]+(?:\.[0-9]+)?)\s*$",
        "system_seconds": r"(?m)^sys\s+([0-9]+(?:\.[0-9]+)?)\s*$",
        "peak_rss_bytes": r"(?m)^\s*([0-9]+)\s+maximum resident set size\s*$",
    }
    matches = {name: re.search(pattern, text) for name, pattern in patterns.items()}
    if any(match is None for match in matches.values()):
        return None
    return {
        "wall_seconds": float(matches["wall_seconds"].group(1)),
        "user_seconds": float(matches["user_seconds"].group(1)),
        "system_seconds": float(matches["system_seconds"].group(1)),
        "peak_rss_bytes": int(matches["peak_rss_bytes"].group(1)),
    }


def parse_gnu_time_metrics(text: str) -> dict | None:
    """Parse the locale-independent format requested from GNU `time`."""
    patterns = {
        "wall_seconds": r"(?m)^wall_seconds=([0-9]+(?:\.[0-9]+)?)$",
        "user_seconds": r"(?m)^user_seconds=([0-9]+(?:\.[0-9]+)?)$",
        "system_seconds": r"(?m)^system_seconds=([0-9]+(?:\.[0-9]+)?)$",
        "peak_rss_kib": r"(?m)^peak_rss_kib=([0-9]+)$",
    }
    matches = {name: re.search(pattern, text) for name, pattern in patterns.items()}
    if any(match is None for match in matches.values()):
        return None
    return {
        "wall_seconds": float(matches["wall_seconds"].group(1)),
        "user_seconds": float(matches["user_seconds"].group(1)),
        "system_seconds": float(matches["system_seconds"].group(1)),
        "peak_rss_bytes": int(matches["peak_rss_kib"].group(1)) * 1024,
    }


def _measurement_command(
    command: Sequence[str], resource_path: Path, system_name: str | None = None
) -> list[str]:
    time_path = Path("/usr/bin/time")
    if not time_path.is_file():
        raise ProcessEvidenceError("resource measurement requires /usr/bin/time")
    system_name = platform.system() if system_name is None else system_name
    if system_name == "Darwin":
        return [str(time_path), "-lp", "-o", str(resource_path), *command]
    if system_name == "Linux":
        output_format = (
            "wall_seconds=%e\nuser_seconds=%U\nsystem_seconds=%S\npeak_rss_kib=%M"
        )
        return [
            str(time_path),
            "-f",
            output_format,
            "-o",
            str(resource_path),
            *command,
        ]
    raise ProcessEvidenceError(
        f"resource measurement is unsupported on {system_name or 'unknown platform'}"
    )


def _resource_metrics(path: Path, system_name: str, limit: int = 1024 * 1024) -> dict | None:
    try:
        with path.open("rb") as stream:
            text = stream.read(limit + 1)[:limit].decode("utf-8", errors="replace")
    except OSError:
        return None
    if system_name == "Darwin":
        return parse_bsd_time_metrics(text)
    if system_name == "Linux":
        return parse_gnu_time_metrics(text)
    return None


def _capture_pipe(
    stream: BinaryIO,
    path: Path,
    max_output_bytes: int,
    capture: dict[str, int | bool | str],
    name: str,
) -> None:
    observed = 0
    retained = 0
    with stream, path.open("wb") as output:
        while True:
            chunk = stream.read(READ_CHUNK_BYTES)
            if not chunk:
                break
            observed += len(chunk)
            remaining = max_output_bytes - retained
            if remaining > 0:
                kept = chunk[:remaining]
                output.write(kept)
                retained += len(kept)
    capture[f"{name}_bytes"] = observed
    capture[f"{name}_truncated"] = observed > max_output_bytes


def _capture_pipe_guarded(
    stream: BinaryIO,
    path: Path,
    max_output_bytes: int,
    capture: dict[str, int | bool | str],
    name: str,
) -> None:
    try:
        _capture_pipe(stream, path, max_output_bytes, capture, name)
    except Exception as exc:  # The main thread re-raises this as policy failure.
        capture[f"{name}_capture_error"] = f"{type(exc).__name__}: {exc}"
    finally:
        stream.close()


def _terminate_process_tree(process: subprocess.Popen[bytes]) -> None:
    if os.name == "posix":
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=0.5)
        except subprocess.TimeoutExpired:
            pass
        # The process-group leader may exit while a descendant ignores SIGTERM.
        # Kill the group even when the immediate child has already been reaped.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    else:
        process.kill()
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired as exc:
        raise ProcessEvidenceError("timed-out process tree could not be terminated") from exc


def run_bounded_command(
    command: Sequence[str],
    *,
    stdout_path: Path,
    stderr_path: Path,
    timeout_secs: int,
    cwd: Path | None = None,
    measure_resources: bool = False,
    max_output_bytes: int = DEFAULT_MAX_OUTPUT_BYTES,
) -> dict:
    """Run one command with capped output and retain uninterpreted lifecycle facts."""
    if not command or any(not isinstance(part, str) or not part for part in command):
        raise ProcessEvidenceError("command must contain non-empty string arguments")
    if timeout_secs <= 0:
        raise ProcessEvidenceError("timeout_secs must be positive")
    if (
        isinstance(max_output_bytes, bool)
        or not isinstance(max_output_bytes, int)
        or max_output_bytes <= 0
    ):
        raise ProcessEvidenceError("max_output_bytes must be a positive integer")
    if stdout_path == stderr_path:
        raise ProcessEvidenceError("stdout and stderr evidence paths must differ")
    stdout_path.parent.mkdir(parents=True, exist_ok=True)
    stderr_path.parent.mkdir(parents=True, exist_ok=True)
    stdout_path.write_bytes(b"")
    stderr_path.write_bytes(b"")

    system_name = platform.system()
    resource_path: Path | None = None
    measured_command = list(command)
    if measure_resources:
        resource_path = stderr_path.with_name(f"{stderr_path.name}.resources.txt")
        resource_path.parent.mkdir(parents=True, exist_ok=True)
        resource_path.write_bytes(b"")
        measured_command = _measurement_command(command, resource_path, system_name)

    started = time.monotonic()
    timed_out = False
    launch_error: str | None = None
    returncode: int | None = None
    capture: dict[str, int | bool | str] = {
        "stdout_bytes": 0,
        "stderr_bytes": 0,
        "stdout_truncated": False,
        "stderr_truncated": False,
    }

    try:
        process = subprocess.Popen(
            measured_command,
            cwd=cwd,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=os.name == "posix",
        )
    except OSError as exc:
        launch_error = f"{type(exc).__name__}: {exc}"
        encoded = (launch_error + "\n").encode("utf-8", errors="replace")
        stderr_path.write_bytes(encoded[:max_output_bytes])
        capture["stderr_bytes"] = len(encoded)
        capture["stderr_truncated"] = len(encoded) > max_output_bytes
    else:
        assert process.stdout is not None
        assert process.stderr is not None
        readers = [
            threading.Thread(
                target=_capture_pipe_guarded,
                args=(process.stdout, stdout_path, max_output_bytes, capture, "stdout"),
                daemon=True,
            ),
            threading.Thread(
                target=_capture_pipe_guarded,
                args=(process.stderr, stderr_path, max_output_bytes, capture, "stderr"),
                daemon=True,
            ),
        ]
        for reader in readers:
            reader.start()
        try:
            process.wait(timeout=timeout_secs)
            returncode = process.returncode
        except subprocess.TimeoutExpired:
            timed_out = True
            _terminate_process_tree(process)
        for reader in readers:
            reader.join(timeout=2)
        if any(reader.is_alive() for reader in readers):
            process.stdout.close()
            process.stderr.close()
            for reader in readers:
                reader.join(timeout=1)
        if any(reader.is_alive() for reader in readers):
            raise ProcessEvidenceError("subprocess output pipes did not close")
        capture_errors = [
            str(capture[key])
            for key in ("stdout_capture_error", "stderr_capture_error")
            if key in capture
        ]
        if capture_errors:
            raise ProcessEvidenceError(
                "subprocess output capture failed: " + "; ".join(capture_errors)
            )

    evidence = {
        "command": list(command),
        "measurement_command": measured_command,
        "returncode": returncode,
        "timed_out": timed_out,
        "launch_error": launch_error,
        "elapsed_seconds": round(time.monotonic() - started, 6),
        "stdout_path": str(stdout_path),
        "stderr_path": str(stderr_path),
        "max_output_bytes": max_output_bytes,
        **capture,
    }
    if resource_path is not None:
        evidence["resource_usage_path"] = str(resource_path)
        evidence["resource_usage"] = _resource_metrics(resource_path, system_name)
    return evidence
