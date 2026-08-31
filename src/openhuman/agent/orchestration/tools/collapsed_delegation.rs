//! `delegate` — every archetype hand-off as one action-dispatched tool.
//!
//! Replaces the per-sub-agent fan-out where `collect_orchestrator_tools`
//! synthesised one [`ArchetypeDelegationTool`] per named sub-agent. On the
//! Master Agent that was **16 tools worth 17,746 bytes, 41% of its whole
//! tool-schema budget** — and the schemas were not 16 different things. Every
//! one of them carried a byte-identical copy of the delegation envelope
//! (`prompt` / `objective` / `evidence` / `constraints` / `must_not_assume` /
//! `expected_output` / `citation_requirement` / `model` / `blocking`), because
//! `ArchetypeDelegationTool::parameters_schema` is one `json!` literal that
//! does not read `self`. The only thing that differed between the 16 was the
//! name and the target's `when_to_use` line.
//!
//! So the envelope is emitted once here and the 16 names become an `agent`
//! enum, with each target's `when_to_use` kept verbatim in the description —
//! the routing information survives in full, the repetition does not.
//!
//! This is the same collapse [`SkillDelegationTool`] already applied to the
//! *other* delegation axis (#1335): one `delegate_to_integrations_agent` with
//! a `toolkit` argument, instead of one `delegate_<toolkit>` per connected
//! Composio integration. That change made the schema constant in the
//! integration dimension; this one makes it constant in the sub-agent
//! dimension. The two are now consistent.
//!
//! # Why collapse rather than pack
//!
//! The toolpack mechanism (`load_skill` / `use_skill`) exists and would also
//! remove these bytes, but it is the wrong tool for this family. A pack costs
//! a round trip on first use, which is the right trade for a capability most
//! turns never touch — crypto, MCP setup, the `.pptx` writer. Delegation is
//! the orchestrator's *job*; putting a round trip in front of it would tax the
//! single most common thing it does, on almost every turn.
//!
//! Collapsing has the opposite cost profile: one extra enum field on a call
//! the model was making anyway, and no round trip at all. Frequency of use is
//! what separates the two mechanisms — see `toolpacks::registry`, whose
//! `DELIBERATELY_UNPACKED_FLEET_TOOLS` note draws the same line for the same
//! reason.
//!
//! # The members stay registered
//!
//! Each `delegate_*` / `research` / `plan` / … tool remains in the registry as
//! [`ToolExposure::Hidden`], exactly like the members of the collapsed `cron`
//! and `memory` tools. They are off the wire, not gone: a replayed transcript,
//! a saved skill, or a flow node that names `research` still resolves. Only
//! the advertised surface shrinks.
//!
//! [`ArchetypeDelegationTool`]: super::ArchetypeDelegationTool
//! [`SkillDelegationTool`]: super::SkillDelegationTool
//! [`ToolExposure::Hidden`]: crate::openhuman::tools::traits::ToolExposure::Hidden

use async_trait::async_trait;
use serde_json::{json, Value};

use super::archetype_delegation::{delegation_envelope_properties, render_structured_handoff};
use crate::openhuman::tools::traits::{
    PermissionLevel, Tool, ToolCallOptions, ToolCategory, ToolResult, ToolTimeout,
};
use tinytools::ToolRunContext;

/// The advertised name. A constant so the synthesis site, the prompt's
/// delegation section and the tests cannot disagree about it.
pub const DELEGATE_TOOL_NAME: &str = "delegate";

/// One routable sub-agent.
pub struct DelegateTarget {
    /// The name this target had when it was its own tool, and the value the
    /// `agent` enum takes. Keeping the old name as the enum value is what lets
    /// the orchestrator prompt go on naming `research` and `schedule_task`
    /// without a rewrite, and keeps dispatch events reading as they did.
    pub tool_name: String,
    /// The registry id the work is actually handed to.
    pub agent_id: String,
    /// The target's `when_to_use`, verbatim.
    pub description: String,
}

/// Every archetype hand-off as one tool.
pub struct DelegateTool {
    targets: Vec<DelegateTarget>,
    description: String,
}

impl DelegateTool {
    /// Build the collapsed tool, or `None` when there is nothing to route to.
    ///
    /// `None` rather than an empty enum: a `delegate` tool whose `agent` has no
    /// valid value is a schema the model can only call wrongly, and the
    /// sibling [`SkillDelegationTool::for_connected`] already returns `None` on
    /// an empty toolkit list for the same reason.
    ///
    /// [`SkillDelegationTool::for_connected`]: super::SkillDelegationTool::for_connected
    pub fn for_targets(targets: Vec<DelegateTarget>) -> Option<Self> {
        if targets.is_empty() {
            return None;
        }
        let description = build_description(&targets);
        Some(Self {
            targets,
            description,
        })
    }

    fn resolve(&self, agent: &str) -> Option<&DelegateTarget> {
        self.targets.iter().find(|t| t.tool_name == agent)
    }

