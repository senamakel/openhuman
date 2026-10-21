use super::tier::escalated_origin_for_nested_harness;
use super::*;
use std::borrow::Cow;

impl OpenHumanAgentRunner {
    /// Full harness turn: build a real session agent for `agent_ref` and drive
    /// one `run_single` under the node's model override + timeout. See
    /// [`OpenHumanAgentRunner`] for the security/origin contract.
    ///
    /// `entry_model` is the custom `AgentRegistryEntry`'s own `model` pin (a
    /// `hint:<role>` or raw BYOK model id), when `agent_ref` resolved to one —
    /// `None` for a shipped/TOML harness definition, which has no such entry.
    /// It is the fallback below the node's own `config.model` override (see
    /// `resolve_node_model`), matching the precedence
    /// `run_via_registry_fallback` already gave `entry.model` — without this,
    /// a custom flow agent's model pin (e.g. `hint:reasoning`) silently
    /// dropped to the default chat model once routed through the harness.
    ///
    /// **Synchronous only (B40 / Gap 4).** A flow `agent` node runs here with
    /// no explicit chat thread on its run context. If the
    /// agent it runs is a delegating agent (e.g. the orchestrator) and
    /// calls `spawn_async_subagent` directly, the tool now refuses (see the
    /// `parent_thread_id.is_none()` guard in
    /// `agent_orchestration::tools::spawn_async_subagent`) rather than
    /// silently discarding the background result once it finishes —
    /// `background_delivery` has nowhere to deliver it without a thread id.
    /// This module does not bridge async subagent delivery into flow runs.
    /// For work that needs to happen in parallel, model it as parallel flow
    /// nodes (the tinyflows engine already fans those out) instead of
    /// reaching for a background sub-agent from inside a single node.
    pub(super) async fn run_via_harness(
        &self,
        dispatch: &crate::core::runtime::DispatchCtx,
        agent_ref: &str,
        request: Value,
        conn: Option<&str>,
        entry_model: Option<&str>,
        host: Option<crate::agent::host_agents::HostAgent>,
    ) -> Result<Value> {
        use crate::agent::OpenHumanSessionHost;

        // Hold a slot for the whole turn: a fanned-out node can call this
        // hundreds of times at once, and each call below builds a full agent
        // session. Acquired before any work so waiters queue rather than pile
        // up half-built agents. `_permit` is released on drop at end of scope,
        // including on every early return below.
        let _permit = HARNESS_AGENT_SLOTS.acquire().await.map_err(|_| {
            EngineError::Capability(
                "agent node: harness concurrency limiter closed unexpectedly".to_string(),
            )
        })?;

        if let Some(c) = conn {
            tracing::debug!(
                target: "flows",
                conn = %c,
                "[flows] agent_runner: connection_ref present but not resolved to a BYOK account \
                 for the harness turn (matches OpenHumanLlm)"
            );
        }

        // Model precedence for a harness node: node `config.model` > the
        // custom registry entry's own `model` pin (if any) > the definition's
        // own default.
        let initial = self
            .prepare_agent_run(request.clone(), entry_model, None, 0)
            .await?;
        let node_model = initial.model.clone();

        // Apply the override the cron way (`run_agent_job`): a cloned `Config`
        // with a new `default_model`, so we never mutate the shared config or
        // invent a new Agent setter API. The tier is normalised to the
        // `hint:<role>` form the session builder routes on.
        // A host-registered agent starts from its own config (its provider
        // model and route), not the flow runner's.
        let base: &Config = host
            .as_ref()
            .map_or(self.config.as_ref(), |host| &host.config);
        let effective: Cow<'_, Config> = match node_model.as_deref() {
            Some(model) => {
                let mut config = base.clone();
                config.default_model = Some(harness_model_default_override(model));
                Cow::Owned(config)
            }
            None => Cow::Borrowed(base),
        };

        let built = match &host {
            Some(host) => host.session_host(effective.as_ref(), None),
            None => {
                OpenHumanSessionHost::from_config_for_agent(dispatch, effective.as_ref(), agent_ref)
            }
        };
        let mut agent = built.map_err(|e| {
            EngineError::Capability(format!(
                "agent node: failed to build harness agent '{agent_ref}': {e:#}"
            ))
        })?;
        agent.set_agent_definition_name(agent_ref.to_string());
        agent.set_on_progress(self.progress.clone());

