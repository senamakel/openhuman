//! System prompt builder for the `workflow_builder` built-in agent (Phase 5a).
//!
//! Composes the module-owned archetype text with OpenHuman's scoped runtime
//! sections. The caller fetches the archetype through TinyFlows Copilot before
//! invoking this synchronous builder.

use crate::agent::prompts::PromptContext;
use anyhow::{anyhow, Result};

pub fn build(ctx: &PromptContext<'_>) -> Result<String> {
    let archetype = ctx.flow_archetype_text.ok_or_else(|| {
        anyhow!("workflow_builder requires a scoped TinyFlows GetPrompt response")
    })?;
    super::super::flow_prompts::build_with_archetype(ctx, archetype)
}
