use super::*;
use crate::openhuman::inference::embeddings::NoopEmbedding;
use crate::openhuman::integrations::composio::providers::sync_state::KV_NAMESPACE;
use chrono::{TimeZone, Utc};
use rusqlite::params;
use std::sync::Arc;
use tempfile::TempDir;
use tinycortex::memory::ingest::canonicalize::chat::{ChatBatch, ChatMessage};
use tinymemory_core::ingest_pipeline::ingest_chat;
use tinymemory_core::queue::drain_until_idle;
use tinymemory_core::store::content::raw::{write_raw_items, RawItem, RawKind};
use tinymemory_core::store::namespace_store::UnifiedMemory;

fn test_config() -> (TempDir, Config) {
    let tmp = TempDir::new().unwrap();
    let mut cfg = Config::default();
    cfg.workspace_dir = tmp.path().to_path_buf();
    // Point config_path inside the tempdir so any persistence during
    // tests stays inside disposable workspace state.
    cfg.config_path = tmp.path().join("config.toml");
    cfg.memory_tree.embedding_endpoint = None;
    cfg.memory_tree.embedding_model = None;
    cfg.memory_tree.embedding_strict = false;
    // Default llm is Cloud — but the cloud provider needs a bearer
    // token to actually fire. Tests that exercise the LLM path
    // override either the backend or the extractor. The read RPCs
    // below don't touch the LLM, so this default is fine.
    (tmp, cfg)
}

async fn seed_chat_chunk(cfg: &Config, source: &str, body: &str) {
    let batch = ChatBatch {
        platform: "slack".into(),
        channel_label: source.into(),
        messages: vec![ChatMessage {
            author: "alice".into(),
            timestamp: Utc.timestamp_millis_opt(1_700_000_000_000).unwrap(),
            text: body.into(),
            source_ref: Some("slack://x".into()),
        }],
    };
    ingest_chat(cfg, source, "alice", vec![], batch)
        .await
        .unwrap();
}

async fn seed_slack_chunk_with_raw_archive(cfg: &Config) -> String {
    let timestamp = Utc.timestamp_opt(1_700_000_000, 0).single().unwrap();
    write_raw_items(
        &cfg.memory_tree_content_root(),
        "slack:conn-slack-1",
        &[RawItem {
            uid: "1700000000.000100",
            created_at_ms: timestamp.timestamp_millis(),
            markdown: "**Channel:** #engineering\n**Author:** alice\n\nPhoenix migration launch window is Friday at 22:00 UTC.",
            kind: RawKind::Chat,
        }],
    )
    .expect("seed raw Slack artifact");
    let batch = ChatBatch {
        platform: "slack".into(),
        channel_label: "#engineering".into(),
        messages: vec![ChatMessage {
            author: "alice".into(),
            timestamp,
            text: "Phoenix migration launch window is Friday at 22:00 UTC.".into(),
            source_ref: Some("slack://archives/C123/1700000000.000100".into()),
        }],
    };
    ingest_chat(
        cfg,
        "slack:conn-slack-1",
        "alice",
        vec!["slack".into(), "ingested".into()],
        batch,
    )
    .await
    .expect("seed slack ingest");
    drain_until_idle(cfg).await.expect("drain slack ingest");

    list_chunks_rpc(cfg, ChunkFilter::default())
        .await
        .expect("list chunks")
        .value
        .chunks
        .into_iter()
        .find(|chunk| chunk.source_id == "slack:conn-slack-1")
        .expect("seeded slack chunk")
        .id
}

fn update_chunk_timestamp(cfg: &Config, chunk_id: &str, timestamp_ms: i64) {
    with_connection(cfg, |conn| {
        conn.execute(
            "UPDATE mem_tree_chunks
                SET timestamp_ms = ?1,
                    time_range_start_ms = ?1,
                    time_range_end_ms = ?1
              WHERE id = ?2",
            params![timestamp_ms, chunk_id],
        )?;
        Ok(())
    })
    .unwrap();
}

fn insert_raw_chunk(
    cfg: &Config,
    id: &str,
    source_kind: &str,
    source_id: &str,
    timestamp_ms: i64,
    tags_json: &str,
    content: &str,
    token_count: i64,
) {
    with_connection(cfg, |conn| {
        conn.execute(
            "INSERT INTO mem_tree_chunks (
                id, source_kind, source_id, source_ref, owner, timestamp_ms,
                time_range_start_ms, time_range_end_ms, tags_json, content,
                token_count, seq_in_source, created_at_ms, lifecycle_status, content_path
             ) VALUES (?1, ?2, ?3, NULL, 'tester', ?4, ?4, ?4, ?5, ?6, ?7, 0, ?4, 'seeded', NULL)",
            params![
                id,
                source_kind,
                source_id,
                timestamp_ms,
                tags_json,
                content,
                token_count
            ],
        )?;
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn list_chunks_returns_seeded_chunk() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "hello @alice phoenix migration").await;
    let resp = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value;
    assert!(!resp.chunks.is_empty());
    assert_eq!(resp.total, resp.chunks.len() as u64);
}

#[tokio::test]
async fn list_chunks_filters_by_source_id() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#a", "alpha").await;
    seed_chat_chunk(&cfg, "slack:#b", "beta").await;
    let only_a = list_chunks_rpc(
        &cfg,
        ChunkFilter {
            source_ids: Some(vec!["slack:#a".into()]),
            ..ChunkFilter::default()
        },
    )
    .await
    .unwrap()
    .value;
    assert!(only_a.chunks.iter().all(|c| c.source_id == "slack:#a"));
    assert!(only_a.total >= 1);
}

