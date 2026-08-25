use wsi_dicom::{
    AnnotationCoordinateSpace, CodecValidation, ColorManagement, JpegDirectHtj2kProfile,
    TransferSyntax,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GuiColorManagement {
    RequireSource,
    SourceOrSrgb,
    SourceOrDisplayP3,
}

impl From<GuiColorManagement> for ColorManagement {
    fn from(value: GuiColorManagement) -> Self {
        match value {
            GuiColorManagement::RequireSource => Self::RequireSource,
            GuiColorManagement::SourceOrSrgb => Self::SourceOrSrgb,
            GuiColorManagement::SourceOrDisplayP3 => Self::SourceOrDisplayP3,
        }
    }
}

pub(super) fn transfer_syntax_label(value: TransferSyntax) -> &'static str {
    match value {
        TransferSyntax::JpegBaseline8Bit => "JPEG Baseline 8-bit",
        TransferSyntax::Jpeg2000 => "JPEG 2000",
        TransferSyntax::Jpeg2000Lossless => "JPEG 2000 Lossless",
        TransferSyntax::Htj2k => "HTJ2K",
        TransferSyntax::Htj2kLossless => "HTJ2K Lossless",
        TransferSyntax::Htj2kLosslessRpcl => "HTJ2K Lossless RPCL",
        TransferSyntax::ExplicitVrLittleEndian => "Explicit VR Little Endian",
        _ => "Unknown transfer syntax",
    }
}

pub(super) fn htj2k_profile_label(value: JpegDirectHtj2kProfile) -> &'static str {
    match value {
        JpegDirectHtj2kProfile::Lossless53 => "5/3 lossless",
        JpegDirectHtj2kProfile::Lossy97 => "9/7 balanced",
        JpegDirectHtj2kProfile::Lossy97Near => "9/7 near-lossless",
        JpegDirectHtj2kProfile::Lossy97Balanced => "9/7 balanced",
        JpegDirectHtj2kProfile::Lossy97Aggressive => "9/7 aggressive",
        JpegDirectHtj2kProfile::Lossy97Preview => "9/7 preview",
        JpegDirectHtj2kProfile::Lossy97Thumbnail => "9/7 thumbnail",
        _ => "Unknown profile",
    }
}

pub(super) fn color_management_label(value: GuiColorManagement) -> &'static str {
    match value {
        GuiColorManagement::RequireSource => "Require source",
        GuiColorManagement::SourceOrSrgb => "Source or sRGB",
        GuiColorManagement::SourceOrDisplayP3 => "Source or Display P3",
    }
}

pub(super) fn codec_validation_label(value: CodecValidation) -> &'static str {
    match value {
        CodecValidation::Disabled => "Disabled",
        CodecValidation::RoundTrip => "Round trip",
        _ => "Unknown validation",
    }
}

pub(super) fn annotation_coordinate_space_label(value: AnnotationCoordinateSpace) -> &'static str {
    match value {
        AnnotationCoordinateSpace::Level0Pixels => "Level-0 pixels",
        AnnotationCoordinateSpace::SourcePixels => "DICOM source pixels",
        AnnotationCoordinateSpace::SlideMillimeters => "Slide millimetres",
    }
}
