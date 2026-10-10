//! `openhuman-core serve` on a workspace that already has legacy approval
//! rows: with no storage URL configured (the default), the store opens the
//! existing `approval.db` through the SQLite storage driver, imports the old
//! tables once and keeps them as `_legacy_<name>`. Through the real binary
//! and its JSON-RPC surface.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use openhuman_core::config::Config;
use openhuman_core::security::approval::store;
use openhuman_core::security::approval::types::{ApprovalDecision, PendingApproval};
use serde_json::{json, Value};

const TOKEN: &str = "storage-default-import-e2e-token";

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn pending(id: &str) -> PendingApproval {
    PendingApproval::new(
        id,
        "shell",
        format!("run {id}"),
        json!({ "cmd": "ls" }),
        None,
    )
}

/// A legacy `approval.db` under `workspace`, written by the legacy tables'
/// own code (the `classic` opt-out).
fn legacy_workspace(workspace: &Path) {
    let mut config = Config {
        workspace_dir: workspace.to_path_buf(),
        ..Config::default()
    };
    config.storage.url = Some("classic".into());
    store::insert_pending(&config, &pending("still-pending"), "sess").unwrap();
    store::insert_pending(&config, &pending("already-approved"), "sess").unwrap();
    store::decide(&config, "already-approved", ApprovalDecision::ApproveOnce).unwrap();
    store::insert_flow_trust(&config, "flow-1", "shell").unwrap();
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start(home: &Path, workspace: &Path, url: Option<&str>) -> (Server, String) {
    // The port is chosen, then released for the child to bind; if another
    // process takes it in between, the child exits and a fresh port is tried.
    for _attempt in 0..3 {
        if let Some(started) = try_start(home, workspace, url) {
            return started;
        }
    }
    panic!("openhuman-core never started on a free port");
}

fn try_start(home: &Path, workspace: &Path, url: Option<&str>) -> Option<(Server, String)> {
    let port = free_port();
    // A debug build of the full product set needs more than the default 8 MiB
    // main-thread stack to finish booting, so raise the limit first.
    let mut cmd = Command::new("sh");
    cmd.args([
        "-c",
        "ulimit -s unlimited 2>/dev/null || ulimit -s 1048576 2>/dev/null; exec \"$0\" \"$@\"",
        env!("CARGO_BIN_EXE_openhuman-core"),
        "serve",
        "--port",
        &port.to_string(),
    ])
    .env("HOME", home)
    .env("USERPROFILE", home)
    .env("OPENHUMAN_WORKSPACE", workspace)
    .env("OPENHUMAN_CORE_TOKEN", TOKEN)
    .env_remove("OPENHUMAN_STORAGE_URL")
    .env_remove("OPENHUMAN_MODE")
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    if let Some(url) = url {
        cmd.env("OPENHUMAN_STORAGE_URL", url);
    }
    let mut server = Server(cmd.spawn().expect("spawn openhuman-core"));
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::new();
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if let Ok(response) = client.get(format!("{base}/health")).send() {
            if response.status().is_success() {
                return Some((server, base));
            }
        }
        if let Ok(Some(_)) = server.0.try_wait() {
            return None;
        }
        assert!(
            Instant::now() < deadline,
            "openhuman-core never became healthy"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn rpc(base: &str, method: &str, params: Value) -> Value {
    reqwest::blocking::Client::new()
        .post(format!("{base}/rpc"))
        .bearer_auth(TOKEN)
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
        .send()
        .expect("POST /rpc")
        .json()
        .expect("json-rpc response")
}

fn tables(db: &Path) -> Vec<String> {
    let conn = rusqlite::Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .unwrap();
    stmt.query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn serve_imports_legacy_approvals_by_default_and_classic_leaves_them() {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path().join("workspace");
    legacy_workspace(&workspace);
    let db = workspace.join("approval").join("approval.db");
    assert!(tables(&db).contains(&"pending_approvals".to_string()));

    // Opted out: the server reads the legacy table and changes nothing.
    {
        let (_server, base) = start(tmp.path(), &workspace, Some("classic"));
        let listed = rpc(&base, "openhuman.approval_list_pending", json!({})).to_string();
        assert!(listed.contains("still-pending"), "{listed}");
    }
    assert!(tables(&db).contains(&"pending_approvals".to_string()));
    assert!(!tables(&db).contains(&"_legacy_pending_approvals".to_string()));

    // The default: the same rows are served, now from the document tables.
    {
        let (_server, base) = start(tmp.path(), &workspace, None);
        let listed = rpc(&base, "openhuman.approval_list_pending", json!({})).to_string();
        assert!(listed.contains("still-pending"), "{listed}");
        assert!(!listed.contains("already-approved"), "{listed}");
        let recent = rpc(&base, "openhuman.approval_list_recent_decisions", json!({})).to_string();
        assert!(recent.contains("already-approved"), "{recent}");
    }
    let after = tables(&db);
    assert!(after.contains(&"_legacy_pending_approvals".to_string()));
    assert!(after.contains(&"_legacy_flow_tool_trust".to_string()));
    assert!(!after.contains(&"pending_approvals".to_string()));

    // A second boot serves the same rows and does not import again.
    {
        let (_server, base) = start(tmp.path(), &workspace, None);
        let listed = rpc(&base, "openhuman.approval_list_pending", json!({})).to_string();
        assert!(listed.contains("still-pending"), "{listed}");
    }
    assert!(!tables(&db).contains(&"pending_approvals".to_string()));
    let conn = rusqlite::Connection::open(&db).unwrap();
    let kept: i64 = conn
        .query_row("SELECT COUNT(*) FROM _legacy_pending_approvals", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(kept, 2, "the retired table is kept as it was");
}
