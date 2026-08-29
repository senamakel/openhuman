use async_trait::async_trait;
use serde_json::json;
use serde_json::Value;

use crate::openhuman::tools::traits::{
    PermissionLevel, Tool, ToolCallOptions, ToolCategory, ToolResult, ToolTimeout,
};
use tinytools::ToolRunContext;

pub struct ArchetypeDelegationTool {
    pub tool_name: String,
    pub agent_id: String,
    pub tool_description: String,
}

#[async_trait]
impl Tool for ArchetypeDelegationTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    /// The delegation envelope — deliberately description-light.
    ///
    /// This one literal is emitted for **every** synthesised `delegate_*` tool
    /// (19 of them on the Master Agent after tool-pack withholding), so each
    /// word of `description` here is billed 19× on every single turn. Fully
    /// described the envelope was 356 tokens × 19 = 6,764 tokens — 39% of the
    /// orchestrator's whole tool-schema budget, for the same JSON 19 times.
    ///
    /// The field *semantics* now live once in the parent's system prompt
    /// (`registry/agents/orchestrator/prompt.md`, "Structured handoffs"),
    /// which is where policy like "only observed facts" belonged anyway. The
    /// property names stay self-describing, and they are the only thing
    /// `render_structured_handoff` below reads.
    ///
    /// Four descriptions survive, each well under the 50-token cap, because
    /// their property name does not carry the meaning:
    ///
    /// * `blocking` — the default is behaviour-critical and not inferable from
    ///   the name. Getting it wrong is silent and asymmetric: async when it
    ///   should have blocked finalizes the turn before the result lands, the
    ///   exact failure the prompt's result-gating rule exists to prevent.
    /// * `evidence` — "actually observed" is the anti-fabrication contract,
    ///   not a label.
    /// * `citation_requirement` / `model` — a bare name reads as neither.
    ///
    /// Enforced by `envelope_descriptions_stay_within_budget` below. If you
    /// are about to add a description here, put it in prompt.md instead.
    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["prompt"],
            "properties": {
                "prompt": { "type": "string" },
                "objective": { "type": "string" },
                "evidence": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Only facts, paths, URLs, ids or tool outputs you actually observed."
                },
                "constraints": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "must_not_assume": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "expected_output": { "type": "string" },
                "citation_requirement": {
                    "type": "string",
                    "enum": ["none", "file_paths", "urls", "retrieval_hits", "tool_outputs"],
                    "description": "Evidence style the child must preserve in its result."
                },
                "model": {
                    "type": "string",
                    "description": "Pin the child to this exact model id. Omit unless you have a reason."
                },
                "blocking": {
                    "type": "boolean",
                    "description": "Default false: async worker, result arrives as a later turn. true: waits, and the result gates this reply."
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Execute
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    /// Run **without** the global per-tool wall-clock deadline. This tool is a
    /// delegation primitive: it hands a task to a bounded sub-agent
    /// (`tools_agent` → `delegate_tools_agent`, `code_executor` → `run_code`,
    /// …) and awaits that agent's full run. Under the default `Inherit` policy
    /// the whole delegation is hard-killed at the single-tool timeout (120s) —
    /// so any sub-agent run that legitimately exceeds two minutes is truncated
    /// mid-flight (Sentry TAURI-RUST-K29 `delegate_tools_agent` and
    /// TAURI-RUST-8HB `run_code`: thousands of 120.000s truncations). The
    /// child's lifetime is already bounded internally — by its `max_iterations`,
    /// the run cancellation token, and each inner tool's own timeout — so it
    /// governs its own duration, exactly like the sibling `spawn_parallel_agents`
    /// fan-out and the long-running scripting tools (`shell`, `node_exec`).
    fn timeout_policy(&self, _args: &serde_json::Value) -> ToolTimeout {
        ToolTimeout::Unbounded
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        self.execute_with_context(args, ToolCallOptions::default(), None)
            .await
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        _options: ToolCallOptions,
        tool_context: Option<&dyn ToolRunContext>,
    ) -> anyhow::Result<ToolResult> {
        let raw_prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        if raw_prompt.is_empty() {
            return Ok(ToolResult::error(format!(
                "{}: `prompt` is required",
                self.tool_name
            )));
        }
        let prompt = render_structured_handoff(&raw_prompt, &args);

        let model_override = args
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());

        // Async by default: the delegated specialist runs as a durable,
        // resumable worker and its result comes back as a new chat turn.
        // `blocking: true` is the opt-in for results that must gate this
        // reply. (`dispatch_subagent` itself falls back to blocking when
        // there is no chat thread to deliver an async result into.)
        let blocking = args
            .get("blocking")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let mode = if blocking {
            super::dispatch::DispatchMode::Blocking
        } else {
            super::dispatch::DispatchMode::PreferAsync
        };

        super::dispatch_subagent(
            &self.agent_id,
            &self.tool_name,
            &prompt,
            None,
            model_override,
            tool_context,
            mode,
        )
        .await
    }
}

