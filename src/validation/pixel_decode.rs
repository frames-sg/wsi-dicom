use std::ffi::OsString;
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use dicom_core::value::Value;
use dicom_dictionary_std::tags;

use crate::{Error, TransferSyntax};

use super::pixel_structure::{
    assemble_encapsulated_frames, dicom_frame_count, optional_u64_values,
};
use super::process::ValidationCommandRunner;
use super::{
    failed_check, run_named_command_check, skipped_check, CommandCheckRequest, ValidationCheck,
    ValidationOptions, ValidationStatus,
};

pub(super) const AUTO_HTJ2K_DECODER_COMMAND: &str = "grk_decompress";

pub(super) fn auto_htj2k_decoder_template(runner: &impl ValidationCommandRunner) -> Option<String> {
    let path = runner.find_command(AUTO_HTJ2K_DECODER_COMMAND)?;
    path.is_absolute().then(|| {
        format!(
            "{} -i {{input}} -o {{output}}",
            shlex_quote_path_for_template(&path)
        )
    })
}

fn shlex_quote_path_for_template(path: &Path) -> String {
    let path = path.to_string_lossy();
    if path.chars().any(char::is_whitespace) || path.contains('\'') || path.contains('"') {
        let escaped = path.replace('\'', r"'\''");
        format!("'{escaped}'")
    } else {
        path.into_owned()
    }
}

