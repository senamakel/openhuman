//! Functional `render_*` wrappers over the section structs in
//! [`crate::agent::prompts::sections`], plus the per-turn datetime stamp and the
//! ambient-environment composer that `agents/<id>/prompt.rs` builders call.

use super::super::sections::*;
use super::super::types::*;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::sync::OnceLock;

// ─────────────────────────────────────────────────────────────────────────────
// Section render helpers (functional wrappers over section structs)
// ─────────────────────────────────────────────────────────────────────────────

/// Render the `## Project Context` identity block
/// (`SOUL.md` / `IDENTITY.md` / `ROLE.md` for the user-facing agent).
pub fn render_identity(ctx: &PromptContext<'_>) -> Result<String> {
    IdentitySection.build(ctx)
}

/// Render the `PROFILE.md` + `MEMORY.md` user-file injection.
/// Empty when neither `ctx.include_profile` nor `ctx.include_memory_md`
/// is set.
pub fn render_user_files(ctx: &PromptContext<'_>) -> Result<String> {
    UserFilesSection.build(ctx)
}

/// Render the tree-summariser user-memory block.
pub fn render_user_memory(ctx: &PromptContext<'_>) -> Result<String> {
    UserMemorySection.build(ctx)
}

/// Render the `## Project instructions (AGENTS.md)` block from the pre-loaded
/// global + local content on [`PromptContext`]. Empty when neither layer
/// carries content. Dynamic `agents/<id>/prompt.rs` builders call this so they
/// inherit the same AGENTS.md injection as the default section chain.
pub fn render_agents_md(ctx: &PromptContext<'_>) -> Result<String> {
    AgentsInstructionsSection.build(ctx)
}

/// Render the privileged `## User Reflections` block. Empty when the
/// learning subsystem has not captured any reflections yet.
pub fn render_user_reflections(ctx: &PromptContext<'_>) -> Result<String> {
    UserReflectionsSection.build(ctx)
}

/// Render the `## Tools` catalogue in the dispatcher's tool-call format.
pub fn render_tools(ctx: &PromptContext<'_>) -> Result<String> {
    ToolsSection.build(ctx)
}

/// Render the static `## Safety` block.
pub fn render_safety() -> String {
    SafetySection
        .build(&empty_prompt_context_for_static_sections())
        .expect("SafetySection::build is infallible")
}

/// Render the canonical grounding / anti-hallucination contract
/// ([`GROUNDING_BODY`]). Dynamic `agents/<id>/prompt.rs` builders call this
/// so they inherit the exact same anti-fabrication floor as the static
/// section chain — single source of truth, no drift.
pub fn render_grounding() -> &'static str {
    GROUNDING_BODY
}

// `render_skills` and `render_connected_integrations` helpers are
// gone — `## Available Skills` lives in `integrations_agent/prompt.rs`, and
// the connected-integrations / delegation-guide blocks each live in
// their owning agent's `prompt.rs` so no branching-on-agent-id logic
// needs to exist here.

/// Render the `## Workspace` block (working directory + file listing
/// bounds) — part of the dynamic, per-request suffix.
pub fn render_workspace(ctx: &PromptContext<'_>) -> Result<String> {
    WorkspaceSection.build(ctx)
}

/// Render the `## Runtime` block (model name, dispatcher format) —
/// dynamic.
pub fn render_runtime(ctx: &PromptContext<'_>) -> Result<String> {
    RuntimeSection.build(ctx)
}

/// Render the `## Current Date & Time` block: the static time-discipline
/// *rules* (greeting/clock grounding + the gated `resolve_time` rule). The
/// concrete "now" is **not** here — it rides the user message per turn via
/// [`current_datetime_line`] so it stays fresh and keeps this section
/// byte-stable for prefix caching (#3602).
pub fn render_datetime(ctx: &PromptContext<'_>) -> Result<String> {
    DateTimeSection.build(ctx)
}

