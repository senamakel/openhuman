//! Topic tree registry — get-or-create / archive (#709 Phase 3c).
//!
//! Topic trees share the same `mem_tree_trees` schema as source and global
//! trees; the only difference is `kind = 'topic'` and
//! `scope = <entity canonical id>`. The underlying get-or-create dance
//! (including UNIQUE-race recovery) is handled by the shared
//! [`crate::openhuman::memory_tree::tree::registry::get_or_create_tree`].
//!
//! Callers should NOT reach into this module to create topic trees
//! eagerly — use the curator ([`super::curator::maybe_spawn_topic_tree`])
//! so creation is gated on hotness. Admin flows (future RPC) that want to
//! bypass the gate can call [`force_create_topic_tree`] directly.

use anyhow::{Context, Result};
use chrono::Utc;

use crate::openhuman::config::Config;
use crate::openhuman::memory_tree::tree::registry::get_or_create_tree;
use crate::openhuman::memory_tree::tree::types::{Tree, TreeKind, TreeStatus};

/// Look up the topic tree for `entity_id`, or create a new one.
///
/// The `entity_id` is a canonical id from the entity resolver (e.g.
/// `"email:alice@example.com"` or `"hashtag:launch"`). Scope uses the
/// canonical id verbatim so re-lookups are stable.
pub fn get_or_create_topic_tree(config: &Config, entity_id: &str) -> Result<Tree> {
    let entity_kind = entity_id
        .split_once(':')
        .map(|(k, _)| k)
        .unwrap_or("unknown");
    log::debug!(
        "[tree_topic::registry] get_or_create_topic_tree entity_kind={}",
        entity_kind
    );
    let tree = get_or_create_tree(config, TreeKind::Topic, entity_id)?;
    log::debug!(
        "[tree_topic::registry] topic tree id={} entity_kind={}",
        tree.id,
        entity_kind
    );
    Ok(tree)
}

/// Public alias used by the admin "force materialise" path — semantically
/// identical to [`get_or_create_topic_tree`] but named to make intent at
/// the call site obvious.
pub fn force_create_topic_tree(config: &Config, entity_id: &str) -> Result<Tree> {
    get_or_create_topic_tree(config, entity_id)
}

/// List all topic trees (both active and archived). Ordered by creation time
/// ascending for stable output.
pub fn list_topic_trees(config: &Config) -> Result<Vec<Tree>> {
    use rusqlite::params;
    crate::openhuman::memory_tree::store::with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, kind, scope, root_id, max_level, status,
                    created_at_ms, last_sealed_at_ms
               FROM mem_tree_trees
              WHERE kind = ?1
              ORDER BY created_at_ms ASC",
        )?;
        let rows = stmt
            .query_map(params![TreeKind::Topic.as_str()], row_to_tree_loose)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to list topic trees")?;
        Ok(rows)
    })
}

/// Flip a topic tree's status to `archived`. Existing rows remain queryable;
/// new leaves will NOT be routed to this tree until it's manually unarchived
/// (unarchive is not a Phase 3c primitive — Phase 3c just stops routing).
pub fn archive_topic_tree(config: &Config, tree_id: &str) -> Result<()> {
    use rusqlite::params;
    crate::openhuman::memory_tree::store::with_connection(config, |conn| {
        let n = conn
            .execute(
                "UPDATE mem_tree_trees
                    SET status = ?1
                  WHERE id = ?2 AND kind = ?3",
                params![
                    TreeStatus::Archived.as_str(),
                    tree_id,
                    TreeKind::Topic.as_str()
                ],
            )
            .with_context(|| format!("failed to archive topic tree {tree_id}"))?;
        if n == 0 {
            log::warn!(
                "[tree_topic::registry] archive_topic_tree: no topic tree with id={tree_id}"
            );
        } else {
            log::info!("[tree_topic::registry] archived topic tree id={tree_id}");
        }
        Ok(())
    })
}

