<!-- SPDX-License-Identifier: Apache-2.0 -->

# wsi-dicom

`wsi-dicom` is the DICOM whole-slide export layer for `statumen`.

It is intentionally a sibling crate, not a `statumen` module and not
SlideViewer application code. The dependency direction is:

```text
vendor WSI files -> statumen -> wsi-dicom -> app or CLI integration
```

`statumen` owns WSI format detection, vendor parsing, pyramid geometry, and
tile/region reads. `wsi-dicom` will own DICOM VL Whole Slide Microscopy export:
transfer syntax selection, output layout, metadata validation, and writer
errors. SlideViewer should call into this crate for user-facing export flows
without taking ownership of DICOM writing rules.

## Name

`wsi-dicom` is intentionally literal: it converts whole-slide imaging data into
DICOM VL Whole Slide Microscopy objects. The package imports as `wsi_dicom` in
Rust code.

## Packaging

`wsi-dicom` is the public facade crate and CLI. Users should depend on this
crate directly; JPEG 2000 and WSI reader crates are internal implementation
dependencies.

Default builds are CPU-only:

```toml
[dependencies]
wsi-dicom = "0.1"
```

GPU support is opt-in:

```toml
[dependencies]
wsi-dicom = { version = "0.1", features = ["gpu"] }
```

Feature flags:

| Feature | Effect |
| --- | --- |
| `default` | CPU-only DICOM export. |
| `gpu` | Enables both `cuda` and `metal` backends. |
| `cuda` | Enables CUDA JPEG 2000 encode acceleration when available. |
| `metal` | Enables Metal JPEG 2000 encode acceleration on macOS, Metal codestream validation decode, and statumen Metal tile decode plumbing. |
| `vendored-codecs` | Reserved facade feature for bundled codec builds; currently no extra native codec dependency is required. |

Runtime backend selection is still controlled by the CLI/API backend option:
`auto`, `cpu`, `prefer-device`, or `require-device`. If GPU features are not
compiled in, `prefer-device` falls back to CPU and `require-device` reports a
clear unsupported-device error.

With the `metal` feature on macOS, statumen source tiles are requested as
batched row runs of Metal device tiles and bridged directly into the Metal J2K
encoder when they align to the requested DICOM tile size. WholeLevel virtual
tile grids, including NDPI strip grids, are decoded in source-tile batches and
composed into DICOM frame buffers by a private Metal kernel before encoding.
Edge tiles are padded on Metal before encoding. The export report and CLI output
include frame counts for CPU input, GPU input decode, GPU encode, and GPU
validation decode. Statumen's compressed TIFF-family device decode paths remain
opt-in: set `STATUMEN_JPEG_DEVICE_DECODE=1` for JPEG-backed WSI tiles and
`STATUMEN_JP2K_DEVICE_DECODE=1` for JPEG 2000-backed WSI tiles.

Supported lossless export transfer syntaxes:

| CLI value | UID | Description |
| --- | --- | --- |
| `jpeg2000-lossless` | `1.2.840.10008.1.2.4.90` | JPEG 2000 Lossless |
| `htj2k-lossless` | `1.2.840.10008.1.2.4.201` | HTJ2K Lossless |
| `htj2k-lossless-rpcl` | `1.2.840.10008.1.2.4.202` | HTJ2K Lossless RPCL with RPCL progression and TLM markers |