fn render_structured_handoff(prompt: &str, args: &Value) -> String {
    let mut out = String::new();
    out.push_str("Task:\n");
    out.push_str(prompt.trim());

    push_optional_string(&mut out, "Objective", args.get("objective"));
    push_optional_array(&mut out, "Evidence", args.get("evidence"));
    push_optional_array(&mut out, "Constraints", args.get("constraints"));
    push_optional_array(&mut out, "Must not assume", args.get("must_not_assume"));
    push_optional_string(&mut out, "Expected output", args.get("expected_output"));
    push_optional_string(
        &mut out,
        "Citation requirement",
        args.get("citation_requirement"),
    );

    out
}

fn push_optional_string(out: &mut String, label: &str, value: Option<&Value>) {
    let Some(text) = value.and_then(Value::as_str).map(str::trim) else {
        return;
    };
    if text.is_empty() {
        return;
    }
    out.push_str("\n\n");
    out.push_str(label);
    out.push_str(":\n");
    out.push_str(text);
}

fn push_optional_array(out: &mut String, label: &str, value: Option<&Value>) {
    let Some(items) = value.and_then(Value::as_array) else {
        return;
    };
    let strings: Vec<&str> = items
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if strings.is_empty() {
        return;
    }
    out.push_str("\n\n");
    out.push_str(label);
    out.push_str(":\n");
    for item in strings {
        out.push_str("- ");
        out.push_str(item);
        out.push('\n');
    }
    if out.ends_with('\n') {
        out.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::agent::harness::definition::AgentDefinitionRegistry;

    fn sample_tool() -> ArchetypeDelegationTool {
        ArchetypeDelegationTool {
            tool_name: "delegate_researcher".to_string(),
            agent_id: "researcher".to_string(),
            tool_description: "Use for web and docs research.".to_string(),
        }
    }

    #[test]
    fn metadata_methods_expose_name_description_and_system_category() {
        let tool = sample_tool();
        assert_eq!(tool.name(), "delegate_researcher");
        assert_eq!(tool.description(), "Use for web and docs research.");
        assert_eq!(tool.permission_level(), PermissionLevel::Execute);
        assert_eq!(tool.category(), ToolCategory::System);
    }

    #[test]
    fn delegation_opts_out_of_the_global_tool_timeout() {
        // A delegated sub-agent run (delegate_tools_agent / run_code / …) can
        // legitimately outlast the single-tool wall-clock default (120s): under
        // `Inherit` every such run is hard-killed and truncated (Sentry
        // TAURI-RUST-K29 / TAURI-RUST-8HB). The child bounds its own lifetime
        // via its max_iterations, the run cancellation token, and each inner
        // tool's own timeout — so this primitive must be Unbounded, like
        // spawn_parallel_agents and the long-running scripting tools.
        assert_eq!(
            sample_tool().timeout_policy(&json!({})),
            ToolTimeout::Unbounded,
        );
    }

    #[test]
    fn parameters_schema_advertises_async_default_blocking_opt_in() {
        // Delegations are async by default (durable worker + follow-up
        // delivery turn); `blocking: true` is the explicit opt-in for
        // results that must gate the current reply. The flag must be
        // advertised but never required.
        let schema = sample_tool().parameters_schema();
        let blocking = &schema["properties"]["blocking"];
        assert_eq!(blocking["type"], "boolean");
        let desc = blocking["description"].as_str().unwrap_or_default();
        assert!(desc.contains("async"), "explains the async default: {desc}");
        assert!(
            desc.contains("Default false"),
            "names which value is the default: {desc}"
        );
        // The resume contract (`subagent_session_id`, `continue_subagent`,
        // `steer_subagent`, …) used to be spelled out here, at 19x the cost.
        // It now lives once in the orchestrator prompt, which
        // `prompt_documents_the_stripped_envelope_fields` pins.
        assert_eq!(schema["required"], json!(["prompt"]));
    }

    #[test]
    fn parameters_schema_requires_prompt_only() {
        let tool = sample_tool();
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["prompt"]));
        assert_eq!(schema["properties"]["prompt"]["type"], "string");
        assert_eq!(schema["properties"]["objective"]["type"], "string");
        assert_eq!(schema["properties"]["evidence"]["type"], "array");
        assert_eq!(
            schema["properties"]["citation_requirement"]["enum"],
            json!([
                "none",
                "file_paths",
                "urls",
                "retrieval_hits",
                "tool_outputs"
            ])
        );

        // Stripping descriptions must not become stripping FIELDS: every one
        // is read back by `render_structured_handoff`, so a "trim" that drops
        // one silently removes a section of the child prompt.
        let props = schema["properties"]
            .as_object()
            .expect("properties is an object");
        let mut present: Vec<&str> = props.keys().map(String::as_str).collect();
        present.sort_unstable();
        assert_eq!(
            present,
            vec![
                "blocking",
                "citation_requirement",
                "constraints",
                "evidence",
                "expected_output",
                "model",
                "must_not_assume",
                "objective",
                "prompt",
            ]
        );
    }

    /// Every `description` in the envelope, as `(json-pointer-ish path, text)`.
    fn collect_descriptions(node: &Value, path: &str, out: &mut Vec<(String, String)>) {
        match node {
            Value::Object(map) => {
                for (key, value) in map {
                    if key == "description" {
                        if let Some(text) = value.as_str() {
                            out.push((path.to_string(), text.to_string()));
                        }
                    } else {
                        collect_descriptions(value, &format!("{path}/{key}"), out);
                    }
                }
            }
            Value::Array(items) => {
                for (idx, item) in items.iter().enumerate() {
                    collect_descriptions(item, &format!("{path}/{idx}"), out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn envelope_descriptions_stay_within_budget() {
        // This schema is emitted once per synthesised `delegate_*` tool — 19
        // times on the Master Agent — so prose here is billed 19x per turn.
        // Fully described it was 356 tokens each, 6,764 in total and 39% of
        // the agent's whole tool-schema budget; it is now 193.
        //
        // Two rules hold that: only the four fields whose NAME does not carry
        // their meaning may carry a description, and none may exceed the
        // ~50-token cap. Anything else belongs in prompt.md, where it is
        // charged once. See `parameters_schema`'s doc comment for why each
        // survivor survives.
        let schema = sample_tool().parameters_schema();
        let mut found = Vec::new();
        collect_descriptions(&schema, "", &mut found);

        let mut fields: Vec<&str> = found.iter().map(|(path, _)| path.as_str()).collect();
        fields.sort_unstable();
        assert_eq!(
            fields,
            vec![
                "/properties/blocking",
                "/properties/citation_requirement",
                "/properties/evidence",
                "/properties/model",
            ],
            "a description came back into the delegation envelope; put it in \
             orchestrator/prompt.md instead — every word here costs 19x"
        );

        // ~4 chars per token on this vocabulary, so 220 chars ~= the 50-token
        // cap. A byte budget alone gets nibbled away, which is why the field
        // set above is the load-bearing half of this test.
        for (field, text) in &found {
            assert!(
                text.len() <= 220,
                "{field} description is {} chars, over the ~50-token cap: {text}",
                text.len()
            );
        }
    }

    #[test]
    fn prompt_documents_the_stripped_envelope_fields() {
        // The contract MOVED, it did not vanish. Stripping the per-field
        // descriptions is only safe while the parent prompt still teaches
        // them, so couple the two directly: this fails the moment someone
        // rewrites prompt.md without the "Structured handoffs" block.
        const ORCHESTRATOR_PROMPT: &str =
            include_str!("../../registry/agents/orchestrator/prompt.md");

        for needle in [
            "objective",
            "evidence",
            "constraints",
            "must_not_assume",
            "expected_output",
            "citation_requirement",
            "blocking",
            "subagent_session_id",
            "continue_subagent",
        ] {
            assert!(
                ORCHESTRATOR_PROMPT.contains(needle),
                "orchestrator/prompt.md no longer documents `{needle}`, which \
                 the delegation envelope stopped describing to save 19x the tokens"
            );
        }
    }

    #[test]
    fn structured_handoff_renders_compact_child_prompt() {
        let rendered = render_structured_handoff(
            "Check this",
            &json!({
                "prompt": "Check this",
                "objective": "Answer with supported claims only.",
                "evidence": ["file:src/lib.rs", "tool output: count=3", ""],
                "constraints": ["Do not edit files"],
                "must_not_assume": ["Current service state"],
                "expected_output": "Findings list",
                "citation_requirement": "file_paths",
            }),
        );

        assert!(rendered.contains("Task:\nCheck this"));
        assert!(rendered.contains("Objective:\nAnswer with supported claims only."));
        assert!(rendered.contains("Evidence:\n- file:src/lib.rs\n- tool output: count=3"));
        assert!(rendered.contains("Must not assume:\n- Current service state"));
        assert!(rendered.contains("Citation requirement:\nfile_paths"));
        assert!(!rendered.contains("\"model\""));
    }

    #[tokio::test]
    async fn execute_rejects_missing_or_blank_prompt() {
        let tool = sample_tool();

        let missing = tool.execute(json!({})).await.unwrap();
        assert!(missing.is_error);
        assert!(missing.output().contains("`prompt` is required"));

        let blank = tool.execute(json!({ "prompt": "   " })).await.unwrap();
        assert!(blank.is_error);
        assert!(blank.output().contains("`prompt` is required"));
    }

    #[tokio::test]
    async fn execute_accepts_non_empty_prompt_and_reaches_dispatch_path() {
        let _ = AgentDefinitionRegistry::init_global_builtins();
        let tool = sample_tool();
        let result = tool
            .execute(json!({ "prompt": "find the answer" }))
            .await
            .unwrap();

        let out = result.output();
        assert!(
            !out.contains("`prompt` is required"),
            "non-empty prompt should bypass local validation, got: {out}"
        );
    }
}