/// Canonical one-line "now" stamp, injected per turn alongside the user
/// message by both the main session loop (`session::turn`) and the
/// sub-agent runner so every flow reports the current time identically
/// (#3602). Local time + IANA zone + `%Z`/offset + weekday, so the model
/// can localize greetings and date math without a tool call.
///
/// Deliberately lives on the *user message*, never the cached
/// system-prompt prefix: `Local::now()` is volatile, so freezing it into
/// the prefix both busts the KV cache and goes stale across a long-lived
/// session. The static grounding *rule* that tells the model to read this
/// line lives in [`DateTimeSection`] / [`render_datetime`].
pub fn current_datetime_line() -> String {
    // `library-cpu.sh` sets `OPENHUMAN_PROFILE_FORCE_UTC=1` to skip
    // `iana_time_zone`/CoreFoundation timezone resolution, which is itself a
    // measurable cost in a cold CPU profile. Gated on `rss-bench`, so it does
    // not exist in any shipped build.
    #[cfg(feature = "rss-bench")]
    if std::env::var_os("OPENHUMAN_PROFILE_FORCE_UTC").is_some() {
        let now = chrono::Utc::now();
        return format!(
            "Current Date & Time: {} UTC (UTC, UTC+00:00), {}",
            now.format("%Y-%m-%d %H:%M:%S"),
            now.format("%A"),
        );
    }

    // When the host resolves an IANA zone, stamp local time + that zone. When
    // it can't (CI, stripped containers), fall back to true UTC — formatting
    // `Utc::now()` so the time, offset, and zone label all agree rather than
    // pairing a "UTC" label with a local clock/offset.
    match iana_time_zone::get_timezone() {
        Ok(iana) => {
            let now = chrono::Local::now();
            format!(
                "Current Date & Time: {} {} ({}, UTC{}), {}",
                now.format("%Y-%m-%d %H:%M:%S"),
                iana,
                now.format("%Z"),
                now.format("%:z"),
                now.format("%A"),
            )
        }
        Err(_) => {
            let now = chrono::Utc::now();
            format!(
                "Current Date & Time: {} UTC (UTC, UTC+00:00), {}",
                now.format("%Y-%m-%d %H:%M:%S"),
                now.format("%A"),
            )
        }
    }
}

/// Render the `## User` identity block. Empty when
/// [`PromptContext::user_identity`] is unset or has no populated
/// fields. See issue #926.
pub fn render_user_identity(ctx: &PromptContext<'_>) -> Result<String> {
    UserIdentitySection.build(ctx)
}

/// Compose the full ambient-environment block — runtime + user
/// identity + current date/time, in that order.
///
/// Per-agent `prompt.rs` builders call this once near the end of their
/// assembly so every agent reports the same machine-readable view of
/// "where am I, who is the user, what time is it" (issue #926).
/// Datetime is appended last so the time-volatile section sits at the
/// tail of the prompt and the rest of the prefix stays cache-stable
/// across turns within the same minute, matching the convention used
/// by [`crate::agent::prompts::builder::SystemPromptBuilder::with_defaults`].
pub fn render_ambient_environment(ctx: &PromptContext<'_>) -> Result<String> {
    let mut out = String::with_capacity(512);
    let runtime = render_runtime(ctx)?;
    if !runtime.trim().is_empty() {
        out.push_str(runtime.trim_end());
        out.push_str("\n\n");
    }
    let user = render_user_identity(ctx)?;
    if !user.trim().is_empty() {
        out.push_str(user.trim_end());
        out.push_str("\n\n");
    }
    let datetime = render_datetime(ctx)?;
    if !datetime.trim().is_empty() {
        out.push_str(datetime.trim_end());
        out.push('\n');
    }
    Ok(out)
}

/// Format a memory item's `updated_at` as an absolute UTC date label
/// for prompt injection, e.g. `2026-05-25`.
///
/// Absolute (not relative "N days ago") on purpose: memory sections sit
/// near the front of the KV-cache-stable system prompt, so a label that
/// changes daily would bust the cached prefix for everything after it.
/// An absolute date only changes when the underlying memory does. The
/// model judges staleness by comparing this against the injected current
/// date. Shared by [`UserMemorySection`] and the working-memory block in
/// `agent_memory::memory_loader`. (#2944)
pub fn memory_date_label(updated_at: DateTime<Utc>) -> String {
    updated_at.format("%Y-%m-%d").to_string()
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Build a throwaway `PromptContext` for sections whose `build` only
/// uses static/immutable inputs (currently just `SafetySection`). Keeps
/// the `render_safety()` free function from forcing callers to
/// manufacture a full context when they only need the static text.
fn empty_prompt_context_for_static_sections() -> PromptContext<'static> {
    static EMPTY_TOOLS: &[PromptTool<'static>] = &[];
    static EMPTY_WORKFLOWS: &[crate::skills::Workflow] = &[];
    static EMPTY_INTEGRATIONS: &[ConnectedIntegration] = &[];
    // SAFETY: the &HashSet reference must outlive the returned context;
    // a leaked OnceLock-style allocation gives us a permanent 'static
    // anchor without adding runtime cost on the hot path.
    static EMPTY_VISIBLE: OnceLock<std::collections::HashSet<String>> = OnceLock::new();
    let visible = EMPTY_VISIBLE.get_or_init(std::collections::HashSet::new);
    PromptContext {
        workspace_dir: std::path::Path::new(""),
        model_name: "",
        agent_id: "",
        tools: EMPTY_TOOLS,
        workflows: EMPTY_WORKFLOWS,
        dispatcher_instructions: "",
        learned: LearnedContextData::default(),
        visible_tool_names: visible,
        tool_call_format: ToolCallFormat::PFormat,
        connected_integrations: EMPTY_INTEGRATIONS,
        connected_identities_md: String::new(),
        include_profile: false,
        include_memory_md: false,
        curated_snapshot: None,
        user_identity: None,
        personality_soul_md: None,
        personality_memory_md: None,
        personality_roster: vec![],
        agents_md_global: None,
        agents_md_local: None,
    }
}
