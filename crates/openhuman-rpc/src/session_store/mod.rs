//! The on-disk session store the app, the CLI and the TUI install.
//!
//! [`SqliteSessionStores`] is a
//! [`SessionStoreProvider`](tinyagents_session::port::SessionStoreProvider)
//! over the layout OpenHuman has always written under its workspace:
//!
//! ```text
//! {workspace}/session_raw/*.jsonl                         transcripts
//! {workspace}/session_db/sessions.db                      run ledger (SQLite)
//! {workspace}/memory/conversations/turn_states/…          turn snapshots
//! {workspace}/tinyagents_store/{kv,journal}/              run status, goals,
//!                                                         todos, turn journal
//! ```
//!
//! The desktop app, the CLI and the TUI install it ([`install`]). The core
//! and the embed facade reach session state through the port, and keep a
//! fallback to the same files only for hosts that install no store.
//!
//! # Why there are three install paths
//!
//! The store is one provider; what differs is how long it must stay installed:
//!
//! - **Servers (`server::shims::build_and_serve`, used by `host::cli` and
//!   `host::desktop`)** install it for the life of the process
//!   ([`install_for_host`]), before the runtime builds so the recovery sweep
//!   sees it. They deliberately avoid `RuntimeBuilder::session_store`, which
//!   restores the previous provider when the runtime drops: the desktop
//!   restarts its in-process server in place, and a detached turn still
//!   writing across that gap must keep landing in this layout rather than in
//!   the core's no-provider fallback. A builder that carries its own store
//!   skips this step.
//! - **The TUI (`host::tui_builder` / `host::tui`)** hands the provider to the
//!   builder ([`provider`], [`provider_for_host`]). Its runtime lives for the
//!   whole process and is dropped once at exit, so restore-on-drop is correct
//!   and leaves nothing installed behind it.
//! - **SaaS (`server::run_server_saas`)** calls [`install`] directly. The core
//!   boots from `core::runtime::saas::build` rather than a `RuntimeBuilder`, and the
//!   store resolves the workspace of the context each call runs under, so
//!   every user agent keeps its own sessions. It is process-lifetime for the
//!   same reason as the servers.
//!
//! [`install_for_host`] and [`provider_for_host`] differ from [`install`] and
//! [`provider`] only in honouring a configured storage URL
//! (`OPENHUMAN_STORAGE_URL` / `[storage] url`), which swaps this on-disk layout
//! for TinyAgents' `DriverSessionStores` over that backend.
//!
//! It serves one operator: every agent shares the workspace, as it always
//! has, so it does not claim the per-agent isolation a multi-user host's
//! store must provide.
//!
//! The workspace is resolved on every call rather than fixed at
//! construction, because the desktop rebinds it when a different user signs
//! in.
//!
//! ```
//! use std::sync::Arc;
//! use openhuman_rpc::session_store::SqliteSessionStores;
//! use tinyagents_session::port::SessionStoreProvider;
//!
//! let dir = tempfile::tempdir().unwrap();
//! let stores = SqliteSessionStores::at(dir.path());
//! assert_eq!(stores.workspace_dir().as_deref(), Some(dir.path()));
//! let agent = stores.for_agent("orchestrator");
//! assert!(agent.transcripts.root_for_thread("no-such-thread").is_none());
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tinyagents_session::port::{AgentStores, SessionStoreProvider};
use tinyagents_session::transcript::import::ops::open_session_stores;
use tinyagents_session::transcript::FileTranscriptLocator;
use tinyagents_session::turn_state::TurnStateStore;

/// Resolves the workspace the stores live in, at the moment they are needed.
type WorkspaceResolver = dyn Fn() -> PathBuf + Send + Sync;

/// OpenHuman's on-disk session layout as a session store provider.
#[derive(Clone)]
pub struct SqliteSessionStores {
    workspace: Arc<WorkspaceResolver>,
}

impl std::fmt::Debug for SqliteSessionStores {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteSessionStores")
            .field("workspace", &(self.workspace)())
            .finish()
    }
}

impl SqliteSessionStores {
    /// Stores under the fixed `workspace` directory.
    pub fn at(workspace: impl Into<PathBuf>) -> Self {
        let workspace = workspace.into();
        Self::resolving(move || workspace.clone())
    }

    /// Stores under whichever workspace `resolve` names when asked — for a
    /// host whose workspace can change while it runs.
    pub fn resolving(resolve: impl Fn() -> PathBuf + Send + Sync + 'static) -> Self {
        Self {
            workspace: Arc::new(resolve),
        }
    }