pub(super) fn run_pixel_decode_checks(
    file_idx: usize,
    file: &PathBuf,
    options: &ValidationOptions,
    runner: &impl ValidationCommandRunner,
    temp_dir: &Path,
) -> Vec<ValidationCheck> {
    let object = match dicom_object::open_file(file) {
        Ok(object) => object,
        Err(err) => {
            return vec![failed_check(
                "pixel-decode",
                Some(file),
                format!("failed to read DICOM file for pixel decode: {err}"),
            )];
        }
    };
    let transfer_syntax = object.meta().transfer_syntax.trim_end_matches('\0');
    let Some(decoder) = pixel_decoder_for_transfer_syntax(transfer_syntax, options, runner) else {
        return vec![skipped_check(
            "pixel-decode",
            Some(file),
            format!("pixel decode not needed for transfer syntax {transfer_syntax}"),
        )];
    };
    if let PixelDecoder::Htj2kUnconfigured = decoder {
        let status = if options.strict {
            ValidationStatus::Failed
        } else {
            ValidationStatus::Skipped
        };
        return vec![ValidationCheck {
            name: "pixel-htj2k".to_string(),
            path: Some(file.clone()),
            status,
            command: Vec::new(),
            message: "HTJ2K decoder command is not configured".to_string(),
            stdout: String::new(),
            stderr: String::new(),
        }];
    }

    let expected = match decoded_frame_expectation(&object) {
        Ok(expected) => expected,
        Err(message) => return vec![failed_check("pixel-decode", Some(file), message)],
    };
    let frame_count = match dicom_frame_count(&object) {
        Ok(frame_count) => frame_count,
        Err(message) => return vec![failed_check("pixel-decode", Some(file), message)],
    };

    let pixel_data = match object.element(tags::PIXEL_DATA) {
        Ok(pixel_data) => pixel_data,
        Err(err) => {
            return vec![failed_check(
                "pixel-decode",
                Some(file),
                format!("failed to read Pixel Data: {err}"),
            )];
        }
    };
    let Value::PixelSequence(pixel_sequence) = pixel_data.value() else {
        return vec![failed_check(
            "pixel-decode",
            Some(file),
            "compressed Pixel Data is not encapsulated".to_string(),
        )];
    };
    if pixel_sequence.fragments().is_empty() {
        return vec![failed_check(
            "pixel-decode",
            Some(file),
            "Pixel Data has no fragments".to_string(),
        )];
    }

    let extended_offsets = match optional_u64_values(&object, tags::EXTENDED_OFFSET_TABLE) {
        Ok(values) => values,
        Err(message) => return vec![failed_check("pixel-decode", Some(file), message)],
    };
    let extended_lengths = match optional_u64_values(&object, tags::EXTENDED_OFFSET_TABLE_LENGTHS) {
        Ok(values) => values,
        Err(message) => return vec![failed_check("pixel-decode", Some(file), message)],
    };
    let frames = match assemble_encapsulated_frames(
        pixel_sequence,
        frame_count,
        extended_offsets.as_deref(),
        extended_lengths.as_deref(),
        options.max_pixel_frames,
        options.max_pixel_frame_bytes,
    ) {
        Ok(frames) => frames,
        Err(message) => return vec![failed_check("pixel-decode", Some(file), message)],
    };

    let mut checks = Vec::new();
    for (frame_idx, frame) in frames.iter().enumerate() {
        checks.push(run_pixel_decoder_for_fragment(
            &decoder,
            PixelFragmentDecode {
                file_idx,
                frame_idx,
                fragment: frame,
                file,
                runner,
                temp_dir,
                strict: options.strict,
                timeout: options.command_timeout(),
                max_output_bytes: options.max_child_output_bytes,
                max_decoded_bytes: options.max_pixel_frame_bytes,
                expected,
            },
        ));
    }
    checks
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DecodedFrameExpectation {
    pub(super) columns: u32,
    pub(super) rows: u32,
    pub(super) samples_per_pixel: Option<u16>,
    pub(super) bits_allocated: Option<u16>,
}

fn decoded_frame_expectation(
    object: &dicom_object::DefaultDicomObject,
) -> Result<DecodedFrameExpectation, String> {
    let columns = object
        .element(tags::COLUMNS)
        .map_err(|err| format!("DICOM Columns is missing: {err}"))?
        .to_int::<u32>()
        .map_err(|err| format!("failed to read DICOM Columns: {err}"))?;
    let rows = object
        .element(tags::ROWS)
        .map_err(|err| format!("DICOM Rows is missing: {err}"))?
        .to_int::<u32>()
        .map_err(|err| format!("failed to read DICOM Rows: {err}"))?;
    if columns == 0 || rows == 0 {
        return Err("DICOM Rows and Columns must be greater than zero".to_string());
    }
    let samples_per_pixel = object
        .element(tags::SAMPLES_PER_PIXEL)
        .ok()
        .map(|element| {
            element
                .to_int::<u16>()
                .map_err(|err| format!("failed to read DICOM Samples per Pixel: {err}"))
        })
        .transpose()?;
    let bits_allocated = object
        .element(tags::BITS_ALLOCATED)
        .ok()
        .map(|element| {
            element
                .to_int::<u16>()
                .map_err(|err| format!("failed to read DICOM Bits Allocated: {err}"))
        })
        .transpose()?;
    Ok(DecodedFrameExpectation {
        columns,
        rows,
        samples_per_pixel,
        bits_allocated,
    })
}

enum PixelDecoder {
    Djpeg,
    OpenJpeg,
    Htj2kUnconfigured,
    Htj2k { template: String },
}

fn pixel_decoder_for_transfer_syntax(
    transfer_syntax_uid: &str,
    options: &ValidationOptions,
    runner: &impl ValidationCommandRunner,
) -> Option<PixelDecoder> {
    match transfer_syntax_uid {
        uid if uid == TransferSyntax::JpegBaseline8Bit.uid() => Some(PixelDecoder::Djpeg),
        uid if uid == TransferSyntax::Jpeg2000.uid()
            || uid == TransferSyntax::Jpeg2000Lossless.uid() =>
        {
            Some(PixelDecoder::OpenJpeg)
        }
        uid if uid == TransferSyntax::Htj2k.uid()
            || uid == TransferSyntax::Htj2kLossless.uid()
            || uid == TransferSyntax::Htj2kLosslessRpcl.uid() =>
        {
            Some(
                options
                    .htj2k_decoder
                    .clone()
                    .or_else(|| auto_htj2k_decoder_template(runner))
                    .as_ref()
                    .map(|template| PixelDecoder::Htj2k {
                        template: template.clone(),
                    })
                    .unwrap_or(PixelDecoder::Htj2kUnconfigured),
            )
        }
        _ => None,
    }
}

struct PixelFragmentDecode<'a, R: ValidationCommandRunner> {
    file_idx: usize,
    frame_idx: usize,
    fragment: &'a [u8],
    file: &'a PathBuf,
    runner: &'a R,
    temp_dir: &'a Path,
    strict: bool,
    timeout: Duration,
    max_output_bytes: usize,
    max_decoded_bytes: usize,
    expected: DecodedFrameExpectation,
}

