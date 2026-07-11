//! Core initialization context.
//!
//! [`CoreContext`] owns the core's initialization *order* (Phase 2, Stage A):
//! register controllers, load the master key, seed the RPC bearer, initialize
//! the workspace-bound stores, and run `bootstrap_core_runtime`. Today it is a
//! facade — the store init still targets the process globals — but centralizing
//! the sequence here is the seam the later stages build on (handler-threaded
//! context, per-context stores). See `docs/plans/pluggable-core/phase-2-corecontext.md`.
//!
//! [`init_stores`] initializes the process-global stores bound to a single
//! resolved workspace directory (memory, image attachments, WhatsApp data,
//! people) plus the boot-time Sentry user binding. It preserves the exact
//! behavior and ordering of the original inline `run_server_inner` block,
//! including the deliberate wrong-workspace guard (never seed against a
//! `Config::default` fallback — Sentry OPENHUMAN-CORE-48 / TAURI-RUST-8NM).

use std::future::Future;
use std::sync::{Arc, OnceLock};

use crate::core::runtime::TokenSource;
use crate::core::types::HostKind;

/// The process-wide default context — the first one built. Callers that dispatch
/// RPC without an explicit per-call context (the desktop shell, the CLI, tests)
/// resolve to this. Multi-tenant hosts override it per dispatch via
/// [`CoreContext::scope`].
static DEFAULT_CONTEXT: OnceLock<Arc<CoreContext>> = OnceLock::new();

tokio::task_local! {
    /// The context active for the current dispatch, set by [`CoreContext::scope`]
    /// at the `try_invoke_registered_rpc` chokepoint. Absent outside a scope —
    /// [`CoreContext::current`] then falls back to [`DEFAULT_CONTEXT`].
    static CURRENT_CONTEXT: Arc<CoreContext>;
}

/// A built, initialized core context. Holds the identity of the host and the
/// resolved workspace directory; created by [`CoreContext::init`].
///
/// Handlers reach the context for the current dispatch through
/// [`CoreContext::current`] rather than a threaded parameter — the ambient
/// context is established once per RPC at the dispatch chokepoint. This keeps
/// controller handlers as bare `fn` pointers (no per-handler signature churn)
/// while giving every handler a path to per-context state.
///
/// Stage A/B: state that today still lives in process globals is reached through
/// the globals as before; a domain migrates by reading its store handle off the
/// context ([`CoreContext::current`]) instead of the global. Once a domain's
/// state lives on the context, two contexts dispatched under distinct
/// [`CoreContext::scope`]s read isolated state — the Phase 3 exit criterion.
pub struct CoreContext {
    host_kind: HostKind,
    workspace_dir: std::path::PathBuf,
}

impl CoreContext {
    /// Run the core initialization sequence and return the context plus whether
    /// an operator-supplied RPC bearer exists (for the public-bind safety check
    /// in `CoreRuntime::serve`). Order is load-bearing and mirrors the original
    /// `run_server_inner` sequence:
    ///
    /// 1. register controllers, 2. master key, 3. AgentBox GMI provider,
    /// 4. seed RPC bearer, 5. workspace stores ([`init_stores`]),
    /// 6. `bootstrap_core_runtime`.
    pub async fn init(
        host_kind: HostKind,
        token: &TokenSource,
    ) -> anyhow::Result<(Arc<CoreContext>, bool)> {
        // 1. Ensure all controllers are registered before anything dispatches.
        let _ = crate::core::all::all_registered_controllers();

        // 2. Load the master encryption key before any config/credential op that
        //    needs to decrypt secrets. No-op if already called (e.g. from
        //    run_core_from_args for the CLI).
        crate::openhuman::keyring::init_master_key();

        // 3. AgentBox GMI MaaS provider bridge — no-op when env vars absent. Must
        //    run before the router mounts the AgentBox routes so the inference
        //    catalog knows about "gmi-maas" by the time `/run` accepts traffic.
        crate::openhuman::agentbox::register_gmi_provider_if_present();

        // 4. Seed the per-process RPC bearer. `Fixed` seeds the in-memory value
        //    directly (never touches the env); `EnvOrFile` reads
        //    OPENHUMAN_CORE_TOKEN or generates + writes {root}/core.token.
        //
        //    `has_operator_token` records whether an OPERATOR-supplied bearer
        //    exists (in-memory handoff or env var). The self-generated core.token
        //    file does NOT count — remote clients cannot read it — so it must not
        //    satisfy the public-bind safety check in `serve`.
        let has_operator_token = match token {
            TokenSource::Fixed(token) => {
                crate::core::auth::init_rpc_token_with_value(token)?;
                !token.trim().is_empty()
            }
            TokenSource::EnvOrFile => {
                let token_dir = crate::openhuman::config::default_root_openhuman_dir()
                    .unwrap_or_else(|_| {
                        dirs::home_dir()
                            .unwrap_or_else(|| std::path::PathBuf::from("."))
                            .join(".openhuman")
                    });
                crate::core::auth::init_rpc_token(&token_dir)?;
                std::env::var(crate::core::auth::CORE_TOKEN_ENV_VAR)
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .is_some()
            }
        };

        // 5. Initialize workspace-bound stores (memory, attachments, whatsapp,
        //    people) with the wrong-workspace guard.
        init_stores().await;

        // Resolve the workspace dir for the context handle. Best-effort: on a
        // config-load failure the stores above stayed uninitialised and this
        // falls back to the default root so the context still carries a path.
        let workspace_dir = match crate::openhuman::config::Config::load_or_init().await {
            Ok(cfg) => cfg.workspace_dir,
            Err(_) => crate::openhuman::config::default_root_openhuman_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from(".")),
        };

