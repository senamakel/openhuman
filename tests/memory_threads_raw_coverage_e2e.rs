//! Raw-line oriented E2E coverage for memory, memory_tree, memory_sync,
//! memory_sources, and threads.
//!
//! The tests call public Rust APIs and localhost-only readers so they stay
//! hermetic while still exercising production code paths that are awkward to
//! reach through full JSON-RPC flows.

use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use chrono::{TimeZone, Utc};
use serde_json::json;
use serde_json::{Map, Value};
use std::ffi::OsString;
use std::path::Path;
use tempfile::TempDir;

use openhuman_core::openhuman::config::Config;
use openhuman_core::openhuman::memory::tree_policy::TreePolicy;
use openhuman_core::openhuman::memory::tree_source;
use openhuman_core::openhuman::memory::{
    rpc_models::{
        ApiEnvelope, ApiError, ApiMeta, AppendConversationMessageRequest,
        ConversationMessageRecord, ConversationMessagesRequest, CreateConversationThreadRequest,
        DeleteConversationThreadRequest, EmptyRequest, GenerateConversationThreadTitleRequest,
        PaginationMeta, QueryNamespaceRequest, RecallContextRequest, RecallMemoriesRequest,
        UpdateConversationMessageRequest, UpdateConversationThreadLabelsRequest,
        UpdateConversationThreadTitleRequest, UpsertConversationThreadRequest,
    },
    traits::{MemoryCategory, MemoryEntry, NamespaceSummary, RecallOpts},
};
use openhuman_core::openhuman::memory_sources::readers::reader_for;
use openhuman_core::openhuman::memory_sources::registry;
use openhuman_core::openhuman::memory_sources::rpc as memory_sources_rpc;
use openhuman_core::openhuman::memory_sources::status::{source_status, FreshnessLabel};
use openhuman_core::openhuman::memory_sources::types::{
    ContentType, MemorySourceEntry, SourceContent, SourceItem, SourceKind,
};
use openhuman_core::openhuman::memory_sources::{
    all_memory_sources_controller_schemas, all_memory_sources_registered_controllers,
};
use openhuman_core::openhuman::memory_store::chunks::store::{upsert_chunks, with_connection};
use openhuman_core::openhuman::memory_store::chunks::types::{
    approx_token_count, chunk_id, Chunk, DataSource, Metadata, SourceKind as ChunkSourceKind,
    SourceRef,
};
use openhuman_core::openhuman::memory_store::trees::types::{
    SummaryNode, Tree, TreeKind, TreeStatus as StoredTreeStatus,
};
use openhuman_core::openhuman::memory_sync::canonicalize::chat::{
    canonicalise as canonicalise_chat, ChatBatch, ChatMessage,
};
use openhuman_core::openhuman::memory_sync::canonicalize::document::{
    canonicalise as canonicalise_document, DocumentInput,
};
use openhuman_core::openhuman::memory_sync::canonicalize::email::{
    canonicalise as canonicalise_email, EmailMessage, EmailThread,
};
use openhuman_core::openhuman::memory_sync::canonicalize::email_clean;
use openhuman_core::openhuman::memory_sync::composio;
use openhuman_core::openhuman::memory_sync::composio::providers::profile_md::{
    block_end, block_start, merge_provider_into_profile_md, remove_provider_from_profile_md,
    replace_managed_block,
};
use openhuman_core::openhuman::memory_sync::composio::providers::sync_state::{
    extract_item_id, DailyBudget, SyncState, DEFAULT_DAILY_REQUEST_LIMIT,
};
use openhuman_core::openhuman::memory_sync::composio::providers::{
    agent_ready_toolkits, capability_matrix, catalog_for_toolkit, classify_unknown,
    curated_scope_for, find_curated, is_action_visible_with_pref, toolkit_from_slug,
    toolkit_has_scope, CuratedTool, NormalizedTask, ProviderUserProfile, SyncOutcome, SyncReason,
    TaskFetchFilter, ToolScope, UserScopePref,
};
use openhuman_core::openhuman::memory_tree::score::extract::{
    EntityKind, ExtractedEntities, ExtractedEntity, ExtractedTopic,
};
use openhuman_core::openhuman::memory_tree::score::signals::{
    combine, combine_cheap_only, compute as compute_score_signals, entity_density_score,
    interaction, metadata_weight, source_weight, token_count, unique_words, ScoreSignals,
    SignalWeights,
};
use openhuman_core::openhuman::memory_tree::tree_runtime::store as tree_runtime_store;
use openhuman_core::openhuman::memory_tree::tree_runtime::{
    derive_node_ids, derive_parent_id, estimate_tokens, level_from_node_id, node_id_to_path,
    NodeLevel, TreeNode,
};
use openhuman_core::openhuman::memory_tree::{retrieval, score::embed};
use openhuman_core::openhuman::threads::ops as thread_ops;
use openhuman_core::openhuman::threads::title::{
    build_title_prompt, collapse_whitespace, is_auto_generated_thread_title,
    sanitize_generated_title, title_from_user_message, title_log_fingerprint,
};
use openhuman_core::openhuman::threads::turn_state::{
    self, ClearTurnStateRequest, GetTurnStateRequest, GetTurnStateResponse, ListTurnStatesResponse,
    SubagentActivity, SubagentToolCall, ToolTimelineEntry, ToolTimelineStatus, TurnLifecycle,
    TurnPhase, TurnState,
};
use openhuman_core::openhuman::threads::ThreadsError;

struct EnvVarGuard {
    key: &'static str,
    old: Option<OsString>,
}

impl EnvVarGuard {
    fn set_to_path(key: &'static str, value: &Path) -> Self {
        let old = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value.as_os_str());
        }
        Self { key, old }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.old {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

fn config_in(tmp: &TempDir) -> Config {
    let mut config = Config::default();
    config.workspace_dir = tmp.path().to_path_buf();
    config
}

fn source(kind: SourceKind, id: &str) -> MemorySourceEntry {
    MemorySourceEntry {
        id: id.to_string(),
        kind,
        label: format!("{id} label"),
        enabled: true,
        toolkit: None,
        connection_id: None,
        path: None,
        glob: None,
        url: None,
        branch: None,
        paths: Vec::new(),
        query: None,
        since_days: None,
        max_items: None,
        selector: None,
    }
}

fn chunk(source_id: &str, seq: u32, timestamp_ms: i64) -> Chunk {
    let content = format!("chunk {source_id} {seq}");
    let ts = Utc.timestamp_millis_opt(timestamp_ms).unwrap();
    Chunk {
        id: chunk_id(ChunkSourceKind::Document, source_id, seq, &content),
        content,
        metadata: Metadata::point_in_time(ChunkSourceKind::Document, source_id, "owner", ts),
        token_count: approx_token_count(source_id),
        seq_in_source: seq,
        created_at: ts,
        partial_message: false,
    }
}

fn tree_node(namespace: &str, node_id: &str, summary: &str) -> TreeNode {
    let created_at = Utc.with_ymd_and_hms(2026, 5, 29, 12, 0, 0).unwrap();
    TreeNode {
        node_id: node_id.to_string(),
        namespace: namespace.to_string(),
        level: level_from_node_id(node_id),
        parent_id: derive_parent_id(node_id),
        summary: summary.to_string(),
        token_count: estimate_tokens(summary),
        child_count: 0,
        created_at,
        updated_at: created_at,
        metadata: Some(json!({ "kind": "coverage", "node": node_id }).to_string()),
    }
}

async fn serve_routes(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    format!("http://{addr}")
}

async fn html_page() -> Html<&'static str> {
    Html(
        "<html><head><title>Raw Page</title></head><body><main><h1>Hello</h1><p>Selected body</p></main><aside>Skip me</aside></body></html>",
    )
}

async fn large_header() -> Response {
    vec![b'x'; 10 * 1024 * 1024 + 1].into_response()
}

async fn rss_feed(headers: HeaderMap) -> Response {
    let mode = headers
        .get("x-feed-mode")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("rss");
    if mode == "atom" {
        return Html(
            r#"<feed><entry><id>atom-1</id><title>Atom One</title><content>Atom body</content><link href="https://example.test/atom-1"/><updated>2026-05-29T12:00:00Z</updated></entry></feed>"#,
        )
        .into_response();
    }
    Html(
        r#"<rss><channel><item><guid>rss-1</guid><title>RSS One</title><link>https://example.test/rss-1</link><description><![CDATA[<p>RSS body</p>]]></description><pubDate>Fri, 29 May 2026 12:00:00 +0000</pubDate></item><item><guid>rss-2</guid><title>RSS Two</title><description>Second</description></item></channel></rss>"#,
    )
    .into_response()
}