#[tokio::test]
async fn list_chunks_query_substring_works() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "phoenix migration ships friday").await;
    seed_chat_chunk(&cfg, "slack:#eng", "different unrelated text").await;
    let resp = list_chunks_rpc(
        &cfg,
        ChunkFilter {
            query: Some("phoenix".into()),
            ..ChunkFilter::default()
        },
    )
    .await
    .unwrap()
    .value;
    assert!(resp.chunks.iter().any(|c| {
        c.content_preview
            .as_deref()
            .unwrap_or("")
            .contains("phoenix")
    }));
}

#[tokio::test]
async fn list_chunks_filters_by_source_kind_and_applies_limit_offset() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#a", "first chat").await;
    seed_chat_chunk(&cfg, "slack:#b", "second chat").await;

    let filtered = list_chunks_rpc(
        &cfg,
        ChunkFilter {
            source_kinds: Some(vec!["chat".into()]),
            limit: Some(1),
            offset: Some(1),
            ..ChunkFilter::default()
        },
    )
    .await
    .unwrap()
    .value;
    assert_eq!(filtered.chunks.len(), 1);
    assert_eq!(filtered.total, 2);
    assert!(filtered.chunks.iter().all(|c| c.source_kind == "chat"));
}

#[tokio::test]
async fn list_chunks_filters_by_entity_id_and_time_window() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "alice@example.com handles phoenix").await;
    seed_chat_chunk(&cfg, "slack:#eng", "bob@example.com handles atlas").await;

    let seeded = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value
        .chunks;
    let alice = seeded
        .iter()
        .find(|chunk| {
            chunk
                .content_preview
                .as_deref()
                .unwrap_or("")
                .contains("alice@example.com")
        })
        .expect("alice chunk present");
    let bob = seeded
        .iter()
        .find(|chunk| {
            chunk
                .content_preview
                .as_deref()
                .unwrap_or("")
                .contains("bob@example.com")
        })
        .expect("bob chunk present");

    update_chunk_timestamp(&cfg, &alice.id, 1_700_000_000_100);
    update_chunk_timestamp(&cfg, &bob.id, 1_700_000_000_900);

    let filtered = list_chunks_rpc(
        &cfg,
        ChunkFilter {
            entity_ids: Some(vec!["email:alice@example.com".into()]),
            since_ms: Some(1_700_000_000_000),
            until_ms: Some(1_700_000_000_500),
            ..ChunkFilter::default()
        },
    )
    .await
    .unwrap()
    .value;

    assert_eq!(filtered.total, 1);
    assert_eq!(filtered.chunks.len(), 1);
    assert_eq!(filtered.chunks[0].id, alice.id);
}

#[tokio::test]
async fn list_chunks_ignores_empty_filter_lists_and_blank_query() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#a", "alpha").await;
    seed_chat_chunk(&cfg, "slack:#b", "beta").await;

    let resp = list_chunks_rpc(
        &cfg,
        ChunkFilter {
            source_kinds: Some(vec![]),
            source_ids: Some(vec![]),
            entity_ids: Some(vec![]),
            query: Some("   ".into()),
            limit: Some(10),
            ..ChunkFilter::default()
        },
    )
    .await
    .unwrap()
    .value;

    assert_eq!(resp.total, 2);
    assert_eq!(resp.chunks.len(), 2);
}

#[tokio::test]
async fn list_chunks_normalizes_invalid_tags_negative_tokens_and_empty_content() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    insert_raw_chunk(
        &cfg,
        "raw-empty",
        "document",
        "notion:page-1",
        1_700_000_000_123,
        "not-json",
        "",
        -7,
    );

    let resp = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value;
    let row = resp
        .chunks
        .into_iter()
        .find(|chunk| chunk.id == "raw-empty")
        .expect("raw chunk listed");

    assert_eq!(row.token_count, 0);
    assert_eq!(row.tags, Vec::<String>::new());
    assert_eq!(row.content_preview, None);
    assert!(!row.has_embedding);
}

#[tokio::test]
async fn list_sources_aggregates() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#a", "x").await;
    seed_chat_chunk(&cfg, "slack:#a", "y").await;
    seed_chat_chunk(&cfg, "slack:#b", "z").await;
    let sources = list_sources_rpc(&cfg, None).await.unwrap().value;
    let a = sources
        .iter()
        .find(|s| s.source_id == "slack:#a")
        .expect("expected slack:#a");
    let b = sources
        .iter()
        .find(|s| s.source_id == "slack:#b")
        .expect("expected slack:#b");
    assert_eq!(a.chunk_count, 2);
    assert_eq!(b.chunk_count, 1);
}

#[tokio::test]
async fn list_sources_formats_email_threads_with_trimmed_user_hint() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    insert_raw_chunk(
        &cfg,
        "email-thread",
        "email",
        "gmail:Alice@Example.com|bob@example.com|carol@example.com",
        1_700_000_000_123,
        "[]",
        "thread body",
        12,
    );

    let sources = list_sources_rpc(&cfg, Some(" alice@example.com ".into()))
        .await
        .unwrap()
        .value;
    let source = sources
        .iter()
        .find(|row| row.source_id == "gmail:Alice@Example.com|bob@example.com|carol@example.com")
        .expect("email thread source present");
    assert_eq!(source.display_name, "bob@example.com, carol@example.com");
}

#[tokio::test]
async fn entity_index_for_returns_extracted_entities() {
    let (_tmp, cfg) = test_config();
    // The entity index is read through `MemoryEntities::chunk_entities`, so the
    // handler needs a driver that serves that family — the null fallback does not.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "alice@example.com owns it").await;
    // Find the chunk we just seeded.
    let chunks = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value
        .chunks;
    let id = &chunks[0].id;
    let refs = entity_index_for_rpc(&cfg, id.clone()).await.unwrap().value;
    assert!(
        refs.iter().any(|r| r.entity_id.contains("alice")),
        "expected alice entity in index, got: {refs:?}"
    );
}

