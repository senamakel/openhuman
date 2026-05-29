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
use tempfile::TempDir;

use openhuman_core::openhuman::config::Config;
use openhuman_core::openhuman::memory::tree_policy::TreePolicy;
use openhuman_core::openhuman::memory::tree_source;
use openhuman_core::openhuman::memory_sources::readers::reader_for;
use openhuman_core::openhuman::memory_sources::status::{source_status, FreshnessLabel};
use openhuman_core::openhuman::memory_sources::types::{
    ContentType, MemorySourceEntry, SourceKind,
};
use openhuman_core::openhuman::memory_store::chunks::store::{upsert_chunks, with_connection};
use openhuman_core::openhuman::memory_store::chunks::types::{
    approx_token_count, chunk_id, Chunk, Metadata, SourceKind as ChunkSourceKind,
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
use openhuman_core::openhuman::threads::title::{
    build_title_prompt, collapse_whitespace, is_auto_generated_thread_title,
    sanitize_generated_title, title_from_user_message, title_log_fingerprint,
};
use openhuman_core::openhuman::threads::turn_state::{
    GetTurnStateResponse, ToolTimelineEntry, ToolTimelineStatus, TurnLifecycle, TurnPhase,
    TurnState,
};
use openhuman_core::openhuman::threads::ThreadsError;

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