#[tokio::test]
async fn canonicalizers_clean_sort_and_preserve_metadata() {
    let doc_json = json!({
        "title": "Doc",
        "body": "  Document body  ",
        "modified_at": "2026-05-29T12:00:00Z",
        "source_ref": " file://doc "
    });
    let doc: DocumentInput = serde_json::from_value(doc_json).expect("document input");
    let doc_out = canonicalise_document("doc:1", "alice", &["plans".into()], doc)
        .expect("document canonicalise")
        .expect("document output");
    assert_eq!(doc_out.markdown, "Document body\n");
    assert_eq!(doc_out.metadata.source_id, "doc:1");
    assert_eq!(doc_out.metadata.source_ref.unwrap().value, "file://doc");

    let empty_doc = DocumentInput {
        provider: "drive".into(),
        title: " ".into(),
        body: "\n ".into(),
        modified_at: Utc.timestamp_millis_opt(1_700_000_000_000).unwrap(),
        source_ref: Some(" ".into()),
    };
    assert!(canonicalise_document("doc:empty", "alice", &[], empty_doc)
        .unwrap()
        .is_none());

    let older = Utc.timestamp_millis_opt(1_700_000_000_000).unwrap();
    let newer = Utc.timestamp_millis_opt(1_700_000_060_000).unwrap();
    let email_out = canonicalise_email(
        "gmail:thread-1",
        "alice@example.com",
        &["inbox".into()],
        EmailThread {
            provider: "gmail".into(),
            thread_subject: "Launch".into(),
            messages: vec![
                EmailMessage {
                    from: "Bob <bob@example.com>".into(),
                    to: vec!["alice@example.com".into()],
                    cc: vec!["team@example.com".into()],
                    subject: "Re: Launch".into(),
                    sent_at: newer,
                    body: "Reply\n\nUnsubscribe here".into(),
                    source_ref: Some("<msg-new@example.com>".into()),
                    list_unsubscribe: Some("<mailto:unsubscribe@example.com>".into()),
                },
                EmailMessage {
                    from: "alice@example.com".into(),
                    to: vec!["bob@example.com".into()],
                    cc: Vec::new(),
                    subject: "Launch".into(),
                    sent_at: older,
                    body: "First\n\n> quoted one\n> quoted two\n> quoted three".into(),
                    source_ref: Some("<msg-old@example.com>".into()),
                    list_unsubscribe: None,
                },
            ],
        },
    )
    .expect("email canonicalise")
    .expect("email output");
    assert!(email_out.markdown.find("Subject: Launch") < email_out.markdown.find("Re: Launch"));
    assert!(email_out.markdown.contains("List-Unsubscribe:"));
    assert!(!email_out.markdown.contains("quoted three"));
    assert_eq!(email_out.metadata.time_range, (older, newer));
    assert_eq!(
        email_out.metadata.source_ref.as_ref().unwrap().value,
        "<msg-old@example.com>"
    );

    let chat_out = canonicalise_chat(
        "slack:#eng",
        "alice",
        &["eng".into()],
        ChatBatch {
            platform: "slack".into(),
            channel_label: "#eng".into(),
            messages: vec![
                ChatMessage {
                    author: "Bob".into(),
                    timestamp: newer,
                    text: " second ".into(),
                    source_ref: Some("slack://2".into()),
                },
                ChatMessage {
                    author: "Alice".into(),
                    timestamp: older,
                    text: "first".into(),
                    source_ref: Some("slack://1".into()),
                },
            ],
        },
    )
    .expect("chat canonicalise")
    .expect("chat output");
    assert!(chat_out.markdown.find("Alice") < chat_out.markdown.find("Bob"));
    assert_eq!(chat_out.metadata.source_ref.unwrap().value, "slack://1");

    assert_eq!(
        email_clean::drop_footer_noise("Real\n\nView in browser\nFooter"),
        "Real"
    );
    assert_eq!(
        email_clean::parse_message_date(&json!({ "date": "2026-05-29" }))
            .unwrap()
            .date_naive()
            .to_string(),
        "2026-05-29"
    );
    assert_eq!(
        email_clean::extract_email("Name <n@example.com>").as_deref(),
        Some("n@example.com")
    );
    assert_eq!(email_clean::md_escape("a*b_c|d"), "a\\*b\\_c\\|d");
}

#[tokio::test]
async fn memory_source_readers_validate_and_use_local_inputs_only() {
    let tmp = TempDir::new().expect("tempdir");
    let config = config_in(&tmp);

    let mut folder = source(SourceKind::Folder, "src_folder");
    folder.path = Some(tmp.path().to_string_lossy().to_string());
    folder.glob = Some("**/*".into());
    std::fs::write(tmp.path().join("note.md"), "# Note").expect("write note");
    std::fs::write(tmp.path().join("page.html"), "<p>Body</p>").expect("write html");
    std::fs::write(tmp.path().join("plain.txt"), "Plain").expect("write txt");
    std::fs::create_dir_all(tmp.path().join("nested")).expect("nested dir");

    let folder_reader = reader_for(&SourceKind::Folder);
    assert_eq!(folder_reader.kind(), SourceKind::Folder);
    let items = folder_reader
        .list_items(&folder, &config)
        .await
        .expect("folder list");
    assert!(items.iter().any(|item| item.id == "note.md"));
    assert!(items.iter().any(|item| item.id == "page.html"));
    let html = folder_reader
        .read_item(&folder, "page.html", &config)
        .await
        .expect("folder read html");
    assert_eq!(html.content_type, ContentType::Html);
    let traversal = folder_reader
        .read_item(&folder, "../outside.md", &config)
        .await
        .unwrap_err();
    assert!(traversal.contains("not found") || traversal.contains("traversal"));

    let web_base = serve_routes(
        Router::new()
            .route("/page", get(html_page))
            .route("/too-large", get(large_header))
            .route("/missing", get(|| async { StatusCode::NOT_FOUND })),
    )
    .await;
    let mut page = source(SourceKind::WebPage, "src_web");
    page.url = Some(format!("{web_base}/page"));
    page.selector = Some("main.content".into());
    let web_reader = reader_for(&SourceKind::WebPage);
    let page_items = web_reader
        .list_items(&page, &config)
        .await
        .expect("web list");
    assert_eq!(page_items[0].id, format!("{web_base}/page"));
    let page_content = web_reader
        .read_item(&page, &page_items[0].id, &config)
        .await
        .expect("web read");
    assert_eq!(page_content.title, "Raw Page");
    assert!(page_content.body.contains("Selected body"));
    assert!(!page_content.body.contains("Skip me"));
    page.url = Some("file:///etc/passwd".into());
    let bad_scheme = web_reader
        .read_item(&page, "relative-id", &config)
        .await
        .unwrap_err();
    assert!(bad_scheme.contains("http(s)"));
    page.url = Some(format!("{web_base}/too-large"));
    assert!(web_reader
        .read_item(&page, "relative-id", &config)
        .await
        .unwrap_err()
        .contains("exceeds"));
    page.url = Some(format!("{web_base}/missing"));
    assert!(web_reader
        .read_item(&page, "relative-id", &config)
        .await
        .unwrap_err()
        .contains("404"));

    let rss_base = serve_routes(Router::new().route("/feed", get(rss_feed))).await;
    let mut rss = source(SourceKind::RssFeed, "src_rss");
    rss.url = Some(format!("{rss_base}/feed"));
    rss.max_items = Some(1);
    let rss_reader = reader_for(&SourceKind::RssFeed);
    let feed_items = rss_reader
        .list_items(&rss, &config)
        .await
        .expect("rss list");
    assert_eq!(feed_items.len(), 1);
    assert_eq!(feed_items[0].id, "rss-1");
    let feed_content = rss_reader
        .read_item(&rss, "rss-1", &config)
        .await
        .expect("rss read");
    assert_eq!(feed_content.content_type, ContentType::Html);
    assert!(feed_content.metadata["link"]
        .as_str()
        .unwrap()
        .contains("rss-1"));
    assert!(rss_reader
        .read_item(&rss, "missing", &config)
        .await
        .unwrap_err()
        .contains("not found"));

    let mut twitter = source(SourceKind::TwitterQuery, "src_tw");
    twitter.query = Some("AI safety".into());
    assert!(reader_for(&SourceKind::TwitterQuery)
        .list_items(&twitter, &config)
        .await
        .unwrap_err()
        .contains("not yet configured"));

    let mut composio = source(SourceKind::Composio, "src_cmp");
    composio.toolkit = Some("gmail".into());
    composio.connection_id = Some("conn-1".into());
    let composio_reader = reader_for(&SourceKind::Composio);
    assert_eq!(
        composio_reader
            .list_items(&composio, &config)
            .await
            .expect("composio list")[0]
            .title,
        "gmail connection"
    );
    assert!(composio_reader
        .read_item(&composio, "conn-1", &config)
        .await
        .expect("composio read")
        .body
        .contains("provider sync pipeline"));

    for (kind, expected) in [
        (SourceKind::Composio, "composio"),
        (SourceKind::Folder, "folder"),
        (SourceKind::GithubRepo, "github_repo"),
        (SourceKind::TwitterQuery, "twitter_query"),
        (SourceKind::RssFeed, "rss_feed"),
        (SourceKind::WebPage, "web_page"),
    ] {
        assert_eq!(kind.as_str(), expected);
    }
    assert!(source(SourceKind::GithubRepo, "bad")
        .validate()
        .unwrap_err()
        .contains("url"));
    assert!(source(SourceKind::TwitterQuery, "bad")
        .validate()
        .unwrap_err()
        .contains("query"));
}

