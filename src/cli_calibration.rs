use std::path::PathBuf;

use clap::{Args, Subcommand};
use wsi_dicom::{
    create_icc_calibration_bundle, ColorManagement, Error, IccCalibrationRegistry,
    IccConflictPolicy, IccProfile, ScannerIdentity,
};

#[derive(Debug, Subcommand)]
pub(crate) enum CalibrationCommand {
    /// Validate an existing ICC profile and print its digest and DICOM suitability.
    Inspect {
        #[arg(long)]
        icc: PathBuf,
    },
    /// Package an existing vendor- or target-generated ICC profile into a portable registry.
    Create {
        #[arg(long)]
        icc: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        manufacturer: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        serial: String,
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Debug, Clone, Args)]
pub(crate) struct ColorManagementArgs {
    #[arg(
        long,
        value_enum,
        default_value_t = BasicColorManagement::SourceOrSrgb,
        conflicts_with_all = ["icc_calibration_registry", "icc_profile"]
    )]
    icc: BasicColorManagement,
    #[arg(long, value_name = "PATH", conflicts_with = "icc_profile")]
    icc_calibration_registry: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        requires = "icc_profile_id",
        conflicts_with = "icc_calibration_registry"
    )]
    icc_profile: Option<PathBuf>,
    #[arg(long, value_name = "ID", requires = "icc_profile")]
    icc_profile_id: Option<String>,
    #[arg(long, value_enum, default_value_t = IccConflictPolicy::Fail)]
    icc_conflict: IccConflictPolicy,
}

impl ColorManagementArgs {
    pub(crate) fn resolve(&self) -> Result<ColorManagement, Error> {
        if let Some(path) = &self.icc_calibration_registry {
            return Ok(ColorManagement::Calibration {
                registry: IccCalibrationRegistry::from_file(path)?,
                conflict: self.icc_conflict,
            });
        }
        if let Some(path) = &self.icc_profile {
            let id = self
                .icc_profile_id
                .as_deref()
                .ok_or_else(|| Error::InvalidOptions {
                    reason: "--icc-profile requires --icc-profile-id".into(),
                })?;
            return Ok(ColorManagement::ExplicitProfile {
                profile: IccProfile::from_file(id, path)?,
                conflict: self.icc_conflict,
            });
        }
        if self.icc_conflict != IccConflictPolicy::Fail {
            return Err(Error::InvalidOptions {
                reason:
                    "--icc-conflict applies only with --icc-calibration-registry or --icc-profile"
                        .into(),
            });
        }
        Ok(self.icc.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum BasicColorManagement {
    RequireSource,
    SourceOrSrgb,
    SourceOrDisplayP3,
}

impl From<BasicColorManagement> for ColorManagement {
    fn from(value: BasicColorManagement) -> Self {
        match value {
            BasicColorManagement::RequireSource => Self::RequireSource,
            BasicColorManagement::SourceOrSrgb => Self::SourceOrSrgb,
            BasicColorManagement::SourceOrDisplayP3 => Self::SourceOrDisplayP3,
        }
    }
}

pub(crate) fn handle(command: CalibrationCommand) -> Result<(), Error> {
    match command {
        CalibrationCommand::Inspect { icc } => {
            let profile = IccProfile::from_file("inspection", &icc)?;
            let pcs = match profile.bytes().get(20..24) {
                Some(b"XYZ ") => "XYZ",
                Some(b"Lab ") => "Lab",
                _ => "unknown",
            };
            println!(
                "valid DICOM input-device ICC profile; bytes={}; sha256={}; input_color_space=RGB; pcs={pcs}",
                profile.bytes().len(),
                profile.sha256()
            );
            Ok(())
        }
        CalibrationCommand::Create {
            icc,
            id,
            manufacturer,
            model,
            serial,
            out,
        } => {
            let scanner = ScannerIdentity::new(manufacturer, model, serial)?;
            create_icc_calibration_bundle(icc, id, &scanner, &out)?;
            println!(
                "created portable calibration bundle at {}; packaged an existing ICC profile without deriving calibration from slide pixels",
                out.display()
            );
            Ok(())
        }
    }
}