        // Issue #4868 — the session builder now stamps `agent_ref`'s own
        // `effective_max_iterations()` onto the agent (instead of the global
        // default of 10), so `code_executor`/`tools_agent`/etc. can run up to
        // 50 iterations here. Read the cap actually applied to `agent`
        // (reflects the definition cap or the global fallback, whichever the
        // builder resolved) and scale the DEFAULT timeout accordingly — see
        // `scale_timeout_for_iteration_cap`.
        //
        // Post-merge Codex P2 finding: an EXPLICIT `timeout_secs` the node
        // config supplied is a caller-chosen bound (e.g. a fast-fail/SLA of
        // 120s) and must be honored as-is, never scaled up just because the
        // agent's iteration cap is high — see `resolve_run_timeout_secs`.
        let effective_iteration_cap = agent.agent_config().max_tool_iterations;
        let prepared = self
            .prepare_agent_run(
                request.clone(),
                entry_model,
                None,
                u32::try_from(effective_iteration_cap).unwrap_or(u32::MAX),
            )
            .await?;
        let prompt = prepared.prompt;
        let base_timeout_secs = prepared.base_timeout_secs;
        let timeout_secs = prepared.timeout_secs;

        tracing::debug!(
            target: "flows",
            agent_ref,
            node_model = node_model.as_deref().unwrap_or("<definition-default>"),
            default_model = effective.default_model.as_deref().unwrap_or("<config-default>"),
            effective_iteration_cap,
            base_timeout_secs,
            timeout_secs,
            prompt_len = prompt.len(),
            "[flows] agent_runner: dispatching full harness turn"
        );

        // Nested-harness HITL escalation (issue #4595): the engine future runs
        // under the flow's Workflow origin, but the flow author only pre-
        // declared `agent_ref` — not the concrete tools the harness LLM will
        // pick from the definition's `ToolScope`. If we let the inner turn
        // inherit a `Workflow { require_approval: false }` origin,
        // `ApprovalGate::intercept_audited` treats it as a trust root and
        // auto-`Allow`s external_effect tools (see
        // `crates/openhuman-core/src/security/approval/gate.rs` `Workflow { require_approval: false }`
        // branch), which would let a scheduled / app-event flow reach out to
        // Slack / email / desktop control with no HITL. We force
        // `require_approval: true` around `run_single` so external_effect tools
        // park for a real decision the same way flow acting nodes escalated by
        // [`gate_call_for_tier`] do. Read-only tools (no `external_effect`)
        // aren't gated by `intercept_audited` at all, so this doesn't add noise
        // for pure-read nested agents.
        //
        // Cancellation: the run_registry token aborts the engine future, and the
        // inner turn drops with it (task-local scope unwinds cleanly).
        use crate::security::turn_origin;
        let escalated_origin = escalated_origin_for_nested_harness(self.ctx.turn_origin());
        if let Some(ref escalated) = escalated_origin {
            tracing::debug!(
                target: "flows",
                agent_ref,
                origin = ?escalated,
                "[flows] agent_runner: escalating nested harness turn to Workflow{{require_approval:true}} \
                 so external_effect tools park for HITL (issue #4595)"
            );
        }
        let run: std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>,
        > = if let Some(escalated) = escalated_origin {
            Box::pin(turn_origin::with_origin(
                escalated,
                agent.run_single_with_approval(&prompt, None, self.approval.clone()),
            ))
        } else {
            Box::pin(agent.run_single_with_approval(&prompt, None, self.approval.clone()))
        };
        // A host agent's turn reads its own context (config, domains, tool
        // groups, session store); the origin task-local is untouched.
        let run: std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>,
        > = match host
            .as_ref()
            .map(|host| std::sync::Arc::clone(&host.context))
        {
            Some(context) => Box::pin(crate::core::runtime::CoreContext::scope(context, run)),
            None => run,
        };
        let final_text =
            match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), run).await {
                Ok(Ok(text)) => text,
                Ok(Err(e)) => {
                    tracing::warn!(
                        target: "flows",
                        agent_ref,
                        error = %e,
                        "[flows] agent_runner: harness turn failed"
                    );
                    return Err(EngineError::Capability(format!(
                        "agent node '{agent_ref}' turn failed: {e:#}"
                    )));
                }
                Err(_) => {
                    tracing::warn!(
                        target: "flows",
                        agent_ref,
                        timeout_secs,
                        "[flows] agent_runner: harness turn timed out"
                    );
                    return Err(EngineError::Capability(format!(
                        "agent node '{agent_ref}' timed out after {timeout_secs}s"
                    )));
                }
            };

        self.format_agent_result(agent_ref, request, final_text)
            .await
    }
}
