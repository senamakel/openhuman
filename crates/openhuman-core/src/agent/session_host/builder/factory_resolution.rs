//! Agent-definition fallback resolution and provider-role selection for session construction.

/// Resolve the provider/workload role for a session build.
///
/// Explicit `hint:<role>` markers route to their workload; everything else
/// (incl. the legacy `default_model` tier the bootstrap pinned) falls through
/// to `chat` so `chat_provider` drives the user-facing turn.
pub(crate) fn provider_role_for(default_model: Option<&str>) -> &'static str {
    match default_model.map(str::trim) {
        Some("hint:agentic") => "agentic",
        Some("hint:coding") => "coding",
        Some("hint:summarization") => "summarization",
        Some("hint:reasoning") => "reasoning",
        _ => "chat",
    }
}

/// Resolve the initial harness workload without constructing a session.
/// Flow readiness shares this path so it checks the provider used at runtime.
pub(crate) fn provider_role_for_definition(
    default_model: Option<&str>,
    target_def: Option<&crate::agent_registry::host_definition::AgentDefinition>,
) -> &'static str {
    let master_hint = default_model
        .is_none()
        .then(|| {
            target_def.and_then(|def| {
                if def.id == "orchestrator" {
                    match &def.model {
                        tinyagents_definition::ModelSpec::Hint(hint) => {
                            Some(format!("hint:{hint}"))
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            })
        })
        .flatten();
    provider_role_for(master_hint.as_deref().or(default_model))
}

#[cfg(test)]
#[path = "factory_resolution_tests.rs"]
mod tests;
