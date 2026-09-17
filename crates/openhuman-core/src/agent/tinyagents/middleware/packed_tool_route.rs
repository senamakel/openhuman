//! [`PackedToolRouteMiddleware`]: route a call that names a withheld packed tool
//! by its bare name through `use_skill`, instead of answering "unknown tool".

use std::collections::HashSet;

use async_trait::async_trait;
use serde_json::{json, Value};

use tinyagents_harness::context::RunContext;
use tinyagents_harness::error::Result as TaResult;
use tinyagents_harness::middleware::Middleware;
use tinyinference::tool::ToolCall as TaToolCall;

use crate::tools::agent_policy::ToolPolicySession;
use crate::tools::toolpacks::{self, GroupMode, USE_SKILL};

/// `before_tool`: rewrite `name(args)` into
/// `use_skill { "skill": <pack>, "tool": name, "args": args }` when `name` is a
/// withheld packed tool this turn did not register (#6276).
///
/// A pack withholds a tool's schema, not its name. The pack listing documents
/// every tool under its bare name, and sibling descriptions and prompts refer to
/// it the same way, so a model calls that name as a top-level tool. The crate's
/// unknown-tool answer lists only the registered tools and never says the tool
/// exists behind a pack: live turns re-read the listing and repeated the bare
/// call until the turn ran out, and nothing was ever run.
///
/// **Only where the session says the tool may run.** Routing is gated on the
/// same predicate `ToolPolicyMiddleware` applies to `use_skill`'s inner tool and
/// the pack listing applies to what it shows:
/// `session.decision_for(name).blocks_execution()`. That decision is `Deny` for a
/// name the session does not have, so a tool outside this agent's registry, over
/// the channel ceiling, or refused by policy is never routed. The gate lives here
/// rather than relying on the gate downstream, so a blocked tool keeps the
/// crate's `unknown tool` answer instead of being rewritten at all.
///
/// **No session, no routing.** Sub-agent and channel/CLI turns carry no
/// tool-policy session: they bound their tools with the registration allowlist,
/// they never withhold packs, and nothing on those paths checks `use_skill`'s
/// inner tool. There an unregistered packed name was excluded by the allowlist,
/// so rewriting it into `use_skill` would walk around that boundary.
///
/// When it does route, the rewrite happens ahead of admission, so the call then
/// goes through what an explicit `use_skill` call goes through: schema
/// validation, the approval gate, the CLI-only gate, `ToolPolicyMiddleware`, and
/// permission forwarding. Routing rather than an error because the intent is
/// unambiguous (the name is in the compiled pack table), and an error would spend
/// a round trip telling the model to make this same call.
///
/// Left alone, so the crate's own recovery answers:
/// * a name no pack owns (a hallucinated tool),
/// * a registered name (a pack owner keeps its belt advertised),
/// * a turn without `use_skill`, where the tool has no route at all,
/// * a turn without a tool-policy session (sub-agent, channel/CLI),
/// * a tool the session blocks,
/// * a group that is not `Withheld` (`Off` is not registered anywhere),
/// * arguments that failed to parse or are not an object.
pub(crate) struct PackedToolRouteMiddleware {
    /// Tool names registered on this turn's harness.
    registered: HashSet<String>,
    /// This turn's tool-policy snapshot; `None` disables routing.
    session: Option<ToolPolicySession>,
}

impl PackedToolRouteMiddleware {
    /// Build the middleware over the names this turn registered and the turn's
    /// tool-policy session, if it has one.
    pub(crate) fn new(
        registered: impl IntoIterator<Item = String>,
        session: Option<ToolPolicySession>,
    ) -> Self {
        Self {
            registered: registered.into_iter().collect(),
            session,
        }
    }

    /// Rewrite `call` in place when it should be routed; `true` if it was.
    fn route(&self, call: &mut TaToolCall) -> bool {
        if call.invalid.is_some()
            || self.registered.contains(&call.name)
            || !self.registered.contains(USE_SKILL)
        {
            return false;
        }
        let Some(session) = &self.session else {
            return false;
        };
        if session.decision_for(&call.name).blocks_execution() {
            return false;
        }
        let Some(pack) = toolpacks::pack_for_tool(&call.name) else {
            return false;
        };
        if toolpacks::groups::current().mode_for_tool(&call.name) != GroupMode::Withheld {
            return false;
        }
        let args = match call.arguments {
            Value::Object(_) => call.arguments.take(),
            Value::Null => json!({}),
            _ => return false,
        };
        let tool = std::mem::replace(&mut call.name, USE_SKILL.to_string());
        tracing::info!(
            tool = %tool,
            skill = pack.id,
            call_id = %call.id,
            "[toolpacks] routed bare packed tool call through use_skill"
        );
        call.arguments = json!({ "skill": pack.id, "tool": tool, "args": args });
        true
    }
}

#[async_trait]
impl Middleware<()> for PackedToolRouteMiddleware {
    fn name(&self) -> &str {
        "packed_tool_route"
    }

    async fn before_tool(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        call: &mut TaToolCall,
    ) -> TaResult<()> {
        self.route(call);
        Ok(())
    }
}

#[cfg(test)]
#[path = "packed_tool_route_tests.rs"]
mod tests;
