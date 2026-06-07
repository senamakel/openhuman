//! Skill registry E2E: exercises browse, search, sources, and install
//! JSON-RPC endpoints against a real core router.
//!
//! Run: `cargo test --test skill_registry_e2e`
//! Or:  `pnpm test:rust:e2e -- --suite skill_registry_e2e`
//!
//! NOTE: This test hits real external URLs (GitHub raw content, GitHub API,
//! ClawHub API). Network access is required. ClawHub-specific assertions are
//! guarded so the test still passes if ClawHub is slow or unavailable.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use axum::http::header::AUTHORIZATION;
use serde_json::{json, Value};
use tempfile::tempdir;

use openhuman_core::core::auth::{init_rpc_token, CORE_TOKEN_ENV_VAR};
use openhuman_core::core::jsonrpc::build_core_http_router;

// ── Constants ──────────────────────────────────────────────────────────────

const TEST_RPC_TOKEN: &str = "skill-registry-e2e-token";

// ── One-time auth init ─────────────────────────────────────────────────────

static SKILL_REGISTRY_AUTH_INIT: OnceLock<()> = OnceLock::new();

fn ensure_test_rpc_auth() {
    SKILL_REGISTRY_AUTH_INIT.get_or_init(|| {
        // SAFETY: runs exactly once inside OnceLock before any concurrent env
        // reads occur. Required by Rust 1.81+ for set_var in multi-threaded
        // contexts.
        unsafe { std::env::set_var(CORE_TOKEN_ENV_VAR, TEST_RPC_TOKEN) };
        let token_dir =
            std::env::temp_dir().join("openhuman-skill-registry-e2e-auth");
        init_rpc_token(&token_dir).expect("init rpc auth token for skill_registry_e2e");
    });
}

// ── Env lock (process-global env vars must not race) ──────────────────────

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static KEYRING_INIT: OnceLock<()> = OnceLock::new();

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    KEYRING_INIT.get_or_init(|| unsafe {
        std::env::set_var("OPENHUMAN_KEYRING_BACKEND", "file");
    });
    let mutex = ENV_LOCK.get_or_init(|| Mutex::new(()));
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

// ── EnvVarGuard ───────────────────────────────────────────────────────────

struct EnvVarGuard {
    key: &'static str,
    old: Option<String>,
}

impl EnvVarGuard {
    fn set_to_path(key: &'static str, path: &Path) -> Self {
        let old = std::env::var(key).ok();
        unsafe { std::env::set_var(key, path.as_os_str()) };
        Self { key, old }
    }

    fn set(key: &'static str, value: &str) -> Self {
        let old = std::env::var(key).ok();
        unsafe { std::env::set_var(key, value) };
        Self { key, old }
    }

    fn unset(key: &'static str) -> Self {
        let old = std::env::var(key).ok();
        unsafe { std::env::remove_var(key) };
        Self { key, old }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(v) => unsafe { std::env::set_var(self.key, v) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

// ── Server helpers ─────────────────────────────────────────────────────────

async fn serve_on_ephemeral(
    app: axum::Router,
) -> (
    SocketAddr,
    tokio::task::JoinHandle<Result<(), std::io::Error>>,
) {
    ensure_test_rpc_auth();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move { axum::serve(listener, app).await });
    (addr, handle)
}

// ── JSON-RPC helpers ───────────────────────────────────────────────────────

async fn post_json_rpc(rpc_base: &str, id: i64, method: &str, params: Value) -> Value {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("build reqwest client");
    let body = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    let url = format!("{}/rpc", rpc_base.trim_end_matches('/'));
    let resp = client
        .post(&url)
        .header(AUTHORIZATION, format!("Bearer {TEST_RPC_TOKEN}"))
        .json(&body)
        .send()
        .await
        .unwrap_or_else(|e| panic!("POST {url}: {e}"));
    assert!(
        resp.status().is_success(),
        "HTTP error {} for {method}",
        resp.status()
    );
    resp.json::<Value>()
        .await
        .unwrap_or_else(|e| panic!("parse json for {method}: {e}"))
}

/// Assert the response has no `error` field and return the `result` value.
fn assert_no_jsonrpc_error<'a>(v: &'a Value, context: &str) -> &'a Value {
    if let Some(err) = v.get("error") {
        panic!("{context}: JSON-RPC error: {err}");
    }
    v.get("result")
        .unwrap_or_else(|| panic!("{context}: missing `result` field: {v}"))
}

