//! Singleton registry for the global activity digest tree (#709, Phase 3b).
//!
//! Unlike source trees (one per `source_id`) the global tree is a true
//! singleton per workspace — scope is the literal string `"global"`. The
//! underlying get-or-create dance (including the UNIQUE-race recovery) is
//! handled by the shared [`crate::openhuman::memory_tree::tree::registry::get_or_create_tree`].

use anyhow::Result;

use crate::openhuman::config::Config;
use crate::openhuman::memory_tree::tree::registry::get_or_create_tree;
use crate::openhuman::memory_tree::tree::types::{Tree, TreeKind};
use crate::openhuman::memory_tree::tree_global::GLOBAL_SCOPE;

/// Return the workspace's singleton global tree, creating it lazily on
/// first call. Safe to call on every ingest; subsequent calls short-circuit
/// to the existing row.
pub fn get_or_create_global_tree(config: &Config) -> Result<Tree> {
    log::debug!("[tree_global::registry] get_or_create_global_tree");
    let tree = get_or_create_tree(config, TreeKind::Global, GLOBAL_SCOPE)?;
    log::debug!("[tree_global::registry] global tree id={}", tree.id);
    Ok(tree)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::memory_tree::tree::store;
    use crate::openhuman::memory_tree::tree::types::TreeStatus;
    use chrono::Utc;
    use tempfile::TempDir;

    fn test_config() -> (TempDir, Config) {
        let tmp = TempDir::new().unwrap();
        let mut cfg = Config::default();
        cfg.workspace_dir = tmp.path().to_path_buf();
        (tmp, cfg)
    }

    #[test]
    fn get_or_create_is_idempotent() {
        let (_tmp, cfg) = test_config();
        let first = get_or_create_global_tree(&cfg).unwrap();
        let second = get_or_create_global_tree(&cfg).unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.kind, TreeKind::Global);
        assert_eq!(first.scope, GLOBAL_SCOPE);
        assert_eq!(first.status, TreeStatus::Active);
    }

    #[test]
    fn global_tree_has_expected_id_prefix() {
        let (_tmp, cfg) = test_config();
        let tree = get_or_create_global_tree(&cfg).unwrap();
        assert!(tree.id.starts_with("global:"));
    }

    #[test]
    fn race_recovery_returns_existing_row() {
        use crate::openhuman::memory_tree::tree::types::Tree;
        let (_tmp, cfg) = test_config();
        let pre_existing = Tree {
            id: "global:preexisting".into(),
            kind: TreeKind::Global,
            scope: GLOBAL_SCOPE.into(),
            root_id: None,
            max_level: 0,
            status: TreeStatus::Active,
            created_at: Utc::now(),
            last_sealed_at: None,
        };
        store::insert_tree(&cfg, &pre_existing).unwrap();

        let got = get_or_create_global_tree(&cfg).unwrap();
        assert_eq!(got.id, "global:preexisting");
    }
}
