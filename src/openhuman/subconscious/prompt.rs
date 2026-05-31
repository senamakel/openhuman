//! Prompt builder for the subconscious agent.
//!
//! The subconscious agent is a periodic summarizer that reads the situation
//! report (memory-tree signals, recent activity, hotness deltas) and
//! produces structured thoughts (reflections) about the user's state.

use std::path::Path;

const IDENTITY_EXCERPT_CHARS: usize = 2000;

/// Build the system prompt for the subconscious agent tick. The agent
/// observes the user's world via the situation report and produces
/// structured reflections.
pub fn build_agent_prompt(situation_report: &str, identity_context: &str) -> String {
    format!(
        r#"{identity_context}

# Subconscious Agent — Periodic Summarizer

You are the background awareness layer for the user. You run periodically
to observe the user's recent activity, memory signals, and context — then
surface interesting thoughts, patterns, and observations.

## Current State

{situation_report}

## Your Job

Observe the current state and produce **thoughts** — structured
observations about what's happening in the user's world. Each thought
should be grounded in the signals you see in the situation report.

For each thought:
- `kind`: one of `hotness_spike` | `cross_source_pattern` | `daily_digest`
  | `due_item` | `risk` | `opportunity`.
- `body`: short markdown-friendly observation.
- `proposed_action` (optional): one-tap action text. When the user taps
  the action button, OpenHuman opens a *new* conversation thread seeded
  with the body + this action — never auto-executed.
- `source_refs`: opaque ids from the situation report for provenance.

**Self vs. others**: the situation report includes a *Your Identifiers*
section listing the user's connected-account handles, emails, and
user_ids. Use it as the source of truth for who the user is. Never
attribute another person's activity to the user.

**Anti-double-emit**: the situation report's "Recent reflections" section
shows what you already noticed. Re-emit only if the underlying signal
materially intensified.

**Quality bar**: only surface thoughts that would genuinely help the user.
Skip trivial observations. If nothing interesting happened since the last
tick, return an empty array.

Cap: emit at most **5 thoughts per tick**. Excess is dropped.

## Output format (strict JSON, no other text)

{{
  "thoughts": [
    {{
      "kind": "hotness_spike",
      "body": "Phoenix mentions surged 4× in last hour across Slack + email.",
      "proposed_action": "Pull the last 24h of Phoenix mentions into a thread",
      "source_refs": ["entity:phoenix", "summary:abc123"]
    }}
  ]
}}
"#
    )
}

/// Render a slice of recent reflections as a prompt block for the
/// situation report's "Recent reflections" section.
pub fn format_recent_reflections_for_prompt(
    reflections: &[crate::openhuman::subconscious::reflection::Reflection],
) -> String {
    crate::openhuman::subconscious::situation_report::reflections::build_section(reflections)
}

// ── Identity loading ─────────────────────────────────────────────────────────

pub fn load_identity_context(workspace_dir: &Path) -> String {
    let prompts_dir = resolve_prompts_dir(workspace_dir);
    let mut ctx = String::new();

    if let Some(ref dir) = prompts_dir {
        if let Some(soul) = load_file_excerpt(dir, "SOUL.md") {
            ctx.push_str(&soul);
            ctx.push_str("\n\n");
        }
    }

    if let Some(profile) = load_file_excerpt(workspace_dir, "PROFILE.md") {
        ctx.push_str("## User Profile\n\n");
        ctx.push_str(&profile);
        ctx.push_str("\n\n");
    }

    if ctx.is_empty() {
        "You are OpenHuman, an AI assistant for productivity and collaboration.".to_string()
    } else {
        ctx
    }
}

fn resolve_prompts_dir(workspace_dir: &Path) -> Option<std::path::PathBuf> {
    let workspace_ai = workspace_dir.join("ai");
    if workspace_ai.is_dir() {
        return Some(workspace_ai);
    }

    if let Some(dir) = option_env!("CARGO_MANIFEST_DIR").map(std::path::PathBuf::from) {
        let candidate = dir
            .join("src")
            .join("openhuman")
            .join("agent")
            .join("prompts");
        if candidate.is_dir() {
            return Some(candidate);
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        return crate::openhuman::dev_paths::repo_ai_prompts_dir(&cwd);
    }

    None
}

fn load_file_excerpt(dir: &Path, filename: &str) -> Option<String> {
    let content = std::fs::read_to_string(dir.join(filename)).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().count() > IDENTITY_EXCERPT_CHARS {
        let truncated: String = trimmed.chars().take(IDENTITY_EXCERPT_CHARS).collect();
        Some(format!("{truncated}\n[... truncated]"))
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_prompt_includes_report_and_identity() {
        let prompt = build_agent_prompt("## State\nSome data.", "Identity here");
        assert!(prompt.contains("Some data."));
        assert!(prompt.contains("Identity here"));
        assert!(prompt.contains("thoughts"));
    }

    #[test]
    fn agent_prompt_includes_output_schema() {
        let prompt = build_agent_prompt("", "");
        assert!(prompt.contains("kind"));
        assert!(prompt.contains("body"));
        assert!(prompt.contains("proposed_action"));
        assert!(prompt.contains("source_refs"));
    }

    #[test]
    fn identity_context_loads_or_falls_back() {
        let ctx = load_identity_context(std::path::Path::new("/nonexistent"));
        assert!(ctx.contains("OpenHuman"));
    }
}