#[tokio::test]
async fn memory_source_status_counts_reader_and_composio_prefixes() {
    let tmp = TempDir::new().expect("tempdir");
    let config = config_in(&tmp);
    let now = Utc::now().timestamp_millis();
    let chunks = vec![
        chunk("mem_src:src_folder:note-1", 0, now - 1_000),
        chunk("mem_src:src_folder:note-2", 1, now - 60_000),
        chunk("gmail:acct:msg-1", 0, now - 600_000),
    ];
    upsert_chunks(&config, &chunks).expect("upsert chunks");
    with_connection(&config, |conn| {
        conn.execute(
            "UPDATE mem_tree_chunks SET embedding = X'00010203' WHERE source_id = ?1",
            ["mem_src:src_folder:note-1"],
        )?;
        Ok(())
    })
    .expect("mark one embedded");

    let mut folder = source(SourceKind::Folder, "src_folder");
    folder.path = Some(tmp.path().to_string_lossy().to_string());
    let folder_status = source_status(&config, &folder)
        .await
        .expect("folder status");
    assert_eq!(folder_status.source_id, "src_folder");
    assert_eq!(folder_status.chunks_synced, 2);
    assert_eq!(folder_status.chunks_pending, 1);
    assert_eq!(folder_status.freshness, FreshnessLabel::Active);

    let mut composio = source(SourceKind::Composio, "src_cmp");
    composio.toolkit = Some("gmail".into());
    composio.connection_id = Some("acct".into());
    let composio_status = source_status(&config, &composio)
        .await
        .expect("composio status");
    assert_eq!(composio_status.chunks_synced, 1);
    assert_eq!(composio_status.chunks_pending, 1);
    assert_eq!(composio_status.freshness, FreshnessLabel::Idle);
}

#[test]
fn memory_tree_policy_and_source_registry_write_metadata_mirror() {
    let tmp = TempDir::new().expect("tempdir");
    let config = config_in(&tmp);
    let policy = TreePolicy::topic();
    let now = 1_700_000_000_000_i64;
    assert_eq!(TreePolicy::global(), TreePolicy::Global);
    assert_eq!(TreePolicy::source(), TreePolicy::Source);
    assert!(policy.topic_creation_threshold() > policy.topic_archive_threshold());
    assert_eq!(policy.topic_recency_decay(None, now), 0.0);
    assert_eq!(policy.topic_recency_decay(Some(now + 60_000), now), 1.0);
    assert_eq!(
        policy.topic_recency_decay(Some(now - 60 * 86_400_000), now),
        0.0
    );

    let stats = openhuman_core::openhuman::memory_store::trees::types::EntityIndexStats {
        mention_count_30d: 9,
        distinct_sources: 4,
        last_seen_ms: Some(now - 4 * 86_400_000),
        query_hits_30d: 2,
        graph_centrality: Some(0.75),
    };
    assert!(policy.topic_hotness("user@example.com", &stats, now) > 0.0);

    let first = tree_source::get_or_create_source_tree(&config, "gmail:user@example.com")
        .expect("create source tree");
    let second = tree_source::get_or_create_source_tree(&config, "gmail:user@example.com")
        .expect("reuse source tree");
    assert_eq!(first.id, second.id);

    let mirror = tree_source::file::source_file_path(&config, &first.scope);
    let body = std::fs::read_to_string(&mirror)
        .unwrap_or_else(|err| panic!("read {}: {err}", mirror.display()));
    assert!(body.starts_with("---\n"));
    assert!(body.contains("kind: source"));
    assert!(body.contains("scope: \"gmail:user@example.com\""));
    assert!(body.contains("last_sealed_at: null"));
}

#[test]
fn thread_title_error_and_turn_state_helpers_cover_wire_shapes() {
    assert!(is_auto_generated_thread_title("Chat Jan 1 1:23 AM"));
    assert!(!is_auto_generated_thread_title("Chat Jan 1 1:2 AM"));
    assert!(!is_auto_generated_thread_title("Planning the launch"));
    assert_eq!(collapse_whitespace(" a\tb\n c "), "a b c");
    assert_eq!(
        sanitize_generated_title("\n`Deploy review!`\nsecond").as_deref(),
        Some("Deploy review")
    );
    assert!(sanitize_generated_title("\"\"").is_none());
    assert_eq!(
        title_from_user_message("/briefing Morning update. Then email").as_deref(),
        Some("briefing Morning update")
    );
    assert_ne!(
        title_log_fingerprint("alpha"),
        title_log_fingerprint("beta")
    );
    let prompt = build_title_prompt("hello", "hi");
    assert!(prompt.contains("First user message:\nhello"));
    assert!(prompt.contains("Assistant reply:\nhi"));

    let not_found: String = ThreadsError::not_found("thread-1").into();
    assert!(not_found.contains("ThreadNotFound"));
    let scoped = ThreadsError::from_thread_scoped_store_error(
        "thread-1",
        "thread thread-2 not found".to_string(),
    );
    assert!(matches!(scoped, ThreadsError::Message(_)));

    let mut state = TurnState::started("thread-1", "request-1", 6, "2026-05-29T12:00:00Z");
    state.lifecycle = TurnLifecycle::Streaming;
    state.phase = Some(TurnPhase::ToolUse);
    state.active_tool = Some("memory.search".into());
    state.tool_timeline.push(ToolTimelineEntry {
        id: "tool-1".into(),
        name: "memory.search".into(),
        round: 1,
        status: ToolTimelineStatus::Success,
        args_buffer: Some("{\"q\":\"coverage\"}".into()),
        display_name: Some("Memory Search".into()),
        detail: Some("2 results".into()),
        source_tool_name: Some("memory.search".into()),
        subagent: None,
    });
    let wire = serde_json::to_value(GetTurnStateResponse {
        turn_state: Some(state.clone()),
    })
    .expect("turn state json");
    assert_eq!(wire["turnState"]["threadId"], "thread-1");
    assert_eq!(wire["turnState"]["phase"], "tool_use");
    let decoded: GetTurnStateResponse = serde_json::from_value(wire).expect("decode turn state");
    assert_eq!(decoded.turn_state.unwrap(), state);
}

