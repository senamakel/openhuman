use super::*;
use parking_lot::Mutex;
use std::sync::Arc;

struct FakeDeps {
    gate: anyhow::Result<Option<Vec<String>>>,
    calls: Arc<Mutex<Vec<(String, String, String)>>>,
    fail_keys: Vec<String>,
}

impl FakeDeps {
    fn new(gate: Option<Vec<String>>) -> Self {
        Self {
            gate: Ok(gate),
            calls: Arc::new(Mutex::new(Vec::new())),
            fail_keys: Vec::new(),
        }
    }
}

#[async_trait]
impl TickDeps for FakeDeps {
    async fn fetch_gate(&self) -> anyhow::Result<Option<Vec<String>>> {
        match &self.gate {
            Ok(g) => Ok(g.clone()),
            Err(e) => Err(anyhow::anyhow!("{}", e)),
        }
    }
    async fn ingest_group(
        &self,
        account_id: &str,
        key: &str,
        transcript: String,
    ) -> anyhow::Result<()> {
        if self.fail_keys.iter().any(|k| k == key) {
            anyhow::bail!("forced failure for key={}", key);
        }
        self.calls
            .lock()
            .push((account_id.to_string(), key.to_string(), transcript));
        Ok(())
    }
}

fn chat_db() -> Option<PathBuf> {
    super::super::chat_db_path().filter(|p| p.exists())
}

#[tokio::test]
async fn skips_when_gate_disconnected() {
    let deps = FakeDeps::new(None);
    let out = run_single_tick(
        TickInput {
            db_path: PathBuf::from("/nonexistent/chat.db"),
            last_rowid: 0,
            account_id: "test".into(),
        },
        &deps,
    )
    .await
    .unwrap();
    assert!(out.skipped_unconnected);
    assert_eq!(out.groups_attempted, 0);
    assert_eq!(out.new_rowid, 0);
    assert!(deps.calls.lock().is_empty());
}

#[tokio::test]
#[ignore]
async fn run_single_tick_ingests_groups_from_real_chatdb() {
    let Some(db) = chat_db() else {
        eprintln!("chat.db not available — skipping");
        return;
    };
    let deps = FakeDeps::new(Some(vec!["*".into()]));
    let out = run_single_tick(
        TickInput {
            db_path: db,
            last_rowid: 0,
            account_id: "local".into(),
        },
        &deps,
    )
    .await
    .unwrap();
    assert!(!out.skipped_unconnected);
    assert!(
        out.groups_ingested >= 1,
        "expected at least one group ingested from real chat.db, got {:?}",
        out
    );
    assert!(out.new_rowid > 0);
    let calls = deps.calls.lock();
    assert_eq!(calls.len(), out.groups_ingested);
    for (acct, key, transcript) in calls.iter() {
        assert_eq!(acct, "local");
        assert!(key.contains(':'), "key missing YMD: {}", key);
        assert!(!transcript.is_empty());
    }
}

#[tokio::test]
#[ignore]
async fn run_single_tick_keeps_cursor_on_group_failure() {
    let Some(db) = chat_db() else {
        return;
    };
    // First, sniff one key so we know what to fail.
    let probe = FakeDeps::new(Some(vec!["*".into()]));
    let _ = run_single_tick(
        TickInput {
            db_path: db.clone(),
            last_rowid: 0,
            account_id: "probe".into(),
        },
        &probe,
    )
    .await
    .unwrap();
    let Some(first_key) = probe.calls.lock().first().map(|(_, k, _)| k.clone()) else {
        eprintln!("no groups in chat.db — skipping");
        return;
    };

    let mut deps = FakeDeps::new(Some(vec!["*".into()]));
    deps.fail_keys = vec![first_key];
    let out = run_single_tick(
        TickInput {
            db_path: db,
            last_rowid: 0,
            account_id: "fail".into(),
        },
        &deps,
    )
    .await
    .unwrap();
    assert!(out.had_group_failure);
    assert_eq!(out.new_rowid, 0, "cursor must stay on failure");
}
