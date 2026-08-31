//! `openhuman agent prompt-size` — where an agent's fixed per-turn budget goes.
//!
//! A turn's fixed cost is the system prompt **plus** the tool schemas that ride
//! alongside it in every request, and on this codebase the second is roughly
//! three times the first: the orchestrator renders ~37 KB of prompt text next
//! to ~112 KB of advertised tool schema. Every prior discussion of prompt size
//! here has been about the prose, because the prose is the half that was
//! visible. This report exists to put both halves on one screen.
//!
//! Modelled on Hermes' `hermes prompt-size` (`hermes_cli/prompt_size.py`),
//! which exists for the same reason and states it plainly: *"Lets users see
//! where their fixed prompt budget goes … without parsing a saved session JSON
//! by hand."*
//!
//! # What is measured
//!
//! Everything comes from [`super::dump_agent_prompt`], which builds a real
//! agent through `Agent::from_config_for_agent` and renders the turn-1 prompt.
//! No numbers are re-derived from a second code path, so the report cannot
//! drift from the dump.
//!
//! # Bytes, not tokens, are the unit of record
//!
//! Token counts depend on the tokenizer, which depends on the model, which is
//! a per-session choice. Bytes are exact and reproducible on any host, so the
//! CI ratchet (`scripts/check-prompt-budget.sh`) works on bytes and this report
//! prints an estimate alongside them purely as a reading aid. Do not tighten
//! [`EST_BYTES_PER_TOKEN`] into a claim of accuracy — it is a divisor, not a
//! tokenizer.

use anyhow::Result;

use super::{dump_agent_prompt, DumpPromptOptions, DumpedPrompt};

/// Rough bytes-per-token for English prose and JSON on a BPE tokenizer.
///
/// Used only to render a human-readable `~N tok` column. The ratchet compares
/// bytes.
pub const EST_BYTES_PER_TOKEN: usize = 4;

/// One markdown section of the rendered system prompt.
///
/// Sections are derived by scanning `#` / `##` / `###` headings in the rendered
/// text rather than by asking the builder, because the builder joins its
/// [`crate::openhuman::agent::prompts::PromptSection`]s into one string and a
/// single section routinely emits several headings (the orchestrator archetype
/// alone contributes a dozen). Heading-derived attribution is what a reader
/// editing a prompt actually wants: it points at the block of text to cut.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SectionSize {
    /// The heading line, verbatim, including its leading `#`s.
    pub heading: String,
    /// Bytes from this heading up to the next heading of any level.
    pub bytes: usize,
}

/// One advertised tool's JSON schema cost.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolSize {
    pub name: String,
    /// Bytes of the compact `{name, description, parameters}` JSON — the shape
    /// the provider receives.
    pub bytes: usize,
    /// Bytes attributable to `parameters` alone, so a reader can tell an
    /// over-described tool from an over-parameterised one.
    pub parameters_bytes: usize,
}

/// The full breakdown for one agent.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PromptSizeReport {
    pub agent: String,
    pub toolkit: Option<String>,
    pub model: String,
    /// Rendered system-prompt bytes.
    pub prompt_bytes: usize,
    /// Advertised tool-schema bytes.
    pub tool_bytes: usize,
    /// `prompt_bytes + tool_bytes` — the fixed cost of every turn.
    pub fixed_prefix_bytes: usize,
    /// Number of tools whose schemas reach the provider. This is the
    /// *advertised* set, already narrowed by the agent's `ToolScope` belt and
    /// by toolpack withholding.
    pub tool_count: usize,
    pub sections: Vec<SectionSize>,
    pub tools: Vec<ToolSize>,
}

impl PromptSizeReport {
    /// Build the report for one agent by rendering its real turn-1 prompt.
    pub async fn build(options: DumpPromptOptions) -> Result<Self> {
        let dumped = dump_agent_prompt(options).await?;
        Ok(Self::from_dump(&dumped))
    }

