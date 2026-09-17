//! Live smoke: are the skills and MCP servers the app offers really downloadable?
//!
//! The hermetic suites (`skill_registry_e2e`, `mcp_registry_e2e`, and the
//! registry-to-agent cases in `agent_harness_e2e`) prove the plumbing against
//! loopback fixtures. They cannot tell whether the *real* catalogs hand out
//! entries that install and run. These tests hit the real Hermes skill catalog,
//! the SKILL.md hosts it points at, the official MCP registry and npm.
//!
//! Never run in CI (`#[ignore]`: network, third-party uptime, `npx` on PATH).
//! Run by hand, one test at a time, and read the printed report:
//!
//! ```text
//! RUST_MIN_STACK=67108864 cargo test -p openhuman --test skills_mcp_registry_live \
//!   -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `OPENHUMAN_LIVE_MCP_QUERY` picks the MCP registry search (default `everything`).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use axum::http::header::AUTHORIZATION;
use serde_json::{json, Value};
use tempfile::tempdir;

use openhuman_core::core::auth::{init_rpc_token, CORE_TOKEN_ENV_VAR};
use openhuman_core::core::jsonrpc::build_core_http_router;

const TEST_RPC_TOKEN: &str = "skills-mcp-live-token";

static AUTH_INIT: OnceLock<()> = OnceLock::new();
static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    let mutex = ENV_LOCK.get_or_init(|| Mutex::new(()));
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct EnvVarGuard {
    key: &'static str,
    old: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let old = std::env::var(key).ok();
        unsafe { std::env::set_var(key, value) };
        Self { key, old }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

/// A core RPC stack over a throwaway `HOME`, with no registry overrides.
struct LiveStack {
    rpc_base: String,
    home: tempfile::TempDir,
    _guards: Vec<EnvVarGuard>,
    rpc_join: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}

impl Drop for LiveStack {
    fn drop(&mut self) {
        self.rpc_join.abort();
    }
}

async fn live_stack() -> LiveStack {
    let home = tempdir().expect("tempdir");
    let guards = vec![
        EnvVarGuard::set("HOME", &home.path().to_string_lossy()),
        EnvVarGuard::set(CORE_TOKEN_ENV_VAR, TEST_RPC_TOKEN),
        EnvVarGuard::set("OPENHUMAN_KEYRING_BACKEND", "file"),
    ];
    AUTH_INIT.get_or_init(|| {
        init_rpc_token(&std::env::temp_dir().join("openhuman-skills-mcp-live-auth"))
            .expect("init rpc auth token");
    });

    let config = "api_url = \"http://127.0.0.1:9\"\ndefault_model = \"live-smoke\"\n\n[secrets]\nencrypt = false\n";
    for dir in [
        home.path().join(".openhuman"),
        home.path().join(".openhuman/users/local"),
    ] {
        std::fs::create_dir_all(&dir).expect("create config dir");
        std::fs::write(dir.join("config.toml"), config).expect("write config.toml");
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind rpc");
    let addr: SocketAddr = listener.local_addr().expect("rpc addr");
    let rpc_join =
        tokio::spawn(async move { axum::serve(listener, build_core_http_router(false)).await });
    LiveStack {
        rpc_base: format!("http://{addr}"),
        home,
        _guards: guards,
        rpc_join,
    }
}

/// One JSON-RPC call. Returns the `result` (log envelope peeled) or the error text.
async fn rpc(stack: &LiveStack, method: &str, params: Value) -> Result<Value, String> {
    let response = reqwest::Client::builder()
        // `mcp_clients_connect` may be downloading an npm package through npx.
        .timeout(Duration::from_secs(300))
        .build()
        .expect("reqwest client")
        .post(format!("{}/rpc", stack.rpc_base))
        .header(AUTHORIZATION, format!("Bearer {TEST_RPC_TOKEN}"))
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
        .send()
        .await
        .map_err(|e| format!("{method}: transport: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("{method}: body: {e}"))?;
    if let Some(error) = response.get("error") {
        return Err(format!("{method}: {error}"));
    }
    let result = response.get("result").cloned().unwrap_or(Value::Null);
    Ok(match result.get("logs") {
        Some(_) => result.get("result").cloned().unwrap_or(result),
        None => result,
    })
}

fn installed_skill_count(home: &Path) -> usize {
    std::fs::read_dir(home.join(".openhuman/skills"))
        .map(|dirs| {
            dirs.filter_map(Result::ok)
                .filter(|dir| dir.path().join("SKILL.md").is_file())
                .count()
        })
        .unwrap_or(0)
}

/// Every source the catalog lists must offer at least one skill that actually
/// installs, and no source may list skills that can never be installed.
///
/// For each source this installs one entry that has a download URL (and an id
/// unique in the catalog, since install resolves the first entry with that id)
/// and checks a SKILL.md landed on disk. A source whose entries all lack a
/// download URL is reported as a failure: the app shows those entries with an
/// Install button that can only fail.
#[tokio::test]
#[ignore = "live: real skill catalog + SKILL.md hosts"]
async fn live_every_skill_catalog_source_offers_installable_skills() {
    let _lock = env_lock();
    let stack = live_stack().await;

    let browse = rpc(
        &stack,
        "openhuman.skill_registry_browse",
        json!({ "force_refresh": true }),
    )
    .await
    .expect("browse the live catalog");
    let entries = browse
        .get("entries")
        .and_then(Value::as_array)
        .expect("browse returns entries");
    assert!(!entries.is_empty(), "the live catalog returned no entries");

    let str_of = |entry: &Value, key: &str| {
        entry
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let mut id_counts: BTreeMap<String, usize> = BTreeMap::new();
    for entry in entries {
        *id_counts.entry(str_of(entry, "id")).or_default() += 1;
    }

    // source -> (listed, downloadable, a downloadable entry with a unique id)
    let mut by_source: BTreeMap<String, (usize, usize, Option<String>)> = BTreeMap::new();
    for entry in entries {
        let slot = by_source.entry(str_of(entry, "source")).or_default();
        slot.0 += 1;
        if !str_of(entry, "download_url").trim().is_empty() {
            slot.1 += 1;
            let id = str_of(entry, "id");
            if slot.2.is_none() && id_counts.get(&id) == Some(&1) {
                slot.2 = Some(id);
            }
        }
    }

    let mut failures = Vec::new();
    eprintln!("\nsource                 listed  downloadable  sample install");
    for (source, (listed, downloadable, sample)) in &by_source {
        let outcome = match sample {
            None if *downloadable == 0 => "NONE DOWNLOADABLE".to_string(),
            None => "no unique-id sample".to_string(),
            Some(id) => {
                let before = installed_skill_count(stack.home.path());
                match rpc(
                    &stack,
                    "openhuman.skill_registry_install",
                    json!({ "entry_id": id }),
                )
                .await
                {
                    Ok(_) if installed_skill_count(stack.home.path()) > before => {
                        format!("ok ({id})")
                    }
                    Ok(result) => format!("NO SKILL.md WRITTEN ({id}): {result}"),
                    Err(error) => format!("FAILED ({id}): {error}"),
                }
            }
        };
        eprintln!("{source:<22} {listed:>6}  {downloadable:>12}  {outcome}");
        if !outcome.starts_with("ok") {
            failures.push(format!("{source}: {outcome}"));
        }
        if downloadable < listed {
            failures.push(format!(
                "{source}: {} of {listed} listed entries have no download URL",
                listed - downloadable
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "skill catalog sources offer entries that cannot be installed:\n  {}",
        failures.join("\n  ")
    );
}

/// A server found in the official MCP registry installs, connects, lists tools,
/// and answers a tool call.
///
/// Walks the search results for `OPENHUMAN_LIVE_MCP_QUERY` and takes the first
/// server that connects without credentials, then calls one of its tools that
/// needs no arguments. Requires `npx` (or `uvx`) on PATH for stdio servers.
#[cfg(feature = "mcp")]
#[tokio::test]
#[ignore = "live: official MCP registry + npm/pypi packages"]
async fn live_official_mcp_registry_server_installs_connects_and_answers_a_tool_call() {
    let _lock = env_lock();
    let stack = live_stack().await;
    let query = std::env::var("OPENHUMAN_LIVE_MCP_QUERY").unwrap_or_else(|_| "everything".into());

    let search = rpc(
        &stack,
        "openhuman.mcp_clients_registry_search",
        json!({ "query": query, "page": 1, "page_size": 10 }),
    )
    .await
    .expect("search the official MCP registry");
    let servers = search
        .get("servers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(
        !servers.is_empty(),
        "the official registry returned no servers for {query:?}: {search}"
    );

    let mut attempts = Vec::new();
    for server in servers.iter().take(5) {
        let Some(name) = ["qualified_name", "qualifiedName", "name"]
            .iter()
            .find_map(|key| server.get(*key).and_then(Value::as_str))
        else {
            attempts.push(format!("unnamed search row: {server}"));
            continue;
        };

        let server_id = match rpc(
            &stack,
            "openhuman.mcp_clients_install",
            json!({ "qualified_name": name, "env": {} }),
        )
        .await
        {
            Ok(installed) => match installed
                .pointer("/server/server_id")
                .and_then(Value::as_str)
            {
                Some(id) => id.to_string(),
                None => {
                    attempts.push(format!(
                        "{name}: install returned no server_id: {installed}"
                    ));
                    continue;
                }
            },
            Err(error) => {
                attempts.push(format!("{name}: install failed: {error}"));
                continue;
            }
        };

        let connected = match rpc(
            &stack,
            "openhuman.mcp_clients_connect",
            json!({ "server_id": server_id }),
        )
        .await
        {
            Ok(connected) => connected,
            Err(error) => {
                attempts.push(format!("{name}: connect failed: {error}"));
                continue;
            }
        };
        let tools = connected
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if tools.is_empty() {
            attempts.push(format!("{name}: connected but listed no tools"));
            continue;
        }

        let no_arg_tool = tools.iter().find_map(|tool| {
            let schema = tool.get("input_schema").or_else(|| tool.get("inputSchema"));
            let required = schema
                .and_then(|s| s.get("required"))
                .and_then(Value::as_array)
                .is_some_and(|r| !r.is_empty());
            (!required).then(|| tool.get("name").and_then(Value::as_str))?
        });
        let Some(tool_name) = no_arg_tool else {
            attempts.push(format!(
                "{name}: {} tools, none callable without arguments",
                tools.len()
            ));
            continue;
        };

        match rpc(
            &stack,
            "openhuman.mcp_clients_tool_call",
            json!({ "server_id": server_id, "tool_name": tool_name, "arguments": {} }),
        )
        .await
        {
            Ok(called) if called.get("is_error").and_then(Value::as_bool) == Some(false) => {
                eprintln!(
                    "\n{name}: installed, connected, {} tools, `{tool_name}` answered",
                    tools.len()
                );
                return;
            }
            Ok(called) => {
                attempts.push(format!("{name}: `{tool_name}` returned an error: {called}"))
            }
            Err(error) => attempts.push(format!("{name}: `{tool_name}` call failed: {error}")),
        }
    }

    panic!(
        "no server from the official registry search {query:?} installed, connected and answered a tool call:\n  {}",
        attempts.join("\n  ")
    );
}
