"""Benchmark orchestrator.

Drives (slide x tool x transfer-syntax x level-scope) trials, captures wall
time, peak RSS, output size, tool-specific extra metrics, and computes
fidelity vs. the source slide.

Outputs JSON-per-run + a single aggregated CSV at bench/results/<timestamp>/.
"""
from __future__ import annotations
import argparse
import json
import os
import platform
import signal
import shutil
import subprocess
import sys
import time
import traceback
from dataclasses import asdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

# Ensure adapters can `from manifest import ...`
sys.path.insert(0, str(Path(__file__).parent))

from manifest import (
    SLIDES, TOOLS, TRANSFER_SYNTAXES, LEVEL_SCOPES,
    Slide, Tool, OUTPUTS_ROOT, RESULTS_ROOT,
    N_TRIALS, WARMUP_DROP, slide_by_id,
)
from adapters import wsi_dicom as adapter_wsi_dicom
from adapters import wsidicomizer_adapter as adapter_wsidicomizer
from adapters import highdicom_adapter as adapter_highdicom
import fidelity as fidelity_mod


class TrialTimeoutError(TimeoutError):
    pass


def _raise_trial_timeout(signum, frame):
    raise TrialTimeoutError("trial exceeded timeout")


def hardware_block() -> dict:
    info = {
        "platform": platform.platform(),
        "machine": platform.machine(),
        "python": platform.python_version(),
        "ts_iso": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    }
    try:
        out = subprocess.run(
            ["system_profiler", "SPHardwareDataType"],
            capture_output=True, text=True, timeout=10,
        )
        info["system_profiler"] = out.stdout
    except Exception:
        pass
    try:
        out = subprocess.run(["sysctl", "-n", "hw.model"], capture_output=True, text=True)
        info["hw_model"] = out.stdout.strip()
    except Exception:
        pass
    try:
        out = subprocess.run(["sysctl", "-n", "machdep.cpu.brand_string"],
                             capture_output=True, text=True)
        info["cpu_brand"] = out.stdout.strip()
    except Exception:
        pass
    return info


def build_matrix(slide_filter: Optional[list[str]] = None) -> list[tuple[Slide, Tool, str, str]]:
    rows = []
    for slide in SLIDES:
        if slide_filter and slide.slide_id not in slide_filter:
            continue
        for tool in TOOLS:
            for ts in TRANSFER_SYNTAXES:
                # highdicom adapter does not implement passthrough
                if tool.tool_id == "highdicom" and ts == "passthrough":
                    continue
                for scope in LEVEL_SCOPES:
                    rows.append((slide, tool, ts, scope))
    return rows


def call_adapter(
    tool: Tool,
    slide: Slide,
    transfer_syntax: str,
    scope: str,
    out_dir: Path,
    timeout_seconds: Optional[float] = None,
):
    if tool.tool_id == "wsi_dicom_metal":
        return adapter_wsi_dicom.run(
            slide, transfer_syntax, scope, "prefer-device", out_dir,
            timeout_seconds=timeout_seconds,
        )
    if tool.tool_id == "wsi_dicom_cpu":
        return adapter_wsi_dicom.run(
            slide, transfer_syntax, scope, "cpu", out_dir,
            timeout_seconds=timeout_seconds,
        )
    if tool.tool_id == "wsidicomizer":
        return adapter_wsidicomizer.run(slide, transfer_syntax, scope, out_dir)
    if tool.tool_id == "highdicom":
        return adapter_highdicom.run(slide, transfer_syntax, scope, out_dir)
    raise KeyError(tool.tool_id)


