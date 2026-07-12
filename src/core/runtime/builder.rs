//! `CoreBuilder` → `CoreRuntime`: the embeddable composition surface.
//!
//! This is the first-class library API for hosting the OpenHuman core. It
//! splits the monolithic `run_server_inner` into two phases:
//!
//! 1. [`CoreBuilder::build`] — *initialization only*: register controllers, load
//!    the master key, seed the RPC bearer, initialize workspace-bound stores,
//!    and run the pure-registration part of [`bootstrap_core_runtime`]. No port
//!    is bound and `ServiceSet::none` / `ServiceSet::headless_api` start no
//!    background loops. After `build`, [`CoreRuntime::invoke`] can dispatch any
//!    RPC method in-process, and agent turns can run — so a harness-only embedder
//!    (`ServiceSet::none`) needs nothing more.
//! 2. [`CoreRuntime::serve`] — *transport + background services*: bind the HTTP
//!    listener, mount the router, fire the readiness signal, spawn the selected
//!    background services, and serve until shutdown.
//!
//! The legacy entry points (`run_server`, `run_server_embedded`,
//! `run_server_embedded_with_ready`) are now thin shims over this builder, so
//! the desktop shell, the standalone CLI, and any new embedder share one path.
//! See `docs/plans/pluggable-core/phase-1-corebuilder.md`.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::core::jsonrpc::{self, EmbeddedReadySignal};
use crate::core::runtime::context::CoreContext;
use crate::core::types::HostKind;
use crate::openhuman::config::Config;

/// Selects which background services and transports a [`CoreRuntime`] runs.
///
/// Each flag is independent. Presets cover the common hosts:
/// [`ServiceSet::desktop`] (everything — the Tauri shell / standalone CLI),
/// [`ServiceSet::headless_api`] (HTTP JSON-RPC only — single-core cloud), and
/// [`ServiceSet::none`] (no transport, no background work — library / harness).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceSet {
    /// Bind the axum HTTP server and serve `POST /rpc` (+ the other core routes).
    pub rpc_http: bool,
    /// Mount the Socket.IO realtime layer on the HTTP server (requires `rpc_http`).
    pub socketio: bool,
    /// Spawn the cron scheduler (still gated at runtime by `config.cron.enabled`).
    pub cron: bool,
    /// Spawn realtime channel listeners (Telegram, Discord, …).
    pub channels: bool,
    /// Spawn login-gated services (local AI, voice, autocomplete) + subconscious/heartbeat.
    pub heartbeat: bool,
    /// Spawn the periodic self-update checker.
    pub update_scheduler: bool,
    /// Start memory queue workers during runtime bootstrap.
    pub memory_queue: bool,
    /// Run one-shot harness initialization during runtime bootstrap.
    pub harness_init: bool,
    /// Refresh the skill catalog during runtime bootstrap.
    pub skill_catalog_refresh: bool,
    /// Boot installed MCP servers and supervise reconnects during runtime bootstrap.
    pub mcp_boot: bool,
}

impl ServiceSet {
    /// Everything on — the desktop shell and the standalone `openhuman-core run`.
    pub fn desktop() -> Self {
        Self {
            rpc_http: true,
            socketio: true,
            cron: true,
            channels: true,
            heartbeat: true,
            update_scheduler: true,
            memory_queue: true,
            harness_init: true,
            skill_catalog_refresh: true,
            mcp_boot: true,
        }
    }

    /// HTTP JSON-RPC only — a single-core cloud/server deployment. No Socket.IO,
    /// no cron/channels/heartbeat; the supervisor decides those per plan.
    pub fn headless_api() -> Self {
        Self {
            rpc_http: true,
            socketio: false,
            cron: false,
            channels: false,
            heartbeat: false,
            update_scheduler: false,
            memory_queue: false,
            harness_init: false,
            skill_catalog_refresh: false,
            mcp_boot: false,
        }
    }