    fn current(&self) -> PathBuf {
        (self.workspace)()
    }
}

impl SessionStoreProvider for SqliteSessionStores {
    /// Every agent shares the workspace: the single-operator layout.
    fn for_agent(&self, agent_id: &str) -> AgentStores {
        let workspace = self.current();
        log::trace!(
            "[rpc:session_store] stores for agent={agent_id} workspace={}",
            workspace.display()
        );
        stores_at(&workspace)
    }

    /// Marks turns left in flight by an unclean shutdown interrupted, and
    /// settles run-ledger rows a dead process left running — the two sweeps
    /// the core always ran at boot.
    fn recover(&self) -> anyhow::Result<()> {
        let workspace = self.current();
        let now = chrono::Utc::now().to_rfc3339();
        let turns =
            tinyagents_session::turn_state::store::mark_all_interrupted(workspace.clone(), &now)
                .map_err(anyhow::Error::msg)?;
        let runs = tinyagents_session::run_ledger::interrupt_orphaned_agent_runs(&workspace)?;
        log::info!(
            "[rpc:session_store] recovered workspace={} interrupted_turns={turns} settled_runs={runs}",
            workspace.display()
        );
        Ok(())
    }

    fn destination_key(&self) -> Option<String> {
        Some(self.current().to_string_lossy().into_owned())
    }

    fn workspace_dir(&self) -> Option<PathBuf> {
        Some(self.current())
    }
}

/// The stores the layout keeps under `workspace`.
fn stores_at(workspace: &Path) -> AgentStores {
    let kv_and_journal = open_session_stores(workspace);
    AgentStores {
        transcripts: Arc::new(FileTranscriptLocator::new(workspace)),
        turn_states: Arc::new(TurnStateStore::new(workspace.to_path_buf())),
        kv: Arc::new(kv_and_journal.kv),
        journal: Arc::new(kv_and_journal.journal),
    }
}

/// Installs [`SqliteSessionStores`] over the current workspace as the
/// process's session store. Call it before the core boots, so its recovery
/// sweep runs; the workspace is resolved per call because the desktop rebinds
/// it when a different user signs in.
pub fn install() {
    log::debug!("[rpc:session_store] installing process-wide (context workspace)");
    crate::core_host::agent::session_store::install(provider());
}

/// [`SqliteSessionStores`] over the current context's workspace, as the
/// provider a runtime builder's `session_store` option takes (what
/// [`crate::host::tui`] wires). [`install`] installs the same provider
/// process-wide instead.
pub fn provider() -> Arc<dyn SessionStoreProvider> {
    Arc::new(SqliteSessionStores::resolving(
        crate::core_host::agent::session_store::context_workspace_dir,
    ))
}

/// Installs the session store the host's configuration asks for, before the
/// core boots.
///
/// With no storage URL (`OPENHUMAN_STORAGE_URL`, else `[storage] url`) this is
/// [`install`]: the classic on-disk layout, unchanged. With one, the backend is
/// opened, made the process's storage backend
/// ([`crate::core_host::storage::install`]), and TinyAgents'
/// `DriverSessionStores` is installed over it: every agent's transcripts, turn
/// states, records and journal in that backend, one storage scope per agent.
///
/// A single-process backend (SQLite, memory, files) also interrupts an
/// agent's in-flight turns the first time it is opened, since only an earlier
/// process can have left them. MongoDB does not: several processes may share
/// that database, and another one may own those turns.
///
/// # Errors
///
/// When a URL is configured but cannot be parsed or opened, or the config
/// that may name one cannot be loaded. A deployment that asked for a backend
/// must not quietly fall back to local files.
pub async fn install_for_host() -> anyhow::Result<()> {
    install_for_url(configured_storage_url().await?).await
}

/// The session store the host's configuration asks for, as a provider a
/// runtime builder's `session_store` option takes (what [`crate::host::tui`]
/// wires). [`install_for_host`] installs the same provider process-wide
/// instead.
///
/// Resolves the storage URL exactly as [`install_for_host`] does and has the
/// same side effect on the process's storage backend: a configured backend is
/// opened and installed, and with no URL any earlier backend is cleared.
///
/// # Errors
///
/// When a URL is configured but cannot be parsed or opened.
pub async fn provider_for_host() -> anyhow::Result<Arc<dyn SessionStoreProvider>> {
    provider_for_url(configured_storage_url().await?).await
}