#[test]
fn memory_sync_composio_catalog_scope_and_state_helpers_cover_edge_cases() {
    assert_eq!(SyncReason::ConnectionCreated.as_str(), "connection_created");
    assert_eq!(SyncReason::Periodic.as_str(), "periodic");
    assert_eq!(SyncReason::Manual.as_str(), "manual");

    let mut outcome = SyncOutcome {
        toolkit: "gmail".into(),
        connection_id: Some("conn-1".into()),
        reason: SyncReason::Manual.as_str().into(),
        items_ingested: 3,
        started_at_ms: 200,
        finished_at_ms: 150,
        summary: "done".into(),
        details: json!({ "pages": 1 }),
    };
    assert_eq!(outcome.elapsed_ms(), 0);
    outcome.finished_at_ms = 275;
    assert_eq!(outcome.elapsed_ms(), 75);

    assert_eq!(TaskFetchFilter::default().effective_max(), 25);
    assert_eq!(
        TaskFetchFilter {
            max: 7,
            ..Default::default()
        }
        .effective_max(),
        7
    );
    let task_json = json!({
        "externalId": "issue-1",
        "provider": "github",
        "title": "Fix coverage",
        "labels": ["test"],
        "raw": { "number": 1 }
    });
    let task: NormalizedTask = serde_json::from_value(task_json).expect("task");
    assert_eq!(task.external_id, "issue-1");
    assert_eq!(task.source_id, "");
    assert_eq!(task.labels, vec!["test"]);

    let profile = ProviderUserProfile {
        toolkit: "github".into(),
        email: Some("dev@example.com".into()),
        extras: json!({ "login": "dev" }),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(profile).unwrap()["extras"]["login"],
        "dev"
    );

    assert_eq!(ToolScope::Read.as_str(), "read");
    assert_eq!(ToolScope::Write.as_str(), "write");
    assert_eq!(ToolScope::Admin.as_str(), "admin");
    assert_eq!(classify_unknown("GMAIL_DELETE_DRAFT"), ToolScope::Admin);
    assert_eq!(classify_unknown("NOTION_CREATE_PAGE"), ToolScope::Write);
    assert_eq!(classify_unknown("GMAIL_FETCH_EMAILS"), ToolScope::Read);
    assert_eq!(
        toolkit_from_slug(" MICROSOFT_TEAMS_SEND_MESSAGE "),
        Some("microsoft".into())
    );
    assert_eq!(toolkit_from_slug(""), None);
    let catalog = &[CuratedTool {
        slug: "GMAIL_SEND_EMAIL",
        scope: ToolScope::Write,
    }];
    assert_eq!(
        find_curated(catalog, "gmail_send_email").map(|tool| tool.scope),
        Some(ToolScope::Write)
    );
    assert!(find_curated(catalog, "GMAIL_DELETE_EMAIL").is_none());

    let read_only = UserScopePref {
        read: true,
        write: false,
        admin: false,
    };
    assert!(is_action_visible_with_pref(
        "GMAIL_FETCH_EMAILS",
        &read_only
    ));
    assert!(!is_action_visible_with_pref("GMAIL_SEND_EMAIL", &read_only));
    assert_eq!(
        curated_scope_for("GMAIL_DELETE_MESSAGE"),
        Some(ToolScope::Admin)
    );
    assert!(toolkit_has_scope("gmail", ToolScope::Admin));
    assert!(catalog_for_toolkit("google_calendar").is_some());
    assert!(agent_ready_toolkits()
        .windows(2)
        .all(|pair| pair[0] <= pair[1]));

    let matrix = capability_matrix();
    let gmail = matrix.iter().find(|cap| cap.toolkit == "gmail").unwrap();
    assert!(gmail.native_provider);
    assert!(gmail.curated_tools);
    assert!(gmail.curated_tool_count > 0);
    let spotify = matrix.iter().find(|cap| cap.toolkit == "spotify").unwrap();
    assert!(!spotify.native_provider);
    assert!(spotify.curated_tools);

    let sync_target = composio::SyncTarget {
        toolkit: "gmail".into(),
        connection_id: "conn-1".into(),
    };
    assert_eq!(sync_target.toolkit, "gmail");

    let mut budget = DailyBudget {
        date: "2000-01-01".into(),
        requests_used: DEFAULT_DAILY_REQUEST_LIMIT,
        limit: DEFAULT_DAILY_REQUEST_LIMIT,
    };
    assert_eq!(budget.remaining(), DEFAULT_DAILY_REQUEST_LIMIT);
    budget.record_request();
    assert_eq!(budget.requests_used, 1);
    budget.record_requests(DEFAULT_DAILY_REQUEST_LIMIT + 10);
    assert!(budget.is_exhausted());

    let mut state = SyncState::new("gmail", "conn-1");
    assert_eq!(state.budget_remaining(), DEFAULT_DAILY_REQUEST_LIMIT);
    assert!(!state.budget_exhausted());
    state.record_requests(2);
    state.mark_synced("msg-1");
    state.advance_cursor("cursor-1");
    state.set_last_seen_id("msg-2");
    state.set_last_sync_at_ms(123);
    assert!(state.is_synced("msg-1"));
    assert!(!state.is_synced("msg-2"));
    assert_eq!(state.cursor.as_deref(), Some("cursor-1"));
    assert_eq!(state.last_seen_id.as_deref(), Some("msg-2"));
    assert_eq!(state.last_sync_at_ms, Some(123));

    let item = json!({
        "id": " ",
        "message": { "id": " msg-99 " },
        "nested": { "empty": "" }
    });
    assert_eq!(
        extract_item_id(&item, &["missing", "nested.empty", "message.id"]),
        Some("msg-99".into())
    );
    assert_eq!(extract_item_id(&item, &["missing"]), None);
}

#[test]
fn memory_tree_scoring_signal_helpers_cover_boundaries_and_serialization() {
    assert_eq!(EntityKind::parse("email").unwrap(), EntityKind::Email);
    assert!(EntityKind::Email.is_mechanical());
    assert!(!EntityKind::Person.is_mechanical());
    assert!(EntityKind::parse("unknown").is_err());

    let mut extracted = ExtractedEntities {
        entities: vec![ExtractedEntity {
            kind: EntityKind::Person,
            text: "Alice".into(),
            span_start: 0,
            span_end: 5,
            score: 0.9,
        }],
        topics: vec![ExtractedTopic {
            label: "phoenix".into(),
            score: 0.8,
        }],
        llm_importance: Some(0.3),
        llm_importance_reason: Some("initial".into()),
    };
    assert!(!extracted.is_empty());
    extracted.merge(ExtractedEntities {
        entities: vec![
            ExtractedEntity {
                kind: EntityKind::Person,
                text: "alice".into(),
                span_start: 0,
                span_end: 5,
                score: 1.0,
            },
            ExtractedEntity {
                kind: EntityKind::Organization,
                text: "OpenHuman".into(),
                span_start: 10,
                span_end: 19,
                score: 0.7,
            },
        ],
        topics: vec![
            ExtractedTopic {
                label: "phoenix".into(),
                score: 0.9,
            },
            ExtractedTopic {
                label: "coverage".into(),
                score: 0.6,
            },
        ],
        llm_importance: Some(0.8),
        llm_importance_reason: Some("higher".into()),
    });
    assert_eq!(extracted.entities.len(), 2);
    assert_eq!(extracted.topics.len(), 2);
    assert_eq!(extracted.unique_entity_count(), 2);
    assert_eq!(extracted.llm_importance, Some(0.8));
    assert_eq!(extracted.llm_importance_reason.as_deref(), Some("higher"));

    assert_eq!(token_count::score(0), 0.0);
    assert_eq!(token_count::score(30), 1.0);
    assert_eq!(token_count::score(9_000), 0.5);
    assert_eq!(unique_words::score("hi bob"), 0.5);
    assert!(unique_words::score("repeat repeat repeat repeat repeat repeat") < 0.1);
    assert!(unique_words::score("alpha beta gamma delta epsilon zeta eta") > 0.9);

    let mut meta = Metadata::point_in_time(ChunkSourceKind::Chat, "thread-1", "owner", Utc::now());
    assert_eq!(interaction::score(&meta), 0.5);
    meta.tags = vec![
        "sent".into(),
        "reply".into(),
        "mention".into(),
        "provider:whatsapp".into(),
    ];
    assert_eq!(interaction::score(&meta), 1.0);
    assert_eq!(
        source_weight::infer_data_source(&meta),
        Some(DataSource::Whatsapp)
    );
    assert!(source_weight::score(&meta) > 0.7);
    assert_eq!(metadata_weight::score(&meta), 0.5);

    let email_meta = Metadata::point_in_time(ChunkSourceKind::Email, "mail", "owner", Utc::now());
    let doc_meta = Metadata::point_in_time(ChunkSourceKind::Document, "doc", "owner", Utc::now());
    assert!(metadata_weight::score(&doc_meta) > metadata_weight::score(&email_meta));
    assert_eq!(source_weight::score(&email_meta), 0.75);

    let computed = compute_score_signals(
        &meta,
        "Alice from OpenHuman mentioned Phoenix migration on Friday",
        120,
        &extracted,
    );
    assert!(computed.token_count > 0.0);
    assert!(computed.unique_words > 0.0);
    assert_eq!(computed.llm_importance, 0.8);
    assert_eq!(entity_density_score(0, &extracted), 0.0);
    assert!(entity_density_score(120, &extracted) > 0.0);

    let weights = SignalWeights::with_llm_enabled();
    let total = combine(&computed, &weights);
    let cheap_total = combine_cheap_only(&computed, &weights);
    assert!((0.0..=1.0).contains(&total));
    assert!((0.0..=1.0).contains(&cheap_total));
    assert_eq!(
        combine(&ScoreSignals::default(), &SignalWeights::default()),
        0.0
    );

    for data_source in DataSource::all() {
        assert_eq!(
            DataSource::parse(data_source.as_str()).unwrap(),
            *data_source
        );
        assert_eq!(
            data_source.kind(),
            DataSource::parse(data_source.as_str()).unwrap().kind()
        );
    }
    assert!(DataSource::parse("missing").is_err());
}