def run_trial(
    slide: Slide,
    tool: Tool,
    transfer_syntax: str,
    scope: str,
    trial_idx: int,
    out_root: Path,
    clean_output: bool = False,
    trial_timeout_seconds: Optional[float] = None,
) -> dict:
    out_dir = (
        out_root / slide.slide_id / tool.tool_id / transfer_syntax / scope / f"trial-{trial_idx}"
    )
    if clean_output and out_dir.exists():
        try:
            shutil.rmtree(out_dir)
        except Exception as e:
            return {
                "ok": False,
                "wall_seconds": 0.0,
                "error": f"pre-trial cleanup failed: {type(e).__name__}: {e}",
                "traceback": traceback.format_exc(),
                "tool": tool.tool_id, "transfer_syntax": transfer_syntax,
                "scope": scope, "trial": trial_idx, "slide": slide.slide_id,
                "output_dir": str(out_dir),
                "output_retained": True,
            }
    print(f"  [trial {trial_idx}] {tool.tool_id:18s} {transfer_syntax:24s} {scope}", flush=True)
    t0 = time.monotonic()
    old_handler = None
    try:
        adapter_timeout_seconds = (
            trial_timeout_seconds
            if tool.tool_id in {"wsi_dicom_metal", "wsi_dicom_cpu"}
            else None
        )
        signal_timeout_seconds = (
            trial_timeout_seconds
            if tool.tool_id not in {"wsi_dicom_metal", "wsi_dicom_cpu"}
            else None
        )
        if signal_timeout_seconds is not None and signal_timeout_seconds > 0:
            old_handler = signal.signal(signal.SIGALRM, _raise_trial_timeout)
            signal.setitimer(signal.ITIMER_REAL, signal_timeout_seconds)
        outcome = call_adapter(
            tool, slide, transfer_syntax, scope, out_dir,
            timeout_seconds=adapter_timeout_seconds,
        )
    except Exception as e:
        outcome = None
        return {
            "ok": False,
            "wall_seconds": time.monotonic() - t0,
            "error": f"{type(e).__name__}: {e}",
            "traceback": traceback.format_exc(),
            "tool": tool.tool_id, "transfer_syntax": transfer_syntax,
            "scope": scope, "trial": trial_idx, "slide": slide.slide_id,
        }
    finally:
        if old_handler is not None:
            signal.setitimer(signal.ITIMER_REAL, 0.0)
            signal.signal(signal.SIGALRM, old_handler)
    rec = {
        "slide": slide.slide_id,
        "tool": tool.tool_id,
        "transfer_syntax": transfer_syntax,
        "scope": scope,
        "trial": trial_idx,
        "ok": outcome.ok,
        "wall_seconds": outcome.wall_seconds,
        "peak_rss_bytes": outcome.peak_rss_bytes,
        "output_dir": outcome.output_dir,
        "output_size_bytes": outcome.output_size_bytes,
        "extra_metrics": outcome.extra_metrics,
        "stderr_tail": outcome.stderr_tail,
        "error": outcome.error,
    }
    return rec


def maybe_clean_output(rec: dict, clean_output: bool) -> dict:
    if not clean_output:
        rec["output_retained"] = True
        return rec
    output_dir = rec.get("output_dir")
    if not output_dir:
        rec["output_retained"] = False
        return rec
    try:
        shutil.rmtree(Path(output_dir))
    except FileNotFoundError:
        rec["output_retained"] = False
    except Exception as e:
        rec["output_retained"] = True
        rec["cleanup_error"] = f"{type(e).__name__}: {e}"
    else:
        rec["output_retained"] = False
    return rec


