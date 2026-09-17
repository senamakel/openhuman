use super::*;

struct DropNotify(Option<tokio::sync::oneshot::Sender<()>>);

impl Drop for DropNotify {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(());
        }
    }
}

#[tokio::test]
async fn registry_shutdown_aborts_stored_scanner_and_is_repeatable() {
    let registry = ScannerRegistry::new();
    let (drop_tx, drop_rx) = tokio::sync::oneshot::channel();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let task = tauri::async_runtime::spawn(async move {
        let _notify = DropNotify(Some(drop_tx));
        let _ = started_tx.send(());
        std::future::pending::<()>().await;
    });
    started_rx.await.expect("scanner task should start");
    *registry.inner.lock() = Some(task);

    registry.shutdown();

    assert!(registry.inner.lock().is_none());
    tokio::time::timeout(std::time::Duration::from_secs(1), drop_rx)
        .await
        .expect("iMessage scanner task should be cancelled promptly")
        .expect("drop notifier should send on cancellation");

    registry.shutdown();
    assert!(registry.inner.lock().is_none());
}

#[test]
fn apple_ns_to_unix_converts_apple_epoch_zero() {
    assert_eq!(apple_ns_to_unix(0), 978_307_200);
}

#[test]
fn apple_ns_to_unix_converts_one_second_past_apple_epoch() {
    assert_eq!(apple_ns_to_unix(1_000_000_000), 978_307_201);
}

#[test]
fn seconds_to_ymd_formats_known_date_in_local_tz() {
    // 2001-01-01 00:00:00 UTC. In US timezones this falls on 2000-12-31
    // in local time, so assert only the shape (YYYY-MM-DD) and that the
    // year is 2000 or 2001 — keeps the test robust across CI timezones.
    let out = seconds_to_ymd(978_307_200);
    assert_eq!(out.len(), 10);
    assert!(
        out.starts_with("2000-") || out.starts_with("2001-"),
        "got {}",
        out
    );
}

#[test]
fn extract_text_from_attributed_body_finds_message() {
    // Fake typedstream-style blob with 'hello world' as the longest
    // printable run embedded between type markers.
    let mut blob = b"streamtyped\x81\xe8\x03\x84\x01@\x84\x84\x84\x08NSString\x00\x84\x84\x08NSObject\x00\x85\x84\x01+\x0bhello world\x86".to_vec();
    blob.extend_from_slice(b"\x00\x00\x00");
    let out = extract_text_from_attributed_body(&blob).unwrap_or_default();
    assert!(out.contains("hello world"), "got {:?}", out);
}

#[test]
fn message_body_prefers_text_then_attributed_body() {
    let m = chatdb::Message {
        rowid: 1,
        guid: None,
        text: Some("direct".into()),
        attributed_body: Some(b"ignored".to_vec()),
        date_ns: 0,
        is_from_me: false,
        handle_id: None,
        chat_identifier: None,
        chat_name: None,
        service: None,
    };
    assert_eq!(message_body(&m), "direct");

    let m2 = chatdb::Message {
        rowid: 2,
        guid: None,
        text: None,
        attributed_body: Some(b"\x00\x00fallback body\x00".to_vec()),
        date_ns: 0,
        is_from_me: false,
        handle_id: None,
        chat_identifier: None,
        chat_name: None,
        service: None,
    };
    let body = message_body(&m2);
    assert!(body.contains("fallback body"), "got {:?}", body);
}

#[test]
fn chat_allowed_empty_list_allows_all() {
    assert!(chat_allowed("+15551234567", &[]));
}

#[test]
fn chat_allowed_wildcard_allows_all() {
    assert!(chat_allowed("+15551234567", &["*".to_string()]));
}

#[test]
fn chat_allowed_matches_exact_entry_case_insensitive() {
    let allowed = vec!["+15551234567".to_string(), "USER@Example.com".to_string()];
    assert!(chat_allowed("+15551234567", &allowed));
    assert!(chat_allowed("user@example.com", &allowed));
    assert!(!chat_allowed("+15550000000", &allowed));
}

#[test]
fn format_transcript_renders_known_messages() {
    let msgs = vec![
        chatdb::Message {
            rowid: 1,
            guid: None,
            text: Some("hi".into()),
            attributed_body: None,
            date_ns: 0,
            is_from_me: false,
            handle_id: Some("+15551234567".into()),
            chat_identifier: Some("+15551234567".into()),
            chat_name: None,
            service: None,
        },
        chatdb::Message {
            rowid: 2,
            guid: None,
            text: Some("yo".into()),
            attributed_body: None,
            date_ns: 0,
            is_from_me: true,
            handle_id: None,
            chat_identifier: Some("+15551234567".into()),
            chat_name: None,
            service: None,
        },
    ];
    let transcript = format_transcript(&msgs);
    let groups =
        std::collections::HashMap::from([("+15551234567:day".to_string(), transcript.clone())]);
    let _ = groups;
    assert_eq!(groups.len(), 1);
    let transcript = groups.values().next().expect("one group").clone();
    assert!(transcript.contains("hi"));
    assert!(transcript.contains("yo"));
    assert!(transcript.contains("me:"));
}

/// Real chat.db integration test. Gated with `#[ignore]` — run with
/// `cargo test --manifest-path crates/openhuman-app/Cargo.toml \
///   imessage_scanner -- --ignored`. Requires Full Disk Access granted
/// to the test-runner binary. Asserts we can open chat.db read-only,
/// run our JOIN query, and deserialize at least one row.
#[test]
#[ignore]
fn real_chat_db_opens_and_returns_messages() {
    let path = match chat_db_path() {
        Some(p) => p,
        None => {
            eprintln!("HOME not set — skipping");
            return;
        }
    };
    if !path.exists() {
        eprintln!("chat.db not found at {} — skipping", path.display());
        return;
    }
    let msgs = match chatdb::read_since(&path, 0, 5) {
        Ok(m) => m,
        Err(e) => panic!("read_since failed: {}", e),
    };
    assert!(
        !msgs.is_empty(),
        "expected at least one message from a real chat.db — is it empty?"
    );
    // Each message should have a rowid and a date_ns in Apple-epoch range.
    for m in &msgs {
        assert!(m.rowid > 0);
        assert!(m.date_ns >= 0);
    }
}

/// Sanity: `read_since` with cursor past max rowid returns empty.
#[test]
#[ignore]
fn real_chat_db_empty_past_cursor() {
    let path = match chat_db_path() {
        Some(p) => p,
        None => return,
    };
    if !path.exists() {
        return;
    }
    // rowid way past any real value
    let msgs = chatdb::read_since(&path, i64::MAX - 1, 10).unwrap();
    assert!(msgs.is_empty());
}