/// Row mapper — kept intentionally loose: topic-tree listing is not a hot
/// path so the string parsing cost is immaterial.
fn row_to_tree_loose(row: &rusqlite::Row<'_>) -> rusqlite::Result<Tree> {
    use chrono::TimeZone;
    let id: String = row.get(0)?;
    let kind_s: String = row.get(1)?;
    let scope: String = row.get(2)?;
    let root_id: Option<String> = row.get(3)?;
    let max_level: i64 = row.get(4)?;
    let status_s: String = row.get(5)?;
    let created_ms: i64 = row.get(6)?;
    let last_sealed_ms: Option<i64> = row.get(7)?;

    let kind = TreeKind::parse(&kind_s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, e.into())
    })?;
    let status = TreeStatus::parse(&status_s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, e.into())
    })?;
    let created_at = Utc
        .timestamp_millis_opt(created_ms)
        .single()
        .ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Integer,
                format!("invalid created_at_ms {created_ms}").into(),
            )
        })?;
    let last_sealed_at = last_sealed_ms
        .map(|ms| {
            Utc.timestamp_millis_opt(ms).single().ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    7,
                    rusqlite::types::Type::Integer,
                    format!("invalid last_sealed_at_ms {ms}").into(),
                )
            })
        })
        .transpose()?;

    Ok(Tree {
        id,
        kind,
        scope,
        root_id,
        max_level: max_level.max(0) as u32,
        status,
        created_at,
        last_sealed_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::memory_tree::tree::store;
    use tempfile::TempDir;

    fn test_config() -> (TempDir, Config) {
        let tmp = TempDir::new().unwrap();
        let mut cfg = Config::default();
        cfg.workspace_dir = tmp.path().to_path_buf();
        (tmp, cfg)
    }

    #[test]
    fn get_or_create_is_idempotent_on_entity_id() {
        let (_tmp, cfg) = test_config();
        let first = get_or_create_topic_tree(&cfg, "email:alice@example.com").unwrap();
        let second = get_or_create_topic_tree(&cfg, "email:alice@example.com").unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.kind, TreeKind::Topic);
        assert_eq!(first.status, TreeStatus::Active);
        assert_eq!(first.scope, "email:alice@example.com");
    }

    #[test]
    fn different_entities_yield_different_trees() {
        let (_tmp, cfg) = test_config();
        let a = get_or_create_topic_tree(&cfg, "email:alice@example.com").unwrap();
        let b = get_or_create_topic_tree(&cfg, "email:bob@example.com").unwrap();
        assert_ne!(a.id, b.id);
        assert_ne!(a.scope, b.scope);
    }

    #[test]
    fn topic_tree_and_source_tree_share_scope_space_cleanly() {
        let (_tmp, cfg) = test_config();
        let source = crate::openhuman::memory_tree::sources::registry::get_or_create_source_tree(
            &cfg,
            "shared:slack:#eng",
        )
        .unwrap();
        let topic = get_or_create_topic_tree(&cfg, "shared:slack:#eng").unwrap();
        assert_ne!(source.id, topic.id);
        assert_eq!(source.kind, TreeKind::Source);
        assert_eq!(topic.kind, TreeKind::Topic);
    }

    #[test]
    fn topic_tree_id_has_expected_prefix() {
        let (_tmp, cfg) = test_config();
        let tree = get_or_create_topic_tree(&cfg, "email:alice@example.com").unwrap();
        assert!(tree.id.starts_with("topic:"));
    }

    #[test]
    fn archive_flips_status_and_keeps_rows_readable() {
        let (_tmp, cfg) = test_config();
        let t = get_or_create_topic_tree(&cfg, "email:alice@example.com").unwrap();
        archive_topic_tree(&cfg, &t.id).unwrap();
        let refetched = store::get_tree(&cfg, &t.id).unwrap().unwrap();
        assert_eq!(refetched.status, TreeStatus::Archived);
        let again = get_or_create_topic_tree(&cfg, "email:alice@example.com").unwrap();
        assert_eq!(again.id, t.id);
        assert_eq!(again.status, TreeStatus::Archived);
    }

    #[test]
    fn archive_is_noop_on_nonexistent() {
        let (_tmp, cfg) = test_config();
        archive_topic_tree(&cfg, "topic:does-not-exist").unwrap();
    }

    #[test]
    fn list_topic_trees_returns_only_topics() {
        let (_tmp, cfg) = test_config();
        crate::openhuman::memory_tree::sources::registry::get_or_create_source_tree(
            &cfg,
            "slack:#eng",
        )
        .unwrap();
        get_or_create_topic_tree(&cfg, "email:alice@example.com").unwrap();
        get_or_create_topic_tree(&cfg, "email:bob@example.com").unwrap();

        let topics = list_topic_trees(&cfg).unwrap();
        assert_eq!(topics.len(), 2);
        for t in &topics {
            assert_eq!(t.kind, TreeKind::Topic);
        }
    }
}