    fn agent_enum(&self) -> Vec<&str> {
        self.targets.iter().map(|t| t.tool_name.as_str()).collect()
    }

    /// The routable names, for the prompt renderer and the tests.
    pub fn target_names(&self) -> Vec<&str> {
        self.agent_enum()
    }
}

fn build_description(targets: &[DelegateTarget]) -> String {
    let mut buf = String::from(
        "Hand a task to a specialist sub-agent. Set `agent` to one of the values below and pass \
         the task as `prompt`. Choose by what the task needs:",
    );
    for target in targets {
        buf.push_str("\n- `");
        buf.push_str(&target.tool_name);
        buf.push('`');
        let trimmed = target.description.trim();
        if !trimmed.is_empty() {
            buf.push_str(": ");
            buf.push_str(trimmed);
        }
    }
    buf
}

#[async_trait]
impl Tool for DelegateTool {
    fn name(&self) -> &str {
        DELEGATE_TOOL_NAME
    }

    fn description(&self) -> &str {
        &self.description
    }

    /// The envelope, emitted **once**, plus the `agent` selector.
    ///
    /// The properties come from `delegation_envelope_properties` rather than a
    /// second literal: two copies of this object would be two places for the
    /// collapsed tool and its hidden members to disagree about what a hand-off
    /// carries, and `render_structured_handoff` reads the property names
    /// directly. One definition, both callers.
    fn parameters_schema(&self) -> Value {
        let mut schema = json!({
            "type": "object",
            "required": ["agent", "prompt"],
            "properties": {
                "agent": {
                    "type": "string",
                    "enum": self.agent_enum(),
                    "description": "Which specialist to hand this to."
                }
            }
        });
        let properties = schema["properties"]
            .as_object_mut()
            .expect("properties is an object literal above");
        if let Value::Object(envelope) = delegation_envelope_properties() {
            for (key, value) in envelope {
                properties.insert(key, value);
            }
        }
        schema
    }

    fn permission_level(&self) -> PermissionLevel {
        // Every member declares `Execute`, so there is no per-action variation
        // to resolve here. If a target ever needs more, this must become an
        // args-aware lookup like `cron`'s — a single level would then be
        // laundering one target's risk down to another's.
        PermissionLevel::Execute
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    /// Unbounded, matching the member tools this replaces.
    ///
    /// Under the default `Inherit` policy the whole delegation is hard-killed
    /// at the single-tool timeout (120s), truncating any sub-agent run that
    /// legitimately takes longer — the Sentry regression (TAURI-RUST-K29,
    /// TAURI-RUST-8HB) that put `Unbounded` on `ArchetypeDelegationTool` in the
    /// first place. The child bounds its own lifetime through `max_iterations`,
    /// the run cancellation token and each inner tool's own timeout.
    fn timeout_policy(&self, _args: &Value) -> ToolTimeout {
        ToolTimeout::Unbounded
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.execute_with_context(args, ToolCallOptions::default(), None)
            .await
    }

    async fn execute_with_context(
        &self,
        args: Value,
        _options: ToolCallOptions,
        tool_context: Option<&dyn ToolRunContext>,
    ) -> anyhow::Result<ToolResult> {
        let requested = args.get("agent").and_then(Value::as_str).map(str::trim);
        let Some(target) = requested.and_then(|agent| self.resolve(agent)) else {
            return Ok(ToolResult::error(format!(
                "`agent` must be one of: {}. Got: {}",
                self.agent_enum().join(", "),
                requested.filter(|s| !s.is_empty()).unwrap_or("(missing)")
            )));
        };

        let raw_prompt = args
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if raw_prompt.is_empty() {
            return Ok(ToolResult::error(format!(
                "{DELEGATE_TOOL_NAME}: `prompt` is required"
            )));
        }
        let prompt = render_structured_handoff(&raw_prompt, &args);

        let model_override = args
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());

        // Async by default, exactly as the member tools were: the specialist
        // runs as a durable, resumable worker and its result arrives as a new
        // chat turn. `blocking: true` is the opt-in for a result that must gate
        // this reply.
        let blocking = args
            .get("blocking")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mode = if blocking {
            super::dispatch::DispatchMode::Blocking
        } else {
            super::dispatch::DispatchMode::PreferAsync
        };

        tracing::debug!(
            agent = %target.agent_id,
            via = %target.tool_name,
            "[delegate] dispatch"
        );
        // `target.tool_name`, not `DELEGATE_TOOL_NAME`: the dispatch name rides
        // into run records and the UI, and reporting every hand-off as
        // `delegate` would erase which specialist was chosen from every trace.
        super::dispatch_subagent(
            &target.agent_id,
            &target.tool_name,
            &prompt,
            None,
            model_override,
            tool_context,
            mode,
        )
        .await
    }
}

#[cfg(test)]
#[path = "collapsed_delegation_tests.rs"]
mod tests;
