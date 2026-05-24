//! End-to-end: clean → compose → push the conversation into a tree as one
//! leaf. Uses the [`crate::openhuman::memory_tree`] write contract so the
//! archivist stays unaware of tree internals.
//!
//! The archivist intentionally writes one leaf per archived conversation
//! rather than persisting another bespoke store. `chunk_id_for_session`
//! hashes `(session_id, composed_markdown)` so retries are deterministic for
//! the same conversation snapshot while distinct sessions or edits produce a
//! fresh leaf id.

use anyhow::Result;
use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::openhuman::config::Config;
use crate::openhuman::memory_archivist::clip::clean_conversation;
use crate::openhuman::memory_archivist::compose::compose_conversation_md;
use crate::openhuman::memory_archivist::types::Turn;
use crate::openhuman::memory_store::trees::{Tree, TreeKind};
use crate::openhuman::memory_tree::io::{
    TreeLabelStrategy, TreeLeafPayload, TreeWriteOutcome, TreeWriteRequest,
};
use crate::openhuman::memory_tree::tree::bucket_seal::{append_leaf, LabelStrategy};

const TOKEN_DIVISOR: usize = 4;

/// Clean the conversation, compose it as md, and append a single leaf to
/// the supplied tree. Returns the resulting [`TreeWriteOutcome`] including
/// any summary ids that sealed during the cascade.
pub async fn archive_to_tree(
    config: &Config,
    tree: &Tree,
    session_id: &str,
    turns: &[Turn],
) -> Result<TreeWriteOutcome> {
    let cleaned = clean_conversation(turns);
    let md = compose_conversation_md(&cleaned);
    let chunk_id = chunk_id_for_session(session_id, &md);
    let token_count = (md.len() / TOKEN_DIVISOR).max(1) as u32;
    let timestamp = cleaned
        .last()
        .map(|t| t.timestamp)
        .unwrap_or_else(Utc::now);

    let request = TreeWriteRequest {
        tree_id: tree.id.clone(),
        tree_kind: tree.kind,
        leaf: TreeLeafPayload {
            chunk_id: chunk_id.clone(),
            token_count,
            timestamp,
            content: md,
            entities: Vec::new(),
            topics: Vec::new(),
            score: 0.0,
        },
        label_strategy: TreeLabelStrategy::Inherit,
        deferred: false,
    };

    let leaf_ref = (&request.leaf).into();
    // Cleaned conversations have no extractor-derived entities/topics
    // riding along, so the only meaningful strategy is `Empty`. Callers
    // that want extraction can extend memory_tree::io::TreeLabelStrategy
    // and the dispatch below.
    let _ = request.label_strategy;
    let strategy = LabelStrategy::Empty;
    let new_summary_ids = append_leaf(config, tree, &leaf_ref, &strategy).await?;
    log::debug!(
        "[memory_archivist] archive_to_tree tree_id={} session={} chunk_id={} new_summaries={}",
        tree.id,
        session_id,
        chunk_id,
        new_summary_ids.len()
    );
    Ok(TreeWriteOutcome {
        new_summary_ids,
        seal_pending: false,
    })
}

fn chunk_id_for_session(session_id: &str, md: &str) -> String {
    let mut h = Sha256::new();
    h.update(session_id.as_bytes());
    h.update(b"\0");
    h.update(md.as_bytes());
    let digest = h.finalize();
    let hex = hex::encode(digest);
    format!("archivist:{}", &hex[..32])
}

// Kind helper so callers don't have to import TreeKind themselves when
// they pass a `Tree` they already have. (Re-export for ergonomic match.)
#[allow(dead_code)]
fn _kind_compile_check(t: &Tree) -> TreeKind {
    t.kind
}

#[cfg(test)]
mod tests {
    use super::chunk_id_for_session;

    #[test]
    fn chunk_id_is_stable_for_same_session_and_markdown() {
        let a = chunk_id_for_session("session-1", "## user\nhello\n");
        let b = chunk_id_for_session("session-1", "## user\nhello\n");
        assert_eq!(a, b);
        assert!(a.starts_with("archivist:"));
    }

    #[test]
    fn chunk_id_changes_when_session_changes() {
        let a = chunk_id_for_session("session-1", "## user\nhello\n");
        let b = chunk_id_for_session("session-2", "## user\nhello\n");
        assert_ne!(a, b);
    }

    #[test]
    fn chunk_id_changes_when_markdown_changes() {
        let a = chunk_id_for_session("session-1", "## user\nhello\n");
        let b = chunk_id_for_session("session-1", "## user\nhello again\n");
        assert_ne!(a, b);
    }
}
