//! Host-installed, per-run recovery evaluator factory. Core has no wire client.
use crate::config::Config;
use std::sync::{Arc, OnceLock, RwLock};
pub use tinytools_jev::recovery::{
    RecoveryAnswer, RecoveryDecision, RecoveryEffect, RecoveryEvaluator, RecoveryObservation,
    RecoveryPhase, RecoveryQuestion, RecoveryRequest,
};

/// Resolves an evaluator against an immutable turn configuration and freezes
/// its endpoint and credential for that run. No default provider is installed.
pub type RecoveryProviderFactory =
    Arc<dyn Fn(&Config) -> Option<Arc<dyn RecoveryEvaluator>> + Send + Sync>;
static PROVIDER: OnceLock<RwLock<Option<RecoveryProviderFactory>>> = OnceLock::new();
fn slot() -> &'static RwLock<Option<RecoveryProviderFactory>> {
    PROVIDER.get_or_init(|| RwLock::new(None))
}
/// Snapshot the process factory without creating an evaluator.
pub fn installed_recovery_provider() -> Option<RecoveryProviderFactory> {
    slot().read().unwrap_or_else(|e| e.into_inner()).clone()
}
/// Replace the process factory. RuntimeBuilder installations restore the previous
/// factory on drop; direct installations persist until replaced or cleared.
pub fn install_recovery_provider(provider: RecoveryProviderFactory) {
    *slot().write().unwrap_or_else(|e| e.into_inner()) = Some(provider);
}
/// Remove the process factory; the core retains its keyword recovery fallback.
pub fn clear_recovery_provider() {
    *slot().write().unwrap_or_else(|e| e.into_inner()) = None;
}
