"""Compare matched CPU and Metal format-coverage outputs."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import tempfile
from pathlib import Path

import pydicom
from pydicom.dataset import Dataset
from pydicom.encaps import generate_frames

from bench.cli_values import positive_int
from bench.file_digest import sha256_file
from bench.json_document import write_json
from bench.process_evidence import ProcessEvidenceError, run_bounded_command

from .manifest import FormatCoverageError, validate_case_identifier
from .report import verify_checksums, write_checksums


COMPARISON_SCHEMA = "wsi-dicom-format-coverage-cpu-metal-comparison-v1"
PIXEL_DATA_TAG = 0x7FE00010
PIXEL_TRANSPORT_TAGS = {PIXEL_DATA_TAG, 0x7FE00001, 0x7FE00002}


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Compare matched CPU and Metal format-coverage bundles."
    )
    parser.add_argument("--cpu", type=Path, required=True)
    parser.add_argument("--metal", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--htj2k-decoder", type=Path, default=Path("/opt/homebrew/bin/grk_decompress"))
    parser.add_argument("--decode-timeout-secs", type=positive_int, default=300)
    return parser.parse_args(argv)


def canonical_element(element) -> object | None:
    if int(element.tag) in PIXEL_TRANSPORT_TAGS or element.VR == "UI":
        return None
    if element.VR == "SQ":
        value = [canonical_dataset(item) for item in element.value]
    elif isinstance(element.value, bytes):
        value = {
            "bytes": len(element.value),
            "sha256": hashlib.sha256(element.value).hexdigest(),
        }
    elif isinstance(element.value, (list, tuple)) or type(element.value).__name__ == "MultiValue":
        value = [str(item) for item in element.value]
    else:
        value = str(element.value)
    return {"tag": f"{int(element.tag):08x}", "vr": element.VR, "value": value}


def canonical_dataset(dataset: Dataset) -> list[dict]:
    elements = []
    for element in dataset:
        canonical = canonical_element(element)
        if canonical is not None:
            elements.append(canonical)
    return elements


def semantic_digest(datasets: list[Dataset]) -> str:
    documents = [canonical_dataset(dataset) for dataset in datasets]
    documents.sort(key=lambda value: json.dumps(value, sort_keys=True, separators=(",", ":")))
    encoded = json.dumps(documents, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return hashlib.sha256(encoded.encode("utf-8")).hexdigest()


def parse_pnm(data: bytes) -> tuple[str, int, int, int, bytes]:
    position = 0

    def token() -> bytes:
        nonlocal position
        while position < len(data):
            if data[position] == ord("#"):
                newline = data.find(b"\n", position)
                if newline < 0:
                    raise FormatCoverageError("PNM comment is not newline terminated")
                position = newline + 1
            elif chr(data[position]).isspace():
                position += 1
            else:
                break
        start = position
        while position < len(data) and not chr(data[position]).isspace():
            position += 1
        if start == position:
            raise FormatCoverageError("PNM header is truncated")
        return data[start:position]

    try:
        magic = token().decode("ascii")
        width = int(token())
        height = int(token())
        maximum = int(token())
    except (UnicodeDecodeError, ValueError) as exc:
        raise FormatCoverageError(f"invalid PNM header: {exc}") from exc
    if magic not in {"P5", "P6"} or width <= 0 or height <= 0 or not 0 < maximum <= 65535:
        raise FormatCoverageError("unsupported PNM geometry or sample range")
    if position >= len(data) or not chr(data[position]).isspace():
        raise FormatCoverageError("PNM header is missing its payload delimiter")
    position += 1
    channels = 1 if magic == "P5" else 3
    bytes_per_sample = 1 if maximum <= 255 else 2
    expected = width * height * channels * bytes_per_sample
    payload = data[position:]
    if len(payload) != expected:
        raise FormatCoverageError(
            f"PNM payload has {len(payload)} bytes; expected {expected}"
        )
    return magic, width, height, maximum, payload


def performance_comparison(
    *,
    cpu_wall: float,
    metal_wall: float,
    cpu_peak_rss: int,
    metal_peak_rss: int,
    frames: int,
) -> dict:
    cpu_throughput = frames / cpu_wall if cpu_wall > 0 else None
    metal_throughput = frames / metal_wall if metal_wall > 0 else None
    if cpu_wall == metal_wall:
        faster = "tie"
    else:
        faster = "cpu" if cpu_wall < metal_wall else "metal"
    if cpu_peak_rss == metal_peak_rss:
        lower_memory = "tie"
    else:
        lower_memory = "cpu" if cpu_peak_rss < metal_peak_rss else "metal"
    return {
        "frames": frames,
        "cpu_wall_seconds": cpu_wall,
        "metal_wall_seconds": metal_wall,
        "cpu_throughput_frames_per_second": cpu_throughput,
        "metal_throughput_frames_per_second": metal_throughput,
        "cpu_to_metal_wall_ratio": cpu_wall / metal_wall if metal_wall > 0 else None,
        "faster_backend": faster,
        "cpu_peak_rss_bytes": cpu_peak_rss,
        "metal_peak_rss_bytes": metal_peak_rss,
        "metal_to_cpu_peak_rss_ratio": (
            metal_peak_rss / cpu_peak_rss if cpu_peak_rss > 0 else None
        ),
        "lower_peak_memory_backend": lower_memory,
    }


def load_bundle(root: Path, expected_backend: str) -> tuple[dict, dict[str, dict]]:
    report_path = root / "format-coverage-report.json"
    if not report_path.is_file():
        raise FormatCoverageError(f"missing format coverage report: {report_path}")
    verify_checksums(root)
    try:
        report = json.loads(report_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise FormatCoverageError(
            f"failed to load format coverage report {report_path}: {exc}"
        ) from exc
    if not isinstance(report, dict):
        raise FormatCoverageError(
            f"format coverage report must be an object: {report_path}"
        )
    if report.get("status") != "passed":
        raise FormatCoverageError(f"format coverage bundle is not passed: {root}")
    policy = report.get("policy")
    if not isinstance(policy, dict) or policy.get("backend") != expected_backend:
        raise FormatCoverageError(
            f"expected backend {expected_backend!r} in {report_path}"
        )
    raw_cases = report.get("cases")
    if not isinstance(raw_cases, list):
        raise FormatCoverageError(
            f"format coverage report cases must be an array: {report_path}"
        )
    cases = {}
    for index, case in enumerate(raw_cases):
        if not isinstance(case, dict):
            raise FormatCoverageError(
                f"format coverage report case {index} must be an object"
            )
        case_id = validate_case_identifier(
            case.get("id"), f"format coverage report case {index} id"
        )
        if case_id in cases:
            raise FormatCoverageError(
                f"duplicate format coverage report case id: {case_id}"
            )
        cases[case_id] = case
    return report, cases


def matched_metal_case_ids(cpu_cases: dict[str, dict], metal_cases: dict[str, dict]) -> list[str]:
    if set(cpu_cases) != set(metal_cases):
        raise FormatCoverageError("CPU and Metal bundles contain different case IDs")
    return sorted(
        case_id
        for case_id, case in metal_cases.items()
        if case["conversion"]["route_classification"] == "metal_encode"
    )


def dicom_paths(root: Path, case_id: str) -> list[Path]:
    validate_case_identifier(case_id)
    paths = sorted((root / "cases" / case_id / "dicom").rglob("*.dcm"))
    if not paths:
        raise FormatCoverageError(f"no DICOM files for case {case_id} under {root}")
    return paths


def decoded_instance_digest(
    dicom_path: Path,
    decoder: Path,
    output_root: Path,
    timeout: int,
) -> dict:
    dataset = pydicom.dcmread(dicom_path)
    frame_count = int(dataset.NumberOfFrames)
    frames = list(generate_frames(dataset.PixelData, number_of_frames=frame_count))
    if len(frames) != frame_count:
        raise FormatCoverageError(
            f"encapsulated frame count mismatch in {dicom_path}: {len(frames)} != {frame_count}"
        )
    output_root.mkdir(parents=True, exist_ok=True)
    frame_reports = []
    combined = hashlib.sha256()
    for index, frame in enumerate(frames):
        codestream = output_root / f"frame-{index:06}.jhc"
        decoded = output_root / f"frame-{index:06}.pnm"
        codestream.write_bytes(frame)
        command = [str(decoder), "-i", str(codestream), "-o", str(decoded)]
        stdout_path = output_root / f"frame-{index:06}.decoder.stdout.txt"
        stderr_path = output_root / f"frame-{index:06}.decoder.stderr.txt"
        try:
            execution = run_bounded_command(
                command,
                stdout_path=stdout_path,
                stderr_path=stderr_path,
                timeout_secs=timeout,
                max_output_bytes=64 * 1024,
            )
        except ProcessEvidenceError as exc:
            raise FormatCoverageError(
                f"HTJ2K decoder process policy failed for {dicom_path} frame {index}: {exc}"
            ) from exc
        if execution["timed_out"]:
            raise FormatCoverageError(
                f"HTJ2K decode timed out for {dicom_path} frame {index} after {timeout}s"
            )
        if execution["launch_error"] is not None:
            raise FormatCoverageError(
                f"HTJ2K decoder launch failed for {dicom_path} frame {index}: "
                f"{execution['launch_error']}"
            )
        if execution.get("stdout_truncated") or execution.get("stderr_truncated"):
            raise FormatCoverageError(
                f"HTJ2K decoder output was truncated for {dicom_path} frame {index}"
            )
        if execution["returncode"] != 0 or not decoded.is_file():
            stderr = stderr_path.read_text(encoding="utf-8", errors="replace")
            raise FormatCoverageError(
                f"HTJ2K decode failed for {dicom_path} frame {index}: "
                f"returncode={execution['returncode']} stderr={stderr[:4096]!r}"
            )
        magic, width, height, maximum, payload = parse_pnm(decoded.read_bytes())
        frame_digest = hashlib.sha256()
        frame_digest.update(f"{magic}:{width}:{height}:{maximum}:".encode("ascii"))
        frame_digest.update(payload)
        digest = frame_digest.hexdigest()
        combined.update(bytes.fromhex(digest))
        frame_reports.append(
            {
                "index": index,
                "magic": magic,
                "width": width,
                "height": height,
                "maximum": maximum,
                "pixel_bytes": len(payload),
                "pixel_sha256": digest,
                "command": command,
                "decoder_process": execution,
                "decoder_stdout": stdout_path.read_text(
                    encoding="utf-8", errors="replace"
                )[:4096],
                "decoder_stderr": stderr_path.read_text(
                    encoding="utf-8", errors="replace"
                )[:4096],
            }
        )
    return {
        "dicom": dicom_path.name,
        "frames": frame_reports,
        "decoded_pixel_sha256": combined.hexdigest(),
    }


def decode_case(root: Path, case_id: str, decoder: Path, output: Path, timeout: int) -> dict:
    instance_reports = []
    combined = hashlib.sha256()
    for path in dicom_paths(root, case_id):
        report = decoded_instance_digest(
            path, decoder, output / path.stem, timeout
        )
        combined.update(bytes.fromhex(report["decoded_pixel_sha256"]))
        instance_reports.append(report)
    return {
        "instances": instance_reports,
        "decoded_pixel_sha256": combined.hexdigest(),
        "frame_count": sum(len(instance["frames"]) for instance in instance_reports),
    }


def semantic_case(root: Path, case_id: str) -> dict:
    paths = dicom_paths(root, case_id)
    datasets = [pydicom.dcmread(path) for path in paths]
    return {
        "metadata_semantic_sha256": semantic_digest(datasets),
        "dicom_bytes": sum(path.stat().st_size for path in paths),
        "instances": len(paths),
    }


def workbench_status(root: Path, case_id: str) -> dict:
    path = root / "cases" / case_id / "workbench" / "workbench-report.json"
    report = json.loads(path.read_text(encoding="utf-8"))
    return {
        "status": report["status"],
        "domains": {name: value["status"] for name, value in report["domains"].items()},
        "validator_comparison": report["validator_comparison"],
    }


def case_comparison(
    case_id: str,
    cpu_root: Path,
    metal_root: Path,
    cpu_case: dict,
    metal_case: dict,
    decoder: Path,
    staging: Path,
    timeout: int,
) -> dict:
    validate_case_identifier(case_id)
    cpu_pixels = decode_case(
        cpu_root, case_id, decoder, staging / "cases" / case_id / "decoded" / "cpu", timeout
    )
    metal_pixels = decode_case(
        metal_root,
        case_id,
        decoder,
        staging / "cases" / case_id / "decoded" / "metal",
        timeout,
    )
    cpu_semantics = semantic_case(cpu_root, case_id)
    metal_semantics = semantic_case(metal_root, case_id)
    cpu_workbench = workbench_status(cpu_root, case_id)
    metal_workbench = workbench_status(metal_root, case_id)
    cpu_usage = cpu_case["conversion"]["resource_usage"]
    metal_usage = metal_case["conversion"]["resource_usage"]
    frames = cpu_pixels["frame_count"]
    performance = performance_comparison(
        cpu_wall=cpu_usage["wall_seconds"],
        metal_wall=metal_usage["wall_seconds"],
        cpu_peak_rss=cpu_usage["peak_rss_bytes"],
        metal_peak_rss=metal_usage["peak_rss_bytes"],
        frames=frames,
    )
    pixel_equal = (
        cpu_pixels["frame_count"] == metal_pixels["frame_count"]
        and cpu_pixels["decoded_pixel_sha256"] == metal_pixels["decoded_pixel_sha256"]
    )
    semantic_equal = (
        cpu_semantics["metadata_semantic_sha256"]
        == metal_semantics["metadata_semantic_sha256"]
    )
    conformance_equal = (
        cpu_workbench["status"] == "passed"
        and metal_workbench["status"] == "passed"
        and cpu_workbench["domains"] == metal_workbench["domains"]
        and not cpu_workbench["validator_comparison"]["disagreement"]
        and not metal_workbench["validator_comparison"]["disagreement"]
    )
    result = {
        "id": case_id,
        "format": cpu_case["format"],
        "status": "passed" if pixel_equal and semantic_equal and conformance_equal else "failed",
        "routes": {
            "cpu": cpu_case["conversion"]["route_classification"],
            "metal": metal_case["conversion"]["route_classification"],
        },
        "source_stages": {
            "cpu": cpu_case["conversion"]["source_stage"],
            "metal": metal_case["conversion"]["source_stage"],
        },
        "pixel_fidelity": {
            "equal": pixel_equal,
            "cpu": cpu_pixels,
            "metal": metal_pixels,
        },
        "semantic_fidelity": {
            "equal": semantic_equal,
            "cpu": cpu_semantics,
            "metal": metal_semantics,
        },
        "dicom_conformance": {
            "equivalent_pass": conformance_equal,
            "cpu": cpu_workbench,
            "metal": metal_workbench,
        },
        "performance": performance,
    }
    write_json(staging / "cases" / case_id / "comparison.json", result)
    return result


def render_summary(report: dict) -> str:
    lines = [
        "# CPU versus Metal format-coverage comparison",
        "",
        f"- Status: `{report['status']}`",
        f"- Matched Metal-encode cases: {len(report['cases'])}",
        "- Repetitions: 1 per backend; performance is descriptive, not inferential",
        "",
        "| Format | Frames | Pixel equal | Semantic equal | CPU wall (s) | Metal wall (s) | Faster | CPU peak RSS | Metal peak RSS |",
        "| --- | ---: | --- | --- | ---: | ---: | --- | ---: | ---: |",
    ]
    for case in report["cases"]:
        perf = case["performance"]
        lines.append(
            f"| {case['format']} | {perf['frames']} | {case['pixel_fidelity']['equal']} | "
            f"{case['semantic_fidelity']['equal']} | {perf['cpu_wall_seconds']:.3f} | "
            f"{perf['metal_wall_seconds']:.3f} | {perf['faster_backend']} | "
            f"{perf['cpu_peak_rss_bytes']} | {perf['metal_peak_rss_bytes']} |"
        )
    lines.extend(
        [
            "",
            "SCN is a non-accelerated one-frame JPEG passthrough route profile. CZI is an explicit unsupported case. Neither is included in CPU-versus-Metal performance or output-fidelity summaries.",
            "",
            "All source decode/composition remained on CPU in this extension; `require-device` applied to encoding and no CPU encode fallback was accepted.",
            "",
        ]
    )
    return "\n".join(lines)


def run(args: argparse.Namespace) -> dict:
    args.cpu = args.cpu.resolve()
    args.metal = args.metal.resolve()
    args.output = args.output.resolve()
    args.htj2k_decoder = args.htj2k_decoder.resolve()
    if args.output.exists():
        raise FormatCoverageError(f"output already exists: {args.output}")
    if not args.htj2k_decoder.is_file():
        raise FormatCoverageError(f"HTJ2K decoder does not exist: {args.htj2k_decoder}")
    cpu_report, cpu_cases = load_bundle(args.cpu, "cpu")
    metal_report, metal_cases = load_bundle(args.metal, "require-device")
    matched = matched_metal_case_ids(cpu_cases, metal_cases)
    if not matched:
        raise FormatCoverageError("no matched Metal-encode cases were found")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix=f".{args.output.name}.staging-", dir=args.output.parent
    ) as temporary:
        staging = Path(temporary)
        cases = [
            case_comparison(
                case_id,
                args.cpu,
                args.metal,
                cpu_cases[case_id],
                metal_cases[case_id],
                args.htj2k_decoder,
                staging,
                args.decode_timeout_secs,
            )
            for case_id in matched
        ]
        report = {
            "schema_version": COMPARISON_SCHEMA,
            "status": (
                "passed" if all(case["status"] == "passed" for case in cases) else "failed"
            ),
            "inputs": {
                "cpu": {
                    "path": str(args.cpu),
                    "report_sha256": sha256_file(args.cpu / "format-coverage-report.json"),
                    "checksums_sha256": sha256_file(args.cpu / "SHA256SUMS"),
                    "software": cpu_report["software"],
                },
                "metal": {
                    "path": str(args.metal),
                    "report_sha256": sha256_file(args.metal / "format-coverage-report.json"),
                    "checksums_sha256": sha256_file(args.metal / "SHA256SUMS"),
                    "software": metal_report["software"],
                },
                "htj2k_decoder": {
                    "path": str(args.htj2k_decoder),
                    "sha256": sha256_file(args.htj2k_decoder),
                },
            },
            "non_accelerated": {
                "leica-scn-fluorescence-1": "jpeg_passthrough_route_profile",
            },
            "unsupported": {
                "zeiss-czi-zstd0-preview": "direct_subblock_composition_not_implemented",
            },
            "cases": cases,
            "limitations": [
                "one run per backend; performance comparisons are descriptive only",
                "bounded native levels and one pinned artifact per format",
                "CPU and Metal were run sequentially without randomized order or cache purging",
                "source decode and composition remained CPU stages",
                "viewer and DICOMweb receiver behavior were not evaluated",
            ],
        }
        write_json(staging / "comparison-report.json", report)
        (staging / "summary.md").write_text(render_summary(report), encoding="utf-8")
        write_checksums(staging)
        os.replace(staging, args.output)
    return report


def main(argv: list[str] | None = None) -> int:
    try:
        args = parse_args(argv)
        report = run(args)
    except (FormatCoverageError, OSError, ValueError) as exc:
        print(f"format comparison failed: {exc}", file=sys.stderr)
        return 2
    print(json.dumps({"status": report["status"], "output": str(args.output.resolve())}, sort_keys=True))
    return 0 if report["status"] == "passed" else 1