#[tokio::test]
async fn chunks_for_entity_returns_leaf_chunk_ids_only() {
    let (_tmp, cfg) = test_config();
    // `MemoryEntities::entity_chunk_ids` answers this one; the null fallback
    // serves no entity tier and would report an empty list.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "alice@example.com owns it").await;
    let chunk_id = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value
        .chunks[0]
        .id
        .clone();

    let rows = chunks_for_entity_rpc(&cfg, "email:alice@example.com".into())
        .await
        .unwrap()
        .value;
    assert_eq!(rows, vec![chunk_id]);
}

#[tokio::test]
async fn top_entities_returns_most_frequent() {
    let (_tmp, cfg) = test_config();
    // `MemoryEntities::top_entities` answers this one; the null fallback
    // serves no entity tier and would report an empty ranking.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#a", "alice@example.com x").await;
    seed_chat_chunk(&cfg, "slack:#b", "alice@example.com y").await;
    seed_chat_chunk(&cfg, "slack:#c", "bob@example.com z").await;
    let top = top_entities_rpc(&cfg, Some("email".into()), 10)
        .await
        .unwrap()
        .value;
    assert!(top
        .iter()
        .any(|e| e.entity_id == "email:alice@example.com" && e.count >= 2));
}

#[tokio::test]
async fn delete_chunk_removes_chunk_and_dependent_rows() {
    let (_tmp, cfg) = test_config();
    // The delete goes through `MemorySourceSink::forget_matching`, which the null
    // fallback does not serve — and this handler refuses rather than degrades.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "alice@example.com owns it").await;
    let chunks = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value
        .chunks;
    let id = chunks[0].id.clone();
    let resp = delete_chunk_rpc(&cfg, id.clone()).await.unwrap().value;
    assert!(resp.deleted);
    // Re-list — the chunk should be gone.
    let after = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value;
    assert!(after.chunks.iter().all(|c| c.id != id));
}

#[tokio::test]
async fn delete_missing_chunk_is_idempotent() {
    let (_tmp, cfg) = test_config();
    // Idempotence is the driver's, so this needs a driver: without one the handler
    // refuses outright, which is a different answer from "that chunk was not there".
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    let resp = delete_chunk_rpc(&cfg, "does-not-exist".into())
        .await
        .unwrap()
        .value;
    assert!(!resp.deleted);
    assert_eq!(resp.score_rows_removed, 0);
}

/// The one named behaviour delta of the move onto `MemoryEntities::top_entities`,
/// pinned in both directions.
///
/// The member validates `kind` and answers `MemoryError::Invalid` for one it does
/// not recognise. The SQL this handler used to run compared the string against the
/// stored column, so an unknown kind matched nothing and the caller got an empty
/// list. A migration must not turn that quiet empty result into a user-visible
/// error, so the variant is mapped back — and the second half of this test is what
/// keeps the map-back narrow rather than a blanket swallow.
#[tokio::test]
async fn top_entities_reports_empty_for_an_unknown_kind() {
    let (_tmp, cfg) = test_config();
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "alice@example.com owns it").await;

    let unknown = top_entities_rpc(&cfg, Some("not-a-kind".into()), 10)
        .await
        .expect("an unrecognised kind is an empty ranking, not an error")
        .value;
    assert!(unknown.is_empty(), "got: {unknown:?}");

    let known = top_entities_rpc(&cfg, Some("email".into()), 10)
        .await
        .unwrap()
        .value;
    assert!(
        !known.is_empty(),
        "a recognised kind must still rank rows; the Invalid map-back is narrow"
    );
}

/// `ForgetOutcome` reports `chunks_removed` and `trees_cleaned` and nothing about
/// the per-chunk side rows, so `DeleteChunkResponse`'s two counts are observed
/// before the delete rather than read off the outcome. This pins that they are
/// still real numbers — dropping them to zero would read as "there was nothing to
/// clean up", which is a different claim from "nobody counted".
#[tokio::test]
async fn delete_chunk_still_reports_its_side_row_counts() {
    let (_tmp, cfg) = test_config();
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "alice@example.com owns it").await;
    let id = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value
        .chunks[0]
        .id
        .clone();

    let indexed = entity_index_for_rpc(&cfg, id.clone()).await.unwrap().value;
    let indexed_rows: u32 = indexed.iter().map(|entity| entity.count).sum();
    assert!(
        indexed_rows > 0,
        "expected entity-index rows, got: {indexed:?}"
    );

    let resp = delete_chunk_rpc(&cfg, id).await.unwrap().value;
    assert!(resp.deleted);
    assert_eq!(resp.entity_index_rows_removed, indexed_rows);
    assert_eq!(resp.score_rows_removed, 1, "ingest writes one score row");
}

#[tokio::test]
async fn chunk_score_returns_breakdown_after_ingest() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(
        &cfg,
        "slack:#eng",
        "alice@example.com owns the phoenix migration",
    )
    .await;
    let chunks = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value
        .chunks;
    let id = &chunks[0].id;
    let breakdown = chunk_score_rpc(&cfg, id.clone()).await.unwrap().value;
    assert!(breakdown.is_some(), "expected score row after ingest");
    let b = breakdown.unwrap();
    assert!(b.signals.iter().any(|s| s.name == "metadata_weight"));
    assert!(b.threshold > 0.0);
}

