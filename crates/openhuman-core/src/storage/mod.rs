//! The process's storage backend on the `tinystoragedrivers` ports.
//!
//! One URL picks where durable state that has moved onto the storage ports
//! lives: `OPENHUMAN_STORAGE_URL`, else `[storage] url` in `config.toml`
//! ([`crate::config::StorageConfig`]). With neither set — the desktop default —
//! nothing here is opened and every domain keeps the classic on-disk layout
//! under the workspace.
//!
//! When a URL is set, the host opens it once at startup ([`open`]) and
//! installs it ([`install`]); domains reach it through [`installed`] and bind
//! their records to an agent with [`scope_for_agent`]. The first consumer is
//! the session store: the host installs TinyAgents' `DriverSessionStores`
//! over this backend, so transcripts, turn states, records and journals live
//! in it, one scope per agent.
//!
//! Drivers are Cargo features of this crate: `storage-sqlite`,
//! `storage-mongodb`, `storage-file` (memory is always available). A URL for a
//! driver the build does not carry fails at [`open`] naming the feature, so a
//! misconfigured deployment stops at boot instead of at its first write.

pub mod agents;
pub mod documents;
pub mod lease;
mod lease_documents;
mod lease_local;
pub mod secrets;

use std::future::Future;
use std::sync::{Arc, LazyLock, OnceLock, RwLock};

pub use tinystoragedrivers::{
    Blocking, DocumentStore, DocumentStoreExt, MemoryStorage, Scope, ScopedStorage, StorageBackend,
    StorageConfig as StorageUrl, StorageError,
};

use crate::config::schema::storage::redact_url;
use crate::config::Config;

/// The environment variable that overrides `[storage] url`.
pub const STORAGE_URL_VAR: &str = "OPENHUMAN_STORAGE_URL";

/// A holder for one backend. The process has exactly one ([`BACKEND`]);
/// tests make their own, so they never change what other tests in the same
/// process see.
#[derive(Default)]
struct Slot(RwLock<Option<Arc<dyn StorageBackend>>>);

impl Slot {
    fn install(&self, backend: Arc<dyn StorageBackend>) -> Option<Arc<dyn StorageBackend>> {
        self.0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .replace(backend)
    }

    fn installed(&self) -> Option<Arc<dyn StorageBackend>> {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn clear(&self) -> bool {
        self.0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .is_some()
    }
}

static BACKEND: LazyLock<Slot> = LazyLock::new(Slot::default);

/// The storage URL in effect: [`STORAGE_URL_VAR`], else `config`'s
/// `[storage] url`. Blank values count as unset. `None` keeps the classic
/// on-disk layout.
pub fn configured_url(config: &Config) -> Option<String> {
    url_from(std::env::var(STORAGE_URL_VAR).ok(), config)
}

/// [`configured_url`] with the environment read made explicit, so the rule
/// is testable without mutating process-wide state.
pub fn url_from(env: Option<String>, config: &Config) -> Option<String> {
    env.into_iter()
        .chain(config.storage.url.clone())
        .map(|url| url.trim().to_string())
        .find(|url| !url.is_empty())
}

/// Parses and opens the backend `url` names.
///
/// # Errors
///
/// An unparseable URL, a driver this build was compiled without (the error
/// names the Cargo feature), or a backend that cannot be reached.
pub async fn open(url: &str) -> Result<Arc<dyn StorageBackend>, StorageError> {
    let parsed = StorageUrl::parse(url)?;
    tracing::info!(
        target: "openhuman::storage",
        driver = parsed.driver(),
        url = %redact_url(url),
        "[storage] opening the configured backend"
    );
    tinystoragedrivers::open(&parsed).await
}

/// Makes `backend` the process's storage backend; returns the previous one.
pub fn install(backend: Arc<dyn StorageBackend>) -> Option<Arc<dyn StorageBackend>> {
    let previous = BACKEND.install(backend);
    // What was recorded described the previous backend.
    agents::reset_recorded();
    // Agents derived before the backend existed still need recording.
    agents::record_live();
    previous
}

/// The installed backend, when the host configured one.
pub fn installed() -> Option<Arc<dyn StorageBackend>> {
    BACKEND.installed()
}

/// Removes the installed backend; returns whether there was one.
pub fn clear() -> bool {
    agents::reset_recorded();
    BACKEND.clear()
}

/// The storage scope of the current call: the tenant's profile
/// ([`scope_for_profile`]) when the work runs for one, else the acting
/// agent's ([`scope_for_agent`], `CoreContext::session_agent`), else
/// [`Scope::local`] — except in SaaS mode, where only a profile scope is
/// handed out: a call with no profile (or no task scope at all) is refused
/// rather than given a bucket other users could share.
///
/// # Errors
///
/// In SaaS mode, when the current task serves no profile.
pub fn current_scope() -> Result<Scope, StorageError> {
    let saas = crate::core::runtime::mode::is_saas();
    match crate::core::runtime::current_tenant() {
        Ok(tenant) => scope_from(tenant.profile.as_deref(), tenant.agent.as_deref(), saas),
        Err(no_tenant) => Err(StorageError::invalid_input(no_tenant.to_string())),
    }
}

/// [`current_scope`] with its inputs made explicit, so the rule is testable
/// without a booted context or a locked mode.
///
/// # Errors
///
/// When `saas` and there is no `profile`.
pub fn scope_from(
    profile: Option<&str>,
    agent: Option<&str>,
    saas: bool,
) -> Result<Scope, StorageError> {
    match (profile, agent) {
        (Some(profile), _) => Ok(scope_for_profile(profile)),
        (None, _) if saas => Err(StorageError::invalid_input(
            "no profile in SaaS mode; refusing a shared storage scope",
        )),
        (None, Some(agent)) => Ok(scope_for_agent(agent)),
        (None, None) => Ok(Scope::local()),
    }
}

/// The installed backend bound to [`current_scope`], or `None` when the host
/// configured no backend (the classic on-disk layout).
///
/// # Errors
///
/// When the scope cannot be resolved ([`current_scope`]) or the backend
/// refuses it.
pub fn current_scoped() -> Result<Option<ScopedStorage>, StorageError> {
    installed()
        .map(|backend| backend.for_scope(&current_scope()?))
        .transpose()
}

/// Runs `future` to completion from synchronous code, on one dedicated
/// runtime thread shared by every caller in the process.
///
/// For domain stores whose API is synchronous (most of the core's), so they
/// can call the async storage ports without `block_in_place` — which would
/// panic on a current-thread runtime.
///
/// # Errors
///
/// When the bridge cannot start, or `future` called back into it.
pub fn block_on<T, F>(future: F) -> Result<T, StorageError>
where
    F: Future<Output = Result<T, StorageError>> + Send + 'static,
    T: Send + 'static,
{
    static BRIDGE: OnceLock<Result<Blocking, String>> = OnceLock::new();
    let bridge = BRIDGE
        .get_or_init(|| Blocking::new().map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| StorageError::backend(error.clone()))?;
    bridge.run(future)?
}

/// [`block_on`] for a future whose error is an `anyhow::Error` — the stores
/// tinyflows puts on the ports return those, with typed errors (such as
/// `FlowUpdateError`) a caller may downcast, so they pass through unchanged.
///
/// # Errors
///
/// The future's own error, or the bridge's when it cannot start.
pub fn block_on_anyhow<T, F>(future: F) -> anyhow::Result<T>
where
    F: Future<Output = anyhow::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    block_on(async move { Ok(future.await) })?
}

