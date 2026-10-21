//! Selected-configuration memory checks.

use std::sync::Arc;

use async_trait::async_trait;

use crate::doctor::contract::{ComponentId, DoctorCheck, DoctorCtx, DoctorTier, Finding};

pub(super) fn checks() -> Vec<Arc<dyn DoctorCheck>> {
    vec![Arc::new(BindingCheck), Arc::new(HealthCheck)]
}

struct BindingCheck;

#[async_trait]
impl DoctorCheck for BindingCheck {
    fn id(&self) -> &str {
        "memory/binding"
    }

    fn category(&self) -> &str {
        "memory"
    }

    fn tier(&self) -> DoctorTier {
        DoctorTier::Static
    }

    fn component_gate(&self) -> Option<ComponentId> {
        Some(ComponentId::Memory)
    }

    async fn detect(&self, ctx: &DoctorCtx) -> Vec<Finding> {
        let config = ctx.config();
        if !config.components.memory {
            return vec![Finding::skipped(
                self.id(),
                "Memory is disabled in the selected configuration.",
                "Enable memory in the selected configuration to inspect its binding.",
            )];
        }
        let engine = config.memory.engine.trim();
        if engine.is_empty() {
            return vec![Finding::error(
                self.id(),
                "No memory engine is selected.",
                "Select an available memory engine and rerun doctor.",
            )];
        }
        if engine.eq_ignore_ascii_case(crate::memory::engine::DISABLED_ENGINE) {
            return vec![Finding::skipped(
                self.id(),
                "Memory is turned off in the selected configuration.",
                "Select a memory engine to enable memory.",
            )];
        }
        let known = matches!(
            engine,
            crate::memory::engine::TINYHUMANS_ENGINE | crate::memory::engine::CORTEXDB_ENGINE
        ) || crate::memory::engine_registry::lookup(engine).is_some();
        if !known {
            return vec![Finding::error(
                self.id(),
                "The selected memory engine is not available in this build.",
                "Select a registered memory engine and rerun doctor.",
            )];
        }
        vec![Finding::pass(
            self.id(),
            "A memory engine is selected for the effective configuration.",
        )]
    }
}

struct HealthCheck;

#[async_trait]
impl DoctorCheck for HealthCheck {
    fn id(&self) -> &str {
        "memory/health"
    }

    fn category(&self) -> &str {
        "memory"
    }

    fn tier(&self) -> DoctorTier {
        DoctorTier::Connect
    }

    fn component_gate(&self) -> Option<ComponentId> {
        Some(ComponentId::Memory)
    }

    async fn detect(&self, ctx: &DoctorCtx) -> Vec<Finding> {
        let memory = if let Some(memory) = ctx.memory_engine() {
            // The transitional v1 adapter supplies this only for the legacy
            // `memory/engine` selector. Native checks resolve their own
            // selected-config probe below.
            memory.clone()
        } else {
            probe_selected_config(ctx.config()).await
        };
        match (memory.status.as_str(), memory.reason.as_deref()) {
            ("ok" | "ready", _) => vec![Finding::pass(
                self.id(),
                "The selected memory engine is healthy.",
            )],
            ("degraded", Some(reason)) => vec![Finding::new(
                self.id(),
                crate::doctor::FindingSeverity::Warn,
                safe_reason(reason),
                "Check the memory engine configuration and connectivity.",
            )],
            (_, Some(reason)) => vec![Finding::error(
                self.id(),
                safe_reason(reason),
                "Check the memory engine configuration and connectivity.",
            )],
            _ => vec![Finding::error(
                self.id(),
                "The selected memory engine is unavailable.",
                "Check the memory engine configuration and connectivity.",
            )],
        }
    }
}

fn safe_reason(reason: &str) -> String {
    let lower = reason.to_ascii_lowercase();
    if reason.len() <= 160
        && !reason.contains("://")
        && !lower.contains("token")
        && !lower.contains("bearer")
        && !lower.contains("authorization")
        && !lower.contains("password")
        && !lower.contains("api_key")
        && reason.chars().all(|ch| !ch.is_control())
    {
        reason.to_owned()
    } else {
        "The selected memory engine reported that it is unavailable.".to_owned()
    }
}