#[test]
fn memory_tree_runtime_store_buffers_and_retrieval_wire_helpers() {
    let tmp = TempDir::new().expect("tempdir");
    let config = config_in(&tmp);
    let namespace = "slack:#eng";
    let root = tree_node(namespace, "root", "Workspace root summary");
    let year = tree_node(namespace, "2026", "Year summary");
    let month = tree_node(namespace, "2026/05", "Month summary");
    let day = tree_node(namespace, "2026/05/29", "Day summary");
    let hour = tree_node(namespace, "2026/05/29/12", "Hour leaf body");

    for node in [&root, &year, &month, &day, &hour] {
        tree_runtime_store::write_node(&config, node).expect("write tree node");
    }

    assert_eq!(
        tree_runtime_store::read_node(&config, namespace, "root")
            .unwrap()
            .unwrap()
            .summary,
        "Workspace root summary"
    );
    assert!(tree_runtime_store::read_node(&config, namespace, "missing")
        .unwrap()
        .is_none());
    assert_eq!(
        tree_runtime_store::read_children(&config, namespace, "root")
            .unwrap()
            .into_iter()
            .map(|node| node.node_id)
            .collect::<Vec<_>>(),
        vec!["2026"]
    );
    assert_eq!(
        tree_runtime_store::read_children(&config, namespace, "2026/05/29")
            .unwrap()
            .into_iter()
            .map(|node| node.node_id)
            .collect::<Vec<_>>(),
        vec!["2026/05/29/12"]
    );
    assert_eq!(
        tree_runtime_store::read_ancestors(&config, namespace, "2026/05/29/12")
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        tree_runtime_store::count_nodes(&config, namespace).unwrap(),
        5
    );
    let status = tree_runtime_store::get_tree_status(&config, namespace).unwrap();
    assert_eq!(status.total_nodes, 5);
    assert_eq!(status.depth, 5);
    assert_eq!(
        status.oldest_entry.unwrap().to_rfc3339(),
        "2026-05-29T12:00:00+00:00"
    );

    let summaries = tree_runtime_store::collect_root_summaries_with_caps(tmp.path(), 10, 12);
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].0, "slack_#eng");
    assert!(summaries[0].1.contains("[... truncated]"));
    assert_eq!(
        tree_runtime_store::list_namespaces_with_root(&config).unwrap(),
        vec!["slack_#eng".to_string()]
    );

    let ts = Utc.with_ymd_and_hms(2026, 5, 29, 13, 0, 0).unwrap();
    let first_buffer = tree_runtime_store::buffer_write(
        &config,
        namespace,
        "first buffered body",
        &ts,
        Some(&json!({ "source": "test" })),
    )
    .expect("buffer write");
    let second_buffer =
        tree_runtime_store::buffer_write(&config, namespace, "second buffered body", &ts, None)
            .expect("buffer write second");
    let buffered = tree_runtime_store::buffer_read(&config, namespace).expect("buffer read");
    assert_eq!(buffered.len(), 2);
    assert!(buffered
        .iter()
        .any(|(_, body)| body == "first buffered body"));
    tree_runtime_store::buffer_delete(
        &config,
        namespace,
        &[first_buffer
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string()],
    )
    .expect("buffer delete");
    assert!(!first_buffer.exists());
    assert!(second_buffer.exists());
    let drained = tree_runtime_store::buffer_drain(&config, namespace).expect("buffer drain");
    assert_eq!(drained.len(), 1);
    assert!(tree_runtime_store::buffer_read(&config, namespace)
        .unwrap()
        .is_empty());

    assert_eq!(NodeLevel::Hour.max_tokens(), 1_000);
    assert_eq!(NodeLevel::Month.parent_level(), Some(NodeLevel::Year));
    assert_eq!(NodeLevel::from_str_label("DAY"), Some(NodeLevel::Day));
    assert_eq!(
        derive_parent_id("2026/05/29/12").as_deref(),
        Some("2026/05/29")
    );
    assert_eq!(
        derive_node_ids(&ts),
        (
            "2026/05/29/13".to_string(),
            "2026/05/29".to_string(),
            "2026/05".to_string(),
            "2026".to_string(),
            "root".to_string()
        )
    );
    assert_eq!(node_id_to_path("root").to_string_lossy(), "root.md");
    assert!(tree_runtime_store::validate_namespace("team").is_ok());
    assert!(tree_runtime_store::validate_namespace("../bad").is_err());
    assert!(tree_runtime_store::validate_node_id("2026/05/29/23").is_ok());
    assert!(tree_runtime_store::validate_node_id("2026/13").is_err());

    let legacy = tree_runtime_store::parse_node_markdown_pub("legacy body", namespace, "2026")
        .expect("parse legacy");
    assert_eq!(legacy.level, NodeLevel::Year);
    assert_eq!(legacy.created_at, chrono::DateTime::<Utc>::UNIX_EPOCH);
    assert_eq!(
        tree_runtime_store::delete_tree(&config, namespace).unwrap(),
        5
    );
    assert_eq!(
        tree_runtime_store::delete_tree(&config, namespace).unwrap(),
        0
    );
}

