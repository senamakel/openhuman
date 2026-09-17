//! The prepare→execute quote store: a short-lived, capped, in-memory queue
//! of `PreparedTransaction`s keyed by `quote_id`, plus the chat-thread
//! ownership check that stops a leaked `quote_id` from being executed by a
//! different agent session.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use log::debug;
use once_cell::sync::Lazy;
use parking_lot::Mutex;

use super::types::{PreparedStatus, PreparedTransaction, QuoteOwner};
use super::LOG_PREFIX;

pub(super) const QUOTE_TTL_MS: u64 = 5 * 60 * 1000;
const QUOTE_STORE_CAP: usize = 64;

static QUOTE_STORE: Lazy<Mutex<Vec<PreparedTransaction>>> = Lazy::new(|| Mutex::new(Vec::new()));
static QUOTE_COUNTER: AtomicU64 = AtomicU64::new(1);

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(super) fn next_quote_id() -> String {
    let n = QUOTE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("q_{}_{}", now_ms(), n)
}

/// Read the per-turn chat context that scopes the agent tool loop.
///
/// Returns `Some(owner)` when called from inside an interactive chat turn
/// (the web channel installs `APPROVAL_CHAT_CONTEXT` around `run_chat_task`).
/// Returns `None` for non-chat callers (CLI, direct JSON-RPC, background
/// triage / cron / sub-agents) — these keep the pre-binding behavior and
/// remain executable without an owner gate, since they have no shared
/// channel from which a `quote_id` could leak.
///
// SAFETY: relies on the inline `.await` chain in
// `web_chat::run_chat_task`. `tokio::task_local!` propagates
// across `.await` but **not** across `tokio::spawn`. If the chat path ever
// detaches the tool loop onto a freshly-spawned task without wrapping it in
// `APPROVAL_CHAT_CONTEXT.scope(...)`, this helper will silently start
// returning `None` and the owner gate will become a no-op. Keep the
// prepare/execute calls inline within the scope.
pub(crate) fn current_owner() -> Option<QuoteOwner> {
    crate::security::approval::APPROVAL_CHAT_CONTEXT
        .try_with(|ctx| QuoteOwner {
            thread_id: ctx.thread_id.clone(),
            client_id: ctx.client_id.clone(),
        })
        .ok()
}

pub(super) fn store_quote(quote: PreparedTransaction) -> PreparedTransaction {
    let mut store = QUOTE_STORE.lock();
    let cutoff = now_ms();
    store.retain(|q| q.expires_at_ms > cutoff && q.status != PreparedStatus::Consumed);
    if store.len() >= QUOTE_STORE_CAP {
        store.remove(0);
    }
    store.push(quote.clone());
    quote
}

pub(super) fn get_quote(quote_id: &str) -> Result<PreparedTransaction, String> {
    let store = QUOTE_STORE.lock();
    let now = now_ms();
    let quote = store
        .iter()
        .find(|q| q.quote_id == quote_id)
        .cloned()
        .ok_or_else(|| format!("quote '{quote_id}' not found"))?;
    if quote.status == PreparedStatus::Consumed {
        return Err(format!("quote '{quote_id}' already executed"));
    }
    if quote.expires_at_ms <= now {
        return Err(format!("quote '{quote_id}' expired"));
    }
    Ok(quote)
}

/// Remove a quote from the store and return it to the caller, if and only if
/// the caller's chat-thread owner matches the prepare-time owner.
///
/// On owner mismatch this returns the **exact same** "quote '…' not found"
/// error shape that a missing-row lookup would, so cross-thread callers
/// cannot distinguish "wrong owner" from "no such quote" — i.e. no
/// enumeration oracle for leaked `quote_id`s.
///
/// Callers with no chat context (`caller_owner == None`, e.g. CLI / direct
/// JSON-RPC / background turns) can only execute quotes that were also
/// prepared with no chat context. This intentionally prevents privilege-drop
/// where a background flow could pick up an interactive user's quote.
pub(super) fn take_quote_for(
    quote_id: &str,
    caller_owner: Option<QuoteOwner>,
) -> Result<PreparedTransaction, String> {
    let not_found = || format!("quote '{quote_id}' not found");
    let mut store = QUOTE_STORE.lock();
    let now = now_ms();
    let pos = store
        .iter()
        .position(|q| q.quote_id == quote_id)
        .ok_or_else(not_found)?;
    // Owner check happens before status / expiry checks so the error shape on
    // mismatch can be byte-equal to the not-found path. Removing the quote
    // only happens *after* this check passes — a mismatched caller cannot
    // poison the store by consuming someone else's quote.
    if store[pos].owner != caller_owner {
        debug!(
            "{LOG_PREFIX} take_quote_for quote_id={} owner_mismatch (caller_has_ctx={})",
            quote_id,
            caller_owner.is_some()
        );
        return Err(not_found());
    }
    let quote = store.remove(pos);
    if quote.status == PreparedStatus::Consumed {
        return Err(format!("quote '{quote_id}' already executed"));
    }
    if quote.expires_at_ms <= now {
        return Err(format!("quote '{quote_id}' expired"));
    }
    Ok(quote)
}

pub fn prepared_quotes_for_test() -> Vec<PreparedTransaction> {
    let now = now_ms();
    QUOTE_STORE
        .lock()
        .iter()
        .filter(|q| q.expires_at_ms > now && q.status != PreparedStatus::Consumed)
        .cloned()
        .collect()
}

#[cfg(test)]
pub(crate) fn reset_quote_store_for_tests() {
    QUOTE_STORE.lock().clear();
}

#[cfg(test)]
pub(crate) fn insert_quote_for_test(quote: PreparedTransaction) -> PreparedTransaction {
    store_quote(quote)
}