async fn probe_selected_config(config: &crate::config::Config) -> crate::doctor::core::MemoryEngineCheck {
    use tinymemory_api::EngineHealth;

    let engine_id = config.memory.engine.trim();
    #[cfg(test)]
    if let Some(health) = crate::memory::engine::doctor_test_health(config).await {
        return memory_check(engine_id, health);
    }
    let health = match engine_id {
        crate::memory::engine::TINYHUMANS_ENGINE => {
            let Some(probe) = crate::memory::engine_registry::read_only_health_probe(engine_id)
            else {
                return unavailable(engine_id, "No read-only health probe is registered for this engine.");
            };
            probe(config.clone()).await
        }
        crate::memory::engine::CORTEXDB_ENGINE => cortexdb_health(config).await,
        crate::memory::engine::DISABLED_ENGINE => {
            return unavailable(engine_id, "Memory is disabled in the selected configuration.");
        }
        other => {
            let Some(probe) = crate::memory::engine_registry::read_only_health_probe(other) else {
                return unavailable(other, "No read-only health probe is registered for this engine.");
            };
            probe(config.clone()).await
        }
    };
    memory_check(engine_id, health)
}

fn memory_check(
    engine_id: &str,
    health: tinymemory_api::EngineHealth,
) -> crate::doctor::core::MemoryEngineCheck {
    use tinymemory_api::EngineHealth;
    let (status, reason) = match health {
        EngineHealth::Ok => ("ok", None),
        EngineHealth::Degraded(reason) => ("degraded", Some(reason)),
        EngineHealth::Down(reason) => ("down", Some(reason)),
    };
    crate::doctor::core::MemoryEngineCheck {
        engine: Some(engine_id.to_owned()),
        status: status.to_owned(),
        reason,
    }
}

async fn cortexdb_health(config: &crate::config::Config) -> tinymemory_api::EngineHealth {
    use crate::security::credentials::{AuthService, DEFAULT_AUTH_PROFILE_NAME};
    use tinymemory_api::{EngineHealth, MemoryEngine};
    use tinymemory_integrations::{EngineCredential, EngineSettings};

    let profile = match AuthService::get_profile_read_only(
        config,
        crate::config::schema::MEMORY_CORTEXDB_KEY_NAME,
        None,
    ) {
        Ok(Some(profile)) => profile,
        Ok(None) => return EngineHealth::Down("CortexDB API key is not configured.".to_owned()),
        Err(_) => return EngineHealth::Down("CortexDB credentials could not be read.".to_owned()),
    };
    let key = match profile.kind {
        crate::security::credentials::profiles::AuthProfileKind::Token => profile.token,
        crate::security::credentials::profiles::AuthProfileKind::OAuth => {
            profile.token_set.map(|tokens| tokens.access_token)
        }
    };
    let Some(key) = key.filter(|key| !key.trim().is_empty()) else {
        return EngineHealth::Down("CortexDB API key is not configured.".to_owned());
    };
    let endpoint = config
        .memory
        .endpoint_for(crate::memory::engine::CORTEXDB_ENGINE)
        .unwrap_or_else(|| tinymemory_integrations::cortex::CORTEX_API_ENDPOINT.to_owned());
    let settings = EngineSettings {
        endpoint: Some(endpoint),
        observed_actor: config.memory.observed_actor,
        ..EngineSettings::default()
    };
    match tinymemory_integrations::build_engine(
        crate::memory::engine::CORTEXDB_ENGINE,
        &settings,
        EngineCredential::Static(key),
    ) {
        Ok(engine) => engine.health().await,
        Err(_) => EngineHealth::Down("CortexDB engine configuration is invalid.".to_owned()),
    }
}

fn unavailable(engine: &str, reason: &str) -> crate::doctor::core::MemoryEngineCheck {
    crate::doctor::core::MemoryEngineCheck {
        engine: Some(engine.to_owned()),
        status: "down".to_owned(),
        reason: Some(reason.to_owned()),
    }
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
