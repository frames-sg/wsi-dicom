"""Slide registry, tool registry, and transfer-syntax mapping for the benchmark."""
from __future__ import annotations
from dataclasses import dataclass
import os
from pathlib import Path


def _repo_root() -> Path:
    if root := os.environ.get("WSI_DICOM_REPO_ROOT"):
        return Path(root).expanduser().resolve()
    return Path(__file__).resolve().parents[1]


def _slide_path(env_var: str, fallback_relative: str) -> str:
    if path := os.environ.get(env_var):
        return str(Path(path).expanduser())
    return str(BENCH_ROOT / fallback_relative)


REPO_ROOT = _repo_root()
WSI_DICOM_BIN = REPO_ROOT / "target" / "release" / "wsi-dicom"
BENCH_ROOT = REPO_ROOT / "bench"
OUTPUTS_ROOT = BENCH_ROOT / "outputs"
RESULTS_ROOT = BENCH_ROOT / "results"


@dataclass(frozen=True)
class Slide:
    slide_id: str
    path: str
    vendor: str
    size_class: str  # "small" | "mid" | "large"
    native_codec: str  # "jpeg" | "j2k" | "other"
    base_dim: tuple[int, int]
    level_count: int
    bytes_on_disk: int


SLIDES: list[Slide] = [
    Slide(
        slide_id="tiff_cmu1",
        path=_slide_path("WSI_DICOM_BENCH_TIFF_CMU1", "testdata/CMU-1.tiff"),
        vendor="generic-tiff",
        size_class="mid",
        native_codec="jpeg",
        base_dim=(46000, 32914),
        level_count=9,
        bytes_on_disk=195 * 1024 * 1024,
    ),
    Slide(
        slide_id="ndpi_mid",
        path=_slide_path("WSI_DICOM_BENCH_NDPI_MID", "slides/ndpi-mid.ndpi"),
        vendor="hamamatsu",
        size_class="mid",
        native_codec="jpeg",
        base_dim=(34816, 27136),
        level_count=10,
        bytes_on_disk=158 * 1024 * 1024,
    ),
    Slide(
        slide_id="ndpi_large",
        path=_slide_path("WSI_DICOM_BENCH_NDPI_LARGE", "slides/ndpi-large.ndpi"),
        vendor="hamamatsu",
        size_class="large",
        native_codec="jpeg",
        base_dim=(115072, 93184),
        level_count=8,
        bytes_on_disk=1500 * 1024 * 1024,
    ),
    Slide(
        slide_id="svs_mid",
        path=_slide_path("WSI_DICOM_BENCH_SVS_MID", "slides/svs-mid.svs"),
        vendor="aperio",
        size_class="mid",
        native_codec="j2k",
        base_dim=(35492, 41525),
        level_count=3,
        bytes_on_disk=94 * 1024 * 1024,
    ),
    Slide(
        slide_id="svs_large",
        path=_slide_path("WSI_DICOM_BENCH_SVS_LARGE", "slides/svs-large.svs"),
        vendor="aperio",
        size_class="large",
        native_codec="jpeg",
        base_dim=(126616, 93196),
        level_count=4,
        bytes_on_disk=2500 * 1024 * 1024,
    ),
]


# Transfer syntax labels used in our matrix.
TRANSFER_SYNTAXES = [
    "passthrough",          # native re-encapsulation: routes to JPEG Baseline or J2K depending on source
    "htj2k-lossless-rpcl",  # full transcode to HTJ2K-Lossless-RPCL
    "jpeg-baseline",        # full transcode to JPEG Baseline 8-bit
]


# Map our label -> wsi-dicom CLI value
WSI_DICOM_TS = {
    # passthrough: pick CLI value that matches source codec
    "htj2k-lossless-rpcl": "htj2k-lossless-rpcl",
    "htj2k-lossless": "htj2k-lossless",
    "jpeg-baseline": "jpeg-baseline8-bit",
}


def passthrough_cli_value(slide: Slide) -> str:
    """Pick the wsi-dicom --transfer-syntax value that yields native passthrough."""
    if slide.native_codec == "jpeg":
        return "jpeg-baseline8-bit"
    if slide.native_codec == "j2k":
        return "jpeg2000"
    raise ValueError(f"no passthrough for {slide.slide_id}")


# Tool registry
@dataclass(frozen=True)
class Tool:
    tool_id: str
    label: str
    family: str  # "wsi-dicom" | "wsidicomizer" | "highdicom"


TOOLS: list[Tool] = [
    Tool("wsi_dicom_metal", "wsi-dicom (Metal)", "wsi-dicom"),
    Tool("wsi_dicom_cpu",   "wsi-dicom (CPU)",   "wsi-dicom"),
    Tool("wsidicomizer",    "wsidicomizer",      "wsidicomizer"),
    Tool("highdicom",       "highdicom+manual",  "highdicom"),
]


LEVEL_SCOPES = ["base", "pyramid"]


# How many trials and which to drop as warmup
N_TRIALS = 3
WARMUP_DROP = 1  # we will run N_TRIALS+WARMUP_DROP and report the last N_TRIALS


def slide_by_id(sid: str) -> Slide:
    for s in SLIDES:
        if s.slide_id == sid:
            return s
    raise KeyError(sid)
