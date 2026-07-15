use super::*;

mod addressing;
mod compose;
mod pack;
mod types;

use types::MetalComposeStripsParams;
pub(super) use types::{MetalComposeTileRequest, PackedMetalStrips};

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) struct MetalStripComposer {
    pub(super) device: metal::Device,
    pub(super) queue: metal::CommandQueue,
    pub(super) library: metal::Library,
    pub(super) pipeline_u32: metal::ComputePipelineState,
    pub(super) pipeline_u64: OnceLock<Result<metal::ComputePipelineState, String>>,
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn metal_profile_stages_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        matches!(
            std::env::var("J2K_METAL_PROFILE_STAGES"),
            Ok(value) if value == "1"
        )
    })
}

#[cfg(all(feature = "metal", target_os = "macos"))]
impl MetalStripComposer {
    pub(super) fn new(device: metal::Device) -> Result<Self, Error> {
        let options = metal::CompileOptions::new();
        let library = device
            .new_library_with_source(WSI_COMPOSE_STRIPS_METAL, &options)
            .map_err(|message| Error::Encode {
                message: format!("Metal strip compose shader failed to compile: {message}"),
            })?;
        let function = library
            .get_function("wsi_compose_strips_u32", None)
            .map_err(|message| Error::Encode {
                message: format!("Metal u32 strip compose function unavailable: {message}"),
            })?;
        let pipeline_u32 = device
            .new_compute_pipeline_state_with_function(&function)
            .map_err(|message| Error::Encode {
                message: format!("Metal u32 strip compose pipeline unavailable: {message}"),
            })?;
        let queue = device.new_command_queue();
        Ok(Self {
            device,
            queue,
            library,
            pipeline_u32,
            pipeline_u64: OnceLock::new(),
        })
    }

    pub(super) fn pipeline_u64(&self) -> Result<&metal::ComputePipelineStateRef, Error> {
        self.pipeline_u64
            .get_or_init(|| {
                let function = self
                    .library
                    .get_function("wsi_compose_strips", None)
                    .map_err(|message| {
                        format!("Metal u64 strip compose function unavailable: {message}")
                    })?;
                self.device
                    .new_compute_pipeline_state_with_function(&function)
                    .map_err(|message| {
                        format!("Metal u64 strip compose pipeline unavailable: {message}")
                    })
            })
            .as_deref()
            .map_err(|message| Error::Encode {
                message: message.clone(),
            })
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) const WSI_COMPOSE_STRIPS_METAL: &str = include_str!("metal_compose/compose.metal");

#[cfg(all(test, feature = "metal", target_os = "macos"))]
mod tests;