/// Whether a backend with this driver name may be shared by several
/// processes at once. A MongoDB database can be; SQLite files, the memory
/// driver and plain files belong to the one process that opened them.
///
/// Boot-time recovery (interrupting in-flight turns, reaping orphaned runs)
/// is only sound on a backend no other process can be writing to.
pub fn driver_is_shared(driver: &str) -> bool {
    driver == "mongodb"
}

/// Whether a backend with this driver name makes a compare-and-swap
/// (`Precondition::Version` / `Absent`) atomic across processes, which a
/// [`lease::DocumentLeases`] needs to exclude other nodes. MongoDB and SQLite
/// do (a database-side conditional write; SQLite's immediate transaction
/// under the file lock); the memory driver lives in one process, and the
/// file driver checks versions in-process only.
pub fn driver_has_cross_process_cas(driver: &str) -> bool {
    matches!(driver, "mongodb" | "sqlite")
}

/// Whether the installed backend may be shared with other processes; `false`
/// when none is installed. See [`driver_is_shared`].
pub fn installed_is_shared() -> bool {
    installed().is_some_and(|backend| driver_is_shared(backend.driver()))
}

/// The storage scope agent `agent_id`'s records live under — the same
/// mapping the session store uses, so every domain agrees on it.
pub fn scope_for_agent(agent_id: &str) -> Scope {
    tinyagents_session::DriverSessionStores::scope_for(agent_id)
}

/// The storage scope profile `profile_id`'s records live under:
/// `profile:<id>`, or `profile-sha256:<hex of the id>` when that is not a
/// valid scope (too long, whitespace, control characters). Injective: a
/// literal scope always starts with `profile:`, a hashed one never does, and
/// two distinct profiles never share either form.
pub fn scope_for_profile(profile_id: &str) -> Scope {
    const PREFIX: &str = "profile:";
    let literal = format!("{PREFIX}{profile_id}");
    if let Ok(scope) = Scope::new(&literal) {
        return scope;
    }
    use sha2::Digest;
    let digest = sha2::Sha256::digest(profile_id.as_bytes());
    Scope::new(format!("profile-sha256:{}", hex::encode(digest)))
        .unwrap_or_else(|_| unreachable!("a sha256 hex scope is always valid"))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
