//! Prompt section that injects a profile's free-form persona blurb, plus the
//! optional cross-profile workspace notice (1b) that tells the model where its
//! dedicated workspace is and that other profiles' directories are off-limits.

use std::path::Path;

use super::types::AgentProfile;
use crate::openhuman::context::prompt::{PromptContext, PromptSection};
use anyhow::Result;

/// Upper bound on the injected persona SOUL.md, mirroring the identity-file cap
/// (`BOOTSTRAP_MAX_CHARS`) so a large per-profile SOUL.md can't bloat the frozen
/// prompt prefix.
const PROFILE_SOUL_MAX_CHARS: usize = 20_000;

/// One-sentence system-prompt notice, mirroring hermes's cross-profile
/// disclosure: names the profile's dedicated workspace and states that other
/// profiles' directories are off-limits. Rendered only when a dedicated
/// workspace is active (the enforcement backstop is the guard in
/// [`crate::openhuman::profiles::guard`]).
pub fn cross_profile_workspace_notice(profile_id: &str, workspace_path: &Path) -> String {
    format!(
        "Your dedicated workspace for profile `{profile_id}` is `{}`. Work only there; the \
         directories of other profiles are off-limits.",
        workspace_path.display()
    )
}

/// Renders a profile's `system_prompt_suffix` (or any free-form persona body)
/// as a `## Agent profile` block in the system prompt, optionally followed by
/// the cross-profile workspace notice.
pub struct AgentProfilePromptSection {
    body: String,
    workspace_notice: Option<String>,
    /// When set, the active profile whose `SOUL.md` identity is hot-read on each
    /// prompt build and rendered ahead of the persona body. Carried (rather than
    /// a pre-resolved string) so the file is re-read at `build` time, matching
    /// the identity section's live-file semantics — a Settings/file edit takes
    /// effect on the next prompt build.
    soul_profile: Option<AgentProfile>,
}

impl AgentProfilePromptSection {
    pub fn new(body: String) -> Self {
        Self {
            body,
            workspace_notice: None,
            soul_profile: None,
        }
    }

    /// Attach the cross-profile workspace notice (1b). Rendered under the
    /// persona body — or on its own when the body is empty — so a
    /// dedicated-workspace profile always discloses its boundary even without a
    /// custom persona suffix.
    #[must_use]
    pub fn with_workspace_notice(mut self, notice: String) -> Self {
        let notice = notice.trim().to_string();
        self.workspace_notice = (!notice.is_empty()).then_some(notice);
        self
    }

    /// Bind the active profile so its resolved `SOUL.md` identity is injected
    /// into the live session prompt (ordinary web-chat / cron turns, not just
    /// delegation). The soul is hot-read via
    /// [`resolve_personality_soul`](crate::openhuman::profiles::resolve_personality_soul)
    /// on each `build`, following the same resolution order as
    /// `PersonalityContext` (`personalities/<id>/SOUL.md` → `soul_md_path` →
    /// inline → `None`). Callers pass this only for non-default profiles; the
    /// default/master profile keeps the workspace root `SOUL.md`.
    #[must_use]
    pub fn with_profile_soul(mut self, profile: AgentProfile) -> Self {
        self.soul_profile = Some(profile);
        self
    }

    /// Hot-read the bound profile's SOUL.md, trimmed and capped. `None` when no
    /// profile is bound or nothing resolves (caller's root fallback stays intact).
    fn resolve_soul(&self, workspace_dir: &Path) -> Option<String> {
        self.soul_profile
            .as_ref()
            .and_then(|profile| super::resolve_personality_soul(workspace_dir, profile))
            .map(|soul| soul.trim().to_string())
            .filter(|soul| !soul.is_empty())
            .map(|soul| {
                soul.chars()
                    .take(PROFILE_SOUL_MAX_CHARS)
                    .collect::<String>()
            })
    }
}