#[test]
fn memory_retrieval_embedding_and_rpc_model_helpers_round_trip() {
    assert_eq!(retrieval::types::NodeKind::Leaf.as_str(), "leaf");
    assert_eq!(retrieval::types::NodeKind::Summary.as_str(), "summary");
    assert!(retrieval::types::QueryResponse::empty().hits.is_empty());

    let now = Utc.with_ymd_and_hms(2026, 5, 29, 12, 0, 0).unwrap();
    let summary = SummaryNode {
        id: "sum-1".into(),
        tree_id: "tree-1".into(),
        tree_kind: TreeKind::Topic,
        level: 2,
        parent_id: Some("root".into()),
        child_ids: vec!["child-1".into(), "child-2".into()],
        content: "Topic summary".into(),
        token_count: 3,
        entities: vec!["person:alice".into()],
        topics: vec!["coverage".into()],
        time_range_start: now,
        time_range_end: now,
        score: 0.8,
        sealed_at: now,
        deleted: false,
        embedding: None,
    };
    let tree = Tree {
        id: "tree-1".into(),
        kind: TreeKind::Topic,
        scope: "topic:coverage".into(),
        root_id: Some("sum-1".into()),
        max_level: 2,
        status: StoredTreeStatus::Active,
        created_at: now,
        last_sealed_at: Some(now),
    };
    let summary_hit = retrieval::types::hit_from_summary_with_tree(&summary, &tree);
    assert_eq!(summary_hit.node_kind, retrieval::types::NodeKind::Summary);
    assert_eq!(summary_hit.tree_scope, "topic:coverage");
    assert_eq!(summary_hit.child_ids, vec!["child-1", "child-2"]);

    let mut leaf_chunk = chunk("gmail:acct:msg-1", 0, now.timestamp_millis());
    leaf_chunk.metadata.source_ref = Some(SourceRef::new("<msg-1@example.test>"));
    let leaf_hit = retrieval::types::hit_from_chunk(&leaf_chunk, "tree-2", "gmail:acct", 0.4);
    assert_eq!(leaf_hit.node_kind, retrieval::types::NodeKind::Leaf);
    assert_eq!(leaf_hit.tree_kind, TreeKind::Source);
    assert_eq!(leaf_hit.source_ref.as_deref(), Some("<msg-1@example.test>"));
    let response = retrieval::types::QueryResponse::new(vec![leaf_hit], 2);
    assert!(response.truncated);
    assert_eq!(
        retrieval::types::leaf_tree_placeholder(ChunkSourceKind::Email),
        TreeKind::Source
    );

    assert_eq!(
        TreeKind::parse(TreeKind::Global.as_str()).unwrap(),
        TreeKind::Global
    );
    assert!(TreeKind::parse("missing").is_err());
    assert_eq!(
        StoredTreeStatus::parse(StoredTreeStatus::Archived.as_str()).unwrap(),
        StoredTreeStatus::Archived
    );
    assert!(StoredTreeStatus::parse("missing").is_err());

    let packed = embed::pack_checked(&vec![0.25; embed::EMBEDDING_DIM]).expect("pack checked");
    let unpacked = embed::unpack_embedding(&packed).expect("unpack");
    assert_eq!(unpacked.len(), embed::EMBEDDING_DIM);
    assert!(embed::pack_checked(&[1.0, 2.0]).is_err());
    assert!(embed::unpack_embedding(&[0, 1, 2]).is_err());
    assert!(embed::decode_optional_blob(None, "none").unwrap().is_none());
    assert!(embed::decode_optional_blob(Some(vec![0; 16]), "bad row").is_err());
    assert_eq!(embed::cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]), 0.0);

    let query = QueryNamespaceRequest {
        namespace: "default".into(),
        query: "coverage".into(),
        include_references: Some(true),
        document_ids: Some(vec!["doc-1".into()]),
        limit: Some(4),
        max_chunks: Some(6),
    };
    assert_eq!(query.resolved_limit(), 6);
    let recall_context = RecallContextRequest {
        namespace: "default".into(),
        include_references: None,
        limit: Some(3),
        max_chunks: None,
    };
    assert_eq!(recall_context.resolved_limit(), 3);
    let recall_memories = RecallMemoriesRequest {
        namespace: "default".into(),
        min_retention: Some(0.2),
        as_of: Some(1.0),
        limit: Some(3),
        max_chunks: Some(7),
        top_k: Some(9),
    };
    assert_eq!(recall_memories.resolved_limit(), 9);

    let envelope = ApiEnvelope {
        data: Some(json!({ "ok": true })),
        error: Some(ApiError {
            code: "coverage".into(),
            message: "covered".into(),
            details: Some(json!({ "line": true })),
        }),
        meta: ApiMeta {
            request_id: "req-1".into(),
            latency_seconds: Some(0.01),
            cached: Some(false),
            counts: None,
            pagination: Some(PaginationMeta {
                limit: 10,
                offset: 0,
                count: 1,
            }),
        },
    };
    let encoded = serde_json::to_value(envelope).expect("api envelope json");
    assert_eq!(encoded["meta"]["pagination"]["count"], 1);

    let entry = MemoryEntry {
        id: "mem-1".into(),
        key: "preference".into(),
        content: "Use deterministic tests".into(),
        namespace: Some("default".into()),
        category: MemoryCategory::Custom("testing".into()),
        timestamp: now.to_rfc3339(),
        session_id: Some("session-1".into()),
        score: Some(0.9),
    };
    assert_eq!(entry.category.to_string(), "testing");
    let opts = RecallOpts {
        namespace: Some("default"),
        category: Some(MemoryCategory::Conversation),
        session_id: Some("session-2"),
        min_score: Some(0.5),
        cross_session: true,
    };
    assert!(opts.cross_session);
    assert_eq!(opts.category.unwrap().to_string(), "conversation");
    let summary = NamespaceSummary {
        namespace: "default".into(),
        count: 1,
        last_updated: Some(now.to_rfc3339()),
    };
    assert_eq!(serde_json::to_value(summary).unwrap()["count"], 1);
}

#[test]
fn memory_sync_profile_markdown_and_status_helpers_are_idempotent() {
    let tmp = TempDir::new().expect("tempdir");
    let mut profile = ProviderUserProfile {
        toolkit: "gmail".into(),
        connection_id: Some("conn-1".into()),
        display_name: Some("Jane\nDoe".into()),
        email: Some("jane@example.com".into()),
        username: Some("jane\tdoe".into()),
        avatar_url: None,
        profile_url: Some("https://example.test/jane|profile".into()),
        extras: json!({ "source": "coverage" }),
    };

    merge_provider_into_profile_md(tmp.path(), &profile).expect("merge profile");
    profile.display_name = Some("Jane D.".into());
    merge_provider_into_profile_md(tmp.path(), &profile).expect("merge profile update");
    let profile_path = tmp.path().join("PROFILE.md");
    let body = std::fs::read_to_string(&profile_path).expect("read profile");
    assert!(body.contains(&block_start("connected-accounts")));
    assert!(body.contains("Jane D."));
    assert!(!body.contains("Jane\nDoe"));
    assert_eq!(body.matches("acct:gmail:conn-1").count(), 1);

    replace_managed_block(
        tmp.path(),
        "style",
        "## Style",
        "Use plain language.".into(),
    )
    .expect("replace style");
    replace_managed_block(tmp.path(), "goals", "## Goals", String::new()).expect("replace goals");
    let body = std::fs::read_to_string(&profile_path).expect("read profile after blocks");
    assert!(body.contains(&block_start("style")));
    assert!(body.contains("Use plain language."));
    assert!(body.contains("*(no entries yet)*"));
    assert!(body.contains(&block_end("goals")));

    remove_provider_from_profile_md(tmp.path(), "gmail", "conn-1").expect("remove provider");
    let body = std::fs::read_to_string(&profile_path).expect("read profile after remove");
    assert!(!body.contains("acct:gmail:conn-1"));

    let skipped = TempDir::new().expect("tempdir");
    let skipped_profile = ProviderUserProfile {
        toolkit: "gmail".into(),
        connection_id: None,
        display_name: Some("Skipped".into()),
        email: None,
        username: None,
        avatar_url: None,
        profile_url: None,
        extras: serde_json::Value::Null,
    };
    merge_provider_into_profile_md(skipped.path(), &skipped_profile).expect("skip profile");
    assert!(!skipped.path().join("PROFILE.md").exists());
    remove_provider_from_profile_md(skipped.path(), "", "").expect("remove missing no-op");

    let now = 1_700_000_000_000_i64;
    assert_eq!(
        openhuman_core::openhuman::memory_sync::sync_status::types::FreshnessLabel::from_age_ms(
            Some(now - 30_000),
            now
        ),
        openhuman_core::openhuman::memory_sync::sync_status::types::FreshnessLabel::Active
    );
    assert_eq!(
        openhuman_core::openhuman::memory_sync::sync_status::types::FreshnessLabel::from_age_ms(
            Some(now - 30_001),
            now
        ),
        openhuman_core::openhuman::memory_sync::sync_status::types::FreshnessLabel::Recent
    );
    assert_eq!(
        openhuman_core::openhuman::memory_sync::sync_status::types::FreshnessLabel::from_age_ms(
            None, now
        ),
        openhuman_core::openhuman::memory_sync::sync_status::types::FreshnessLabel::Idle
    );
}

#[test]
fn memory_source_types_and_freshness_cover_validation_matrix() {
    let kinds = [
        SourceKind::Composio,
        SourceKind::Folder,
        SourceKind::GithubRepo,
        SourceKind::TwitterQuery,
        SourceKind::RssFeed,
        SourceKind::WebPage,
    ];
    for kind in kinds {
        let encoded = serde_json::to_string(&kind).expect("kind json");
        let decoded: SourceKind = serde_json::from_str(&encoded).expect("kind decode");
        assert_eq!(decoded, kind);
    }

    let now = 1_700_000_000_000_i64;
    assert_eq!(FreshnessLabel::from_age_ms(None, now), FreshnessLabel::Idle);
    assert_eq!(
        FreshnessLabel::from_age_ms(Some(now - 30_000), now),
        FreshnessLabel::Active
    );
    assert_eq!(
        FreshnessLabel::from_age_ms(Some(now - 30_001), now),
        FreshnessLabel::Recent
    );
    assert_eq!(
        FreshnessLabel::from_age_ms(Some(now - 5 * 60_000 - 1), now),
        FreshnessLabel::Idle
    );

    let mut composio_source = source(SourceKind::Composio, "cmp");
    assert!(composio_source.validate().unwrap_err().contains("toolkit"));
    composio_source.toolkit = Some("gmail".into());
    assert!(composio_source
        .validate()
        .unwrap_err()
        .contains("connection_id"));
    composio_source.connection_id = Some("conn-1".into());
    assert!(composio_source.validate().is_ok());

    let mut folder = source(SourceKind::Folder, "folder");
    assert!(folder.validate().unwrap_err().contains("path"));
    folder.path = Some("/tmp".into());
    assert!(folder.validate().is_ok());

    let mut github = source(SourceKind::GithubRepo, "github");
    assert!(github.validate().unwrap_err().contains("url"));
    github.url = Some("https://github.com/tinyhumansai/openhuman".into());
    assert!(github.validate().is_ok());

    let item = SourceItem {
        id: "item-1".into(),
        title: "Item".into(),
        updated_at_ms: Some(now),
    };
    assert_eq!(serde_json::to_value(item).unwrap()["updated_at_ms"], now);
    let content = SourceContent {
        id: "item-1".into(),
        title: "Item".into(),
        body: "Body".into(),
        content_type: ContentType::Markdown,
        metadata: json!({ "source": "test" }),
    };
    assert_eq!(
        serde_json::to_value(content).unwrap()["content_type"],
        "markdown"
    );
}

