use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{
    synthetic_source::{deterministic_rgb_pixels, write_rgb_source_dicom},
    validate_dicom_path, Error, Export, ExportOptions, ExportReport, MetadataSource,
    ValidationOptions, ValidationReport,
};

/// Options for generating and validating a deterministic tiny DICOM export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct SelfTestOptions {
    /// Optional workspace directory for self-test inputs, outputs, and evidence.
    pub output_dir: Option<PathBuf>,
    /// Keep the self-test workspace after the run.
    pub keep_output: bool,
    /// Export options used for the generated source.
    pub export: ExportOptions,
    /// Validation options used against the generated output.
    pub validation: ValidationOptions,
}

impl Default for SelfTestOptions {
    fn default() -> Self {
        let export = ExportOptions {
            transfer_syntax: crate::TransferSyntax::Jpeg2000Lossless,
            ..ExportOptions::default()
        };
        Self {
            output_dir: None,
            keep_output: false,
            export,
            validation: ValidationOptions::default(),
        }
    }
}

/// Report returned by the deterministic DICOM self-test.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SelfTestReport {
    /// Workspace containing generated source and output evidence.
    pub workspace: PathBuf,
    /// Generated source DICOM path.
    pub source_path: PathBuf,
    /// Generated output DICOM directory.
    pub output_dir: PathBuf,
    /// Whether output evidence was preserved on disk.
    pub kept_output: bool,
    /// Export report from converting the generated source.
    pub export_report: ExportReport,
    /// Validation report for the generated output.
    pub validation_report: ValidationReport,
}

/// Generate a tiny deterministic source DICOM, export it, and validate the output.
pub fn run_dicom_self_test(options: SelfTestOptions) -> Result<SelfTestReport, Error> {
    run_dicom_self_test_with_validation(options, |path, validation| {
        validate_dicom_path(path, validation)
    })
}

fn run_dicom_self_test_with_validation(
    options: SelfTestOptions,
    validate: impl FnOnce(&Path, &ValidationOptions) -> Result<ValidationReport, Error>,
) -> Result<SelfTestReport, Error> {
    let workspace = SelfTestWorkspace::create(options.output_dir.as_deref(), options.keep_output)?;
    let source_path = workspace.path().join("source.dcm");
    let output_dir = workspace.path().join("dicom");
    std::fs::create_dir_all(&output_dir).map_err(|source| Error::Io {
        path: output_dir.clone(),
        source,
    })?;
    write_self_test_source_dicom(&source_path)?;

    let export_options = options.export.clone();
    export_options.validate()?;
    let export_report = Export::from_slide(&source_path)
        .to_directory(&output_dir)
        .with_metadata(MetadataSource::ResearchPlaceholder)
        .with_options(export_options)
        .color_management(crate::ColorManagement::SourceOrSrgb)
        .run()?;
    let validation_report = validate(&output_dir, &options.validation)?;
    let kept_output = workspace.kept_output();
    let workspace_path = workspace.path().to_path_buf();
    if kept_output {
        workspace.keep();
    }

    Ok(SelfTestReport {
        workspace: workspace_path,
        source_path,
        output_dir,
        kept_output,
        export_report,
        validation_report,
    })
}

struct SelfTestWorkspace {
    path: PathBuf,
    cleanup: bool,
}

impl SelfTestWorkspace {
    fn create(output_dir: Option<&Path>, keep_output: bool) -> Result<Self, Error> {
        if let Some(path) = output_dir {
            std::fs::create_dir_all(path).map_err(|source| Error::Io {
                path: path.to_path_buf(),
                source,
            })?;
            return Ok(Self {
                path: path.to_path_buf(),
                cleanup: false,
            });
        }

        let base = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        for attempt in 0..1000u32 {
            let path = base.join(format!(
                "wsi-dicom-self-test-{}-{nanos}-{attempt}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    return Ok(Self {
                        path,
                        cleanup: !keep_output,
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(source) => return Err(Error::Io { path, source }),
            }
        }

        Err(Error::Validation {
            reason: "failed to create a unique self-test directory".into(),
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn kept_output(&self) -> bool {
        !self.cleanup
    }

    fn keep(mut self) {
        self.cleanup = false;
    }
}

impl Drop for SelfTestWorkspace {
    fn drop(&mut self) {
        if self.cleanup {
            if let Err(err) = std::fs::remove_dir_all(&self.path) {
                if err.kind() != std::io::ErrorKind::NotFound {
                    eprintln!(
                        "wsi-dicom: failed to remove self-test workspace {}: {err}",
                        self.path.display()
                    );
                }
            }
        }
    }
}

fn write_self_test_source_dicom(path: &Path) -> Result<(), Error> {
    let width = 4u32;
    let height = 4u32;
    let sop_instance_uid = "1.2.826.0.1.3680043.10.999.2000";
    write_rgb_source_dicom(
        path,
        sop_instance_uid,
        "1.2.826.0.1.3680043.10.999.200",
        width,
        height,
        deterministic_rgb_pixels(width, height)?,
    )
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::{run_dicom_self_test_with_validation, SelfTestOptions};
    use crate::validation::{
        validate_dicom_path_with_runner, CommandOutcome, ValidationCommandRunner,
    };
    use crate::ValidationOptions;

    struct NoExternalTools;

    impl ValidationCommandRunner for NoExternalTools {
        fn find_command(&self, _name: &str) -> Option<PathBuf> {
            None
        }

        fn run(
            &self,
            _program: &Path,
            _args: &[OsString],
            _timeout: Duration,
            _max_output_bytes: usize,
        ) -> Result<CommandOutcome, std::io::Error> {
            unreachable!("the no-tool runner cannot execute a command")
        }
    }

    #[test]
    fn self_test_writes_output_and_validation_report_when_output_is_kept() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let workspace = tmp.path().join("evidence");

        let report = run_dicom_self_test_with_validation(
            SelfTestOptions {
                output_dir: Some(workspace.clone()),
                keep_output: true,
                validation: ValidationOptions {
                    max_pixel_frames: 0,
                    ..ValidationOptions::default()
                },
                ..SelfTestOptions::default()
            },
            |path, validation| validate_dicom_path_with_runner(path, validation, &NoExternalTools),
        )
        .expect("self-test report");

        assert!(report.kept_output);
        assert_eq!(report.workspace, workspace);
        assert!(report.output_dir.is_dir());
        assert!(!report.export_report.instances.is_empty());
        assert!(report.export_report.instances.iter().all(|instance| {
            instance.transfer_syntax_uid == crate::TransferSyntax::Jpeg2000Lossless.uid()
        }));
        assert_eq!(report.validation_report.failed_checks(), 0);
    }
}