fn run_pixel_decoder_for_fragment<R: ValidationCommandRunner>(
    decoder: &PixelDecoder,
    request: PixelFragmentDecode<'_, R>,
) -> ValidationCheck {
    let input = request.temp_dir.join(format!(
        "file-{:04}-frame-{:06}.codestream",
        request.file_idx, request.frame_idx
    ));
    let output = request.temp_dir.join(format!(
        "file-{:04}-frame-{:06}.ppm",
        request.file_idx, request.frame_idx
    ));
    if let Err(err) = write_private_validation_file(&input, request.fragment) {
        return failed_check(
            "pixel-decode",
            Some(request.file),
            format!("failed to write temporary codestream: {err}"),
        );
    }

    let check = match decoder {
        PixelDecoder::Djpeg => run_named_command_check(
            request.runner,
            CommandCheckRequest {
                check_name: "pixel-djpeg",
                command_name: "djpeg",
                args: vec![
                    OsString::from("-outfile"),
                    output.as_os_str().to_os_string(),
                    input.as_os_str().to_os_string(),
                ],
                path: Some(request.file),
                required: request.strict,
                error_line_is_failure: false,
                timeout: request.timeout,
                max_output_bytes: request.max_output_bytes,
            },
        ),
        PixelDecoder::OpenJpeg => run_named_command_check(
            request.runner,
            CommandCheckRequest {
                check_name: "pixel-opj-decompress",
                command_name: "opj_decompress",
                args: vec![
                    OsString::from("-i"),
                    input.as_os_str().to_os_string(),
                    OsString::from("-o"),
                    output.as_os_str().to_os_string(),
                ],
                path: Some(request.file),
                required: request.strict,
                error_line_is_failure: false,
                timeout: request.timeout,
                max_output_bytes: request.max_output_bytes,
            },
        ),
        PixelDecoder::Htj2k { template } => {
            let (command, args) = match htj2k_decoder_command(template, &input, &output) {
                Ok(command) => command,
                Err(message) => {
                    return failed_check("pixel-htj2k", Some(request.file), message);
                }
            };
            run_named_command_check(
                request.runner,
                CommandCheckRequest {
                    check_name: "pixel-htj2k",
                    command_name: &command,
                    args,
                    path: Some(request.file),
                    required: request.strict,
                    error_line_is_failure: false,
                    timeout: request.timeout,
                    max_output_bytes: request.max_output_bytes,
                },
            )
        }
        PixelDecoder::Htj2kUnconfigured => skipped_check(
            "pixel-htj2k",
            Some(request.file),
            "HTJ2K decoder command is not configured".to_string(),
        ),
    };
    validate_decoded_output(check, &output, request.expected, request.max_decoded_bytes)
}

fn validate_decoded_output(
    mut check: ValidationCheck,
    output: &Path,
    expected: DecodedFrameExpectation,
    max_decoded_bytes: usize,
) -> ValidationCheck {
    if check.status != ValidationStatus::Passed {
        return check;
    }
    if let Err(message) = inspect_pnm_output(output, expected, max_decoded_bytes) {
        check.status = ValidationStatus::Failed;
        check.message = message;
    }
    check
}

pub(super) fn inspect_pnm_output(
    output: &Path,
    expected: DecodedFrameExpectation,
    max_decoded_bytes: usize,
) -> Result<(), String> {
    let mut file = fs::File::open(output).map_err(|err| {
        format!(
            "decoder did not create readable output {}: {err}",
            output.display()
        )
    })?;
    let file_len = file
        .metadata()
        .map_err(|err| format!("inspect decoder output {}: {err}", output.display()))?
        .len();
    let max_decoded_bytes = u64::try_from(max_decoded_bytes).unwrap_or(u64::MAX);
    if file_len > max_decoded_bytes {
        return Err(format!(
            "decoder output {} exceeds {max_decoded_bytes} byte validation limit",
            output.display()
        ));
    }

    let magic = read_pnm_token(&mut file)?;
    let components = match magic.as_str() {
        "P5" => 1u64,
        "P6" => 3u64,
        _ => {
            return Err(format!(
                "decoder output uses unsupported PNM magic {magic:?}"
            ))
        }
    };
    let columns = parse_pnm_u32(&mut file, "width")?;
    let rows = parse_pnm_u32(&mut file, "height")?;
    let max_value = parse_pnm_u32(&mut file, "maximum sample value")?;
    if columns != expected.columns || rows != expected.rows {
        return Err(format!(
            "decoder output dimensions {columns}x{rows} do not match DICOM {}x{}",
            expected.columns, expected.rows
        ));
    }
    if let Some(samples_per_pixel) = expected.samples_per_pixel {
        if u64::from(samples_per_pixel) != components {
            return Err(format!(
                "decoder output has {components} component(s), expected {samples_per_pixel}"
            ));
        }
    }
    if !matches!(max_value, 255 | 65_535) {
        return Err(format!(
            "decoder output maximum sample value {max_value} is unsupported"
        ));
    }
    if let Some(bits_allocated) = expected.bits_allocated {
        let expected_max = match bits_allocated {
            8 => 255,
            16 => 65_535,
            other => {
                return Err(format!(
                    "DICOM Bits Allocated {other} is unsupported for PNM validation"
                ));
            }
        };
        if max_value != expected_max {
            return Err(format!(
                "decoder output maximum sample value {max_value} does not match {bits_allocated}-bit DICOM pixels"
            ));
        }
    }
    let bytes_per_sample = if max_value > 255 { 2u64 } else { 1u64 };
    let payload_len = u64::from(columns)
        .checked_mul(u64::from(rows))
        .and_then(|value| value.checked_mul(components))
        .and_then(|value| value.checked_mul(bytes_per_sample))
        .ok_or_else(|| "decoder output dimensions overflow payload length".to_string())?;
    let payload_start = file
        .stream_position()
        .map_err(|err| format!("inspect decoder output payload: {err}"))?;
    let expected_file_len = payload_start
        .checked_add(payload_len)
        .ok_or_else(|| "decoder output length overflows u64".to_string())?;
    if file_len != expected_file_len {
        return Err(format!(
            "decoder output payload has {} bytes, expected {payload_len}",
            file_len.saturating_sub(payload_start)
        ));
    }
    Ok(())
}

