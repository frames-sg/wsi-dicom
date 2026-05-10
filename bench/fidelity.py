"""Fidelity comparison: source pixels vs. output DICOM pixels.

Strategy: sample N random patches at deterministic seed across the requested level.
For each patch, decode both source (openslide) and output (wsidicom), then compute:
  - PSNR (skimage)
  - SSIM (skimage, multichannel)
  - mean and 95th-percentile ΔE2000 in CIELAB (colour-science)
  - bit-exact boolean (all pixels equal)

Also checks ICC profile presence and byte-equality at the file level.
"""
from __future__ import annotations
from dataclasses import dataclass, asdict
from typing import Optional
import numpy as np

import openslide

# wsidicom for reading output (it indexes a directory of DICOM files into a slide)
from wsidicom import WsiDicom


N_PATCHES = 24
PATCH = 256
RNG_SEED = 1234


@dataclass
class FidelityResult:
    bit_exact_fraction: float       # fraction of patches with bit-exact match
    psnr_mean_db: float
    psnr_min_db: float
    ssim_mean: float
    ssim_min: float
    deltaE_mean: float
    deltaE_p95: float
    deltaE_max: float
    n_patches: int
    icc_source_present: bool
    icc_output_present: bool
    icc_match: Optional[bool]       # None if either is absent
    error: Optional[str] = None


def _icc_from_openslide(s: openslide.OpenSlide) -> Optional[bytes]:
    # openslide-python 1.4 exposes color_profile when available
    cp = getattr(s, "color_profile", None)
    if cp is None:
        return None
    return cp.tobytes() if hasattr(cp, "tobytes") else bytes(cp)


def _icc_from_wsidicom(w: WsiDicom) -> Optional[bytes]:
    try:
        # OpticalPathSequence may carry ICCProfile bytes
        for ds in (w.metadata.optical_paths or []):
            icc = getattr(ds, "icc_profile", None)
            if icc:
                return bytes(icc)
    except Exception:
        return None
    return None


def _delta_e2000(rgb_a: np.ndarray, rgb_b: np.ndarray) -> np.ndarray:
    import colour
    a01 = (rgb_a.astype(np.float32) / 255.0).reshape(-1, 3)
    b01 = (rgb_b.astype(np.float32) / 255.0).reshape(-1, 3)
    lab_a = colour.XYZ_to_Lab(colour.sRGB_to_XYZ(a01))
    lab_b = colour.XYZ_to_Lab(colour.sRGB_to_XYZ(b01))
    return colour.delta_E(lab_a, lab_b, method="CIE 2000")