        // 6. Long-lived runtime infrastructure: event bus, domain subscribers,
        //    ledgers, agent-definition registry, live security policy, approval
        //    gate, socket manager. Idempotent (Once-guarded internally).
        crate::core::jsonrpc::bootstrap_core_runtime(host_kind).await;

        let ctx = Arc::new(CoreContext {
            host_kind,
            workspace_dir,
        });

        // Register the process default context (first build wins). Dispatch
        // resolves to this when no per-call context is scoped.
        let _ = DEFAULT_CONTEXT.set(ctx.clone());

        Ok((ctx, has_operator_token))
    }

    /// The host that constructed this context (Tauri shell / CLI / Docker).
    pub fn host_kind(&self) -> HostKind {
        self.host_kind
    }

    /// The resolved per-user workspace directory this context is bound to.
    pub fn workspace_dir(&self) -> &std::path::Path {
        &self.workspace_dir
    }

    /// The context for the current dispatch: the one scoped by
    /// [`CoreContext::scope`] if inside a scope, else the process
    /// [`DEFAULT_CONTEXT`]. Returns `None` only before any context is built
    /// (e.g. a unit test that dispatches without initializing the core).
    ///
    /// Handlers migrating off process globals read their state through this.
    pub fn current() -> Option<Arc<CoreContext>> {
        CURRENT_CONTEXT
            .try_with(|ctx| ctx.clone())
            .ok()
            .or_else(|| DEFAULT_CONTEXT.get().cloned())
    }

    /// The process default context (first built), independent of any active
    /// scope. Used by the dispatch chokepoint to establish the ambient scope.
    pub fn default_context() -> Option<Arc<CoreContext>> {
        DEFAULT_CONTEXT.get().cloned()
    }

    /// Run `fut` with `ctx` as the ambient [`CoreContext::current`]. The dispatch
    /// layer wraps each handler invocation in this; multi-tenant hosts pass the
    /// tenant's context here so the handler's `current()` reads isolated state.
    pub async fn scope<F: Future>(ctx: Arc<CoreContext>, fut: F) -> F::Output {
        CURRENT_CONTEXT.scope(ctx, fut).await
    }
}

