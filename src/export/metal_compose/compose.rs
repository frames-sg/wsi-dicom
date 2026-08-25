use super::addressing::{ComposeAddressPlan, ComposeAddressWidth};
use super::types::{MetalComposeStripsParams, MetalComposeTileRequest, PackedMetalStrips};
use super::{metal_profile_stages_enabled, MetalStripComposer};
use crate::error::Error;
use j2k_core::DeviceSubmission as _;
use objc2_foundation::NSString;
use objc2_metal::{MTLCommandBuffer, MTLCommandEncoder, MTLComputeCommandEncoder};

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) struct MetalComposeTileDispatch {
    pub(super) request: MetalComposeTileRequest,
    pub(super) params: MetalComposeStripsParams,
    pub(super) dst_buffer: crate::metal_interop::MetalBuffer,
    pub(super) output_layout: j2k_metal_support::MetalImageLayout,
}

impl MetalStripComposer {
    pub(in crate::export) fn compose_tiles(
        &self,
        packed: &PackedMetalStrips,
        requests: &[MetalComposeTileRequest],
    ) -> Result<Vec<wsi_rs::output::metal::MetalDeviceTile>, Error> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }
        let first_col = u32::try_from(packed.first_col).map_err(|_| Error::Unsupported {
            reason: "Metal WholeLevel first source tile column exceeds u32".into(),
        })?;
        let first_row = u32::try_from(packed.first_row).map_err(|_| Error::Unsupported {
            reason: "Metal WholeLevel first source tile row exceeds u32".into(),
        })?;
        let bytes_per_pixel = packed.format.bytes_per_pixel();
        let bytes_per_pixel_u32 =
            u32::try_from(bytes_per_pixel).map_err(|_| Error::Unsupported {
                reason: "Metal composed tile bytes-per-pixel exceeds u32".into(),
            })?;
        let mut address_plans = Vec::new();
        address_plans
            .try_reserve_exact(requests.len())
            .map_err(|_| Error::Unsupported {
                reason: "Metal compose address-plan batch exceeds available memory".into(),
            })?;
        for request in requests {
            address_plans.push(ComposeAddressPlan::new(
                *request,
                packed,
                first_col,
                first_row,
                bytes_per_pixel_u32,
            )?);
        }
        let address_width = if address_plans
            .iter()
            .all(|plan| plan.address_width == ComposeAddressWidth::U32)
        {
            ComposeAddressWidth::U32
        } else {
            ComposeAddressWidth::U64
        };
        let mut dispatches = Vec::new();
        dispatches
            .try_reserve_exact(address_plans.len())
            .map_err(|_| Error::Unsupported {
                reason: "Metal compose dispatch batch exceeds available memory".into(),
            })?;
        for plan in address_plans {
            let dst_buffer = j2k_metal_support::checked_shared_buffer_for_len::<u8>(
                &self.device,
                plan.dst_bytes,
            )
            .map_err(|source| {
                crate::metal_interop::support_error("Metal composed tile allocation", source)
            })?;
            let output_layout = j2k_metal_support::MetalImageLayout::new(
                0,
                (plan.request.output_width, plan.request.output_height),
                plan.dst_stride,
                packed.format,
            )
            .map_err(|source| {
                crate::metal_interop::support_error("Metal composed tile layout", source)
            })?;
            dispatches.push(MetalComposeTileDispatch {
                request: plan.request,
                params: plan.params,
                dst_buffer,
                output_layout,
            });
        }

        packed
            .image
            .validate_device(&self.device)
            .map_err(|source| {
                crate::metal_interop::support_error("Metal compose packed input device", source)
            })?;
        let command_buffer =
            j2k_metal_support::checked_command_buffer(&self.queue).map_err(|source| {
                crate::metal_interop::support_error("Metal compose command", source)
            })?;
        if metal_profile_stages_enabled() {
            command_buffer.setLabel(Some(&NSString::from_str("wsi-dicom compose tiles")));
        }
        let encoder = j2k_metal_support::checked_compute_command_encoder(&command_buffer).map_err(
            |source| crate::metal_interop::support_error("Metal compose command encoder", source),
        )?;
        if metal_profile_stages_enabled() {
            encoder.setLabel(Some(&NSString::from_str("WSI compose tiles")));
        }
        let pipeline = match address_width {
            ComposeAddressWidth::U32 => self.pipeline_u32.as_ref(),
            ComposeAddressWidth::U64 => self.pipeline_u64()?,
        };
        encoder.setComputePipelineState(pipeline);
        crate::metal_interop::bind_resident_compute_input(&encoder, 0, &packed.image);
        for dispatch in &dispatches {
            crate::metal_interop::bind_compute_buffer(&encoder, 1, &dispatch.dst_buffer);
            crate::metal_interop::bind_compose_params(&encoder, 2, &dispatch.params);
            j2k_metal_support::dispatch_2d_pipeline(
                &encoder,
                pipeline,
                (
                    dispatch.request.output_width,
                    dispatch.request.output_height,
                ),
            );
        }
        encoder.endEncoding();
        let outputs = dispatches
            .into_iter()
            .map(|dispatch| (dispatch.dst_buffer, dispatch.output_layout))
            .collect();
        let submitted = crate::metal_interop::submit_images(
            &self.device,
            command_buffer,
            outputs,
            vec![packed.image.clone()],
        )?;
        submitted
            .wait()
            .map_err(|source| {
                crate::metal_interop::support_error("Metal compose completion", source)
            })?
            .into_iter()
            .map(|image| {
                wsi_rs::output::metal::MetalDeviceTile::from_resident(image).map_err(|source| {
                    Error::Encode {
                        message: format!(
                            "Metal composed resident tile conversion failed: {source}"
                        ),
                    }
                })
            })
            .collect()
    }
}
