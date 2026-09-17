//! The TUI's session owner: `openhuman-session` over an in-process
//! [`CoreRuntime`]. Login-token exchange and `/auth/me` happen here, in the
//! TUI process; the core only receives the resulting credential through
//! `auth.set_credential`.

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use openhuman_core::core::runtime::CoreRuntime;
use openhuman_session::{ClientHeaders, CoreLink, SessionManager};

/// `CoreLink` over `CoreRuntime::invoke` — no HTTP, no bearer.
pub struct InProcessLink(pub Arc<CoreRuntime>);

#[async_trait]
impl CoreLink for InProcessLink {
    async fn invoke(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.0.invoke(method, params).await
    }
}

static MANAGER: OnceLock<Arc<SessionManager<InProcessLink>>> = OnceLock::new();

/// The one session manager for this TUI process, bound to `runtime` on first
/// use. A TUI process hosts exactly one runtime, so later callers get the
/// same manager regardless of the handle they pass.
pub fn session_manager(runtime: &Arc<CoreRuntime>) -> Arc<SessionManager<InProcessLink>> {
    Arc::clone(MANAGER.get_or_init(|| {
        let headers = ClientHeaders::new(openhuman_core::api::product_identity().as_str())
            .with_core_version(env!("CARGO_PKG_VERSION"));
        SessionManager::new(Arc::new(InProcessLink(Arc::clone(runtime))), headers)
    }))
}
