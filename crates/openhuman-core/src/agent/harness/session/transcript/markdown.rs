//! Human-readable markdown rendering of a transcript (the `.md` companion).

use super::types::{TranscriptMeta, TurnUsage};
use crate::agent::messages::ChatMessage;
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;

/// Render a human-readable markdown representation of the transcript.
///
/// This output is **for humans only** — it is never read back by the
/// application. All resume / round-trip logic uses the JSONL source of truth.
pub(super) fn render_markdown(
    messages: &[ChatMessage],
    meta: &TranscriptMeta,
    per_message_usage: &HashMap<usize, &TurnUsage>,
) -> String {
    let mut buf = String::new();

    let _ = writeln!(buf, "# Session transcript — {}", meta.agent_name);
    buf.push('\n');
    let _ = writeln!(buf, "- Dispatcher: {}", meta.dispatcher);
    if let Some(agent_id) = meta.agent_id.as_deref() {
        let _ = writeln!(buf, "- Agent ID: `{agent_id}`");
    }
    if let Some(agent_type) = meta.agent_type.as_deref() {
        let _ = writeln!(buf, "- Agent type: `{agent_type}`");
    }
    if let Some(provider) = meta.provider.as_deref() {
        let _ = writeln!(buf, "- Provider: `{provider}`");
    }
    if let Some(model) = meta.model.as_deref() {
        let _ = writeln!(buf, "- Model: `{model}`");
    }
    if let Some(task_id) = meta.task_id.as_deref() {
        let _ = writeln!(buf, "- Task: `{task_id}`");
    }
    if let Some(tid) = meta.thread_id.as_deref() {
        let _ = writeln!(buf, "- Thread: `{tid}`");
    }
    let _ = writeln!(buf, "- Turns: {}", meta.turn_count);
    if meta.input_tokens > 0 || meta.output_tokens > 0 {
        let cache_pct = if meta.input_tokens > 0 {
            (meta.cached_input_tokens as f64 / meta.input_tokens as f64) * 100.0
        } else {
            0.0
        };
        let _ = writeln!(
            buf,
            "- Tokens: {} in / {} out / {} cached ({:.1}% hit)",
            meta.input_tokens, meta.output_tokens, meta.cached_input_tokens, cache_pct
        );
    }
    if meta.charged_amount_usd > 0.0 {
        let _ = writeln!(buf, "- Charged: ${:.6}", meta.charged_amount_usd);
    }
    let _ = writeln!(buf, "- Updated: {}", meta.updated);

    for (i, msg) in messages.iter().enumerate() {
        buf.push_str("\n---\n\n");

        if let Some(tu) = per_message_usage.get(&i) {
            let _ = writeln!(
                buf,
                "## [{}] · {} · {} in / {} out / {} cached · ${:.6}",
                msg.role,
                tu.model,
                tu.usage.input,
                tu.usage.output,
                tu.usage.cached_input,
                tu.usage.cost_usd
            );
            if !tu.provider.is_empty() || tu.usage.context_window > 0 {
                let _ = writeln!(
                    buf,
                    "_provider: `{}` · iteration: {} · context window: {}_",
                    tu.provider, tu.iteration, tu.usage.context_window
                );
            }
            if let Some(reasoning) = tu.reasoning_content.as_deref().filter(|s| !s.is_empty()) {
                let _ = writeln!(buf, "\n### Thoughts\n\n{reasoning}\n");
            }
        } else {
            let _ = writeln!(buf, "## [{}]", msg.role);
        }

        buf.push('\n');
        buf.push_str(&msg.content);
        buf.push('\n');
    }

    buf
}