#[tokio::test]
async fn search_returns_matching_chunks() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "phoenix migration scheduled friday").await;
    seed_chat_chunk(&cfg, "slack:#eng", "different unrelated text").await;
    let hits = search_rpc(&cfg, "phoenix".into(), 10).await.unwrap().value;
    assert!(hits.iter().any(|c| {
        c.content_preview
            .as_deref()
            .unwrap_or("")
            .contains("phoenix")
    }));
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
chunk detail is read through the bound driver, not the in-process engine"]
async fn read_chunk_row_returns_preview_and_metadata() {
    let (_tmp, cfg) = test_config();
    seed_chat_chunk(
        &cfg,
        "slack:#eng",
        "phoenix migration scheduled friday with context and source refs",
    )
    .await;
    let chunk = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value
        .chunks
        .into_iter()
        .next()
        .expect("seeded chunk");

    let row = read_chunk_row(&chunk.id).await.unwrap().expect("chunk row");
    assert_eq!(row.id, chunk.id);
    assert_eq!(row.source_kind, "chat");
    assert_eq!(row.source_id, "slack:#eng");
    assert_eq!(row.source_ref.as_deref(), Some("slack://x"));
    assert_eq!(row.owner, "alice");
    assert_eq!(row.lifecycle_status, "pending_extraction");
    assert!(row.content_path.is_some());
    assert!(row
        .content_preview
        .as_deref()
        .unwrap_or("")
        .contains("phoenix migration scheduled friday"));
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
chunk detail is read through the bound driver, not the in-process engine"]
async fn read_chunk_row_falls_back_to_sqlite_preview_when_file_missing() {
    let (_tmp, cfg) = test_config();
    let body = "sqlite preview survives missing file";
    seed_chat_chunk(&cfg, "slack:#eng", body).await;
    let chunk = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value
        .chunks
        .into_iter()
        .next()
        .expect("seeded chunk");

    let rel_path = chunk.content_path.clone().expect("content path present");
    let abs_path = cfg.memory_tree_content_root().join(rel_path);
    std::fs::remove_file(&abs_path).expect("remove chunk file");

    let row = read_chunk_row(&chunk.id).await.unwrap().expect("chunk row");
    assert_eq!(row.content_path, chunk.content_path);
    assert!(row.content_preview.as_deref().unwrap_or("").contains(body));
}

/// The handler forwards the driver's flush outcome onto the wire unchanged.
///
/// The behaviour this test used to stage — an ingest producing a stale buffer,
/// the second flush deduplicating inside the window — is the *driver's* and
/// moved with the SQL to the conformance suite
/// (`flushing_twice_in_a_window_schedules_the_work_once`), where a real store
/// exists. What is the host's here is only the mapping: both fields pass
/// through, and the u64→u32 buffer count clamps rather than wraps.
#[tokio::test]
async fn flush_now_reports_the_drivers_outcome() {
    use crate::openhuman::memory::api::provider::types::FlushOutcome;

    let (_tmp, cfg) = test_config();
    crate::openhuman::memory::binding::install_for_test(
        &cfg.workspace_dir,
        &cfg.subsystems.memory,
        std::sync::Arc::new(
            crate::openhuman::memory::binding::FixedDiagnostics::new(
                Default::default(),
                Default::default(),
            )
            .flushing(FlushOutcome {
                enqueued: false,
                stale_buffers: u64::from(u32::MAX) + 7,
            }),
        ) as std::sync::Arc<dyn crate::openhuman::memory::api::provider::MemoryProvider>,
    );

    let resp = flush_now_rpc(&cfg).await.expect("flush_now").value;
    assert!(
        !resp.enqueued,
        "`enqueued: false` passes through — with a non-zero buffer count it \
         means \"already scheduled\", not \"nothing to do\""
    );
    assert_eq!(
        resp.stale_buffers,
        u32::MAX,
        "a count past the wire type's range clamps rather than wraps to a small lie"
    );
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
asserts chunk lifecycle through `read_chunk_row`, which reads via the bound driver"]
async fn reset_tree_preserves_raw_archive_and_source_registry() {
    let (_tmp, cfg) = test_config();
    let chunk_id = seed_slack_chunk_with_raw_archive(&cfg).await;
    let content_root = cfg.memory_tree_content_root();
    let raw_file = content_root
        .join("raw")
        .join("slack-conn-slack-1")
        .join("chats")
        .join("1700000000000_1700000000.000100.md");
    let source_file = content_root
        .join("raw")
        .join("slack-conn-slack-1")
        .join("_source.md");
    assert!(raw_file.exists(), "raw archive should exist before reset");
    assert!(
        source_file.exists(),
        "source registry should exist before reset"
    );

    let stale_summary = content_root
        .join("wiki")
        .join("summaries")
        .join("source-slack-conn-slack-1")
        .join("L1")
        .join("summary-stale.md");
    std::fs::create_dir_all(
        stale_summary
            .parent()
            .expect("stale summary parent should exist"),
    )
    .expect("create stale summary dir");
    std::fs::write(&stale_summary, "stale summary body").expect("write stale summary");
    assert!(stale_summary.exists(), "stale summary fixture should exist");

    let outcome = reset_tree_rpc(&cfg).await.expect("reset_tree");
    assert_eq!(outcome.value.chunks_requeued, 1);
    assert_eq!(outcome.value.jobs_enqueued, 1);
    assert!(
        outcome.value.tree_rows_deleted >= 1,
        "buffer/tree rows should be removed during reset"
    );

    let row = read_chunk_row(&chunk_id)
        .await
        .expect("read chunk row")
        .expect("chunk row present after reset");
    assert_eq!(row.lifecycle_status, "pending_extraction");
    assert!(raw_file.exists(), "raw archive must survive reset_tree");
    assert!(
        source_file.exists(),
        "source registry must survive reset_tree"
    );
    assert!(
        !content_root.join("wiki").join("summaries").exists(),
        "derived wiki summaries should be removed"
    );
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
chunk detail is read through the bound driver, not the in-process engine"]
async fn read_chunk_row_returns_none_for_missing_chunk() {
    let (_tmp, _cfg) = test_config();
    assert!(read_chunk_row("missing-chunk").await.unwrap().is_none());
}

#[test]
fn display_name_unslugs_email_thread_with_user_hint() {
    let name = display_name_for_source(
        "gmail:alice@example.com|bob@example.com",
        Some("alice@example.com"),
    );
    assert_eq!(name, "bob@example.com");
}

#[test]
fn display_name_falls_back_to_arrow_when_user_unknown() {
    let name = display_name_for_source("gmail:alice@example.com|bob@example.com", None);
    assert!(name.contains("alice@example.com"));
    assert!(name.contains("bob@example.com"));
    assert!(name.contains("↔"));
}

#[test]
fn display_name_strips_platform_prefix() {
    assert_eq!(
        display_name_for_source("slack:#engineering", None),
        "#engineering"
    );
}

#[test]
fn display_name_handles_multiple_participants_and_trimmed_hint() {
    let name = display_name_for_source(
        "gmail:Alice@Example.com|bob@example.com|carol@example.com",
        Some(" alice@example.com "),
    );
    assert_eq!(name, "bob@example.com, carol@example.com");
}

#[test]
fn display_name_handles_no_prefix() {
    assert_eq!(display_name_for_source("loose-id", None), "loose-id");
}

#[test]
fn sanitize_basename_replaces_windows_illegal_characters() {
    assert_eq!(
        sanitize_basename(r#"chat:slack/#eng\name*?"<>|"#),
        "chat-slack-#eng-name------"
    );
    assert_eq!(sanitize_basename("safe-name.md"), "safe-name.md");
}

#[test]
fn parse_source_kind_str_accepts_known_values_only() {
    assert_eq!(parse_source_kind_str("chat"), Some(SourceKind::Chat));
    assert_eq!(parse_source_kind_str("email"), Some(SourceKind::Email));
    assert_eq!(
        parse_source_kind_str("document"),
        Some(SourceKind::Document)
    );
    assert_eq!(parse_source_kind_str("unknown"), None);
}

/// The namespace clear is routed through the bound driver's key/value tier
/// (`kv_list` + `kv_delete`) instead of a `rusqlite::Connection` this handler
/// opened on a path it built itself. The claim is unchanged and is what the
/// raw read-back below still checks: only the composio namespace goes.
#[tokio::test]
async fn clear_composio_sync_state_removes_only_target_namespace() {
    let (_tmp, cfg) = test_config();
    // Created up front so the raw fixture rows below have a schema to land in;
    // the driver installed further down opens this same store.
    let _memory =
        UnifiedMemory::new(cfg.workspace_dir.as_path(), Arc::new(NoopEmbedding), None).unwrap();
    let db_path = cfg.workspace_dir.join("memory").join("memory.db");
    let conn = rusqlite::Connection::open(&db_path).unwrap();

    conn.execute(
        "INSERT INTO kv_namespace (namespace, key, value_json, updated_at)
         VALUES (?1, 'cursor', '{}', 1.0)",
        params![KV_NAMESPACE],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO kv_namespace (namespace, key, value_json, updated_at)
         VALUES ('other-namespace', 'cursor', '{}', 2.0)",
        [],
    )
    .unwrap();
    drop(conn);

    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    let removed = clear_composio_sync_state(&cfg).await.unwrap();
    assert_eq!(removed, 1);

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let composio_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM kv_namespace WHERE namespace = ?1",
            params![KV_NAMESPACE],
            |row| row.get(0),
        )
        .unwrap();
    let other_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM kv_namespace WHERE namespace = 'other-namespace'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(composio_count, 0);
    assert_eq!(other_count, 1);
}

// ── tree-mode graph export (summaries + leaf chunks) ────────────────────

/// Insert a tree row and one summary node under it.
fn insert_tree_summary(cfg: &Config, tree_id: &str, scope: &str, summary_id: &str, level: i64) {
    with_connection(cfg, |conn| {
        conn.execute(
            "INSERT OR IGNORE INTO mem_tree_trees (id, kind, scope, created_at_ms)
             VALUES (?1, 'source', ?2, 0)",
            params![tree_id, scope],
        )?;
        conn.execute(
            "INSERT INTO mem_tree_summaries (
                id, tree_id, tree_kind, level, child_ids_json, content, token_count,
                entities_json, topics_json, time_range_start_ms, time_range_end_ms,
                score, sealed_at_ms, deleted
             ) VALUES (?1, ?2, 'source', ?3, '[]', 'summary body', 1, '[]', '[]', 0, 0, 0.0, 0, 0)",
            params![summary_id, tree_id, level],
        )?;
        Ok(())
    })
    .unwrap();
}

/// Insert a leaf chunk, optionally linked to a parent summary.
fn insert_chunk_with_parent(
    cfg: &Config,
    id: &str,
    parent_summary_id: Option<&str>,
    timestamp_ms: i64,
    content: &str,
) {
    with_connection(cfg, |conn| {
        conn.execute(
            "INSERT INTO mem_tree_chunks (
                id, source_kind, source_id, source_ref, owner, timestamp_ms,
                time_range_start_ms, time_range_end_ms, tags_json, content,
                token_count, seq_in_source, created_at_ms, lifecycle_status,
                content_path, parent_summary_id
             ) VALUES (?1, 'chat', 'slack:#eng', NULL, 'tester', ?2, ?2, ?2, '[]', ?3, 1, 0, ?2, 'seeded', NULL, ?4)",
            params![id, timestamp_ms, content, parent_summary_id],
        )?;
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn tree_graph_includes_leaf_chunks_linked_to_their_summary() {
    let (_tmp, cfg) = test_config();
    // The forest and its leaves are read through `MemoryTree`, so the graph
    // needs a driver that serves that family — the null fallback does not.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    insert_tree_summary(&cfg, "tree-1", "slack:#eng", "summary:1:L1-aaa", 1);
    insert_chunk_with_parent(
        &cfg,
        "chunk-sealed",
        Some("summary:1:L1-aaa"),
        1_700_000_000_000,
        "first line of sealed chunk\nmore body",
    );
    insert_chunk_with_parent(
        &cfg,
        "chunk-orphan",
        None,
        1_700_000_000_001,
        "orphan chunk body",
    );

    let resp = graph_export_rpc(&cfg, GraphMode::Tree).await.unwrap().value;

    // 1 source root + 1 summary + 2 leaf chunks = 4 nodes.
    assert_eq!(
        resp.nodes.len(),
        4,
        "source root + summary + both leaf chunks"
    );

    let source_root = resp.nodes.iter().find(|n| n.kind == "source").unwrap();
    assert!(source_root.id.starts_with("source:"));

    let summary = resp.nodes.iter().find(|n| n.kind == "summary").unwrap();
    assert_eq!(summary.id, "summary:1:L1-aaa");
    // Orphan summary links to source root.
    assert_eq!(summary.parent_id.as_deref(), Some(source_root.id.as_str()));

    let sealed = resp.nodes.iter().find(|n| n.id == "chunk-sealed").unwrap();
    assert_eq!(sealed.kind, "chunk");
    assert_eq!(sealed.parent_id.as_deref(), Some("summary:1:L1-aaa"));
    assert_eq!(sealed.label, "first line of sealed chunk");

    let orphan = resp.nodes.iter().find(|n| n.id == "chunk-orphan").unwrap();
    assert!(
        orphan.parent_id.is_none(),
        "unsealed chunk has no parent → renders as an orphan node"
    );

    assert!(resp.edges.is_empty());
}

#[tokio::test]
async fn tree_graph_keeps_summaries_first_then_chunks() {
    let (_tmp, cfg) = test_config();
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    insert_tree_summary(&cfg, "tree-1", "slack:#eng", "summary:1:L1-aaa", 1);
    insert_chunk_with_parent(
        &cfg,
        "chunk-1",
        Some("summary:1:L1-aaa"),
        1_700_000_000_000,
        "a chunk",
    );

    let resp = graph_export_rpc(&cfg, GraphMode::Tree).await.unwrap().value;
    // Source roots are emitted first, then summaries, then chunks — so a
    // budget truncation drops chunk tails, never the tree skeleton.
    assert_eq!(resp.nodes[0].kind, "source");
    assert!(resp.nodes.iter().any(|n| n.kind == "summary"));
    assert!(resp.nodes.iter().any(|n| n.kind == "chunk"));
}

/// Insert one `mem_tree_entity_index` row.
///
/// Seeded directly because the `person` kind only ever comes from the LLM
/// extractor, which these tests deliberately do not run — the mechanical
/// extractor emits `email`/`url`/`handle`/`hashtag` and nothing else.
fn insert_entity_row(
    cfg: &Config,
    entity_id: &str,
    node_id: &str,
    entity_kind: &str,
    surface: &str,
    timestamp_ms: i64,
) {
    with_connection(cfg, |conn| {
        conn.execute(
            "INSERT OR REPLACE INTO mem_tree_entity_index (
                entity_id, node_id, node_kind, entity_kind, surface,
                score, timestamp_ms, tree_id, is_user
             ) VALUES (?1, ?2, 'leaf', ?3, ?4, 1.0, ?5, NULL, 0)",
            params![entity_id, node_id, entity_kind, surface, timestamp_ms],
        )?;
        Ok(())
    })
    .unwrap();
}

/// Contacts mode selects chunks by entity *kind* and labels them from one
/// batched entity read.
///
/// The two chunks carry a different number of person rows on purpose. A reader
/// that indexed the flat `chunk_entities` result by position against the ids it
/// sent — the trap the contract's docs call out — would attribute the second
/// chunk's row to the first and still produce two edges, so an asymmetric
/// fixture is what makes the grouping observable.
#[tokio::test]
async fn contacts_graph_selects_person_chunks_and_groups_edges_by_chunk() {
    let (_tmp, cfg) = test_config();
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);

    insert_chunk_with_parent(
        &cfg,
        "chunk-a",
        None,
        1_700_000_000_002,
        "alice and bob met",
    );
    insert_chunk_with_parent(&cfg, "chunk-b", None, 1_700_000_000_001, "carol shipped it");
    // No person row: this chunk must not reach the graph at all.
    insert_chunk_with_parent(&cfg, "chunk-c", None, 1_700_000_000_000, "no people here");

    insert_entity_row(
        &cfg,
        "person:alice",
        "chunk-a",
        "person",
        "Alice",
        1_700_000_000_002,
    );
    insert_entity_row(
        &cfg,
        "person:bob",
        "chunk-a",
        "person",
        "Bob",
        1_700_000_000_002,
    );
    insert_entity_row(
        &cfg,
        "person:carol",
        "chunk-b",
        "person",
        "Carol",
        1_700_000_000_001,
    );
    // A non-person row on a person-bearing chunk: it must not become an edge.
    insert_entity_row(
        &cfg,
        "topic:shipping",
        "chunk-b",
        "topic",
        "shipping",
        1_700_000_000_001,
    );

    let resp = graph_export_rpc(&cfg, GraphMode::Contacts)
        .await
        .unwrap()
        .value;

    let chunk_ids: Vec<&str> = resp
        .nodes
        .iter()
        .filter(|n| n.kind == "chunk")
        .map(|n| n.id.as_str())
        .collect();
    assert_eq!(
        chunk_ids,
        vec!["chunk-a", "chunk-b"],
        "only person-bearing chunks, newest first"
    );

    let mut edges: Vec<(&str, &str)> = resp
        .edges
        .iter()
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect();
    edges.sort_unstable();
    assert_eq!(
        edges,
        vec![
            ("chunk-a", "person:alice"),
            ("chunk-a", "person:bob"),
            ("chunk-b", "person:carol"),
        ],
        "every edge names the chunk its row came from, and the topic row is filtered out"
    );

    let contacts: std::collections::BTreeSet<(&str, &str)> = resp
        .nodes
        .iter()
        .filter(|n| n.kind == "contact")
        .map(|n| (n.id.as_str(), n.label.as_str()))
        .collect();
    let expected: std::collections::BTreeSet<(&str, &str)> = [
        ("person:alice", "Alice"),
        ("person:bob", "Bob"),
        ("person:carol", "Carol"),
    ]
    .into_iter()
    .collect();
    assert_eq!(contacts, expected);
    assert!(resp
        .nodes
        .iter()
        .filter(|n| n.kind == "contact")
        .all(|n| n.entity_kind.as_deref() == Some("person")));
}

/// An empty candidate set must short-circuit, not become an unfiltered read.
///
/// The failure this pins is quiet: an empty predicate means *unfiltered* on
/// this seam, so a batch read handed the empty id list could answer with every
/// row in the store and the graph would fill with contacts for chunks it never
/// selected.
#[tokio::test]
async fn contacts_graph_with_no_person_chunks_is_empty() {
    let (_tmp, cfg) = test_config();
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    insert_chunk_with_parent(&cfg, "chunk-a", None, 1_700_000_000_000, "no people here");
    insert_entity_row(
        &cfg,
        "topic:shipping",
        "chunk-a",
        "topic",
        "shipping",
        1_700_000_000_000,
    );

    let resp = graph_export_rpc(&cfg, GraphMode::Contacts)
        .await
        .unwrap()
        .value;

    assert!(resp.nodes.is_empty(), "nodes: {:?}", resp.nodes);
    assert!(resp.edges.is_empty(), "edges: {:?}", resp.edges);
}

#[tokio::test]
async fn obsidian_status_registered_when_override_config_lists_content_root() {
    let (_tmp, cfg) = test_config();
    let content_root = cfg.memory_tree_content_root();
    // A separate dir standing in for a non-standard Obsidian config
    // location, with an obsidian.json that registers the content root.
    let cfg_dir = TempDir::new().unwrap();
    let body = format!(
        "{{ \"vaults\": {{ \"id0\": {{ \"path\": {}, \"open\": true }} }} }}",
        serde_json::to_string(&content_root.to_string_lossy().to_string()).unwrap()
    );
    std::fs::write(cfg_dir.path().join("obsidian.json"), body).unwrap();

    let outcome =
        obsidian_vault_status_rpc(&cfg, Some(cfg_dir.path().to_string_lossy().to_string()))
            .await
            .unwrap();

    assert!(outcome.value.registered);
    assert!(outcome.value.config_found);
    assert_eq!(
        outcome.value.content_root_abs,
        content_root.to_string_lossy().to_string()
    );
    // The log reports the booleans but redacts the absolute path (it
    // embeds the user's home / username).
    assert!(
        outcome.logs[0].contains("registered=true"),
        "log: {}",
        outcome.logs[0]
    );
    assert!(
        !outcome.logs[0].contains(content_root.to_str().unwrap()),
        "log leaked content root: {}",
        outcome.logs[0]
    );
}

#[tokio::test]
async fn obsidian_status_not_registered_for_empty_override_dir() {
    let (_tmp, cfg) = test_config();
    // Empty override dir → no obsidian.json there → content root is not a
    // registered vault. (A temp content root can't be under any real host
    // vault either, so this stays false regardless of the dev machine.)
    let cfg_dir = TempDir::new().unwrap();
    let outcome =
        obsidian_vault_status_rpc(&cfg, Some(cfg_dir.path().to_string_lossy().to_string()))
            .await
            .unwrap();
    assert!(!outcome.value.registered);
}

#[tokio::test]
async fn obsidian_status_blank_override_is_treated_as_none() {
    // A whitespace-only override must be normalized to None rather than
    // resolving to "." and probing a stray local ./obsidian.json. The temp
    // content root isn't under any real host vault, so this stays false.
    let (_tmp, cfg) = test_config();
    let outcome = obsidian_vault_status_rpc(&cfg, Some("   ".to_string()))
        .await
        .unwrap();
    assert!(!outcome.value.registered);
}

#[tokio::test]
async fn vault_health_check_reports_missing_content_root_for_fresh_workspace() {
    // `pipeline_healthy` reads the process-global degraded flags (via
    // `pipeline_status_rpc` → `current_degraded_state`), which sibling
    // `memory_tree` tests set and never clear. Serialise + reset to a clean
    // baseline so the assertion is deterministic. See #4691.
    let _g = crate::openhuman::memory::tree::health::test_guard();
    let (_tmp, cfg) = test_config();
    // `vault_health_check_rpc` folds in `pipeline_status_rpc`, which reads
    // through the bound driver. Bind an empty one explicitly: resolving the
    // real driver means loading the compiled module, which a test process
    // can block on rather than fail.
    crate::openhuman::memory::binding::install_diagnostics_for_test(
        &cfg.workspace_dir,
        &cfg.subsystems.memory,
        Default::default(),
        Default::default(),
    );
    let outcome = vault_health_check_rpc(&cfg, None).await.unwrap();

    assert!(!outcome.value.exists);
    assert!(!outcome.value.readable);
    assert!(!outcome.value.writable);
    assert!(!outcome.value.obsidian_registered);
    assert!(outcome.value.pipeline_healthy);
    assert_eq!(outcome.value.last_sync_ms, 0);
}

/// #4278: both vault RPCs stamp the core host's OS so a frontend attached
/// from a different OS can tell `content_root_abs` is a foreign-host path and
/// must not open/reveal it locally.
#[tokio::test]
async fn vault_rpcs_report_core_host_os() {
    let (_tmp, cfg) = test_config();
    // `vault_health_check_rpc` folds in `pipeline_status_rpc`, which reads
    // through the bound driver. Bind an empty one explicitly: resolving the
    // real driver means loading the compiled module, which a test process
    // can block on rather than fail.
    crate::openhuman::memory::binding::install_diagnostics_for_test(
        &cfg.workspace_dir,
        &cfg.subsystems.memory,
        Default::default(),
        Default::default(),
    );

    let status = obsidian_vault_status_rpc(&cfg, None).await.unwrap();
    assert_eq!(status.value.host_os, std::env::consts::OS);

    let health = vault_health_check_rpc(&cfg, None).await.unwrap();
    assert_eq!(health.value.host_os, std::env::consts::OS);
    assert!(
        !health.value.host_os.is_empty(),
        "host_os must be populated"
    );
}

#[tokio::test]
async fn vault_health_check_reports_writable_and_obsidian_registered_when_ready() {
    let (_tmp, cfg) = test_config();
    // `vault_health_check_rpc` folds in `pipeline_status_rpc`, and
    // `last_sync_ms` comes from the bound driver rather than from a `SELECT`
    // over the seeded chunk. The seed below still matters — it is what makes
    // `content_root` exist, which is what this test is actually about — but
    // the sync time has to come from a driver that reports one.
    crate::openhuman::memory::binding::install_diagnostics_for_test(
        &cfg.workspace_dir,
        &cfg.subsystems.memory,
        crate::openhuman::memory::api::provider::types::StoreStats {
            chunks: 1,
            chunks_with_structure: 0,
            most_recent_chunk_ms: Some(1_800_000_000_000),
        },
        Default::default(),
    );
    seed_chat_chunk(
        &cfg,
        "slack:#eng",
        "Vault health seed chunk so content_root exists and last_sync_ms > 0",
    )
    .await;

    let content_root = cfg.memory_tree_content_root();
    let cfg_dir = TempDir::new().unwrap();
    let body = format!(
        "{{ \"vaults\": {{ \"id0\": {{ \"path\": {}, \"open\": true }} }} }}",
        serde_json::to_string(&content_root.to_string_lossy().to_string()).unwrap()
    );
    std::fs::write(cfg_dir.path().join("obsidian.json"), body).unwrap();

    let outcome = vault_health_check_rpc(&cfg, Some(cfg_dir.path().to_string_lossy().to_string()))
        .await
        .unwrap();

    assert!(outcome.value.exists);
    assert!(outcome.value.readable);
    assert!(outcome.value.writable);
    assert!(outcome.value.obsidian_registered);
    // Intentionally NOT asserting `pipeline_healthy` here: with a seeded chunk
    // (total_chunks > 0) the derived status depends on the process-global
    // degraded flags, which unguarded parallel `memory_tree` extraction/pipeline
    // tests set and never clear (structure degrades only clears on a *successful*
    // extraction, which never happens under test). Post-#4691 a leaked "degraded"
    // correctly reads as unhealthy, so asserting healthy here would be flaky.
    // The health mapping is covered deterministically by the `pipeline_is_healthy`
    // unit tests in `read_rpc/vault.rs`; this test covers the filesystem readiness
    // wiring. See also `memory_tree::tree::rpc::pipeline_status_renders_the_drivers_chunk_aggregates`.
    assert!(outcome.value.last_sync_ms > 0);
    assert!(
        !outcome.logs[0].contains(content_root.to_str().unwrap()),
        "log leaked content root: {}",
        outcome.logs[0]
    );
}

/// Regression: `wipe_all` MUST also clear the source-ingest gate
/// (`mem_tree_ingested_sources`). Before the fix it cleared chunks/summaries
/// but left the gate claimed, so a wiped document source could never
/// re-ingest — the next sync saw `already_ingested` and wrote 0 chunks / 0
/// seal jobs. This pins that a wipe leaves the gate empty so re-sync works.
#[tokio::test]
async fn wipe_all_clears_ingest_gate() {
    use tinymemory_api::chunks::SourceKind;
    use tinymemory_core::store::chunks::store as chunk_store;

    let (_tmp, cfg) = test_config();
    // `wipe_all` asks the bound driver to purge; without one bound the workspace
    // resolves to the placeholder, which serves no Maintenance family and
    // refuses rather than reporting a wipe it did not do.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    let gate_key = "notion:conn-1:page-abc@1700000000000";

    // Claim the gate exactly as a document ingest does.
    chunk_store::with_connection(&cfg, |conn| {
        let tx = conn.unchecked_transaction()?;
        let claimed = chunk_store::claim_source_ingest_tx(
            &tx,
            SourceKind::Document,
            gate_key,
            1_700_000_000_000,
        )?;
        assert!(claimed, "first claim should succeed");
        tx.commit()?;
        Ok(())
    })
    .unwrap();
    assert!(
        chunk_store::is_source_ingested(&cfg, SourceKind::Document, gate_key).unwrap(),
        "gate must be claimed before wipe"
    );

    wipe_all_rpc(&cfg).await.expect("wipe_all_rpc");

    assert!(
        !chunk_store::is_source_ingested(&cfg, SourceKind::Document, gate_key).unwrap(),
        "wipe_all must clear mem_tree_ingested_sources so a wiped source can re-ingest"
    );
}