    /// Derive the report from an already-rendered dump.
    ///
    /// Split out from [`Self::build`] so `dump-all` can report every agent
    /// without paying a second render per agent — each render fetches live
    /// Composio connections and walks the memory tree.
    pub fn from_dump(dumped: &DumpedPrompt) -> Self {
        let sections = split_sections(&dumped.text);
        let mut tools: Vec<ToolSize> = dumped
            .tool_specs
            .iter()
            .map(|spec| {
                let name = spec
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unnamed>")
                    .to_string();
                let parameters_bytes = spec
                    .get("parameters")
                    .map(|v| serde_json::to_string(v).map(|s| s.len()).unwrap_or(0))
                    .unwrap_or(0);
                ToolSize {
                    name,
                    bytes: serde_json::to_string(spec).map(|s| s.len()).unwrap_or(0),
                    parameters_bytes,
                }
            })
            .collect();
        // Descending by cost: the point of the table is to name what to cut
        // first, and registration order carries no information a reader wants.
        tools.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));

        let prompt_bytes = dumped.text.len();
        let tool_bytes = tools.iter().map(|t| t.bytes).sum();
        Self {
            agent: dumped.agent_id.clone(),
            toolkit: dumped.toolkit.clone(),
            model: dumped.model.clone(),
            prompt_bytes,
            tool_bytes,
            fixed_prefix_bytes: prompt_bytes + tool_bytes,
            tool_count: tools.len(),
            sections,
            tools,
        }
    }

    /// Estimated tokens for the whole fixed prefix. Reading aid only.
    pub fn est_tokens(&self) -> usize {
        self.fixed_prefix_bytes / EST_BYTES_PER_TOKEN
    }
}

/// Attribute every byte of `text` to the heading it falls under.
///
/// Bytes before the first heading are attributed to a synthetic `(preamble)`
/// entry rather than dropped — silently losing them would make the section
/// table fail to sum to `prompt_bytes`, which is exactly the kind of quiet
/// inaccuracy that makes a budget tool untrustworthy.
fn split_sections(text: &str) -> Vec<SectionSize> {
    let mut out: Vec<SectionSize> = Vec::new();
    let mut current = SectionSize {
        heading: "(preamble)".to_string(),
        bytes: 0,
    };
    for line in text.split_inclusive('\n') {
        if is_heading(line) {
            if current.bytes > 0 || current.heading != "(preamble)" {
                out.push(std::mem::replace(
                    &mut current,
                    SectionSize {
                        heading: line.trim_end().to_string(),
                        bytes: line.len(),
                    },
                ));
            } else {
                current = SectionSize {
                    heading: line.trim_end().to_string(),
                    bytes: line.len(),
                };
            }
            continue;
        }
        current.bytes += line.len();
    }
    out.push(current);
    out
}

/// A markdown ATX heading: one to three `#` followed by a space.
///
/// Deliberately does not match `####` and deeper — at that depth the blocks are
/// small enough that the table becomes noise rather than signal — and does not
/// match a `#` inside a fenced code block, because the prompts do not contain
/// fenced blocks with leading-`#` lines and carrying a fence state machine here
/// would be more machinery than the report is worth. If that changes, this is
/// the function to fix.
fn is_heading(line: &str) -> bool {
    let trimmed = line.trim_start_matches('\u{feff}');
    matches!(
        (
            trimmed.starts_with("# "),
            trimmed.starts_with("## "),
            trimmed.starts_with("### ")
        ),
        (true, _, _) | (_, true, _) | (_, _, true)
    )
}

