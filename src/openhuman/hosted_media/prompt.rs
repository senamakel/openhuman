//! Prompt guidance for hosted media tools.

use super::{HostedMediaToolConfig, IMAGE_GENERATION_TOOL_NAME, IMAGE_VIEW_TOOL_NAME};

/// Rendering options for hosted-media prompt guidance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedMediaPromptOptions {
    /// Include final-answer artifact-reference guidance for generated images.
    pub include_final_answer_rules: bool,
    /// Include local-file privacy boundaries for viewed images.
    pub include_local_file_boundaries: bool,
}

impl Default for HostedMediaPromptOptions {
    fn default() -> Self {
        Self {
            include_final_answer_rules: true,
            include_local_file_boundaries: true,
        }
    }
}

/// Render concise model guidance for enabled hosted media tools.
pub fn render_hosted_media_prompt_guidance(
    config: &HostedMediaToolConfig,
    options: &HostedMediaPromptOptions,
) -> String {
    let image_generation_available =
        config.image_generation_enabled && config.generated_image_writes_allowed;
    let image_view_available = config.image_view_enabled && config.local_image_reads_allowed;

    if !image_generation_available && !image_view_available {
        return String::new();
    }

    let mut out = String::from("## Hosted Media Tools\n\n");

    if image_view_available {
        out.push_str(&format!(
            "- Use `{IMAGE_VIEW_TOOL_NAME}` when image pixels are needed for UI review, OCR, chart inspection, visual comparison, or understanding a local screenshot.\n"
        ));
        if options.include_local_file_boundaries {
            out.push_str(
                "- Only view local images that are in the approved workspace, were created during this session, or were explicitly referenced by the user or trusted tool output.\n",
            );
        }
    }

    if image_generation_available {
        out.push_str(&format!(
            "- Use `{IMAGE_GENERATION_TOOL_NAME}` for requested raster image creation or image edits; provide a specific prompt and an output path when the destination matters.\n"
        ));
        if options.include_final_answer_rules {
            out.push_str(
                "- After generating images, mention the saved artifact path in the final answer so the user can find it.\n",
            );
        }
    }

    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::hosted_media::ImageGenerationOutputFormat;

    #[test]
    fn prompt_guidance_is_empty_when_no_media_tools_are_enabled() {
        let rendered = render_hosted_media_prompt_guidance(
            &HostedMediaToolConfig::default(),
            &HostedMediaPromptOptions::default(),
        );

        assert!(rendered.is_empty());
    }

    #[test]
    fn prompt_guidance_renders_image_generation_and_view_rules() {
        let config = HostedMediaToolConfig {
            image_generation_enabled: true,
            image_view_enabled: true,
            image_generation_output_format: ImageGenerationOutputFormat::Png,
            local_image_reads_allowed: true,
            generated_image_writes_allowed: true,
        };

        let rendered =
            render_hosted_media_prompt_guidance(&config, &HostedMediaPromptOptions::default());

        assert!(rendered.contains("## Hosted Media Tools"));
        assert!(rendered.contains("`view_image`"));
        assert!(rendered.contains("`image_generation`"));
        assert!(rendered.contains("saved artifact path"));
    }

    #[test]
    fn prompt_guidance_can_omit_optional_rule_text() {
        let config = HostedMediaToolConfig {
            image_generation_enabled: true,
            image_view_enabled: true,
            image_generation_output_format: ImageGenerationOutputFormat::Png,
            local_image_reads_allowed: true,
            generated_image_writes_allowed: true,
        };
        let options = HostedMediaPromptOptions {
            include_final_answer_rules: false,
            include_local_file_boundaries: false,
        };

        let rendered = render_hosted_media_prompt_guidance(&config, &options);

        assert!(rendered.contains("`view_image`"));
        assert!(rendered.contains("`image_generation`"));
        assert!(!rendered.contains("approved workspace"));
        assert!(!rendered.contains("saved artifact path"));
    }

    #[test]
    fn prompt_guidance_respects_policy_gates() {
        let config = HostedMediaToolConfig {
            image_generation_enabled: true,
            image_view_enabled: true,
            image_generation_output_format: ImageGenerationOutputFormat::Png,
            local_image_reads_allowed: false,
            generated_image_writes_allowed: false,
        };

        let rendered =
            render_hosted_media_prompt_guidance(&config, &HostedMediaPromptOptions::default());

        assert!(rendered.is_empty());
    }

    #[test]
    fn prompt_guidance_renders_single_available_tool() {
        let generation_only = HostedMediaToolConfig {
            image_generation_enabled: true,
            image_view_enabled: false,
            image_generation_output_format: ImageGenerationOutputFormat::Png,
            local_image_reads_allowed: true,
            generated_image_writes_allowed: true,
        };
        let view_only = HostedMediaToolConfig {
            image_generation_enabled: false,
            image_view_enabled: true,
            image_generation_output_format: ImageGenerationOutputFormat::Png,
            local_image_reads_allowed: true,
            generated_image_writes_allowed: true,
        };

        let generation_rendered = render_hosted_media_prompt_guidance(
            &generation_only,
            &HostedMediaPromptOptions::default(),
        );
        let view_rendered =
            render_hosted_media_prompt_guidance(&view_only, &HostedMediaPromptOptions::default());

        assert!(generation_rendered.contains("`image_generation`"));
        assert!(!generation_rendered.contains("`view_image`"));
        assert!(view_rendered.contains("`view_image`"));
        assert!(!view_rendered.contains("`image_generation`"));
    }
}
