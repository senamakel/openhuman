//! [`active_memory_guard`] — how a memory RPC handler reaches the **guarded**
//! driver.
//!
//! This is the read-write twin of
//! [`helpers::active_memory_client`](super::helpers::active_memory_client): the
//! same store, reached through
//! [`MemoryGuard`](crate::openhuman::memory::guard::MemoryGuard) so the seven
//! policy steps in `docs/specs/kernel.md` §3.4 actually run. A handler that has
//! a typed contract twin for what it does calls this; one that does not stays
//! on `active_memory_client` and is listed, with its reason, in
//! `docs/specs/memory-guard-allowlist.md`.
//!
//! ## Two resolution paths, and why the second one exists
//!
//! The primary path is the ambient [`CoreContext`], exactly as
//! [`ops::provider`](super::provider) already resolves the binding for the
//! health probe. Under RPC dispatch a context is always present and its
//! workspace is always bound, so that is the only path production takes.
//!
//! The fallback path exists for callers that run *before* a context is built —
//! in practice the roughly four thousand pre-boot unit tests, which bind the
//! process-global client through `ensure_shared_memory_client()` and never
//! build a `CoreContext`. `CoreContext::memory()` cannot serve them:
//! `memory_binding()` goes through `workspace_dir()`, which errors outright
//! when the context has no bound workspace. `active_memory_client()` already
//! has an answer for that state — it lazily initialises — so this mirrors it,
//! with one refinement: it prefers the workspace the global client is
//! **already** bound to over whatever `Config::load_or_init` reports. Resolving
//! through the config instead would hand a test a binding over a *different*
//! workspace than the client its fixtures wrote to, which is a silently wrong
//! store rather than a visible failure.
//!
//! The fallback binds with [`MemorySubsystemConfig::default`] (driver
//! `"tinycortex"`, default hook budgets). That is the right default precisely
//! because it is only reachable with no context: a context always carries the
//! operator's real `[subsystems.memory]` block and takes the first path.

use std::sync::Arc;

use crate::core::runtime::context::CoreContext;
use crate::openhuman::config::schema::MemorySubsystemConfig;
use crate::openhuman::memory::binding;
use crate::openhuman::memory::guard::MemoryGuard;
use tinymemory_core::global;

/// The guarded memory driver for this dispatch.
///
/// # Errors
///
/// When neither resolution path can name a workspace — no ambient context and
/// no global client, with `Config::load_or_init` also failing — or when the
/// binding cache lock is poisoned.
pub(crate) async fn active_memory_guard() -> Result<Arc<MemoryGuard>, String> {
    if let Some(ctx) = CoreContext::current() {
        match ctx.memory() {
            Ok(guard) => return Ok(guard),
            Err(error) => log::debug!(
                "[memory:guard] ambient context has no bound workspace ({error}); \
                 resolving as active_memory_client does"
            ),
        }
    }

    let workspace_dir = match global::active_workspace_dir() {
        Some(dir) => dir,
        None => super::helpers::current_workspace_dir().await?,
    };
    log::debug!(
        "[memory:guard] no context binding; guarding workspace={}",
        workspace_dir.display()
    );
    Ok(binding::for_workspace(&workspace_dir, &MemorySubsystemConfig::default())?.guard())
}

/// The guarded memory driver for **one named workspace**, rather than the
/// ambient one.
///
/// # Why this exists beside [`active_memory_guard`]
///
/// A handler that takes `config: &Config` and reads through it is naming a
/// store, and callers do name a non-ambient one. `active_memory_guard`
/// resolves the *ambient* binding, so using it in such a handler silently
/// ignores the argument — and under RPC dispatch the two happen to agree, so
/// the mistake is invisible in production and only shows up for a caller with
/// its own config. That is the same "silently wrong store rather than a
/// visible failure" this module's header warns about for the fallback path,
/// reached through the argument instead.
///
/// This keeps the bypass ratchet's single-site property: `for_workspace` is a
/// policed needle because a raw `MemoryBinding` skips the guard, so the call
/// stays here and callers get an `Arc<MemoryGuard>` they cannot unwrap.
///
/// # Errors
///
/// Only binding-cache lock poisoning; an inadmissible driver falls back rather
/// than failing.
pub(crate) fn guard_for_config(
    config: &crate::openhuman::config::Config,
) -> Result<Arc<MemoryGuard>, String> {
    log::debug!(
        "[memory:guard] guarding caller-named workspace={}",
        config.workspace_dir.display()
    );
    Ok(binding::for_workspace(&config.workspace_dir, &config.subsystems.memory)?.guard())
}

#[cfg(test)]
#[path = "guard_tests.rs"]
mod tests;
