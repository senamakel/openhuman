use super::*;

impl CoreContext {
    pub fn current() -> Option<Arc<CoreContext>> {
        CURRENT_CONTEXT
            .try_with(|ctx| ctx.clone())
            .ok()
            .or_else(|| DEFAULT_CONTEXT.get().cloned())
    }

    /// The context scoped onto the current task, with no fallback to the
    /// process default. SaaS uses it: there, a task that lost its scope must
    /// fail rather than act as the operator.
    pub fn scoped() -> Option<Arc<CoreContext>> {
        CURRENT_CONTEXT.try_with(|ctx| ctx.clone()).ok()
    }

    /// The process default context (first built), independent of any active
    /// scope. Used by the dispatch chokepoint to establish the ambient scope.
    pub fn default_context() -> Option<Arc<CoreContext>> {
        DEFAULT_CONTEXT.get().cloned()
    }

    /// Typed origin bound to this immutable per-turn context, if supplied by
    /// its entrypoint. Missing authority retains library/CLI behavior.
    pub fn current_turn_origin() -> Option<crate::agent::turn_origin::AgentTurnOrigin> {
        Self::current().and_then(|ctx| ctx.turn_origin.clone())
    }

    /// Run a turn under a derived immutable context carrying its explicit
    /// entrypoint authority. The shared parent and other concurrent turns are
    /// untouched.
    pub async fn scope_with_turn_origin<F: Future>(
        ctx: Arc<CoreContext>,
        origin: Option<crate::agent::turn_origin::AgentTurnOrigin>,
        fut: F,
    ) -> F::Output {
        let context = Arc::new(CoreContext {
            host_kind: ctx.host_kind,
            workspace_binding: RwLock::new(
                ctx.workspace_binding
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone(),
            ),
            domains: ctx.domains,
            tool_groups: ctx.tool_groups.clone(),
            embedder_config: ctx.embedder_config.clone(),
            user_skill_roots: ctx.user_skill_roots,
            backend_transport: ctx.backend_transport.clone(),
            turn_origin: origin.or_else(|| ctx.turn_origin.clone()),
            session_agent: ctx.session_agent.clone(),
            profile: ctx.profile.clone(),
            agent: ctx.agent.clone(),
        });
        CURRENT_CONTEXT.scope(context, fut).await
    }
}
