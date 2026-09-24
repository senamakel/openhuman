//! Typed dispatch for `use_skill` (regression R3).
//!
//! `use_skill` is registered as a plain canonical tool, so a call reaching a
//! packed archetype delegation (`create_image`, `do_crypto`,
//! `make_presentation`, …) through it used to run through
//! `tinytools::Tool::execute_with_context`, which has no live parent
//! `RunContext`. `dispatch_subagent_with_live_parent` refuses to run without
//! one — "delegation requires a live harness run context." — so every packed
//! delegate was reachable only when a model happened to call it under its
//! bare name, and `PackedToolRouteMiddleware` actively rewrites bare packed
//! calls INTO `use_skill`, making the bug unconditional for any tool this
//! build packs.
//!
//! [`UseSkillDispatch`] closes the gap: it resolves the inner tool exactly as
//! [`crate::tools::toolpacks::tools::UseSkillTool::execute_with_context`]
//! does, then re-selects the same
//! [`super::harness_tool_registration::typed_dispatch_for`] the harness would
//! have picked had the inner tool been natively advertised, and hands it the
//! REAL parent this dispatch itself received. The disclosure half (no `tool`
//! named), a missing `skill`, and a not-found tool are all delegated verbatim
//! to the wrapped `use_skill` adapter — those paths render `UseSkillTool`'s
//! existing schema listing / error text and need no live parent, so
//! duplicating that logic here would only create a second place for it to
//! drift.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tinyagents_harness::context::RunContext;
use tinyagents_harness::tool::{ToolDispatch, ToolExecutionContext};
use tinytools::{Tool, ToolCallOptions, ToolResult};

use super::harness_tool_registration::typed_dispatch_for;
use super::host::OpenHumanRunContext;
use crate::tools::toolpacks::{named_tool, PackRegistryHandle};

/// Live-parent dispatch for the `use_skill` proxy tool.
///
/// `tool` is the `CanonicalSharedToolAdapter` the harness registered for
/// `use_skill` itself (used for the disclosure/error fallback paths and for
/// `tool()`); `handle` is the same pack-registry handle `use_skill`'s own
/// tool object carries, read off its erased host extension at registration
/// time.
pub(crate) struct UseSkillDispatch {
    tool: Arc<dyn Tool>,
    handle: PackRegistryHandle,
}

impl UseSkillDispatch {
    pub(crate) fn new(tool: Arc<dyn Tool>, handle: PackRegistryHandle) -> Self {
        Self { tool, handle }
    }
}

#[async_trait]
impl ToolDispatch<(), OpenHumanRunContext> for UseSkillDispatch {
    fn tool(&self) -> Arc<dyn Tool> {
        self.tool.clone()
    }

    async fn execute(
        &self,
        _state: &(),
        call_id: tinyagents_harness::CallId,
        arguments: Value,
        options: ToolCallOptions,
        parent: &RunContext<OpenHumanRunContext>,
    ) -> anyhow::Result<ToolResult> {
        // Resolve `skill` + `tool` against the pack registry exactly as
        // `UseSkillTool::execute_with_context` does. Anything that does not
        // resolve here — no `skill`, the disclosure half (no `tool` named),
        // or a name the pack does not own — is a path that tool already
        // renders correctly and that touches no live parent, so fall through
        // to it verbatim rather than re-deriving the same schema listing or
        // not-found message.
        let resolved = arguments
            .get("skill")
            .and_then(Value::as_str)
            .zip(named_tool(&arguments))
            .and_then(|(skill, name)| {
                self.handle
                    .resolve_registry_for(skill, name)
                    .map(|tools| (name.to_string(), tools))
            });

        let Some((name, tools)) = resolved else {
            return self
                .tool
                .execute_with_context(arguments, options, None)
                .await;
        };

        let inner_args = arguments
            .get("args")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));

        // Re-wrap the resolved tool in the same `CanonicalSharedToolAdapter`
        // seam the harness itself builds at registration: typed-dispatch
        // selection keys off the tool's name and schema
        // (`DelegationDispatch::for_tool`), not object identity, so this
        // adapter is equivalent to the one that would have been registered
        // had the model reached `name` directly instead of through
        // `use_skill`.
        let Some(inner_adapter) =
            super::tools::CanonicalSharedToolAdapter::for_name(vec![tools], &name)
                .map(|adapter| Arc::new(adapter) as Arc<dyn Tool>)
        else {
            return self
                .tool
                .execute_with_context(arguments, options, None)
                .await;
        };

        if let Some(dispatch) = typed_dispatch_for(&name, inner_adapter.clone()) {
            return dispatch
                .execute(&(), call_id, inner_args, options, parent)
                .await;
        }

        // Not a typed-dispatch tool: run it the way `use_skill` always has,
        // through `Tool::execute_with_context`, but still hand it a real
        // `ToolExecutionContext` built from the live parent rather than
        // `None` — a non-recursive packed tool that reads call id, thread id
        // or workspace off the erased host extension gets the same facts a
        // native registration would have given it.
        let tool_context = ToolExecutionContext::from_run_context(parent, call_id);
        inner_adapter
            .execute_with_context(inner_args, options, Some(&tool_context))
            .await
    }
}

#[cfg(test)]
#[path = "use_skill_dispatch_tests.rs"]
mod tests;