def maybe_compute_fidelity(rec: dict, slide: Slide) -> dict:
    """Run the fidelity sampler against the trial's output. Cheap on small
    slides, somewhat slower on huge ones (24 random patches at level 0)."""
    if not rec.get("ok"):
        rec["fidelity"] = None
        return rec
    try:
        fr = fidelity_mod.compute_fidelity(
            source_path=slide.path,
            output_dicom_dir=rec["output_dir"],
            output_level_index_in_source=0,
        )
        rec["fidelity"] = fidelity_mod.to_json_dict(fr)
    except Exception as e:
        rec["fidelity"] = {"error": f"{type(e).__name__}: {e}"}
    return rec


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true",
                    help="Run only a single trial, only on the smallest slide (CMU-1.tiff). "
                         "Useful for wiring verification.")
    ap.add_argument("--slides", nargs="+", default=None,
                    help="Limit to specific slide IDs (e.g. tiff_cmu1 ndpi_mid)")
    ap.add_argument("--tools", nargs="+", default=None,
                    help="Limit to specific tool IDs")
    ap.add_argument("--transfer-syntaxes", nargs="+", default=None,
                    help="Limit to specific transfer syntaxes")
    ap.add_argument("--scopes", nargs="+", default=None,
                    help="Limit to specific level scopes")
    ap.add_argument("--no-fidelity", action="store_true",
                    help="Skip the fidelity comparison step (speed only).")
    ap.add_argument("--clean-output", action="store_true",
                    help="Delete each trial output directory after metrics and fidelity are captured.")
    ap.add_argument("--trial-timeout-seconds", type=float, default=None,
                    help="Mark a trial failed if adapter execution exceeds this many seconds.")
    ap.add_argument("--start-cell", type=int, default=1,
                    help="Resume at this 1-based cell index after filters are applied.")
    ap.add_argument("--end-cell", type=int, default=None,
                    help="Stop after this 1-based cell index after filters are applied.")
    ap.add_argument("--trials", type=int, default=N_TRIALS,
                    help=f"Reported trials (default {N_TRIALS}); a warmup run is added.")
    ap.add_argument("--results-tag", default=None)
    args = ap.parse_args()

    OUTPUTS_ROOT.mkdir(parents=True, exist_ok=True)
    RESULTS_ROOT.mkdir(parents=True, exist_ok=True)

    tag = args.results_tag or datetime.now().strftime("%Y%m%d-%H%M%S")
    if args.dry_run:
        tag = f"dry-{tag}"
    results_dir = RESULTS_ROOT / tag
    results_dir.mkdir(parents=True, exist_ok=True)

    # Hardware metadata
    hw = hardware_block()
    (results_dir / "environment.json").write_text(json.dumps(hw, indent=2, default=str))

    if args.dry_run:
        slide_filter = ["tiff_cmu1"]
        n_warmup = 0
        n_report = 1
    else:
        slide_filter = args.slides
        n_warmup = WARMUP_DROP
        n_report = args.trials

    matrix = build_matrix(slide_filter=slide_filter)

    if args.tools:
        matrix = [(s, t, ts, sc) for (s, t, ts, sc) in matrix if t.tool_id in args.tools]
    if args.transfer_syntaxes:
        matrix = [(s, t, ts, sc) for (s, t, ts, sc) in matrix if ts in args.transfer_syntaxes]
    if args.scopes:
        matrix = [(s, t, ts, sc) for (s, t, ts, sc) in matrix if sc in args.scopes]

    total_cells = len(matrix)
    end_cell = args.end_cell or total_cells
    if args.start_cell < 1 or end_cell < args.start_cell or end_cell > total_cells:
        raise SystemExit(
            f"invalid cell range {args.start_cell}..{end_cell}; matrix has {total_cells} cells"
        )
    indexed_matrix = [
        (idx, s, t, ts, sc)
        for idx, (s, t, ts, sc) in enumerate(matrix, start=1)
        if args.start_cell <= idx <= end_cell
    ]

    print(f"Matrix size: {len(indexed_matrix)} selected of {total_cells} cells × {n_warmup + n_report} trials")
    print(f"Results dir: {results_dir}")

    all_records = []
    for (cell_idx, slide, tool, ts, scope) in indexed_matrix:
        print(f"\n=== Cell {cell_idx}/{total_cells}  slide={slide.slide_id} tool={tool.tool_id} ts={ts} scope={scope}", flush=True)

        # Warmup trial(s)
        for w in range(n_warmup):
            warmup = run_trial(
                slide, tool, ts, scope, trial_idx=-1 - w,
                out_root=OUTPUTS_ROOT, clean_output=args.clean_output,
                trial_timeout_seconds=args.trial_timeout_seconds,
            )
            maybe_clean_output(warmup, args.clean_output)

        # Reported trials
        for tri in range(n_report):
            rec = run_trial(
                slide, tool, ts, scope, trial_idx=tri,
                out_root=OUTPUTS_ROOT, clean_output=args.clean_output,
                trial_timeout_seconds=args.trial_timeout_seconds,
            )
            if not args.no_fidelity:
                rec = maybe_compute_fidelity(rec, slide)
            rec = maybe_clean_output(rec, args.clean_output)
            all_records.append(rec)
            # Persist incrementally so partial runs survive interruption.
            with (results_dir / "raw.jsonl").open("a") as f:
                f.write(json.dumps(rec, default=str) + "\n")

    # Final aggregate dump (also raw, for convenience)
    (results_dir / "all.json").write_text(json.dumps(all_records, indent=2, default=str))
    print(f"\nDone. {len(all_records)} trials. Records at {results_dir}")


if __name__ == "__main__":
    main()
