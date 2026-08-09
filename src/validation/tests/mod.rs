use super::{
    assemble_encapsulated_frames, doctor_dicom_environment_with_runner,
    fragment_payload_without_padding, htj2k_decoder_command, inspect_pnm_output,
    run_named_command_check, staged_dicom3tools_probe_enabled_from,
    validate_dicom_path_with_runner, write_private_validation_file, CommandCheckRequest,
    CommandOutcome, DecodedFrameExpectation, DoctorOptions, DoctorStatus, SystemCommandRunner,
    ValidationCommandRunner, ValidationOptions, ValidationStatus, ValidationTempDir,
    AUTO_HTJ2K_DECODER_COMMAND, VALIDATOR_SET_FILE_CHUNK_SIZE,
};
use crate::TransferSyntax;
use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::tags;
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(not(windows))]
const ABSOLUTE_HTJ2K_DECODER: &str = "/usr/local/bin/ojph_expand";
#[cfg(windows)]
const ABSOLUTE_HTJ2K_DECODER: &str = "C:/Tools/ojph_expand.exe";
#[cfg(not(windows))]
const ABSOLUTE_GROK_DECODER: &str = "/usr/local/bin/grk_decompress";
#[cfg(windows)]
const ABSOLUTE_GROK_DECODER: &str = "C:/Tools/grk_decompress.exe";

#[derive(Default)]
struct FakeRunner {
    commands: BTreeSet<String>,
    command_paths: BTreeMap<String, PathBuf>,
    outcomes: BTreeMap<String, CommandOutcome>,
}

impl FakeRunner {
    fn with_command(mut self, name: &str) -> Self {
        self.commands.insert(name.to_string());
        self
    }

    fn with_command_path(mut self, name: &str, path: &str) -> Self {
        self.command_paths
            .insert(name.to_string(), PathBuf::from(path));
        self.commands.insert(path.to_string());
        self
    }

    fn with_outcome(mut self, command: &str, outcome: CommandOutcome) -> Self {
        self.outcomes.insert(command.to_string(), outcome);
        self
    }
}

impl ValidationCommandRunner for FakeRunner {
    fn find_command(&self, name: &str) -> Option<PathBuf> {
        if let Some(path) = self.command_paths.get(name) {
            return Some(path.clone());
        }
        self.commands.contains(name).then(|| PathBuf::from(name))
    }

    fn run(
        &self,
        program: &Path,
        args: &[OsString],
        _timeout: Duration,
        _max_output_bytes: usize,
    ) -> Result<CommandOutcome, std::io::Error> {
        let mut key = program.display().to_string();
        for arg in args {
            key.push(' ');
            key.push_str(&arg.to_string_lossy());
        }
        let outcome = self.outcomes.get(&key).cloned().unwrap_or(CommandOutcome {
            success: true,
            timed_out: false,
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        });
        if outcome.success {
            for pair in args.windows(2) {
                if matches!(pair[0].to_str(), Some("-o" | "-outfile")) {
                    std::fs::write(PathBuf::from(&pair[1]), b"P6\n1 1\n255\n\x00\x00\x00")?;
                }
            }
        }
        Ok(outcome)
    }
}

mod conformance;
mod doctor;
mod orchestration;
mod pixel_decode;
mod runner;

fn write_encapsulated_dicom(path: &Path, transfer_syntax: &str, frame: &[u8]) {
    let mut object = InMemDicomObject::new_empty();
    object.put(DataElement::<InMemDicomObject>::new(
        tags::SOP_CLASS_UID,
        VR::UI,
        PrimitiveValue::from("1.2.840.10008.5.1.4.1.1.77.1.6"),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::SOP_INSTANCE_UID,
        VR::UI,
        PrimitiveValue::from("1.2.826.0.1.3680043.10.999.200"),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::ROWS,
        VR::US,
        PrimitiveValue::from(1u16),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::COLUMNS,
        VR::US,
        PrimitiveValue::from(1u16),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::NUMBER_OF_FRAMES,
        VR::IS,
        PrimitiveValue::from("1"),
    ));
    let meta = FileMetaTableBuilder::new()
        .media_storage_sop_class_uid("1.2.840.10008.5.1.4.1.1.77.1.6")
        .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.999.200")
        .transfer_syntax(transfer_syntax);
    let file = File::create(path).expect("create DICOM");
    let mut output = BufWriter::new(file);
    object
        .with_meta(meta)
        .expect("file meta")
        .write_all(&mut output)
        .expect("write DICOM object");
    crate::writer::write_encapsulated_pixel_data_from_frames(
        &mut output,
        &[u64::try_from(frame.len()).expect("frame len")],
        |_, output| output.write_all(frame),
    )
    .expect("write pixel data");
    output.flush().expect("flush DICOM");
}

fn write_primitive_pixel_dicom(path: &Path, transfer_syntax: &str, bytes: &[u8]) {
    const SECONDARY_CAPTURE_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.1.7";
    let mut object = InMemDicomObject::new_empty();
    object.put(DataElement::<InMemDicomObject>::new(
        tags::SOP_CLASS_UID,
        VR::UI,
        PrimitiveValue::from(SECONDARY_CAPTURE_SOP_CLASS_UID),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::SOP_INSTANCE_UID,
        VR::UI,
        PrimitiveValue::from("1.2.826.0.1.3680043.10.999.201"),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::ROWS,
        VR::US,
        PrimitiveValue::from(1u16),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::COLUMNS,
        VR::US,
        PrimitiveValue::from(1u16),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::NUMBER_OF_FRAMES,
        VR::IS,
        PrimitiveValue::from("1"),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::PIXEL_DATA,
        VR::OB,
        PrimitiveValue::U8(bytes.to_vec().into()),
    ));
    object
        .with_meta(
            FileMetaTableBuilder::new()
                .media_storage_sop_class_uid(SECONDARY_CAPTURE_SOP_CLASS_UID)
                .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.999.201")
                .transfer_syntax(transfer_syntax),
        )
        .expect("file meta")
        .write_to_file(path)
        .expect("write primitive Pixel Data DICOM");
}

fn export_valid_color_wsi_for_validation(root: &Path, name: &str) -> PathBuf {
    let source = root.join(format!("{name}-source.dcm"));
    let pixels = crate::synthetic_source::deterministic_rgb_pixels(2, 2).unwrap();
    crate::synthetic_source::write_rgb_source_dicom(
        &source,
        "1.2.826.0.1.3680043.10.999.301",
        "1.2.826.0.1.3680043.10.999.302",
        2,
        2,
        pixels,
    )
    .unwrap();
    crate::export_dicom(crate::ExportRequest {
        source_path: source,
        output_dir: root.join(format!("{name}-out")),
        options: crate::ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::Jpeg2000Lossless,
            encode_backend: crate::EncodeBackendPreference::CpuOnly,
            ..crate::ExportOptions::default()
        },
        color_management: crate::ColorManagement::SourceOrSrgb,
        metadata: crate::MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap()
    .instances
    .remove(0)
    .path
}

fn assert_intrinsic_rule_fails(path: &Path, name: &str) {
    let report = validate_dicom_path_with_runner(
        path,
        &ValidationOptions {
            max_pixel_frames: 0,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .unwrap();
    assert!(
        report
            .checks
            .iter()
            .any(|check| { check.name == name && check.status == ValidationStatus::Failed }),
        "missing failed intrinsic check {name}"
    );
}