#[test]
fn turn_state_store_persists_lists_marks_and_clears_snapshots() {
    let tmp = TempDir::new().expect("tempdir");
    let workspace = tmp.path().to_path_buf();
    let mut first = TurnState::started("thread/a", "request-1", 4, "2026-05-29T12:00:00Z");
    first.lifecycle = TurnLifecycle::Streaming;
    first.phase = Some(TurnPhase::Subagent);
    first.active_subagent = Some("research".into());
    first.tool_timeline.push(ToolTimelineEntry {
        id: "subagent-1".into(),
        name: "subagent:research".into(),
        round: 2,
        status: ToolTimelineStatus::Running,
        args_buffer: None,
        display_name: Some("Research".into()),
        detail: None,
        source_tool_name: None,
        subagent: Some(SubagentActivity {
            task_id: "task-1".into(),
            agent_id: "agent-1".into(),
            mode: Some("focused".into()),
            dedicated_thread: Some(true),
            child_iteration: Some(1),
            child_max_iterations: Some(3),
            iterations: Some(1),
            elapsed_ms: Some(250),
            output_chars: Some(42),
            tool_calls: vec![SubagentToolCall {
                call_id: "call-1".into(),
                tool_name: "memory.search".into(),
                status: ToolTimelineStatus::Success,
                iteration: Some(1),
                elapsed_ms: Some(100),
                output_chars: Some(10),
            }],
        }),
    });
    let second = TurnState::started("thread/b", "request-2", 2, "2026-05-29T12:01:00Z");

    turn_state::store::put(workspace.clone(), &first).expect("put first");
    turn_state::store::put(workspace.clone(), &second).expect("put second");
    assert_eq!(
        turn_state::store::get(workspace.clone(), "thread/a")
            .unwrap()
            .unwrap()
            .active_subagent
            .as_deref(),
        Some("research")
    );
    assert!(turn_state::store::get(workspace.clone(), "missing")
        .unwrap()
        .is_none());

    let mut listed = turn_state::store::list(workspace.clone()).expect("list states");
    listed.sort_by(|a, b| a.thread_id.cmp(&b.thread_id));
    assert_eq!(listed.len(), 2);
    let wire = serde_json::to_value(ListTurnStatesResponse {
        turn_states: listed.clone(),
        count: listed.len(),
    })
    .expect("list response json");
    assert_eq!(wire["count"], 2);
    assert_eq!(wire["turnStates"][0]["threadId"], "thread/a");

    let marked = turn_state::store::mark_all_interrupted(workspace.clone(), "2026-05-29T12:02:00Z")
        .expect("mark interrupted");
    assert_eq!(marked, 2);
    let marked_again =
        turn_state::store::mark_all_interrupted(workspace.clone(), "2026-05-29T12:03:00Z")
            .expect("mark interrupted again");
    assert_eq!(marked_again, 0);
    let interrupted = turn_state::store::get(workspace.clone(), "thread/a")
        .unwrap()
        .unwrap();
    assert_eq!(interrupted.lifecycle, TurnLifecycle::Interrupted);
    assert!(interrupted.active_subagent.is_none());
    assert_eq!(interrupted.updated_at, "2026-05-29T12:02:00Z");

    assert!(turn_state::store::delete(workspace.clone(), "thread/a").expect("delete one"));
    assert!(!turn_state::store::delete(workspace.clone(), "thread/a").expect("delete missing"));
    let removed = turn_state::store::clear_all(workspace.clone()).expect("clear all");
    assert_eq!(removed, 1);
    assert!(turn_state::store::list(workspace).unwrap().is_empty());
}

#[tokio::test]
async fn threads_rpc_ops_cover_crud_title_fallback_and_turn_state_cleanup() {
    let tmp = TempDir::new().expect("tempdir");
    let _workspace = EnvVarGuard::set_to_path("OPENHUMAN_WORKSPACE", tmp.path());
    let config = Config::load_or_init().await.expect("init isolated config");
    let workspace_dir = config.workspace_dir.clone();

    let thread = thread_ops::thread_upsert(UpsertConversationThreadRequest {
        id: "thread/raw-crud".into(),
        title: "Chat Jan 1 1:00 AM".into(),
        created_at: "2026-05-29T12:00:00Z".into(),
        parent_thread_id: Some("parent-thread".into()),
        labels: Some(vec!["work".into(), "coverage".into()]),
        personality_id: Some("personality-1".into()),
    })
    .await
    .expect("upsert thread")
    .value
    .data
    .expect("thread summary");
    assert_eq!(thread.id, "thread/raw-crud");
    assert_eq!(thread.parent_thread_id.as_deref(), Some("parent-thread"));

    let created = thread_ops::thread_create_new(CreateConversationThreadRequest {
        labels: Some(vec!["scratch".into()]),
        personality_id: None,
    })
    .await
    .expect("create new thread")
    .value
    .data
    .expect("created thread");
    assert!(created.id.starts_with("thread-"));
    assert_eq!(created.labels, vec!["scratch"]);

    let message = ConversationMessageRecord {
        id: "msg-1".into(),
        content: "Please summarize launch blockers. Then inspect follow ups.".into(),
        message_type: "text".into(),
        extra_metadata: json!({ "before": true }),
        sender: "user".into(),
        created_at: "2026-05-29T12:01:00Z".into(),
    };
    let appended = thread_ops::message_append(AppendConversationMessageRequest {
        thread_id: "thread/raw-crud".into(),
        message: message.clone(),
    })
    .await
    .expect("append message")
    .value
    .data
    .expect("appended message");
    assert_eq!(appended.id, "msg-1");
    assert!(
        thread_ops::message_append(AppendConversationMessageRequest {
            thread_id: "missing-thread".into(),
            message,
        })
        .await
        .is_err()
    );

    let listed_messages = thread_ops::messages_list(ConversationMessagesRequest {
        thread_id: "thread/raw-crud".into(),
    })
    .await
    .expect("list messages")
    .value
    .data
    .expect("messages");
    assert_eq!(listed_messages.count, 1);

    let fallback_title =
        thread_ops::thread_generate_title(GenerateConversationThreadTitleRequest {
            thread_id: "thread/raw-crud".into(),
            assistant_message: Some("   ".into()),
        })
        .await
        .expect("fallback title")
        .value
        .data
        .expect("fallback summary");
    assert_eq!(fallback_title.title, "Please summarize launch blockers");

    assert!(
        thread_ops::thread_update_title(UpdateConversationThreadTitleRequest {
            thread_id: "thread/raw-crud".into(),
            title: "   ".into(),
        })
        .await
        .unwrap_err()
        .contains("title must not be empty")
    );
    let renamed = thread_ops::thread_update_title(UpdateConversationThreadTitleRequest {
        thread_id: "thread/raw-crud".into(),
        title: " Manual coverage title ".into(),
    })
    .await
    .expect("manual title")
    .value
    .data
    .expect("renamed");
    assert_eq!(renamed.title, "Manual coverage title");

    let relabeled = thread_ops::thread_update_labels(UpdateConversationThreadLabelsRequest {
        thread_id: "thread/raw-crud".into(),
        labels: Vec::new(),
    })
    .await
    .expect("clear labels")
    .value
    .data
    .expect("relabeled");
    assert!(relabeled.labels.is_empty());

    let updated_message = thread_ops::message_update(UpdateConversationMessageRequest {
        thread_id: "thread/raw-crud".into(),
        message_id: "msg-1".into(),
        extra_metadata: Some(json!({ "after": true })),
    })
    .await
    .expect("update message")
    .value
    .data
    .expect("updated message");
    assert_eq!(updated_message.extra_metadata["after"], true);
    assert!(
        thread_ops::message_update(UpdateConversationMessageRequest {
            thread_id: "thread/raw-crud".into(),
            message_id: "missing".into(),
            extra_metadata: None,
        })
        .await
        .unwrap_err()
        .contains("message missing not found")
    );

    let all_threads = thread_ops::threads_list(EmptyRequest {})
        .await
        .expect("list threads")
        .value
        .data
        .expect("threads");
    assert!(all_threads.count >= 2);
    assert!(all_threads
        .threads
        .iter()
        .any(|thread| thread.title == "Manual coverage title"));

    let mut turn = TurnState::started("thread/raw-crud", "request-raw", 3, "2026-05-29T12:02:00Z");
    turn.lifecycle = TurnLifecycle::Streaming;
    turn.phase = Some(TurnPhase::Thinking);
    turn_state::store::put(workspace_dir.clone(), &turn).expect("put turn state");
    let turn_get = thread_ops::turn_state_get(GetTurnStateRequest {
        thread_id: "thread/raw-crud".into(),
    })
    .await
    .expect("turn get")
    .value
    .data
    .expect("turn response");
    assert_eq!(turn_get.turn_state.unwrap().request_id, "request-raw");
    let turn_list = thread_ops::turn_state_list(EmptyRequest {})
        .await
        .expect("turn list")
        .value
        .data
        .expect("turn list response");
    assert_eq!(turn_list.count, 1);
    assert!(
        thread_ops::turn_state_clear(ClearTurnStateRequest {
            thread_id: "missing".into(),
        })
        .await
        .expect("clear missing")
        .value
        .data
        .expect("clear response")
        .cleared
            == false
    );
    turn_state::store::put(workspace_dir.clone(), &turn).expect("restore turn state");

    let deleted = thread_ops::thread_delete(DeleteConversationThreadRequest {
        thread_id: "thread/raw-crud".into(),
        deleted_at: "2026-05-29T12:03:00Z".into(),
    })
    .await
    .expect("delete thread")
    .value
    .data
    .expect("delete response");
    assert!(deleted.deleted);
    assert!(turn_state::store::get(workspace_dir, "thread/raw-crud")
        .unwrap()
        .is_none());

    let purged = thread_ops::threads_purge(EmptyRequest {})
        .await
        .expect("purge")
        .value
        .data
        .expect("purge response");
    assert!(purged.agent_threads_deleted >= 1);
}