impl PromptSection for AgentProfilePromptSection {
    fn name(&self) -> &str {
        "agent_profile"
    }

    fn build(&self, ctx: &PromptContext<'_>) -> Result<String> {
        // Hot-read the profile identity each build; `None` (no bound profile or
        // nothing resolves) preserves the prior body/notice-only output exactly.
        let soul = self.resolve_soul(ctx.workspace_dir);
        let body = self.body.trim();
        let notice = self.workspace_notice.as_deref().unwrap_or_default();
        let mut parts: Vec<&str> = Vec::new();
        if let Some(ref soul) = soul {
            parts.push(soul.as_str());
        }
        if !body.is_empty() {
            parts.push(body);
        }
        if !notice.is_empty() {
            parts.push(notice);
        }
        if parts.is_empty() {
            return Ok(String::new());
        }
        Ok(format!("## Agent profile\n\n{}", parts.join("\n\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::context::prompt::{LearnedContextData, PromptContext, ToolCallFormat};
    use std::collections::HashSet;

    #[test]
    fn prompt_section_renders_expected_text() {
        let section = AgentProfilePromptSection::new("  Be concise.  ".into());
        assert_eq!(section.name(), "agent_profile");
        let visible_tool_names = HashSet::new();
        let ctx = PromptContext {
            workspace_dir: std::path::Path::new("/tmp"),
            model_name: "test-model",
            agent_id: "orchestrator",
            tools: &[],
            workflows: &[],
            dispatcher_instructions: "",
            learned: LearnedContextData::default(),
            visible_tool_names: &visible_tool_names,
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
            agents_md_global: None,
            agents_md_local: None,
        };
        let rendered = section.build(&ctx).expect("render profile section");
        assert!(rendered.starts_with("## Agent profile"));
        assert!(rendered.contains("Be concise."));

        let empty = AgentProfilePromptSection::new("   ".into());
        assert_eq!(empty.build(&ctx).expect("empty profile section"), "");
    }

    fn empty_ctx<'a>(visible: &'a HashSet<String>) -> PromptContext<'a> {
        PromptContext {
            workspace_dir: std::path::Path::new("/tmp"),
            model_name: "test-model",
            agent_id: "orchestrator",
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
            agents_md_global: None,
            agents_md_local: None,
        }
    }

    #[test]
    fn cross_profile_workspace_notice_names_path_and_boundary() {
        let notice =
            cross_profile_workspace_notice("alice", std::path::Path::new("/act/profiles/alice"));
        assert!(notice.contains("alice"));
        assert!(notice.contains("/act/profiles/alice"));
        assert!(notice.contains("off-limits"));
    }

    #[test]
    fn workspace_notice_renders_with_and_without_body() {
        let visible = HashSet::new();
        let ctx = empty_ctx(&visible);

        // Notice-only (no persona body) still emits the block + the notice.
        let notice_only = AgentProfilePromptSection::new(String::new())
            .with_workspace_notice("boundary sentence.".into());
        let rendered = notice_only.build(&ctx).expect("render notice-only");
        assert!(rendered.starts_with("## Agent profile"));
        assert!(rendered.contains("boundary sentence."));

        // Body + notice: both present, body first.
        let both = AgentProfilePromptSection::new("Be terse.".into())
            .with_workspace_notice("boundary sentence.".into());
        let rendered = both.build(&ctx).expect("render both");
        let body_at = rendered.find("Be terse.").expect("body present");
        let notice_at = rendered.find("boundary sentence.").expect("notice present");
        assert!(body_at < notice_at, "persona body must precede the notice");

        // A blank notice is dropped, leaving only the body.
        let blank_notice =
            AgentProfilePromptSection::new("Be terse.".into()).with_workspace_notice("  ".into());
        let rendered = blank_notice.build(&ctx).expect("render blank notice");
        assert!(rendered.contains("Be terse."));
        assert!(!rendered.contains("off-limits"));
    }
}
