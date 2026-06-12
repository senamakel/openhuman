//! Ambient per-turn allowlist of memory-source scopes an agent may recall from.
//!
//! Agent profiles can restrict which memory sources a flavour recalls (the
//! `AgentProfile::memory_sources` allowlist). Threading that allowlist through
//! every memory tool and the deep `select_trees` retrieval layer would touch
//! dozens of call sites, so — mirroring [`thread_context`] — the channel sets a
//! [`tokio::task_local`] around the agent turn and the source-tree retrieval
//! reads it.
//!
//! Semantics:
//! - `None` scope (outside any [`with_source_scope`], or `with_source_scope(None, …)`)
//!   means **unrestricted** — every source tree is visible. This is the default
//!   for cron, sub-agents, the CLI, and any profile that left `memory_sources`
//!   unset.
//! - `Some(set)` restricts recall to source trees whose `scope` string is in the
//!   set. An empty set surfaces nothing (the profile selected no sources).
//!
//! The allowlist entries are matched against tree `scope` strings — the same
//! identifiers the `memory_tree_query_source` tool accepts as `source_id`.
//!
//! [`thread_context`]: crate::openhuman::inference::provider::thread_context
//!
//! ```ignore
//! use crate::openhuman::memory::source_scope::{with_source_scope, current_source_scope};
//!
//! with_source_scope(Some(vec!["slack:#eng".into()]), async {
//!     assert!(current_source_scope().unwrap().contains("slack:#eng"));
//! }).await;
//! ```

use std::collections::HashSet;
use std::future::Future;

tokio::task_local! {
    static SOURCE_SCOPE: Option<HashSet<String>>;
}

/// Normalize a raw allowlist into the task-local representation. Trims entries
/// and drops empties. `None` → unrestricted; `Some(vec)` → restricted (an empty
/// vec stays `Some(empty)` = "no sources").
fn normalize(allowlist: Option<Vec<String>>) -> Option<HashSet<String>> {
    allowlist.map(|items| {
        items
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<HashSet<String>>()
    })
}

/// Run `fut` with `allowlist` available to any descendant call to
/// [`current_source_scope`]. `None` leaves recall unrestricted.
pub async fn with_source_scope<F, T>(allowlist: Option<Vec<String>>, fut: F) -> T
where
    F: Future<Output = T>,
{
    let value = normalize(allowlist);
    log::debug!(
        "[memory:source_scope] entering scope: {}",
        match &value {
            None => "unrestricted".to_string(),
            Some(set) => format!("{} source(s)", set.len()),
        }
    );
    SOURCE_SCOPE.scope(value, fut).await
}

/// Return the ambient source-scope allowlist set by an enclosing
/// [`with_source_scope`], or `None` (unrestricted) when called outside one.
pub fn current_source_scope() -> Option<HashSet<String>> {
    SOURCE_SCOPE.try_with(|v| v.clone()).ok().flatten()
}

/// Whether `scope` is recallable under the ambient allowlist. `true` when there
/// is no active scope (unrestricted) or when the scope is explicitly allowed.
pub fn scope_allowed(scope: &str) -> bool {
    match current_source_scope() {
        None => true,
        Some(set) => set.contains(scope),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unrestricted_outside_scope() {
        assert!(current_source_scope().is_none());
        assert!(scope_allowed("anything"));
    }

    #[tokio::test]
    async fn restricts_to_allowlisted_scopes() {
        with_source_scope(
            Some(vec!["slack:#eng".into(), "  gmail:me  ".into()]),
            async {
                let set = current_source_scope().expect("scope set");
                assert_eq!(set.len(), 2);
                assert!(scope_allowed("slack:#eng"));
                assert!(scope_allowed("gmail:me")); // trimmed
                assert!(!scope_allowed("notion:team"));
            },
        )
        .await;
        // Must not leak past the scope.
        assert!(current_source_scope().is_none());
        assert!(scope_allowed("notion:team"));
    }

    #[tokio::test]
    async fn empty_allowlist_blocks_everything() {
        with_source_scope(Some(vec![]), async {
            assert!(current_source_scope().is_some());
            assert!(!scope_allowed("slack:#eng"));
        })
        .await;
    }

    #[tokio::test]
    async fn explicit_none_is_unrestricted() {
        with_source_scope(None, async {
            assert!(current_source_scope().is_none());
            assert!(scope_allowed("slack:#eng"));
        })
        .await;
    }
}