#[tokio::test]
async fn memory_sources_registry_rpc_and_schema_handlers_cover_crud_edges() {
    let tmp = TempDir::new().expect("tempdir");
    let _workspace = EnvVarGuard::set_to_path("OPENHUMAN_WORKSPACE", tmp.path());
    Config::load_or_init().await.expect("init isolated config");
    std::fs::write(tmp.path().join("reader-note.md"), "# Reader note").expect("write note");

    let schemas = all_memory_sources_controller_schemas();
    let controllers = all_memory_sources_registered_controllers();
    assert_eq!(schemas.len(), 9);
    assert_eq!(schemas.len(), controllers.len());
    assert_eq!(
        openhuman_core::openhuman::memory_sources::schemas::schemas("read_item").function,
        "read_item"
    );

    let add_controller = controllers
        .iter()
        .find(|controller| controller.schema.function == "add")
        .expect("add controller");
    let mut bad_params = Map::new();
    bad_params.insert("kind".into(), Value::String("folder".into()));
    assert!((add_controller.handler)(bad_params)
        .await
        .unwrap_err()
        .contains("missing field `label`"));

    let invalid_folder = memory_sources_rpc::add_rpc(memory_sources_rpc::AddRequest {
        kind: SourceKind::Folder,
        label: "Invalid folder".into(),
        enabled: true,
        toolkit: None,
        connection_id: None,
        path: None,
        glob: None,
        url: None,
        branch: None,
        paths: Vec::new(),
        query: None,
        since_days: None,
        max_items: None,
        selector: None,
    })
    .await
    .unwrap_err();
    assert!(invalid_folder.contains("path"));

    let added = memory_sources_rpc::add_rpc(memory_sources_rpc::AddRequest {
        kind: SourceKind::Folder,
        label: "Folder source".into(),
        enabled: true,
        toolkit: None,
        connection_id: None,
        path: Some(tmp.path().to_string_lossy().to_string()),
        glob: Some("*.md".into()),
        url: None,
        branch: None,
        paths: Vec::new(),
        query: None,
        since_days: None,
        max_items: Some(4),
        selector: None,
    })
    .await
    .expect("add folder")
    .value
    .source;
    assert_eq!(added.kind, SourceKind::Folder);
    assert!(memory_sources_rpc::add_rpc(memory_sources_rpc::AddRequest {
        kind: SourceKind::Folder,
        label: "Duplicate".into(),
        enabled: true,
        toolkit: None,
        connection_id: None,
        path: Some(tmp.path().to_string_lossy().to_string()),
        glob: None,
        url: None,
        branch: None,
        paths: Vec::new(),
        query: None,
        since_days: None,
        max_items: None,
        selector: None,
    })
    .await
    .is_ok());

    let enabled_folders = registry::list_enabled_by_kind(SourceKind::Folder)
        .await
        .expect("enabled folders");
    assert!(enabled_folders.len() >= 2);
    assert_eq!(
        memory_sources_rpc::get_rpc(memory_sources_rpc::GetRequest {
            id: added.id.clone(),
        })
        .await
        .expect("get source")
        .value
        .source
        .unwrap()
        .label,
        "Folder source"
    );
    assert!(memory_sources_rpc::get_rpc(memory_sources_rpc::GetRequest {
        id: "missing".into(),
    })
    .await
    .expect("get missing")
    .value
    .source
    .is_none());

    let list_items = memory_sources_rpc::list_items_rpc(memory_sources_rpc::ListItemsRequest {
        source_id: added.id.clone(),
    })
    .await
    .expect("list items")
    .value
    .items;
    assert!(list_items.iter().any(|item| item.id == "reader-note.md"));
    let read_item = memory_sources_rpc::read_item_rpc(memory_sources_rpc::ReadItemRequest {
        source_id: added.id.clone(),
        item_id: "reader-note.md".into(),
    })
    .await
    .expect("read item")
    .value
    .content;
    assert_eq!(read_item.content_type, ContentType::Markdown);

    let disabled = memory_sources_rpc::update_rpc(memory_sources_rpc::UpdateRequest {
        id: added.id.clone(),
        patch: serde_json::from_value(json!({
            "label": "Disabled folder",
            "enabled": false,
            "glob": "**/*.md",
            "max_items": 2
        }))
        .expect("patch"),
    })
    .await
    .expect("update source")
    .value
    .source;
    assert_eq!(disabled.label, "Disabled folder");
    assert!(!disabled.enabled);
    assert!(
        memory_sources_rpc::sync_rpc(memory_sources_rpc::SyncRequest {
            source_id: added.id.clone(),
        })
        .await
        .unwrap_err()
        .contains("disabled")
    );
    assert!(
        memory_sources_rpc::update_rpc(memory_sources_rpc::UpdateRequest {
            id: "missing".into(),
            patch: Default::default(),
        })
        .await
        .unwrap_err()
        .contains("not found")
    );
    assert!(
        memory_sources_rpc::list_items_rpc(memory_sources_rpc::ListItemsRequest {
            source_id: "missing".into(),
        })
        .await
        .unwrap_err()
        .contains("not found")
    );

    let statuses = memory_sources_rpc::status_list_rpc()
        .await
        .expect("status list")
        .value
        .statuses;
    assert!(statuses.iter().any(|status| status.source_id == added.id));

    assert!(
        memory_sources_rpc::remove_rpc(memory_sources_rpc::RemoveRequest {
            id: added.id.clone(),
        })
        .await
        .expect("remove source")
        .value
        .removed
    );
    assert!(
        !memory_sources_rpc::remove_rpc(memory_sources_rpc::RemoveRequest { id: added.id })
            .await
            .expect("remove missing")
            .value
            .removed
    );
}