/// Assert the response has an `error` field and return it.
fn assert_jsonrpc_error<'a>(v: &'a Value, context: &str) -> &'a Value {
    v.get("error")
        .unwrap_or_else(|| panic!("{context}: expected JSON-RPC error, got: {v}"))
}

// ── Test ───────────────────────────────────────────────────────────────────

/// End-to-end coverage for the `openhuman.skill_registry_*` endpoints.
///
/// Steps executed in sequence (shared catalog state):
/// 1. `sources`  — lists the three default sources.
/// 2. `browse`   — fetches the live catalog (force_refresh = true).
/// 3. `search`   — queries for "git" and expects at least one match.
/// 4. `install`  — happy-path install of a community skill.
/// 5. `install`  — duplicate-rejection: same install must return an error.
/// 6. `install`  — ClawHub rejection: clawhub:// entries must be rejected with CLI hint.
#[tokio::test]
async fn skill_registry_e2e_sources_browse_search_install() {
    let _env_lock = env_lock();

    // Create an isolated temp HOME so installed skills don't pollute the real
    // system and each test run starts with a clean slate.
    let tmp = tempdir().expect("create tempdir");
    let home = tmp.path();
    let openhuman_home = home.join(".openhuman");

    // Redirect HOME so `dirs::home_dir()` (used by the install path) resolves
    // to our temp directory.
    let _home_guard = EnvVarGuard::set_to_path("HOME", home);
    // Unset OPENHUMAN_WORKSPACE so the core uses the default `~/.openhuman` path
    // derived from our temp HOME.
    let _workspace_guard = EnvVarGuard::unset("OPENHUMAN_WORKSPACE");
    // Expose the test token via the env-var path that the core auth layer reads.
    let _token_guard = EnvVarGuard::set(CORE_TOKEN_ENV_VAR, TEST_RPC_TOKEN);
    // Use file-based keyring so tests don't touch the system keychain.
    let _keyring_guard = EnvVarGuard::set("OPENHUMAN_KEYRING_BACKEND", "file");

    // Seed a minimal config.toml so `load_config` doesn't fail on missing file.
    // Point api_url at a non-existent port — skill-registry endpoints don't
    // hit the backend API.
    let cfg_dir = openhuman_home.clone();
    std::fs::create_dir_all(&cfg_dir).expect("create .openhuman dir");
    std::fs::write(
        cfg_dir.join("config.toml"),
        r#"api_url = "http://127.0.0.1:9"
default_model = "skill-e2e-model"

[secrets]
encrypt = false
"#,
    )
    .expect("write config.toml");

    // Also write the pre-login user-scoped config.
    let user_cfg_dir = openhuman_home.join("users").join("local");
    std::fs::create_dir_all(&user_cfg_dir).expect("create users/local dir");
    std::fs::write(
        user_cfg_dir.join("config.toml"),
        r#"api_url = "http://127.0.0.1:9"
default_model = "skill-e2e-model"

[secrets]
encrypt = false
"#,
    )
    .expect("write users/local/config.toml");

    let (rpc_addr, rpc_join) = serve_on_ephemeral(build_core_http_router(false)).await;
    let rpc_base = format!("http://{rpc_addr}");

    // ── Step 1: sources ────────────────────────────────────────────────────

    let sources_resp =
        post_json_rpc(&rpc_base, 9001, "openhuman.skill_registry_sources", json!({})).await;
    let sources_result = assert_no_jsonrpc_error(&sources_resp, "skill_registry_sources");

    let sources = sources_result
        .get("sources")
        .and_then(Value::as_array)
        .expect("sources result must contain a `sources` array");

    assert!(
        sources.len() >= 3,
        "expected at least 3 default sources (openhuman-community, hermeshub, clawhub), got {}",
        sources.len()
    );

    let required_source_ids = ["openhuman-community", "hermeshub", "clawhub"];
    for expected_id in &required_source_ids {
        assert!(
            sources
                .iter()
                .any(|s| s.get("id").and_then(Value::as_str) == Some(expected_id)),
            "expected source '{expected_id}' in sources list"
        );
    }

    let required_source_fields = ["id", "name", "url", "kind", "enabled"];
    for source in sources {
        for field in &required_source_fields {
            assert!(
                source.get(field).is_some(),
                "source entry missing field '{field}': {source}"
            );
        }
    }

    // ── Step 2: browse (force_refresh = true) ─────────────────────────────

    let browse_resp = post_json_rpc(
        &rpc_base,
        9002,
        "openhuman.skill_registry_browse",
        json!({ "force_refresh": true }),
    )
    .await;
    let browse_result = assert_no_jsonrpc_error(&browse_resp, "skill_registry_browse");

    let entries = browse_result
        .get("entries")
        .and_then(Value::as_array)
        .expect("browse result must contain an `entries` array");

    assert!(
        !entries.is_empty(),
        "browse catalog must return at least one entry after force_refresh"
    );

    // At least one entry from the openhuman-community source must be present.
    let community_entries: Vec<&Value> = entries
        .iter()
        .filter(|e| {
            e.get("source_id").and_then(Value::as_str) == Some("openhuman-community")
        })
        .collect();
    assert!(
        !community_entries.is_empty(),
        "browse must include at least one entry with source_id == 'openhuman-community'"
    );

    // Every entry must carry the required fields.
    let required_entry_fields = [
        "id",
        "name",
        "description",
        "download_url",
        "source_id",
        "format",
    ];
    for entry in entries {
        for field in &required_entry_fields {
            assert!(
                entry.get(field).is_some(),
                "catalog entry missing field '{field}': {entry}"
            );
        }
    }

    // ── Step 3: search ────────────────────────────────────────────────────

    let search_resp = post_json_rpc(
        &rpc_base,
        9003,
        "openhuman.skill_registry_search",
        json!({ "query": "git" }),
    )
    .await;
    let search_result = assert_no_jsonrpc_error(&search_resp, "skill_registry_search (git)");

    let search_entries = search_result
        .get("entries")
        .and_then(Value::as_array)
        .expect("search result must contain an `entries` array");

    assert!(
        !search_entries.is_empty(),
        "search for 'git' must return at least one match"
    );

    // All returned entries must actually match the query in name, description, or tags.
    for entry in search_entries {
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_lowercase();
        let desc = entry
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_lowercase();
        let tags: Vec<String> = entry
            .get("tags")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| t.as_str())
                    .map(str::to_lowercase)
                    .collect()
            })
            .unwrap_or_default();

        let matches = name.contains("git")
            || desc.contains("git")
            || tags.iter().any(|t| t.contains("git"));
        assert!(
            matches,
            "search result entry does not match query 'git': {entry}"
        );
    }

    // ── Step 4: install (happy path) ──────────────────────────────────────

    // Identify a community skill to install. We look for the entry with id
    // "git-summary" that should be in the openhuman-community source. If that
    // exact slug is absent (registry may rename it), we fall back to the first
    // community entry that uses a plain HTTPS download_url (not clawhub://).
    let re_browse_resp = post_json_rpc(
        &rpc_base,
        9004,
        "openhuman.skill_registry_browse",
        json!({ "force_refresh": false }),
    )
    .await;
    let re_browse_result = assert_no_jsonrpc_error(&re_browse_resp, "skill_registry_browse (cached)");
    let all_entries = re_browse_result
        .get("entries")
        .and_then(Value::as_array)
        .expect("re-browse entries array");

    // Prefer git-summary; fall back to the first installable community entry.
    let install_target = all_entries
        .iter()
        .find(|e| {
            e.get("id").and_then(Value::as_str) == Some("git-summary")
                && e.get("source_id").and_then(Value::as_str) == Some("openhuman-community")
        })
        .or_else(|| {
            all_entries.iter().find(|e| {
                e.get("source_id").and_then(Value::as_str) == Some("openhuman-community")
                    && e.get("download_url")
                        .and_then(Value::as_str)
                        .map(|u| u.starts_with("https://"))
                        .unwrap_or(false)
            })
        })
        .expect("expected at least one installable openhuman-community entry");

    let entry_id = install_target
        .get("id")
        .and_then(Value::as_str)
        .expect("install_target id");
    let source_id = install_target
        .get("source_id")
        .and_then(Value::as_str)
        .expect("install_target source_id");

    let install_resp = post_json_rpc(
        &rpc_base,
        9005,
        "openhuman.skill_registry_install",
        json!({ "entry_id": entry_id, "source_id": source_id }),
    )
    .await;
    let install_result = assert_no_jsonrpc_error(&install_resp, "skill_registry_install (happy)");

    // Verify response fields.
    let install_url = install_result
        .get("url")
        .and_then(Value::as_str)
        .expect("install result must contain `url`");
    assert!(
        !install_url.is_empty(),
        "install result `url` must not be empty"
    );

    let install_stdout = install_result
        .get("stdout")
        .and_then(Value::as_str)
        .expect("install result must contain `stdout`");
    assert!(
        install_stdout.contains("Installed to"),
        "install stdout should mention 'Installed to', got: {install_stdout}"
    );

    let _install_stderr = install_result
        .get("stderr")
        .expect("install result must contain `stderr`");

    let new_skills = install_result
        .get("new_skills")
        .and_then(Value::as_array)
        .expect("install result must contain `new_skills` array");
    assert!(
        new_skills
            .iter()
            .any(|s| s.as_str() == Some(entry_id)),
        "new_skills must contain '{entry_id}', got: {new_skills:?}"
    );

    // Verify the SKILL.md file actually landed on disk.
    let skill_file = home
        .join(".openhuman")
        .join("skills")
        .join(entry_id)
        .join("SKILL.md");
    assert!(
        skill_file.exists(),
        "SKILL.md should exist on disk at {}, but was not found",
        skill_file.display()
    );

    // ── Step 5: install (duplicate rejection) ─────────────────────────────

    let dup_resp = post_json_rpc(
        &rpc_base,
        9006,
        "openhuman.skill_registry_install",
        json!({ "entry_id": entry_id, "source_id": source_id }),
    )
    .await;
    let dup_error = assert_jsonrpc_error(&dup_resp, "skill_registry_install (duplicate)");
    let dup_message = dup_error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        dup_message.contains("already installed"),
        "duplicate install error should mention 'already installed', got: {dup_message}"
    );

    // ── Step 6: install (ClawHub rejection) ───────────────────────────────

    // Find a clawhub entry if the API was reachable during the browse. If no
    // clawhub entries came back (API down or rate-limited), we skip this step
    // rather than fail the whole test.
    let clawhub_entry = all_entries.iter().find(|e| {
        e.get("source_id").and_then(Value::as_str) == Some("clawhub")
            && e.get("download_url")
                .and_then(Value::as_str)
                .map(|u| u.starts_with("clawhub://"))
                .unwrap_or(false)
    });

    if let Some(claw_entry) = clawhub_entry {
        let claw_id = claw_entry
            .get("id")
            .and_then(Value::as_str)
            .expect("clawhub entry id");

        let claw_resp = post_json_rpc(
            &rpc_base,
            9007,
            "openhuman.skill_registry_install",
            json!({ "entry_id": claw_id, "source_id": "clawhub" }),
        )
        .await;
        let claw_error = assert_jsonrpc_error(&claw_resp, "skill_registry_install (clawhub)");
        let claw_message = claw_error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(
            claw_message.contains("OpenClaw CLI") || claw_message.contains("cannot be installed"),
            "clawhub install error should mention 'OpenClaw CLI' or 'cannot be installed', got: {claw_message}"
        );
    } else {
        // ClawHub was unreachable or returned no entries — log a notice and
        // skip the assertion so CI does not fail due to external availability.
        eprintln!(
            "[skill_registry_e2e] NOTE: no clawhub entries returned by browse; \
             skipping ClawHub install-rejection assertion (external API may be unavailable)"
        );
    }

    // ── Cleanup ───────────────────────────────────────────────────────────

    rpc_join.abort();
}