def compute_fidelity(
    source_path: str,
    output_dicom_dir: str,
    output_level_index_in_source: int = 0,
) -> FidelityResult:
    """Compare a sampled subset of patches between the source slide and the
    DICOM output produced for ``output_level_index_in_source``.

    The output DICOM directory is expected to be a directory of one or more
    .dcm files (the wsi-dicom and wsidicomizer convention).
    """
    from skimage.metrics import peak_signal_noise_ratio, structural_similarity

    src = openslide.OpenSlide(source_path)
    try:
        # Pick an interior region to dodge background-only sampling.
        lvl = output_level_index_in_source
        if lvl >= src.level_count:
            lvl = src.level_count - 1
        w, h = src.level_dimensions[lvl]
        ds = src.level_downsamples[lvl]

        rng = np.random.default_rng(RNG_SEED)
        # Sample x,y in pixel coords on this level, leave room for a patch.
        coords_lvl = []
        for _ in range(N_PATCHES):
            x = int(rng.integers(0, max(1, w - PATCH)))
            y = int(rng.integers(0, max(1, h - PATCH)))
            coords_lvl.append((x, y))

        # Read output via wsidicom.
        try:
            out = WsiDicom.open(output_dicom_dir)
        except Exception as e:
            return FidelityResult(0, 0, 0, 0, 0, 0, 0, 0, 0,
                                  icc_source_present=_icc_from_openslide(src) is not None,
                                  icc_output_present=False,
                                  icc_match=None,
                                  error=f"wsidicom.open failed: {e}")

        try:
            psnrs, ssims, des, exacts, des_max = [], [], [], [], []
            for (x_lvl, y_lvl) in coords_lvl:
                # source: read_region uses level-0 coordinates
                x0 = int(x_lvl * ds)
                y0 = int(y_lvl * ds)
                src_rgba = np.array(src.read_region((x0, y0), lvl, (PATCH, PATCH)))
                src_rgb = src_rgba[..., :3]

                # output: read same region by level-0 coords + level index
                # wsidicom maps levels by pixel spacing; we use index by output_levels
                # ordering, which mirrors source pyramid order.
                out_levels = sorted(out.levels, key=lambda L: -L.size.width)
                out_level = out_levels[min(lvl, len(out_levels) - 1)]
                out_img = out.read_region(
                    location=(x0, y0),
                    level=out_levels.index(out_level),
                    size=(PATCH, PATCH),
                )
                out_rgb = np.array(out_img)
                if out_rgb.shape[-1] == 4:
                    out_rgb = out_rgb[..., :3]

                if src_rgb.shape != out_rgb.shape:
                    # Pad/crop to match (edge effects from differing tile geometry)
                    h_min = min(src_rgb.shape[0], out_rgb.shape[0])
                    w_min = min(src_rgb.shape[1], out_rgb.shape[1])
                    src_rgb = src_rgb[:h_min, :w_min, :]
                    out_rgb = out_rgb[:h_min, :w_min, :]

                bit_exact = np.array_equal(src_rgb, out_rgb)
                exacts.append(bit_exact)

                if bit_exact:
                    psnrs.append(float("inf"))
                    ssims.append(1.0)
                    des.append(0.0)
                    des_max.append(0.0)
                    continue

                psnrs.append(peak_signal_noise_ratio(src_rgb, out_rgb, data_range=255))
                ssims.append(
                    structural_similarity(
                        src_rgb, out_rgb, channel_axis=2, data_range=255
                    )
                )
                de = _delta_e2000(src_rgb, out_rgb)
                des.append(float(np.mean(de)))
                des_max.append(float(np.max(de)))

            # Aggregate
            finite_psnrs = [p for p in psnrs if np.isfinite(p)]
            psnr_mean = float(np.mean(finite_psnrs)) if finite_psnrs else float("inf")
            psnr_min = float(np.min(finite_psnrs)) if finite_psnrs else float("inf")
            de_arr = np.array(des, dtype=np.float64)
            de_max_arr = np.array(des_max, dtype=np.float64)

            icc_src = _icc_from_openslide(src)
            icc_out = _icc_from_wsidicom(out)
            icc_match = None
            if icc_src is not None and icc_out is not None:
                icc_match = (icc_src == icc_out)

            return FidelityResult(
                bit_exact_fraction=float(np.mean(exacts)),
                psnr_mean_db=psnr_mean,
                psnr_min_db=psnr_min,
                ssim_mean=float(np.mean(ssims)),
                ssim_min=float(np.min(ssims)),
                deltaE_mean=float(np.mean(de_arr)),
                deltaE_p95=float(np.percentile(de_arr, 95)),
                deltaE_max=float(np.max(de_max_arr)),
                n_patches=len(psnrs),
                icc_source_present=icc_src is not None,
                icc_output_present=icc_out is not None,
                icc_match=icc_match,
            )
        finally:
            out.close()
    finally:
        src.close()


def to_json_dict(r: FidelityResult) -> dict:
    d = asdict(r)
    # JSON can't serialize inf, replace with string
    for k, v in list(d.items()):
        if isinstance(v, float) and not np.isfinite(v):
            d[k] = "inf" if v > 0 else "-inf"
    return d
