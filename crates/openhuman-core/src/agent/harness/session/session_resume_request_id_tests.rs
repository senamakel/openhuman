use super::*;

/// #6282 review: a resume leaves every row's own `extra_metadata` exactly as it
/// was — a scalar, a caller object that happens to use the wrapper key, and a
/// scalar under *two* host markers (a failed tool row that is also replayed,
/// where removing the first marker must not drop the second) — while each row
/// keeps the request it was first written under.
#[test]
fn resumed_rows_keep_their_own_extra_metadata() {
    use crate::agent::harness::session::transcript::{
        append_transcript_turn, attach_tool_failure_metadata, read_transcript,
        read_transcript_display, DisplayRecord,
    };
    use crate::agent::messages::ChatMessage;

    let dir = tempfile::TempDir::new().expect("temp dir");
    let meta = fake_transcript_meta("thr_metadata");

    let mut noted = ChatMessage::user("noted question");
    noted.extra_metadata = Some(serde_json::json!("pinned"));
    let mut collides = ChatMessage::user("caller uses the wrapper key");
    collides.extra_metadata = Some(serde_json::json!({ "openhuman_wrapped_value": "pinned" }));
    let mut failed_tool = ChatMessage::tool(r#"{"tool_call_id":"call-1","content":"boom"}"#);
    failed_tool.extra_metadata = Some(serde_json::json!("tool-note"));
    attach_tool_failure_metadata(&mut failed_tool, Some("boom"));

    let first = dir.path().join("first.jsonl");
    append_transcript_turn(
        &first,
        &[],
        &[ChatMessage::system("sys"), noted, collides, failed_tool],
        &meta,
        None,
        Some("req-1"),
    )
    .expect("turn 1");

    // Resume into a fresh file: the reader re-attaches each row's provenance,
    // the writer strips the markers again.
    let resumed = read_transcript(&first).expect("read").messages;
    let second = dir.path().join("second.jsonl");
    append_transcript_turn(&second, &[], &resumed, &meta, None, Some("req-2")).expect("turn 2");

    let rows: Vec<_> = read_transcript_display(&second)
        .expect("display read")
        .records
        .into_iter()
        .filter_map(|record| match record {
            DisplayRecord::Message(m) => Some(m),
            _ => None,
        })
        .collect();
    assert_eq!(
        rows.iter()
            .map(|row| row.request_id.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("req-1"); 4],
        "every replayed row keeps the request that wrote it"
    );
    assert_eq!(
        rows[0].message.extra_metadata, None,
        "a row that had no metadata keeps none"
    );
    assert_eq!(
        rows[1].message.extra_metadata,
        Some(serde_json::json!("pinned")),
        "a scalar survives the resume unwrapped"
    );
    assert_eq!(
        rows[2].message.extra_metadata,
        Some(serde_json::json!({ "openhuman_wrapped_value": "pinned" })),
        "a caller object using the wrapper key stays an object"
    );
    assert_eq!(
        rows[3].message.extra_metadata,
        Some(serde_json::json!("tool-note")),
        "a scalar under both the failure and replay markers survives intact"
    );
    assert!(
        rows[3].failure && rows[3].failure_detail.as_deref() == Some("boom"),
        "the replayed tool row keeps its failure flag and detail"
    );

    // The display reader reconstructs from the line fields, so assert the file
    // itself too: the markers are an in-memory side-channel and must never be
    // persisted, and each row's caller metadata must be on disk in its original
    // shape.
    let raw = std::fs::read_to_string(&second).expect("read persisted transcript");
    for marker in [
        "openhuman_replayed",
        "openhuman_tool_failure",
        "openhuman_wrapped_value\":{",
    ] {
        assert!(
            !raw.contains(marker),
            "the persisted transcript must not carry {marker}: {raw}"
        );
    }
    assert!(
        raw.contains(r#""extra_metadata":"pinned""#)
            && raw.contains(r#""extra_metadata":{"openhuman_wrapped_value":"pinned"}"#)
            && raw.contains(r#""extra_metadata":"tool-note""#),
        "each row's caller metadata must be persisted in its original shape: {raw}"
    );
}

/// #6282 review: when a resumed transcript is later reduced, the writer's
/// compaction record carries the full reduced set. Replayed rows inside it must
/// keep the request they were first written under, the same as on a plain
/// append.
#[test]
fn replayed_rows_keep_their_request_id_inside_a_compaction_record() {
    use crate::agent::harness::session::transcript::{
        append_transcript_turn, read_transcript, read_transcript_display, DisplayRecord,
    };
    use crate::agent::messages::ChatMessage;

    let dir = tempfile::TempDir::new().expect("temp dir");
    let meta = fake_transcript_meta("thr_compaction");

    let first = dir.path().join("first.jsonl");
    let turn1 = vec![
        ChatMessage::system("sys"),
        ChatMessage::user("old question"),
        ChatMessage::assistant("old answer"),
    ];
    append_transcript_turn(&first, &[], &turn1, &meta, None, Some("req-1")).expect("turn 1");

    // Resume into a fresh file, then append the resuming turn.
    let mut resumed = read_transcript(&first).expect("read").messages;
    resumed.push(ChatMessage::user("new question"));
    let second = dir.path().join("second.jsonl");
    append_transcript_turn(&second, &[], &resumed, &meta, None, Some("req-2")).expect("turn 2");

    // A context reduction drops the old question: the persisted set is no
    // longer a prefix, so the writer appends a compaction record.
    let reduced = vec![resumed[0].clone(), resumed[2].clone(), resumed[3].clone()];
    append_transcript_turn(&second, &resumed, &reduced, &meta, None, Some("req-3"))
        .expect("reduced turn");

    let compaction = read_transcript_display(&second)
        .expect("display read")
        .records
        .into_iter()
        .find_map(|record| match record {
            DisplayRecord::Compaction(marker) => Some(marker),
            _ => None,
        })
        .expect("the reduction must be recorded as a compaction");
    let ids: Vec<Option<&str>> = compaction
        .replacement
        .iter()
        .map(|m| m.request_id.as_deref())
        .collect();
    assert_eq!(
        ids,
        vec![Some("req-1"), Some("req-1"), Some("req-3")],
        "replayed rows keep req-1 inside the compaction; only the non-replayed row takes \
         the compacting request, as compaction already does for live rows"
    );
}

/// #6282: rows seeded from the conversation log belong to earlier turns and
/// carry no request id, so persisting them together with the next turn must
/// not stamp them with that turn's request.
#[test]
fn prose_seeded_rows_are_not_restamped_with_the_resuming_request() {
    use crate::agent::harness::session::transcript::{
        append_transcript_turn, read_transcript_display, DisplayRecord,
    };
    use crate::agent::messages::{ChatMessage, ConversationMessage};

    let mut agent = build_minimal_agent_with_definition_name(Some("orchestrator"));
    agent
        .seed_resume_from_messages(
            vec![
                ("user".to_string(), "install it".to_string()),
                ("agent".to_string(), "Something went wrong.".to_string()),
            ],
            "what happened?",
        )
        .expect("seed");
    // The resuming turn: its own message, then the seeded prefix absorbed ahead
    // of it and rendered for persistence, exactly as the turn driver does.
    agent.history = vec![ConversationMessage::Chat(ChatMessage::user(
        "what happened?",
    ))];
    agent.absorb_resumed_transcript_prefix();
    let messages = agent.tool_dispatcher.to_provider_messages(&agent.history);

    let dir = tempfile::TempDir::new().expect("temp dir");
    let path = dir.path().join("seeded.jsonl");
    append_transcript_turn(
        &path,
        &[],
        &messages,
        &fake_transcript_meta("thr_seeded"),
        None,
        Some("req-now"),
    )
    .expect("persist seeded turn");

    let rows: Vec<(String, Option<String>)> = read_transcript_display(&path)
        .expect("display read")
        .records
        .into_iter()
        .filter_map(|record| match record {
            DisplayRecord::Message(m) => Some((m.message.content, m.request_id)),
            _ => None,
        })
        .collect();
    let ids: Vec<Option<&str>> = rows.iter().map(|(_, id)| id.as_deref()).collect();
    assert_eq!(
        ids,
        vec![None, None, None, Some("req-now")],
        "the seeded prefix (its system prompt included) keeps no request id; only \
         this turn's new message takes it: {rows:?}"
    );
}

/// #6282: a thread transcript written without request ids (a CLI or full-rewrite
/// transcript) resumed into a request-scoped turn must not have its replayed
/// rows restamped with the resuming request, which would group every earlier
/// message into the current turn.
#[test]
fn a_resumed_request_less_transcript_is_not_restamped_with_the_resuming_request() {
    use super::super::transcript::{self, read_transcript_display, DisplayRecord};
    use crate::agent::messages::{ChatMessage, ConversationMessage};

    let ws = tempfile::TempDir::new().expect("temp workspace");
    let wsp = ws.path().to_path_buf();
    let thread_id = "thr_request_less";
    let path = transcript::resolve_keyed_transcript_path(&wsp, "1700000000_orchestrator")
        .expect("resolve transcript path");
    transcript::write_transcript(
        &path,
        &[
            ChatMessage::system("stored prompt"),
            ChatMessage::user("first question"),
            ChatMessage::assistant("first answer"),
        ],
        &fake_transcript_meta(thread_id),
        None,
    )
    .expect("write transcript");

    let mem: Arc<dyn Memory> = crate::memory::test_support::noop_memory();
    let mut agent = Agent::builder()
        .chat_model(Arc::new(MockProvider {
            responses: Mutex::new(vec![]),
        }))
        .tools(vec![Box::new(MockTool)])
        .memory(mem)
        .tool_dispatcher(Box::new(NativeToolDispatcher))
        .workspace_dir(wsp.clone())
        .build()
        .expect("agent build should succeed");
    assert!(agent.seed_resume_from_thread_transcript(thread_id));

    // The resuming turn: its own message, then the replayed prefix absorbed
    // ahead of it, exactly as the turn driver does.
    agent.history = vec![ConversationMessage::Chat(ChatMessage::user(
        "second question",
    ))];
    agent.absorb_resumed_transcript_prefix();
    let messages = agent.tool_dispatcher.to_provider_messages(&agent.history);

    let out = wsp.join("resumed.jsonl");
    transcript::append_transcript_turn(
        &out,
        &[],
        &messages,
        &fake_transcript_meta(thread_id),
        None,
        Some("req-2"),
    )
    .expect("persist resumed turn");
    let rows: Vec<(String, Option<String>)> = read_transcript_display(&out)
        .expect("display read")
        .records
        .into_iter()
        .filter_map(|record| match record {
            DisplayRecord::Message(m) => Some((m.message.content, m.request_id)),
            _ => None,
        })
        .collect();
    let ids: Vec<Option<&str>> = rows.iter().map(|(_, id)| id.as_deref()).collect();
    assert_eq!(
        ids,
        vec![None, None, None, Some("req-2")],
        "replayed request-less rows keep no request id; only the resuming turn's row takes it: {rows:?}"
    );
}