fn parse_pnm_u32(file: &mut fs::File, field: &str) -> Result<u32, String> {
    let token = read_pnm_token(file)?;
    token
        .parse::<u32>()
        .map_err(|err| format!("decoder output has invalid PNM {field} {token:?}: {err}"))
}

fn read_pnm_token(file: &mut fs::File) -> Result<String, String> {
    let mut token = Vec::new();
    let mut in_comment = false;
    loop {
        let mut byte = [0u8; 1];
        if file
            .read(&mut byte)
            .map_err(|err| format!("read decoder PNM header: {err}"))?
            == 0
        {
            if token.is_empty() {
                return Err("decoder output ended inside the PNM header".to_string());
            }
            break;
        }
        let byte = byte[0];
        if in_comment {
            if byte == b'\n' {
                in_comment = false;
            }
            continue;
        }
        if token.is_empty() && byte == b'#' {
            in_comment = true;
            continue;
        }
        if byte.is_ascii_whitespace() {
            if token.is_empty() {
                continue;
            }
            break;
        }
        token.push(byte);
        if token.len() > 64 {
            return Err("decoder output PNM header token exceeds 64 bytes".to_string());
        }
    }
    String::from_utf8(token).map_err(|err| format!("decoder output PNM header is not ASCII: {err}"))
}

pub(crate) fn htj2k_decoder_command(
    template: &str,
    input: &Path,
    output: &Path,
) -> Result<(String, Vec<OsString>), String> {
    let mut parts = shlex::split(template)
        .ok_or_else(|| "HTJ2K decoder command has invalid quoting".to_string())?;
    if parts.is_empty() {
        return Err("HTJ2K decoder command is empty".to_string());
    }
    let command = parts.remove(0);
    if command.trim().is_empty() {
        return Err("HTJ2K decoder command is empty".to_string());
    }
    if !Path::new(&command).is_absolute() {
        return Err(
            "HTJ2K decoder command must start with an absolute executable path".to_string(),
        );
    }
    let mut saw_placeholder = false;
    let mut args = parts
        .into_iter()
        .map(|part| {
            let replaced = part
                .replace("{input}", &input.to_string_lossy())
                .replace("{output}", &output.to_string_lossy());
            if replaced != part {
                saw_placeholder = true;
            }
            OsString::from(replaced)
        })
        .collect::<Vec<_>>();
    if !saw_placeholder {
        args.push(input.as_os_str().to_os_string());
    }
    Ok((command, args))
}

pub(super) fn write_private_validation_file(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.write_all(bytes).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.sync_all().map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub(super) struct ValidationTempDir {
    inner: tempfile::TempDir,
}

impl ValidationTempDir {
    pub(super) fn create() -> Result<Self, Error> {
        let inner = tempfile::Builder::new()
            .prefix("wsi-dicom-validation-")
            .tempdir()
            .map_err(|source| Error::Io {
                path: std::env::temp_dir(),
                source,
            })?;
        set_private_validation_dir_permissions(inner.path())?;
        Ok(Self { inner })
    }

    pub(super) fn path(&self) -> &Path {
        self.inner.path()
    }
}

#[cfg(unix)]
fn set_private_validation_dir_permissions(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|source| {
        Error::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn set_private_validation_dir_permissions(_path: &Path) -> Result<(), Error> {
    Ok(())
}
