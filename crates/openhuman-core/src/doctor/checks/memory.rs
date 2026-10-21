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
        let Some(memory) = ctx.memory_engine() else {
            return vec![Finding::skipped(
                self.id(),
                "No read-only memory health probe is available for the selected engine.",
                "Run doctor from a host that provides a read-only memory health probe.",
            )];
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
    // Engine health strings can include URLs or credential-bearing details.
    // Only return a bounded classification unless a later typed health reason
    // explicitly permits safe text.
    let _ = reason;
    "The selected memory engine reported that it is unavailable.".to_owned()
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
