//! [`ArgRecoveryMiddleware`]: repair malformed tool-call arguments before the
//! harness runs its fatal schema gate.

use std::sync::Arc;

use async_trait::async_trait;

use tinyagents_harness::context::RunContext;
use tinyagents_harness::error::Result as TaResult;
use tinyagents_harness::middleware::Middleware;
use tinyinference::tool::{ToolCall as TaToolCall, ToolSchema};

use crate::tools::Tool;

/// `before_tool`: repair a tool call's arguments *before* the harness runs its
/// fatal pre-execution schema gate (issues #4249 / #4451). A model can emit
/// arguments the model adapter parses to a non-object `Value` — invalid JSON
/// decodes to `Value::Null`, and some providers emit the whole arguments blob as
/// a JSON-encoded *string* (optionally wrapped in a ```json markdown fence). Left
/// alone the harness rejects those against an object schema and aborts the whole
/// turn.
///
/// Recovery, in order:
/// 1. Already a JSON object → leave it (the common, valid case).
/// 2. A JSON-encoded string (optionally fenced) that decodes to an object →
///    decode and use it.
/// 3. Otherwise a non-object whose tool schema declares **no** required fields →
///    coerce to `{}` (legacy-engine parity: the tool runs and produces its own
///    recoverable error).
/// 4. Otherwise (non-object + schema has required fields) → leave the arguments
///    untouched so the crate's `InvalidArgsPolicy::ReturnToolError` path reports
///    the original validation failure. Coercing to `{}` would discard the raw
///    malformed value without improving recovery.
pub(crate) struct ArgRecoveryMiddleware {
    /// The same `Arc`-shared tool sets the runner registers, used to resolve a
    /// call's schema so we can tell whether coercing to `{}` is safe.
    tool_sets: Vec<Arc<Vec<Box<dyn Tool>>>>,
}

impl ArgRecoveryMiddleware {
    /// Build the middleware over the runner's shared tool sets.
    pub(crate) fn new(tool_sets: Vec<Arc<Vec<Box<dyn Tool>>>>) -> Self {
        Self { tool_sets }
    }

    fn schema_for(&self, name: &str) -> Option<ToolSchema> {
        schema_for_tool(&self.tool_sets, name)
    }
}

#[async_trait]
impl Middleware<()> for ArgRecoveryMiddleware {
    fn name(&self) -> &str {
        "arg_recovery"
    }

    async fn before_tool(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        call: &mut TaToolCall,
    ) -> TaResult<()> {
        // (1) Already valid object shape — nothing to do.
        if call.arguments.is_object() {
            return Ok(());
        }

        // (2) JSON-encoded-string arguments (optionally markdown-fenced): decode
        // and adopt the inner object.
        if let Some(raw) = call.arguments.as_str() {
            if let Some(obj) = recover_object_from_json_string(raw) {
                tracing::debug!(
                    tool = call.name.as_str(),
                    "[tinyagents::mw] arg_recovery: decoded JSON-encoded-string tool arguments to object"
                );
                call.arguments = obj;
                return Ok(());
            }
        }

        // (3) Non-object with a permissive schema (no required fields): coerce to
        // `{}` so the tool runs and produces its own recoverable error — engine
        // parity for tools that predate the schema gate.
        let has_required = self
            .schema_for(&call.name)
            .map(|schema| schema_has_required_fields(&schema.parameters))
            .unwrap_or(false);
        if !has_required {
            tracing::debug!(
                tool = call.name.as_str(),
                "[tinyagents::mw] arg_recovery: coercing non-object tool arguments to {{}} (schema declares no required fields)"
            );
            call.arguments = serde_json::json!({});
            return Ok(());
        }

        // (4) Non-object + schema has required fields: leave untouched. The
        // crate admission policy surfaces a descriptive, recoverable tool
        // result without executing the real tool.
        tracing::debug!(
            tool = call.name.as_str(),
            args_kind = json_value_kind(&call.arguments),
            "[tinyagents::mw] arg_recovery: leaving non-object tool arguments for crate invalid-args recovery"
        );
        Ok(())
    }
}

/// Resolves the harness [`ToolSchema`] for `name` across the runner's shared
/// tool sets.
///
/// Built via the same [`spec_to_schema`](crate::agent::tinyagents::convert::spec_to_schema)
/// conversion the runner uses for [`SharedToolAdapter::schema`], so the
/// `parameters` we validate against are byte-identical to the ones the crate's
/// fatal `validate_call` gate checks — otherwise our pre-validation could
/// disagree with the crate and either miss a fatal case or stub a call the crate
/// still rejects.
fn schema_for_tool(tool_sets: &[Arc<Vec<Box<dyn Tool>>>], name: &str) -> Option<ToolSchema> {
    tool_sets
        .iter()
        .flat_map(|set| set.iter())
        .find(|tool| tool.name() == name)
        .map(|tool| crate::agent::tinyagents::convert::spec_to_schema(&tool.spec()))
}

/// Whether a tool's JSON-schema `parameters` declares any `required` field.
fn schema_has_required_fields(parameters: &serde_json::Value) -> bool {
    parameters
        .get("required")
        .and_then(serde_json::Value::as_array)
        .map(|required| required.iter().any(serde_json::Value::is_string))
        .unwrap_or(false)
}

/// Attempts to recover a JSON **object** from a string-encoded arguments payload:
/// providers sometimes emit the whole arguments blob as a JSON string, optionally
/// wrapped in a ```json markdown fence. Returns `None` when the string does not
/// decode to a JSON object.
fn recover_object_from_json_string(raw: &str) -> Option<serde_json::Value> {
    let candidate = strip_code_fence(raw);
    serde_json::from_str::<serde_json::Value>(candidate)
        .ok()
        .filter(serde_json::Value::is_object)
}

/// Strips a surrounding markdown code fence (```` ```json … ``` ````) and its
/// optional language tag, returning the inner text. A string with no fence is
/// returned trimmed and unchanged.
fn strip_code_fence(raw: &str) -> &str {
    let trimmed = raw.trim();
    let Some(after_open) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    // Drop an optional language tag on the opening fence line (e.g. `json`).
    let body = match after_open.find('\n') {
        Some(newline)
            if after_open[..newline]
                .chars()
                .all(|c| c.is_ascii_alphanumeric()) =>
        {
            &after_open[newline + 1..]
        }
        _ => after_open,
    };
    body.trim().strip_suffix("```").unwrap_or(body).trim()
}

/// A short, human-readable kind label for a JSON value, for debug logging.
fn json_value_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}