    /// No transport and no background services — for library / harness embedders
    /// that only drive the core through [`CoreRuntime::invoke`] and agent turns.
    pub fn none() -> Self {
        Self {
            rpc_http: false,
            socketio: false,
            cron: false,
            channels: false,
            heartbeat: false,
            update_scheduler: false,
            memory_queue: false,
            harness_init: false,
            skill_catalog_refresh: false,
            mcp_boot: false,
        }
    }
}

/// How the per-process RPC bearer token is seeded.
pub enum TokenSource {
    /// An in-memory bearer supplied by the embedder (the Tauri shell hands its
    /// `CoreProcessHandle.rpc_token` this way). Seeded via
    /// [`crate::core::auth::init_rpc_token_with_value`] — never crosses the
    /// process environment.
    Fixed(Arc<String>),
    /// Standalone fallback: read `OPENHUMAN_CORE_TOKEN` from the environment when
    /// present (operator config), otherwise generate a fresh token and write
    /// `{root}/core.token` (0o600 on Unix) so CLI callers can authenticate.
    EnvOrFile,
}

/// Builder for a [`CoreRuntime`]. Construct with [`CoreBuilder::new`], then
/// [`CoreBuilder::build`] to initialize the core.
pub struct CoreBuilder {
    host_kind: HostKind,
    token: TokenSource,
    services: ServiceSet,
    host: Option<String>,
    port: Option<u16>,
}

impl CoreBuilder {
    /// Start a builder for the given host kind. Defaults: [`TokenSource::EnvOrFile`]
    /// and [`ServiceSet::desktop`].
    pub fn new(host_kind: HostKind) -> Self {
        Self {
            host_kind,
            token: TokenSource::EnvOrFile,
            services: ServiceSet::desktop(),
            host: None,
            port: None,
        }
    }

    /// Choose which background services / transports [`CoreRuntime::serve`] runs.
    pub fn services(mut self, services: ServiceSet) -> Self {
        self.services = services;
        self
    }

    /// Choose how the RPC bearer token is seeded.
    pub fn token(mut self, token: TokenSource) -> Self {
        self.token = token;
        self
    }

    /// Override the bind host (default: `OPENHUMAN_CORE_HOST` env or `127.0.0.1`).
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    /// Override the bind port (default: `OPENHUMAN_CORE_PORT` env or `7788`).
    pub fn port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// Initialize the core: register controllers, load the master key, seed the
    /// RPC bearer, initialize workspace-bound stores, and run
    /// [`bootstrap_core_runtime`]. Binds no port and starts no transport.
    ///
    /// The init sequence itself is owned by [`CoreContext::init`] (Phase 2,
    /// Stage A).
    pub async fn build(self) -> anyhow::Result<CoreRuntime> {
        let (ctx, has_operator_token, config) =
            CoreContext::init(self.host_kind, &self.token).await?;

        Ok(CoreRuntime {
            ctx,
            config,
            services: self.services,
            has_operator_token,
            host: self.host,
            port: self.port,
        })
    }
}

/// A built, initialized core. Dispatch RPC in-process with [`CoreRuntime::invoke`],
/// or run the selected transport + background services with [`CoreRuntime::serve`].
pub struct CoreRuntime {
    ctx: Arc<CoreContext>,
    config: Option<Config>,
    services: ServiceSet,
    has_operator_token: bool,
    host: Option<String>,
    port: Option<u16>,
}

impl CoreRuntime {
    /// The services/transports this runtime is configured to run.
    pub fn services(&self) -> ServiceSet {
        self.services
    }

    /// The initialized core context (host identity + resolved workspace).
    pub fn context(&self) -> &Arc<CoreContext> {
        &self.ctx
    }

    /// Dispatch an RPC method in-process — the same path the HTTP `/rpc` handler
    /// and the CLI use ([`jsonrpc::invoke_method`]). No network involved.
    pub async fn invoke(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        CoreContext::scope(
            Arc::clone(&self.ctx),
            jsonrpc::invoke_method(jsonrpc::default_state(), method, params),
        )
        .await
    }

