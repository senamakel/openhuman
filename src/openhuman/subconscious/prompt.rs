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
pub fn build_agent_prompt(
    situation_report: &str,
    identity_context: &str,
    scratchpad_section: &str,
) -> String {
    format!(
        r#"{identity_context}

# Subconscious Agent

You are the user's background awareness layer — a deep reasoning loop
that wakes up periodically, actively researches the user's world, and
surfaces genuinely useful insights. You are NOT a passive summarizer.
You must USE YOUR TOOLS to investigate before forming conclusions.

## Situation Report (pre-loaded context)

{situation_report}

{scratchpad_section}

## Research Phase — MANDATORY

You MUST use tools before producing thoughts. Do not skip this phase.
Run multiple research passes — the more context you gather, the better
your insights will be.

### Tool usage guide (use these by name):

**Memory retrieval:**

1. **`call_memory_agent`** — Your PRIMARY research tool. Spawns a
   dedicated memory sub-agent that autonomously combines vector search,
   keyword matching, entity lookup, and tree browsing — then returns a
   cited answer. Use this FIRST for any question about the user's world.
   - Parameters: `query: "<your research question>"`,
     optionally `context: "<why you need this>"`, `max_turns: 15`

2. **`memory_recall`** — Quick keyword/vector search in a namespace.
   Good for targeted follow-up lookups of specific facts.
   - Parameters: `namespace: "global"`, `query: "<search terms>"`

3. **`memory_tree`** — Direct tree operations for targeted exploration:
   - `mode: "search_entities"` — find people, projects, topics by name
   - `mode: "query_source"` — filter by source type + time window
   - `mode: "drill_down"` — expand a summary node one level deeper
   - `mode: "fetch_leaves"` — get raw source chunks for citation

**Scratchpad (your persistent working memory):**

4. **`scratchpad_add`** — Save a thought, hypothesis, or follow-up item
   that should persist across ticks. Use `priority` (0-10) to mark
   importance. Example: `{{"body": "User has a meeting with Alice on
   Friday — check if prep is done", "priority": 5}}`

5. **`scratchpad_edit`** — Update an existing scratchpad entry with new
   information or revised thinking. Pass the `id` shown in brackets.

6. **`scratchpad_remove`** — Remove an entry that's no longer relevant
   or has been fully addressed.

### Research strategy:

**Turn 1**: Call `call_memory_agent` with a broad question about the
user's recent activity, open threads, and anything the situation report
or scratchpad highlights.

**Turn 2+**: Follow up on interesting findings. Use `memory_tree` with
`search_entities` or `drill_down` to get specifics. Use `memory_recall`
for targeted fact lookups.

**Scratchpad maintenance**: Throughout your research, update your
scratchpad — add new observations, edit entries with fresh data, remove
stale items. Your scratchpad is loaded at the start of every tick, so
it IS your continuity mechanism.

**Final turn**: Synthesize your research into structured thoughts.

## Observation Guidelines

Based on your research, identify:
- **Patterns** across sources (email + calendar + chat converging on same topic)
- **Deadlines** approaching or overdue
- **Risks** — concentration of negative signals, unresolved blockers
- **Opportunities** — connections the user might not see
- **Activity spikes** — topics getting unusually hot

**Self vs. others**: the *Your Identifiers* section (if present) lists
the user's handles, emails, and user_ids. Never attribute someone else's
activity to the user.

**Anti-double-emit**: the *Recent reflections* section shows what you
already surfaced. Re-emit only if the signal materially intensified or
your research uncovered genuinely new information about a prior topic.

Cap: at most **5 thoughts per tick**.

## Final output

After completing your research, end your final message with a JSON
block containing your thoughts:

```json
{{
  "thoughts": [
    {{
      "kind": "hotness_spike | cross_source_pattern | daily_digest | due_item | risk | opportunity",
      "body": "Short markdown observation grounded in your research findings.",
      "proposed_action": "Optional one-tap action text (or null).",
      "source_refs": ["entity:foo", "summary:bar"]
    }}
  ]
}}
```
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
        let prompt = build_agent_prompt("## State\nSome data.", "Identity here", "");
        assert!(prompt.contains("Some data."));
        assert!(prompt.contains("Identity here"));
        assert!(prompt.contains("thoughts"));
    }

    #[test]
    fn agent_prompt_includes_output_schema() {
        let prompt = build_agent_prompt("", "", "");
        assert!(prompt.contains("kind"));
        assert!(prompt.contains("body"));
        assert!(prompt.contains("proposed_action"));
        assert!(prompt.contains("source_refs"));
    }

    #[test]
    fn agent_prompt_includes_scratchpad_when_provided() {
        let prompt = build_agent_prompt("", "", "## Scratchpad\n- item 1");
        assert!(prompt.contains("## Scratchpad"));
        assert!(prompt.contains("item 1"));
    }

    #[test]
    fn agent_prompt_references_call_memory_agent() {
        let prompt = build_agent_prompt("", "", "");
        assert!(prompt.contains("call_memory_agent"));
        assert!(prompt.contains("scratchpad_add"));
        assert!(prompt.contains("scratchpad_edit"));
        assert!(prompt.contains("scratchpad_remove"));
    }

    #[test]
    fn identity_context_loads_or_falls_back() {
        let ctx = load_identity_context(std::path::Path::new("/nonexistent"));
        assert!(ctx.contains("OpenHuman"));
    }
}