/// Initialize the global `MemoryClient` and the other workspace-bound stores so
/// composio providers (gmail/slack/notion) can persist their `sync_state`, and
/// so any subsystem that calls `memory::global::client_if_ready()` gets a live
/// handle.
///
/// A `Config::load_or_init` failure here is operator-visible and serious
/// (corrupt toml, bad permissions, missing/unwritable `OPENHUMAN_WORKSPACE` —
/// common on headless/containerised deploys with no writable `$HOME`).
/// Previously the fallback to `Config::default()` initialised the memory +
/// whatsapp_data stores against the *wrong* workspace dir, silently causing
/// chunk loss / cross-workspace bleed-over while the app looked healthy (Sentry
/// OPENHUMAN-CORE-48). Instead: skip the workspace-bound init entirely so
/// memory stays explicitly *uninitialised* — callers then get a clear "memory
/// client not ready" error rather than reading/writing the wrong workspace. The
/// server still comes up; the operator sees the loud error and fixes their
/// config or sets `OPENHUMAN_WORKSPACE` to a writable path, then restarts.
pub async fn init_stores() {
    match crate::openhuman::config::Config::load_or_init().await {
        Ok(cfg) => {
            let keyring_dir = crate::openhuman::keyring::store::workspace_dir_for_file_backend();
            log::info!(
                "[boot] paths: config={} workspace={} keyring_dir={} keyring_backend={}",
                cfg.config_path.display(),
                cfg.workspace_dir.display(),
                keyring_dir.display(),
                crate::openhuman::keyring::backend_name(),
            );
            match crate::openhuman::memory::global::init(cfg.workspace_dir.clone()) {
                Ok(_) => log::info!(
                    "[boot] memory::global initialized (workspace={})",
                    cfg.workspace_dir.display()
                ),
                Err(e) => log::warn!("[boot] memory::global init failed: {e}"),
            }
            // Install the on-disk image-attachment sidecar dir so inbound
            // image markers persist under <workspace>/attachments/ instead
            // of an in-memory FIFO (survives restarts + delegation hops).
            // Also fires a best-effort stale-file sweep.
            crate::openhuman::agent::multimodal::init_attachments_dir(
                cfg.workspace_dir.join("attachments"),
            );
            log::info!(
                "[boot] image attachments sidecar dir = {}",
                cfg.workspace_dir.join("attachments").display()
            );
            // Initialize the WhatsApp data store so scanner ingest calls
            // can write data without requiring a lazy-init fallback.
            match crate::openhuman::whatsapp_data::global::init(cfg.workspace_dir.clone()) {
                Ok(_) => log::info!(
                    "[boot] whatsapp_data::global initialized (workspace={})",
                    cfg.workspace_dir.display()
                ),
                Err(e) => log::warn!("[boot] whatsapp_data::global init failed: {e}"),
            }
            // Seed the people store so people controllers + `people_*`
            // tools can read/write. Without this the process-global stays
            // empty and every call fails with "people store not
            // initialised" (Sentry TAURI-RUST-8NM). Sits inside this
            // Ok(cfg) arm so it inherits the wrong-workspace guard above
            // (never seed against a Config::default fallback).
            match crate::openhuman::people::store::init_from_workspace(&cfg.workspace_dir) {
                Ok(_) => log::info!(
                    "[boot] people::store initialized (workspace={})",
                    cfg.workspace_dir.display()
                ),
                Err(e) => log::warn!("[boot] people::store init failed: {e}"),
            }
            // Prune legacy bundled skills (dev-workflow / github-issue-crusher
            // / pr-review-shepherd) that older builds seeded into
            // <workspace>/skills/. OpenHuman no longer ships bundled defaults;
            // this removes the stale dirs on upgrade. Idempotent.
            crate::openhuman::skills::registry::prune_legacy_default_workflows(&cfg.workspace_dir);
            // Boot-time Sentry user binding — issue #3135. If the user is
            // already signed in (typical desktop restart), the auth-profile
            // store has their `user_id` *now*, before any background loop
            // (Composio sync tick, heartbeat, etc.) fires its first event.
            // Reading from the store here means subsequent events carry
            // `user.id` even when no `app_state_snapshot` RPC has run yet.
            match crate::openhuman::credentials::session_support::build_session_state(&cfg) {
                Ok(state) => {
                    if let Some(uid) = state.user_id.as_deref() {
                        crate::openhuman::credentials::sentry_scope::bind(uid);
                    }
                }
                Err(e) => log::debug!(
                    "[boot] sentry scope user bind skipped — build_session_state failed: {e}"
                ),
            }
        }
        Err(e) => {
            log::error!(
                "[boot] memory::global + whatsapp_data init SKIPPED — \
                 Config::load_or_init failed ({e:#}). Memory persistence is \
                 DISABLED for this run; no silent fallback to the default \
                 workspace (which would cause chunk loss / cross-workspace \
                 bleed-over). Fix config.toml or set OPENHUMAN_WORKSPACE to a \
                 writable path, then restart."
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ctx(dir: &str) -> Arc<CoreContext> {
        Arc::new(CoreContext {
            host_kind: HostKind::Cli,
            workspace_dir: PathBuf::from(dir),
        })
    }

    // The ambient-scope primitive is the mechanism Phase 3 multi-tenant
    // isolation is built on: a dispatch scoped to context A must see A's state,
    // not the process default or another tenant's. These assert the primitive
    // directly (independent of the process DEFAULT_CONTEXT global, since
    // `current()` inside a scope resolves the scoped value).

    #[tokio::test]
    async fn scope_sets_current_context() {
        let a = ctx("/tmp/ctx-a");
        let seen = CoreContext::scope(a, async {
            CoreContext::current().map(|c| c.workspace_dir().to_path_buf())
        })
        .await;
        assert_eq!(seen, Some(PathBuf::from("/tmp/ctx-a")));
    }

    #[tokio::test]
    async fn nested_scope_overrides_then_restores() {
        let a = ctx("/tmp/ctx-a");
        let b = ctx("/tmp/ctx-b");
        let (inner, outer) = CoreContext::scope(a, async {
            let inner = CoreContext::scope(b, async {
                CoreContext::current()
                    .unwrap()
                    .workspace_dir()
                    .to_path_buf()
            })
            .await;
            let outer = CoreContext::current()
                .unwrap()
                .workspace_dir()
                .to_path_buf();
            (inner, outer)
        })
        .await;
        // Inner dispatch sees tenant B; the outer scope is restored to A after.
        assert_eq!(inner, PathBuf::from("/tmp/ctx-b"));
        assert_eq!(outer, PathBuf::from("/tmp/ctx-a"));
    }
}