/// The storage-backed session store, only when the host's configuration
/// names a storage URL; `None` leaves the classic layout and any process
/// state untouched. For one-shot CLI commands, which have no reason to
/// install the classic store but must see the same backend the server does.
///
/// # Errors
///
/// When the URL cannot be resolved, parsed or opened.
pub async fn provider_if_configured() -> anyhow::Result<Option<Arc<dyn SessionStoreProvider>>> {
    match configured_storage_url().await? {
        Some(url) => provider_for_url(Some(url)).await.map(Some),
        None => Ok(None),
    }
}

/// The storage URL the host asks for: `OPENHUMAN_STORAGE_URL`, else
/// `[storage] url` from the config, else `None` (the classic layout).
///
/// # Errors
///
/// When no environment URL pins the backend and the config cannot be loaded:
/// the config may name a `[storage] url`, and a deployment that asked for a
/// backend must not quietly fall back to local files.
async fn configured_storage_url() -> anyhow::Result<Option<String>> {
    let env = std::env::var(crate::core_host::storage::STORAGE_URL_VAR).ok();
    // A URL in the environment wins and never reads the config.
    if env.as_deref().is_some_and(|url| !url.trim().is_empty()) {
        return Ok(storage_url_from(env, &Default::default()));
    }
    let config = crate::core_host::config::rpc::load_config_with_timeout()
        .await
        .map_err(|error| {
            anyhow::anyhow!("loading the config to resolve the storage url: {error}")
        })?;
    Ok(storage_url_from(env, &config))
}

/// The core's URL rule ([`crate::core_host::storage::url_from`]) over `config`.
fn storage_url_from(
    env: Option<String>,
    config: &crate::core_host::config::Config,
) -> Option<String> {
    crate::core_host::storage::url_from(env, config)
}

/// [`install_for_host`] with the URL already resolved: `None` installs the
/// classic on-disk store, a URL opens that backend and installs
/// `DriverSessionStores` over it.
///
/// # Errors
///
/// When `url` cannot be parsed or opened.
pub async fn install_for_url(url: Option<String>) -> anyhow::Result<()> {
    let provider = provider_for_url(url).await?;
    log::debug!("[rpc:session_store] installing process-wide (host configuration)");
    crate::core_host::agent::session_store::install(provider);
    Ok(())
}

/// [`provider_for_host`] with the URL already resolved: `None` is the classic
/// on-disk [`provider`] (any earlier storage backend cleared), a URL opens
/// that backend, makes it the process's storage backend and returns
/// `DriverSessionStores` over it.
///
/// # Errors
///
/// When `url` cannot be parsed or opened.
pub async fn provider_for_url(
    url: Option<String>,
) -> anyhow::Result<Arc<dyn SessionStoreProvider>> {
    provider_for_url_with(url, None).await
}

/// [`install_for_url`] for a SaaS core: a backend's session store never
/// sweeps in-flight turns on open, whatever its driver. In SaaS mode recovery
/// is lease-driven — a profile's turns are interrupted only when its lease is
/// taken over from a holder that never released it — because another node
/// may be running them. With no URL this is the classic on-disk store.
///
/// # Errors
///
/// When `url` cannot be parsed or opened.
pub async fn install_for_saas(url: Option<String>) -> anyhow::Result<()> {
    let provider = provider_for_url_with(url, Some(false)).await?;
    log::debug!("[rpc:session_store] installing process-wide (SaaS, lease-driven recovery)");
    crate::core_host::agent::session_store::install(provider);
    Ok(())
}

/// [`provider_for_url`] with the recovery sweep made explicit: `None`
/// recovers on open exactly when the driver is single-process.
async fn provider_for_url_with(
    url: Option<String>,
    recover_on_open: Option<bool>,
) -> anyhow::Result<Arc<dyn SessionStoreProvider>> {
    use anyhow::Context as _;

    let Some(url) = url else {
        // Drop a backend an earlier call installed, so storage operations do
        // not keep writing to it while the classic layout is in force.
        crate::core_host::storage::clear();
        log::debug!("[rpc:session_store] no storage url; classic on-disk layout");
        return Ok(provider());
    };
    let backend = crate::core_host::storage::open(&url)
        .await
        .context("opening the configured storage backend")?;
    let recover = recover_on_open
        .unwrap_or_else(|| !crate::core_host::storage::driver_is_shared(backend.driver()));
    let provider = tinyagents_session::DriverSessionStores::new(Arc::clone(&backend))
        .context("starting the session store bridge")?
        .recover_on_open(recover);
    // Only a fully working bridge makes the backend the process's storage.
    crate::core_host::storage::install(backend);
    log::info!(
        "[rpc:session_store] opened the storage-backed session store \
         recover_on_open={recover}"
    );
    Ok(Arc::new(provider))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
