use std::path::{Path, PathBuf};

use wsi_dicom::{
    run_dicom_self_test, EncodeBackendPreference, ExportOptions, SelfTestOptions, TransferSyntax,
    UidPolicy, ValidationOptions,
};

struct ControlSpec {
    id: &'static str,
    transfer_syntax: TransferSyntax,
    tile_size: u32,
    jpeg_quality: u8,
}

const CONTROLS: [ControlSpec; 10] = [
    ControlSpec {
        id: "VC01-EVRLE-4F-ANISO",
        transfer_syntax: TransferSyntax::Jpeg2000Lossless,
        tile_size: 2,
        jpeg_quality: 90,
    },
    ControlSpec {
        id: "VC02-JPEG-16F",
        transfer_syntax: TransferSyntax::JpegBaseline8Bit,
        tile_size: 1,
        jpeg_quality: 83,
    },
    ControlSpec {
        id: "VC03-JPEG-1F",
        transfer_syntax: TransferSyntax::JpegBaseline8Bit,
        tile_size: 4,
        jpeg_quality: 91,
    },
    ControlSpec {
        id: "VC04-J2K-LOSSLESS-16F",
        transfer_syntax: TransferSyntax::Jpeg2000Lossless,
        tile_size: 1,
        jpeg_quality: 90,
    },
    ControlSpec {
        id: "VC05-J2K-LOSSY-4F",
        transfer_syntax: TransferSyntax::Jpeg2000Lossless,
        tile_size: 2,
        jpeg_quality: 73,
    },
    ControlSpec {
        id: "VC06-HTJ2K-LOSSLESS-16F",
        transfer_syntax: TransferSyntax::Htj2kLossless,
        tile_size: 1,
        jpeg_quality: 90,
    },
    ControlSpec {
        id: "VC07-HTJ2K-RPCL-4F",
        transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
        tile_size: 2,
        jpeg_quality: 90,
    },
    ControlSpec {
        id: "VC08-HTJ2K-LOSSY-4F",
        transfer_syntax: TransferSyntax::Htj2kLossless,
        tile_size: 2,
        jpeg_quality: 67,
    },
    ControlSpec {
        id: "VC09-J2K-LOSSLESS-4F",
        transfer_syntax: TransferSyntax::Jpeg2000Lossless,
        tile_size: 2,
        jpeg_quality: 90,
    },
    ControlSpec {
        id: "VC10-EVRLE-16F",
        transfer_syntax: TransferSyntax::Jpeg2000Lossless,
        tile_size: 1,
        jpeg_quality: 90,
    },
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: generate_negative_bench_controls OUTPUT")?;
    if output.exists() {
        return Err(format!("output already exists: {}", output.display()).into());
    }
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".negative-bench-controls-")
        .tempdir_in(parent)?;

    for spec in &CONTROLS {
        let workspace = staging.path().join("work").join(spec.id);
        let mut export = ExportOptions::default();
        export.transfer_syntax = spec.transfer_syntax;
        export.tile_size = spec.tile_size;
        export.jpeg_quality = spec.jpeg_quality;
        export.encode_backend = EncodeBackendPreference::CpuOnly;
        export.uid_policy = UidPolicy::Deterministic;
        let mut validation = ValidationOptions::default();
        validation.max_pixel_frames = 0;
        let mut options = SelfTestOptions::default();
        options.output_dir = Some(workspace);
        options.keep_output = true;
        options.export = export;
        options.validation = validation;
        let report = run_dicom_self_test(options)?;
        let instance = report
            .export_report
            .instances
            .first()
            .ok_or("self-test produced no VL WSI instance")?;
        std::fs::copy(
            &instance.path,
            staging.path().join(format!("{}.dcm", spec.id)),
        )?;
    }

    let staging_path = staging.keep();
    std::fs::rename(&staging_path, &output)?;
    Ok(())
}
