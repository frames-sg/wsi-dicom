use super::*;

impl DicomJ2kEncoder {
    pub(crate) fn encode_host_samples_batch(
        &mut self,
        samples: &[J2kLosslessSamples<'_>],
        width: u32,
        height: u32,
    ) -> Result<EncodedDicomJ2kMetalTileBatch, Error> {
        let session = self.ensure_metal_session()?.clone();
        let mut tiles = Vec::with_capacity(samples.len());
        for sample in samples {
            let format = match (sample.components, sample.bit_depth, sample.signed) {
                (1, 8, false) => j2k_core::PixelFormat::Gray8,
                (3, 8, false) => j2k_core::PixelFormat::Rgb8,
                (1, 16, false) => j2k_core::PixelFormat::Gray16,
                (3, 16, false) => j2k_core::PixelFormat::Rgb16,
                _ => {
                    return Err(Error::UnsupportedPixelData {
                        reason: "Metal host batch requires unsigned gray/RGB U8/U16 samples".into(),
                    })
                }
            };
            let pitch = (sample.width as usize)
                .checked_mul(format.bytes_per_pixel())
                .ok_or_else(|| Error::Unsupported {
                    reason: "Metal host row byte length overflow".into(),
                })?;
            let layout = j2k_metal_support::MetalImageLayout::new(
                0,
                (sample.width, sample.height),
                pitch,
                format,
            )
            .map_err(|source| {
                crate::metal_interop::support_error("Metal host sample layout", source)
            })?;
            let image = crate::metal_interop::upload_image(session.device(), sample.data, layout)?;
            tiles.push(
                wsi_rs::output::metal::MetalDeviceTile::from_resident(image).map_err(|err| {
                    Error::Encode {
                        message: format!("Metal host tile adoption: {err}"),
                    }
                })?,
            );
        }
        self.submit_metal_tiles_owned(tiles, width, height)?.wait()
    }
}
