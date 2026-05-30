use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    image_generation_spec, image_view_spec, ImageGenerationOutputFormat,
    IMAGE_GENERATION_TOOL_NAME, IMAGE_VIEW_TOOL_NAME,
};

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
