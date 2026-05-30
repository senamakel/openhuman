//! Hosted media-tool contracts for model-facing agents.
//!
//! This module is intentionally a high-level contract layer. OpenHuman already
//! has lower-level image helpers (`image_info`, browser screenshots, and
//! multimodal `[IMAGE:...]` normalization). The hosted-media layer defines the
//! stable tool names, schema, gating, and prompt guidance that agents should see
//! when a runtime can provide Codex-like media tools.
//!
//! The two first-class contracts are:
//!
//! - [`image_generation`] — create or edit raster images and return stored file
//!   references.
//! - [`image_view`] — attach a local image file as model-visible image content
//!   so the agent can inspect it.
//!
//! Keeping this contract separate from execution lets provider runtimes adopt
//! the surface incrementally without duplicating business logic in the tools
//! registry.

pub mod image_generation;
pub mod image_view;
pub mod prompt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use image_generation::{
    image_generation_spec, ImageGenerationOutputFormat, IMAGE_GENERATION_TOOL_NAME,
};
pub use image_view::{image_view_spec, ImageDetail, IMAGE_VIEW_TOOL_NAME};
pub use prompt::{render_hosted_media_prompt_guidance, HostedMediaPromptOptions};

/// Model-facing hosted tool names used for filtering and policy decisions.
pub const HOSTED_MEDIA_TOOL_NAMES: [&str; 2] = [IMAGE_GENERATION_TOOL_NAME, IMAGE_VIEW_TOOL_NAME];

/// A provider/runtime independent hosted tool descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedMediaToolSpec {
    /// Stable model-facing tool name.
    pub name: String,
    /// Concise tool description injected into prompt/tool catalogues.
    pub description: String,
    /// JSON Schema object for tool arguments.
    pub parameters: Value,
    /// Execution permission required by OpenHuman policy gates.
    pub permission: HostedMediaPermission,
    /// Whether the tool payload is expected to become model-visible image
    /// content rather than plain text.
    pub model_visible_image_output: bool,
    /// Whether execution writes files into an output directory.
    pub writes_files: bool,
}

/// Permission class for hosted media tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostedMediaPermission {
    /// Metadata or read-only local inspection.
    ReadOnly,
    /// Creates or edits generated media files.
    Write,
}

/// Session/runtime switches that decide which hosted media tools are exposed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedMediaToolConfig {
    /// Runtime supports hosted image generation.
    pub image_generation_enabled: bool,
    /// Runtime supports local image attachment/viewing.
    pub image_view_enabled: bool,
    /// Desired output format for generated images.
    pub image_generation_output_format: ImageGenerationOutputFormat,
    /// Whether the current filesystem policy allows workspace image reads.
    pub local_image_reads_allowed: bool,
    /// Whether generated files may be written under the configured output root.
    pub generated_image_writes_allowed: bool,
}

impl Default for HostedMediaToolConfig {
    fn default() -> Self {
        Self {
            image_generation_enabled: false,
            image_view_enabled: false,
            image_generation_output_format: ImageGenerationOutputFormat::Png,
            local_image_reads_allowed: true,
            generated_image_writes_allowed: true,
        }
    }
}

/// Build the hosted media specs visible to an agent for this runtime.
pub fn hosted_media_specs(config: &HostedMediaToolConfig) -> Vec<HostedMediaToolSpec> {
    let mut specs = Vec::new();

    if config.image_generation_enabled && config.generated_image_writes_allowed {
        specs.push(image_generation_spec(config.image_generation_output_format));
    }

    if config.image_view_enabled && config.local_image_reads_allowed {
        specs.push(image_view_spec());
    }

    specs
}

/// Return true when a hosted media tool should be hidden from a session.
pub fn is_hosted_media_tool_gated(tool_name: &str, config: &HostedMediaToolConfig) -> bool {
    !hosted_media_specs(config)
        .iter()
        .any(|spec| spec.name == tool_name)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