/// Render the human-readable report.
///
/// `section_limit` / `tool_limit` cap the two tables; `--json` always carries
/// every row.
pub fn render_text(report: &PromptSizeReport, section_limit: usize, tool_limit: usize) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let est = |b: usize| b / EST_BYTES_PER_TOKEN;

    let label = match &report.toolkit {
        Some(t) => format!("{}@{}", report.agent, t),
        None => report.agent.clone(),
    };
    let _ = writeln!(out, "agent:          {label}");
    let _ = writeln!(out, "model:          {}", report.model);
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "fixed prefix:   {:>9} B  ~{:>7} tok",
        report.fixed_prefix_bytes,
        est(report.fixed_prefix_bytes)
    );
    let _ = writeln!(
        out,
        "  system prompt {:>9} B  ~{:>7} tok",
        report.prompt_bytes,
        est(report.prompt_bytes)
    );
    let _ = writeln!(
        out,
        "  tool schemas  {:>9} B  ~{:>7} tok   ({} advertised tools)",
        report.tool_bytes,
        est(report.tool_bytes),
        report.tool_count
    );

    let mut sections: Vec<&SectionSize> = report.sections.iter().collect();
    sections.sort_by_key(|s| std::cmp::Reverse(s.bytes));
    let _ = writeln!(out, "\nPrompt sections by size");
    for s in sections.iter().take(section_limit) {
        let _ = writeln!(
            out,
            "  {:>7} B  ~{:>6} tok  {}",
            s.bytes,
            est(s.bytes),
            s.heading
        );
    }
    if sections.len() > section_limit {
        let rest: usize = sections[section_limit..].iter().map(|s| s.bytes).sum();
        let _ = writeln!(
            out,
            "  {:>7} B  ~{:>6} tok  … {} more sections",
            rest,
            est(rest),
            sections.len() - section_limit
        );
    }

    let _ = writeln!(out, "\nTool schemas by size");
    for t in report.tools.iter().take(tool_limit) {
        let _ = writeln!(
            out,
            "  {:>7} B  ~{:>6} tok  {:<34} (params {} B)",
            t.bytes,
            est(t.bytes),
            t.name,
            t.parameters_bytes
        );
    }
    if report.tools.len() > tool_limit {
        let rest: usize = report.tools[tool_limit..].iter().map(|t| t.bytes).sum();
        let _ = writeln!(
            out,
            "  {:>7} B  ~{:>6} tok  … {} more tools",
            rest,
            est(rest),
            report.tools.len() - tool_limit
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_sum_to_the_whole_prompt() {
        let text = "preamble line\n# Title\nbody\n## Sub\nmore body\n### Deep\ntail\n";
        let sections = split_sections(text);
        let total: usize = sections.iter().map(|s| s.bytes).sum();
        assert_eq!(
            total,
            text.len(),
            "section bytes must account for every byte of the prompt; \
             a table that does not sum is worse than no table"
        );
    }

    #[test]
    fn preamble_is_kept_when_text_starts_before_the_first_heading() {
        let sections = split_sections("loose text\n# Title\nbody\n");
        assert_eq!(sections[0].heading, "(preamble)");
        assert_eq!(sections[0].bytes, "loose text\n".len());
    }

    #[test]
    fn no_preamble_entry_when_the_prompt_opens_on_a_heading() {
        let sections = split_sections("# Title\nbody\n");
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].heading, "# Title");
    }

    #[test]
    fn deeper_headings_do_not_split() {
        // `####` is body text as far as this report is concerned.
        let sections = split_sections("## Sub\na\n#### Deeper\nb\n");
        assert_eq!(sections.len(), 1);
    }

    #[test]
    fn a_bare_hash_is_not_a_heading() {
        // No trailing space: a shell comment in an example block, not a section.
        assert!(!is_heading("#!/usr/bin/env bash\n"));
        assert!(!is_heading("#hashtag\n"));
        assert!(is_heading("## Real\n"));
    }

    #[test]
    fn tools_are_ranked_by_cost_not_registration_order() {
        let dumped = DumpedPrompt {
            agent_id: "t".into(),
            toolkit: None,
            mode: "session",
            model: "m".into(),
            workspace_dir: std::path::PathBuf::from("/tmp"),
            text: "# A\nbody\n".into(),
            tool_names: vec!["small".into(), "big".into()],
            skill_tool_count: 0,
            tool_specs: vec![
                serde_json::json!({"name": "small", "description": "s", "parameters": {}}),
                serde_json::json!({
                    "name": "big",
                    "description": "a much longer description than the other one",
                    "parameters": {"type": "object", "properties": {"a": {"type": "string"}}}
                }),
            ],
        };
        let report = PromptSizeReport::from_dump(&dumped);
        assert_eq!(report.tools[0].name, "big");
        assert_eq!(report.tool_count, 2);
        assert_eq!(
            report.fixed_prefix_bytes,
            report.prompt_bytes + report.tool_bytes
        );
        assert!(report.tools[1].parameters_bytes > 0, "`{{}}` is two bytes");
    }
}
