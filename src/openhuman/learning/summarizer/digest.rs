//! Tool-call digest layer — collapse raw multi-call tool history into
//! per-tool summaries before any reflection / ingest LLM call sees them.
//!
//! Reflection over a raw transcript drags every tool call's full output
//! through the summarizer, which (a) inflates cost and (b) blows past
//! the summarizer's smaller context window. The digest layer fixes both:
//! it groups calls by tool name, keeps aggregate counts plus a small
//! sample of inputs/outputs per tool, and renders to a compact block
//! that fits comfortably in a sub-4k-token prompt.

use crate::openhuman::agent::hooks::ToolCallRecord;

/// How many sample input/output strings we keep per tool. Keeps the
/// digest bounded — more than this and the per-tool block starts
/// dominating the prompt again.
pub const SAMPLES_PER_TOOL: usize = 2;

/// Maximum characters retained per sampled output. Anything longer is
/// truncated with an ellipsis marker — the reflection model only needs
/// a flavour of what happened, not the raw payload.
pub const SAMPLE_OUTPUT_CHARS: usize = 160;

/// Per-tool aggregate over a turn's (or transcript's) tool calls.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallDigest {
    /// Tool name (e.g. `read_file`, `Bash`).
    pub name: String,
    /// Total number of times this tool was invoked.
    pub count: usize,
    /// Number of invocations that reported success.
    pub success_count: usize,
    /// 95th-percentile observed duration in milliseconds. With few
    /// samples this is just the max — accurate enough for prompt
    /// budgeting and trend logging.
    pub p95_duration_ms: u64,
    /// A small bounded sample of input JSON (serialized, truncated).
    pub sample_inputs: Vec<String>,
    /// A small bounded sample of output summaries (truncated).
    pub sample_outputs: Vec<String>,
}

impl ToolCallDigest {
    /// Success rate as `[0.0, 1.0]`. Returns 0.0 when `count == 0`.
    pub fn success_rate(&self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        (self.success_count as f32) / (self.count as f32)
    }
}

/// Group `tool_calls` by tool name and produce a bounded digest per
/// tool. Stable order — first appearance wins — so prompts are
/// deterministic.
pub fn compress_tool_calls(tool_calls: &[ToolCallRecord]) -> Vec<ToolCallDigest> {
    use std::collections::BTreeMap;

    // BTreeMap to keep deterministic ordering for tests; insertion
    // order isn't important here — by-name alphabetical is fine and
    // makes the rendered prompt easier to scan.
    let mut by_name: BTreeMap<String, ToolCallDigest> = BTreeMap::new();
    let mut max_dur: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    for record in tool_calls {
        let entry = by_name
            .entry(record.name.clone())
            .or_insert_with(|| ToolCallDigest {
                name: record.name.clone(),
                count: 0,
                success_count: 0,
                p95_duration_ms: 0,
                sample_inputs: Vec::new(),
                sample_outputs: Vec::new(),
            });
        entry.count += 1;
        if record.success {
            entry.success_count += 1;
        }
        let cap = max_dur.entry(record.name.clone()).or_insert(0);
        if record.duration_ms > *cap {
            *cap = record.duration_ms;
        }
        if entry.sample_inputs.len() < SAMPLES_PER_TOOL {
            entry
                .sample_inputs
                .push(truncate(&record.arguments.to_string(), SAMPLE_OUTPUT_CHARS));
        }
        if entry.sample_outputs.len() < SAMPLES_PER_TOOL {
            entry
                .sample_outputs
                .push(truncate(&record.output_summary, SAMPLE_OUTPUT_CHARS));
        }
    }

    for (name, dur) in max_dur {
        if let Some(entry) = by_name.get_mut(&name) {
            entry.p95_duration_ms = dur;
        }
    }

    by_name.into_values().collect()
}

/// Render a slice of digests into a compact prompt block suitable for
/// inclusion under a `## Tool Calls` heading. Returns the empty string
/// for an empty slice so callers can splice it in unconditionally.
pub fn render_tool_digests(digests: &[ToolCallDigest]) -> String {
    if digests.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for d in digests {
        let pct = (d.success_rate() * 100.0).round() as u32;
        out.push_str(&format!(
            "- {} ×{} ({}% ok, p95={}ms)\n",
            d.name, d.count, pct, d.p95_duration_ms
        ));
        if !d.sample_inputs.is_empty() {
            out.push_str("  inputs: ");
            out.push_str(&d.sample_inputs.join(" | "));
            out.push('\n');
        }
        if !d.sample_outputs.is_empty() {
            out.push_str("  outputs: ");
            out.push_str(&d.sample_outputs.join(" | "));
            out.push('\n');
        }
    }
    out
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let head: String = s.chars().take(max_chars).collect();
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(name: &str, ok: bool, dur: u64, out: &str) -> ToolCallRecord {
        ToolCallRecord {
            name: name.into(),
            arguments: json!({"q": "x"}),
            success: ok,
            output_summary: out.into(),
            duration_ms: dur,
        }
    }

    #[test]
    fn empty_input_yields_empty_output() {
        assert!(compress_tool_calls(&[]).is_empty());
        assert_eq!(render_tool_digests(&[]), "");
    }

    #[test]
    fn groups_by_tool_with_counts_and_success_rate() {
        let calls = vec![
            record("Bash", true, 100, "ok 1"),
            record("Bash", false, 250, "fail"),
            record("read_file", true, 12, "ok read"),
        ];
        let mut digests = compress_tool_calls(&calls);
        digests.sort_by(|a, b| a.name.cmp(&b.name));

        assert_eq!(digests.len(), 2);
        let bash = &digests[0];
        assert_eq!(bash.name, "Bash");
        assert_eq!(bash.count, 2);
        assert_eq!(bash.success_count, 1);
        assert!((bash.success_rate() - 0.5).abs() < f32::EPSILON);
        assert_eq!(bash.p95_duration_ms, 250);

        let read = &digests[1];
        assert_eq!(read.name, "read_file");
        assert_eq!(read.count, 1);
        assert_eq!(read.success_count, 1);
    }

    #[test]
    fn caps_samples_per_tool() {
        let calls: Vec<_> = (0..5)
            .map(|i| record("Bash", true, 10, &format!("out {i}")))
            .collect();
        let digests = compress_tool_calls(&calls);
        assert_eq!(digests.len(), 1);
        assert_eq!(digests[0].count, 5);
        assert_eq!(digests[0].sample_outputs.len(), SAMPLES_PER_TOOL);
        assert_eq!(digests[0].sample_inputs.len(), SAMPLES_PER_TOOL);
    }

    #[test]
    fn truncates_long_outputs() {
        let long = "x".repeat(SAMPLE_OUTPUT_CHARS + 50);
        let digests = compress_tool_calls(&[record("Bash", true, 1, &long)]);
        let sample = &digests[0].sample_outputs[0];
        assert!(
            sample.chars().count() <= SAMPLE_OUTPUT_CHARS + 1,
            "sample={sample}"
        );
        assert!(sample.ends_with('…'));
    }

    #[test]
    fn render_emits_human_block() {
        let digests = compress_tool_calls(&[
            record("Bash", true, 50, "ok"),
            record("Bash", false, 100, "fail"),
        ]);
        let rendered = render_tool_digests(&digests);
        assert!(rendered.contains("Bash ×2"));
        assert!(rendered.contains("50% ok"));
        assert!(rendered.contains("p95=100ms"));
    }
}
