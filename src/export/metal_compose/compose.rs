use super::addressing::{ComposeAddressPlan, ComposeAddressWidth};
use super::*;
use j2k_core::DeviceSubmission as _;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) struct MetalComposeTileDispatch {
    pub(super) request: MetalComposeTileRequest,
    pub(super) params: MetalComposeStripsParams,
    pub(super) dst_buffer: metal::Buffer,
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
        let address_plans = requests
            .iter()
            .map(|request| {
                ComposeAddressPlan::new(*request, packed, first_col, first_row, bytes_per_pixel_u32)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let address_width = if address_plans
            .iter()
            .all(|plan| plan.address_width == ComposeAddressWidth::U32)
        {
            ComposeAddressWidth::U32
        } else {
            ComposeAddressWidth::U64
        };
        let mut dispatches = Vec::with_capacity(address_plans.len());
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
            command_buffer.set_label("wsi-dicom compose tiles");
        }
        let encoder = command_buffer.new_compute_command_encoder();
        if metal_profile_stages_enabled() {
            encoder.set_label("WSI compose tiles");
        }
        let pipeline = match address_width {
            ComposeAddressWidth::U32 => self.pipeline_u32.as_ref(),
            ComposeAddressWidth::U64 => self.pipeline_u64()?,
        };
        encoder.set_compute_pipeline_state(pipeline);
        crate::metal_interop::bind_resident_compute_input(encoder, 0, &packed.image);
        let width = pipeline.thread_execution_width().max(1);
        let max_threads = pipeline.max_total_threads_per_threadgroup().max(width);
        let height = (max_threads / width).max(1);
        for dispatch in &dispatches {
            encoder.set_buffer(1, Some(&dispatch.dst_buffer), 0);
            encoder.set_bytes(
                2,
                core::mem::size_of::<MetalComposeStripsParams>() as u64,
                (&raw const dispatch.params).cast(),
            );
            encoder.dispatch_threads(
                metal::MTLSize {
                    width: u64::from(dispatch.request.output_width),
                    height: u64::from(dispatch.request.output_height),
                    depth: 1,
                },
                metal::MTLSize {
                    width,
                    height,
                    depth: 1,
                },
            );
        }
        encoder.end_encoding();
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