    /// Spawn the selected background services and, when `rpc_http` is set, bind
    /// the HTTP listener and serve until shutdown.
    ///
    /// When `rpc_http` is not selected this returns immediately (a harness-only
    /// embedder has no transport to run); background services selected in the
    /// [`ServiceSet`] are still spawned.
    pub async fn serve(
        &self,
        ready_tx: Option<tokio::sync::oneshot::Sender<EmbeddedReadySignal>>,
        shutdown_token: Option<CancellationToken>,
    ) -> anyhow::Result<()> {
        if !self.services.rpc_http {
            // No transport: just spawn the selected background services and
            // return. The caller owns the process lifetime.
            self.start_selected_services();
            return Ok(());
        }

        // --- Host / port resolution ---
        let (resolved_port, port_source) = match self.port {
            Some(p) => (p, "builder port"),
            None => (
                jsonrpc::core_port(),
                if std::env::var("OPENHUMAN_CORE_PORT").is_ok() {
                    "env OPENHUMAN_CORE_PORT"
                } else {
                    "default"
                },
            ),
        };
        let (resolved_host, host_source) = match &self.host {
            Some(h) => (h.clone(), "builder host"),
            None => (
                jsonrpc::core_host(),
                if std::env::var("OPENHUMAN_CORE_HOST")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .is_some()
                {
                    "env OPENHUMAN_CORE_HOST"
                } else {
                    "default"
                },
            ),
        };

        log::debug!(
            "[core] Bind resolution: host={resolved_host} (from {host_source}), port={resolved_port} (from {port_source})"
        );

        // Safety check: refuse to bind on a non-loopback address without an
        // explicit operator-supplied RPC token. Without this, the entire RPC
        // surface (tool execution, file access, credentials) is unauthenticated
        // and reachable from the network. See issue #1919. The self-generated
        // {workspace}/core.token does NOT count — remote clients cannot read it,
        // so treating it as "explicit" would be fail-open.
        if crate::openhuman::security::pairing::is_public_bind(&resolved_host)
            && !self.has_operator_token
        {
            log::error!(
                "[core] SECURITY: refusing to bind on public address {resolved_host} without an \
                 explicit operator-supplied RPC token. Set {} in your environment (or hand the \
                 bearer in-memory via the embedded core handle) to secure the RPC endpoint.",
                crate::core::auth::CORE_TOKEN_ENV_VAR
            );
            eprintln!(
                "\n\x1b[1;31m[SECURITY]\x1b[0m Refusing to bind on {resolved_host} without {}.\n\
                 The auto-generated {{workspace}}/core.token does NOT secure a public bind —\n\
                 remote clients cannot read it. Set {} in your environment to secure the\n\
                 RPC endpoint, or bind on a loopback address.\n",
                crate::core::auth::CORE_TOKEN_ENV_VAR,
                crate::core::auth::CORE_TOKEN_ENV_VAR
            );
            anyhow::bail!(
                "refusing to bind on non-loopback address {resolved_host} without an explicit \
                 operator-supplied RPC token ({})",
                crate::core::auth::CORE_TOKEN_ENV_VAR
            );
        }

        let preferred_port = resolved_port;
        let host = resolved_host;
        let pick = crate::openhuman::connectivity::rpc::pick_listen_port_for_host(
            host.as_str(),
            preferred_port,
        )
        .await
        .map_err(|err| {
            log::error!("[core] Failed to bind to {host}:{preferred_port}: {err}");
            anyhow::Error::new(err)
        })?;
        let listen_port = pick.port;
        let bind_addr = format!("{host}:{listen_port}");
        let listener = pick.listener;

        // Synchronize OPENHUMAN_CORE_RPC_URL with the actual bound port so
        // connectivity::rpc::resolve_listen_port() reports the live listener
        // instead of the originally-requested port when fallback engaged.
        //
        // SAFETY: set_var is process-global; this runs once during bind. Flagged
        // in the pluggable-core drift ledger as single-runtime-per-process.
        unsafe {
            std::env::set_var("OPENHUMAN_CORE_RPC_URL", format!("http://{bind_addr}/rpc"));
        }

        let ctx = Arc::clone(&self.ctx);
        let app = jsonrpc::build_core_http_router(self.services.socketio).layer(
            axum::middleware::from_fn(
                move |req: axum::extract::Request, next: axum::middleware::Next| {
                    let ctx = Arc::clone(&ctx);
                    async move { CoreContext::scope(ctx, next.run(req)).await }
                },
            ),
        );

        log::info!(
            "[core] OpenHuman core is ready — listening on http://{bind_addr} (version {})",
            env!("CARGO_PKG_VERSION")
        );
        log::info!("[rpc:http] JSON-RPC — POST http://{bind_addr}/rpc (JSON-RPC 2.0)");
        if self.services.socketio {
            log::info!("[rpc:socketio] Socket.IO — ws://{bind_addr}/socket.io/ (same HTTP server)");
        } else {
            log::info!("[rpc:socketio] disabled (--jsonrpc-only)");
        }

        if let Some(tx) = ready_tx {
            let _ = tx.send(EmbeddedReadySignal {
                port: listen_port,
                fallback_from: pick.fallback_from,
            });
        }

        // Background services — gated by the ServiceSet.
        self.start_selected_services();

        if let Some(shutdown_token) = shutdown_token {
            log::info!(
                "[core] embedded server waiting on cancellation token for graceful shutdown"
            );
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    shutdown_token.cancelled().await;
                })
                .await?;
        } else {
            axum::serve(listener, app)
                .with_graceful_shutdown(crate::core::shutdown::signal())
                .await?;
        }

        // Server has stopped accepting and in-flight requests drained. Kill any
        // `ollama serve` openhuman itself spawned (no-op when externally
        // managed) so the next launch doesn't try to reclaim a dead daemon.
        // Bounded so a wedged Ollama can't hold up app shutdown.
        if let Some(svc) = crate::openhuman::inference::local::try_global() {
            let cfg = crate::openhuman::config::Config::load_or_init()
                .await
                .unwrap_or_default();
            log::info!("[core] shutdown: cleaning up openhuman-owned ollama if any");
            let shutdown_fut = svc.shutdown_owned_ollama(&cfg);
            if tokio::time::timeout(std::time::Duration::from_secs(2), shutdown_fut)
                .await
                .is_err()
            {
                log::warn!(
                    "[core] shutdown: ollama cleanup exceeded 2s budget; proceeding with exit"
                );
            }
        }

        Ok(())
    }

    /// Spawn each selected background service. Selection is by [`ServiceSet`];
    /// each service keeps its own runtime config gate.
    fn start_selected_services(&self) {
        use crate::core::runtime::services;
        jsonrpc::start_core_runtime_services(self.services, self.config.as_ref());

        if self.services.heartbeat {
            services::spawn_login_gated_services(self.ctx.host_kind().is_desktop_shell());
        }
        if self.services.update_scheduler {
            services::spawn_update_scheduler();
        }
        if self.services.cron {
            services::spawn_cron_service();
        }
        if self.services.channels {
            services::spawn_channels_service();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ServiceSet;

    #[test]
    fn boot_jobs_are_independent_from_runtime_service_flags() {
        let mut custom = ServiceSet::none();
        custom.rpc_http = true;
        custom.heartbeat = true;
        custom.update_scheduler = true;
        assert!(!custom.memory_queue);
        assert!(!custom.harness_init);
        assert!(!custom.skill_catalog_refresh);
        assert!(!custom.mcp_boot);

        let desktop = ServiceSet::desktop();
        assert!(desktop.memory_queue);
        assert!(desktop.harness_init);
        assert!(desktop.skill_catalog_refresh);
        assert!(desktop.mcp_boot);
    }
}
