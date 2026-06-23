//! System prompt builder for the `context_scout` built-in agent.
//!
//! The scout is a read-only pre-flight context collector. Its prompt is the
//! role markdown ([`prompt.md`]) followed by the user-file injection
//! (PROFILE.md = goals, MEMORY.md = curated long-term memory — both kept in
//! because grounding the orchestrator in *who the user is and what they want*
//! is the scout's whole job), the scout's own read-only tool catalogue, and
//! the workspace block. The orchestrator's tool catalogue (the tools the scout
//! recommends *back* to the parent) is injected at spawn time by
//! `AgentPrepareContextTool`, not here — this builder only describes the
//! scout's own gathering surface.

use crate::openhuman::context::prompt::{
    render_tools, render_user_files, render_workspace, PromptContext,
};
use anyhow::Result;

const ARCHETYPE: &str = include_str!("prompt.md");

pub fn build(ctx: &PromptContext<'_>) -> Result<String> {
    let mut out = String::with_capacity(4096);
    out.push_str(ARCHETYPE.trim_end());
    out.push_str("\n\n");

    // PROFILE.md (goals) + MEMORY.md (long-term memory). Gated on
    // `ctx.include_profile` / `ctx.include_memory_md`, which the runner sets
    // from the definition's `omit_profile = false` / `omit_memory_md = false`.
    let user_files = render_user_files(ctx)?;
    if !user_files.trim().is_empty() {
        out.push_str(user_files.trim_end());
        out.push_str("\n\n");
    }

    let tools = render_tools(ctx)?;
    if !tools.trim().is_empty() {
        out.push_str(tools.trim_end());
        out.push_str("\n\n");
    }

    let workspace = render_workspace(ctx)?;
    if !workspace.trim().is_empty() {
        out.push_str(workspace.trim_end());
        out.push('\n');
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::context::prompt::{LearnedContextData, ToolCallFormat};
    use std::collections::HashSet;

    fn test_ctx() -> PromptContext<'static> {
        // Leak a HashSet so the &reference satisfies the 'static-ish lifetime
        // the helper needs in this throwaway test context.
        let visible: &'static HashSet<String> = Box::leak(Box::new(HashSet::new()));
        PromptContext {
            workspace_dir: std::path::Path::new("."),
            model_name: "test",
            agent_id: "context_scout",
            tools: &[],
            workflows: &[],
            dispatcher_instructions: "",
            learned: LearnedContextData::default(),
            visible_tool_names: visible,
            tool_call_format: ToolCallFormat::PFormat,
            connected_integrations: &[],
            connected_identities_md: String::new(),
            include_profile: false,
            include_memory_md: false,
            curated_snapshot: None,
            user_identity: None,
            personality_soul_md: None,
            personality_memory_md: None,
            personality_roster: vec![],
        }
    }

    #[test]
    fn build_returns_nonempty_body() {
        let body = build(&test_ctx()).unwrap();
        assert!(!body.is_empty());
    }

    #[test]
    fn body_describes_the_context_bundle_contract() {
        let body = build(&test_ctx()).unwrap();
        assert!(body.contains("[context_bundle]"));
        assert!(body.contains("has_enough_context"));
        assert!(body.contains("recommended_tool_calls"));
    }
}
