//! Process-global, synchronous peek at the signed-in user id.
//!
//! Sentry `before_send` hooks and similar sync callers cannot await the
//! manager, so the manager mirrors the current user id into this slot on every
//! state change. Only the id is kept — never a token, never the profile.

use std::sync::{OnceLock, RwLock};

static USER_ID: OnceLock<RwLock<Option<String>>> = OnceLock::new();

fn slot() -> &'static RwLock<Option<String>> {
    USER_ID.get_or_init(|| RwLock::new(None))
}

/// Record the signed-in user id (or clear it with `None`).
pub fn set_user_id(user_id: Option<String>) {
    let mut guard = slot().write().unwrap_or_else(|p| p.into_inner());
    *guard = user_id
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty());
}

/// Forget the signed-in user id.
pub fn clear() {
    set_user_id(None);
}

/// The signed-in user id, if any.
pub fn peek_user_id() -> Option<String> {
    slot().read().unwrap_or_else(|p| p.into_inner()).clone()
}
