//! Temporary adapters that keep the moved v1 checks available until #506.

use std::sync::Arc;

use async_trait::async_trait;

use super::contract::{ComponentId, DoctorCheck, DoctorCtx, DoctorTier, Finding, FindingSeverity};
use super::core::{run_check, MemoryEngineCheck, Severity};
use super::registry::DoctorCheckSource;

/// Source for the checks that still use the moved v1 implementation.
pub(crate) struct LegacyV1CheckSource;

impl DoctorCheckSource for LegacyV1CheckSource {
    fn id(&self) -> &str {
        "doctor-v1-adapter"
    }

    fn checks(&self) -> Vec<Arc<dyn DoctorCheck>> {
        LegacyV1Check::builtins()
    }
}

pub(crate) struct LegacyV1Check {
    id: &'static str,
    category: &'static str,
    tier: DoctorTier,
    component_gate: Option<ComponentId>,
}

impl LegacyV1Check {
    fn adapter(
        id: &'static str,
        category: &'static str,
        tier: DoctorTier,
        component_gate: Option<ComponentId>,
    ) -> Arc<dyn DoctorCheck> {
        Arc::new(Self {
            id,
            category,
            tier,
            component_gate,
        })
    }

    pub(crate) fn builtins() -> Vec<Arc<dyn DoctorCheck>> {
        vec![
            Self::adapter(
                "config/semantics",
                "configuration",
                DoctorTier::Static,
                None,
            ),
            Self::adapter("workspace/layout", "workspace", DoctorTier::Connect, None),
            Self::adapter("runtime/daemon", "daemon", DoctorTier::Static, None),
            Self::adapter(
                "runtime/environment",
                "environment",
                DoctorTier::Static,
                None,
            ),
            Self::adapter(
                "memory/engine",
                "memory engine",
                DoctorTier::Connect,
                Some(ComponentId::Memory),
            ),
            Self::adapter(
                "memory/embedding-model",
                "embedding model",
                DoctorTier::Connect,
                Some(ComponentId::Memory),
            ),
            Self::adapter("agent/claude-sdk", "agent SDK", DoctorTier::Static, None),
        ]
    }
}

#[async_trait]
impl DoctorCheck for LegacyV1Check {
    fn id(&self) -> &str {
        self.id
    }

    fn category(&self) -> &str {
        self.category
    }

    fn tier(&self) -> DoctorTier {
        self.tier
    }

    fn component_gate(&self) -> Option<ComponentId> {
        self.component_gate
    }

    fn is_blocking(&self) -> bool {
        true
    }

    async fn detect(&self, _ctx: &DoctorCtx) -> Vec<Finding> {
        Vec::new()
    }

    fn detect_blocking(&self, ctx: &DoctorCtx) -> Option<Vec<Finding>> {
        let memory = ctx
            .memory_engine()
            .cloned()
            .unwrap_or_else(|| MemoryEngineCheck {
                engine: None,
                status: "off".into(),
                reason: Some("memory status was not supplied to the legacy adapter".into()),
            });
        Some(
            run_check(self.id, ctx.config(), &memory)
                .into_iter()
                .map(|item| {
                    let severity = if item.severity == Severity::Ok {
                        FindingSeverity::Pass
                    } else if item.severity == Severity::Warn {
                        FindingSeverity::Warn
                    } else if item.severity == Severity::Error {
                        FindingSeverity::Error
                    } else {
                        // Allows a legacy check to signal `Skipped` without
                        // binding the framework to a v1 enum variant.
                        FindingSeverity::Skipped
                    };
                    let message = match severity {
                        FindingSeverity::Pass => {
                            format!("The legacy {} check passed.", self.category)
                        }
                        FindingSeverity::Info => {
                            format!("The legacy {} check completed.", self.category)
                        }
                        FindingSeverity::Warn => {
                            format!("The legacy {} check reported a warning.", self.category)
                        }
                        FindingSeverity::Error => {
                            format!("The legacy {} check reported an error.", self.category)
                        }
                        FindingSeverity::Skipped => {
                            format!("The legacy {} check was skipped.", self.category)
                        }
                    };
                    let fix_hint = match severity {
                        FindingSeverity::Pass => "No action required.".to_owned(),
                        FindingSeverity::Skipped => {
                            "No action is needed for this check to be skipped.".to_owned()
                        }
                        _ => format!(
                            "Review the {} configuration or runtime and rerun doctor.",
                            self.category
                        ),
                    };
                    Finding::new(self.id, severity, message, fix_hint)
                })
                .collect(),
        )
    }
}
