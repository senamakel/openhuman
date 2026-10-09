//! `openhuman-core run --mode saas` end to end, through the real binary.
//!
//! A SaaS core must refuse an unsafe deployment before it binds anything, and
//! a safe one must serve nothing but its core built-ins behind the gateway
//! bearer until per-user isolation opens domain families.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const BEARER: &str = "saas-e2e-gateway-bearer-0123456789abcdef";

/// Environment a developer machine may carry that would point the child at a
/// single user, or that the boot guard refuses outright.
const SCRUBBED_ENV: &[&str] = &[
    "OPENHUMAN_WORKSPACE",
    "OPENHUMAN_DEV_CONNECT",
    "OPENHUMAN_BACKEND_SESSION_TOKEN",
    "OPENHUMAN_BACKEND_API_KEY",
    "OPENHUMAN_CORE_TOKEN",
    "OPENHUMAN_APPROVAL_GATE",
    "OPENHUMAN_SANDBOX",
    "OPENHUMAN_MODE",
];

struct Deployment {
    tmp: tempfile::TempDir,
    root: PathBuf,
    config: PathBuf,
}

fn deployment(write_token: bool) -> Deployment {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path().join("saas");
    std::fs::create_dir(&root).unwrap();
    set_mode(&root, 0o700);
    if write_token {
        let token = root.join("service.token");
        std::fs::write(&token, format!("{BEARER}\n")).unwrap();
        set_mode(&token, 0o600);
    }
    let config = tmp.path().join("operator.toml");
    std::fs::write(
        &config,
        format!("root = {:?}\n", root.display().to_string()),
    )
    .unwrap();
    Deployment { tmp, root, config }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(not(unix))]
fn set_mode(_: &Path, _: u32) {}

fn core_command(d: &Deployment, extra: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_openhuman-core"));
    cmd.args(["run", "--mode", "saas", "--saas-config"])
        .arg(&d.config)
        .args(extra)
        // Keep the child away from the developer's real `~/.openhuman`.
        .env("HOME", d.tmp.path())
        .env("USERPROFILE", d.tmp.path());
    for var in SCRUBBED_ENV {
        cmd.env_remove(var);
    }
    cmd
}

#[test]
fn an_unsafe_deployment_is_refused_before_it_binds() {
    let d = deployment(false);
    let output = core_command(&d, &[])
        .env("OPENHUMAN_WORKSPACE", d.tmp.path())
        .output()
        .expect("run openhuman-core");
    assert!(!output.status.success(), "an unsafe SaaS boot must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("refusing to boot"), "{stderr}");
    assert!(stderr.contains("service token"), "{stderr}");
    assert!(stderr.contains("OPENHUMAN_WORKSPACE"), "{stderr}");
    assert!(
        !d.root.join("operator").exists(),
        "a refused boot must not create state"
    );
}

#[test]
fn saas_mode_without_an_operator_config_is_refused() {
    let output = Command::new(env!("CARGO_BIN_EXE_openhuman-core"))
        .args(["run", "--mode", "saas"])
        .env_remove("OPENHUMAN_MODE")
        .output()
        .expect("run openhuman-core");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--saas-config"), "{stderr}");
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn rpc(
    client: &reqwest::blocking::Client,
    base: &str,
    bearer: Option<&str>,
    method: &str,
) -> (u16, Value) {
    rpc_with(client, base, bearer, method, json!({}))
}

fn rpc_with(
    client: &reqwest::blocking::Client,
    base: &str,
    bearer: Option<&str>,
    method: &str,
    params: Value,
) -> (u16, Value) {
    let mut request = client.post(format!("{base}/rpc")).json(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params
    }));
    if let Some(bearer) = bearer {
        request = request.bearer_auth(bearer);
    }
    let response = request.send().expect("POST /rpc");
    let status = response.status().as_u16();
    (status, response.json().unwrap_or(Value::Null))
}

/// Start a SaaS core on deployment `d` and wait until it is healthy.
fn start(d: &Deployment) -> (Server, String, reqwest::blocking::Client) {
    let port = free_port();
    let child = core_command(d, &["--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn openhuman-core");
    let mut server = Server(child);
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if let Ok(response) = client.get(format!("{base}/health")).send() {
            if response.status().is_success() {
                break;
            }
        }
        if let Ok(Some(status)) = server.0.try_wait() {
            panic!("SaaS core exited before serving: {status}");
        }
        assert!(Instant::now() < deadline, "SaaS core never became healthy");
        std::thread::sleep(Duration::from_millis(250));
    }
    (server, base, client)
}

#[test]
fn a_safe_deployment_serves_core_and_the_operator_plane_behind_the_gateway_bearer() {
    let d = deployment(true);
    let (server, base, client) = start(&d);

    let (status, _) = rpc(&client, &base, None, "core.ping");
    assert_eq!(status, 401, "no bearer, no access");
    let (status, _) = rpc(&client, &base, Some("wrong-bearer"), "core.ping");
    assert_eq!(status, 401, "only the gateway bearer is accepted");

    let (status, body) = rpc(&client, &base, Some(BEARER), "core.ping");
    assert_eq!(status, 200);
    assert!(body.get("result").is_some(), "core.ping answers: {body}");

    // No domain family is isolated per user yet, so none is served.
    for method in [
        "openhuman.threads_list",
        "openhuman.config_get_config",
        "openhuman.memory_search",
    ] {
        let (_, body) = rpc(&client, &base, Some(BEARER), method);
        assert!(
            body.get("error").is_some(),
            "{method} must not be served: {body}"
        );
    }

    // The operator plane provisions one agent per user, keyed by a hash of
    // the gateway's user id, which is never echoed back.
    let (_, body) = rpc_with(
        &client,
        &base,
        Some(BEARER),
        "openhuman.user_agents_provision",
        json!({ "user_id": "alice@example.com" }),
    );
    let result = body
        .get("result")
        .unwrap_or_else(|| panic!("provision: {body}"));
    let agent_id = result
        .pointer("/result/agent_id")
        .or_else(|| result.get("agent_id"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("agent_id in {result}"))
        .to_string();
    assert_eq!(
        agent_id,
        openhuman_core::user_agents::UserAgentId::for_user("alice@example.com")
            .unwrap()
            .to_string(),
        "the agent id is the deterministic hash of the user id"
    );
    assert!(!body.to_string().contains("alice"), "{body}");
    assert!(d
        .root
        .join("agents")
        .join(&agent_id)
        .join("workspace")
        .is_dir());

    let (_, body) = rpc(&client, &base, Some(BEARER), "openhuman.user_agents_list");
    assert!(body.to_string().contains(&agent_id), "{body}");
    let (_, body) = rpc_with(
        &client,
        &base,
        Some(BEARER),
        "openhuman.user_agents_status",
        json!({ "agent_id": agent_id }),
    );
    assert!(body.get("result").is_some(), "{body}");
    let (_, body) = rpc_with(
        &client,
        &base,
        Some(BEARER),
        "openhuman.user_agents_deprovision",
        json!({ "agent_id": agent_id }),
    );
    assert!(body.get("result").is_some(), "{body}");
    assert!(!d.root.join("agents").join(&agent_id).exists());
    assert!(
        d.root.join("deprovisioned").is_dir(),
        "archived, not deleted"
    );

    assert!(
        d.root.join("operator").join("workspace").is_dir(),
        "the operator plane lives under the SaaS root"
    );
    assert!(
        !d.tmp
            .path()
            .join(".openhuman")
            .join("active_user.toml")
            .exists(),
        "a SaaS boot never activates a desktop user"
    );
    let desktop = d.tmp.path().join(".openhuman");
    let leaked: Vec<_> = std::fs::read_dir(&desktop)
        .map(|entries| entries.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(
        leaked.is_empty(),
        "a SaaS boot writes nothing under ~/.openhuman (keyring included): {leaked:?}"
    );
    drop(server);
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// POST /rpc for gateway user `user`, signed unless `sig` overrides it.
fn user_rpc(
    client: &reqwest::blocking::Client,
    base: &str,
    bearer: &str,
    user: &str,
    sig: Option<&str>,
    method: &str,
) -> (u16, Value) {
    user_rpc_with(client, base, bearer, user, sig, method, json!({}))
}

fn user_rpc_with(
    client: &reqwest::blocking::Client,
    base: &str,
    bearer: &str,
    user: &str,
    sig: Option<&str>,
    method: &str,
    params: Value,
) -> (u16, Value) {
    use openhuman_core::user_agents::gateway::{sign, USER_HEADER, USER_SIG_HEADER};
    let signature = sig
        .map(str::to_owned)
        .unwrap_or_else(|| sign(BEARER, user, now()));
    let response = client
        .post(format!("{base}/rpc"))
        .bearer_auth(bearer)
        .header(USER_HEADER, user)
        .header(USER_SIG_HEADER, signature)
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
        .send()
        .expect("POST /rpc");
    let status = response.status().as_u16();
    (status, response.json().unwrap_or(Value::Null))
}

#[test]
fn gateway_requests_run_under_the_named_users_agent() {
    let d = deployment(true);
    let (server, base, client) = start(&d);

    // Provision alice and hand the core her credential; bob stays unknown.
    let (_, body) = rpc_with(
        &client,
        &base,
        Some(BEARER),
        "openhuman.user_agents_provision",
        json!({ "user_id": "alice" }),
    );
    let alice = openhuman_core::user_agents::UserAgentId::for_user("alice").unwrap();
    assert!(body.to_string().contains(alice.as_str()), "{body}");
    let (_, body) = rpc_with(
        &client,
        &base,
        Some(BEARER),
        "openhuman.user_agents_set_credential",
        json!({ "agent_id": alice.as_str(), "kind": "session", "token": "alice-session-jwt" }),
    );
    assert!(body.get("result").is_some(), "{body}");
    assert!(!body.to_string().contains("alice-session-jwt"), "{body}");
    let (_, body) = rpc_with(
        &client,
        &base,
        Some(BEARER),
        "openhuman.user_agents_status",
        json!({ "agent_id": alice.as_str() }),
    );
    assert!(
        body.to_string().contains("\"has_credential\":true"),
        "{body}"
    );

    // A signed request for alice runs under her agent.
    let (status, body) = user_rpc(&client, &base, BEARER, "alice", None, "core.ping");
    assert_eq!(status, 200, "{body}");
    assert!(body.get("result").is_some(), "{body}");

    // A user's scope cannot reach the operator plane.
    let (_, body) = user_rpc(
        &client,
        &base,
        BEARER,
        "alice",
        None,
        "openhuman.user_agents_list",
    );
    assert!(
        body.get("error").is_some(),
        "operator methods are not a user's: {body}"
    );

    // Refusals: bad bearer first, then signatures, then provisioning.
    let (status, body) = user_rpc(&client, &base, "wrong-bearer", "alice", None, "core.ping");
    assert_eq!(status, 401, "{body}");
    let (status, body) = user_rpc(&client, &base, "wrong-bearer", "bob", None, "core.ping");
    assert_eq!(
        status, 401,
        "an unauthenticated caller cannot probe users: {body}"
    );
    let (status, body) = user_rpc(
        &client,
        &base,
        BEARER,
        "alice",
        Some("t=1,v1=00"),
        "core.ping",
    );
    assert_eq!(status, 401, "{body}");
    let forged = openhuman_core::user_agents::gateway::sign(BEARER, "alice", now());
    let (status, body) = user_rpc(&client, &base, BEARER, "bob", Some(&forged), "core.ping");
    assert_eq!(status, 401, "alice's signature does not cover bob: {body}");
    let (status, body) = user_rpc(&client, &base, BEARER, "bob", None, "core.ping");
    assert_eq!(status, 403, "bob is not provisioned: {body}");

    // Single-user surfaces are closed.
    for path in ["/events", "/events/domain", "/v1/models", "/dev/connect"] {
        let status = client
            .get(format!("{base}{path}"))
            .bearer_auth(BEARER)
            .send()
            .unwrap()
            .status()
            .as_u16();
        assert_eq!(status, 404, "{path}");
    }

    // The credential lives in alice's own directory.
    let agent_dir = d.root.join("agents").join(alice.as_str());
    let stored: Vec<_> = std::fs::read_dir(&agent_dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        stored.iter().any(|name| name.contains("auth")),
        "credential store beside alice's config: {stored:?}"
    );
    drop(server);
}

fn provision(client: &reqwest::blocking::Client, base: &str, user: &str) -> String {
    let (_, body) = rpc_with(
        client,
        base,
        Some(BEARER),
        "openhuman.user_agents_provision",
        json!({ "user_id": user }),
    );
    assert!(body.get("result").is_some(), "provision {user}: {body}");
    openhuman_core::user_agents::UserAgentId::for_user(user)
        .unwrap()
        .to_string()
}

fn thread_ids(body: &Value) -> Vec<String> {
    let text = body.to_string();
    let mut ids = Vec::new();
    for part in text.split("\"id\":\"").skip(1) {
        if let Some(end) = part.find('"') {
            ids.push(part[..end].to_string());
        }
    }
    ids
}

#[test]
fn each_user_sees_only_their_own_threads() {
    let d = deployment(true);
    let (server, base, client) = start(&d);
    let alice = provision(&client, &base, "alice");
    let bob = provision(&client, &base, "bob");
    let call = |user: &str, method: &str, params: Value| {
        user_rpc_with(&client, &base, BEARER, user, None, method, params)
    };

    let (status, body) = call("alice", "openhuman.threads_create_new", json!({}));
    assert_eq!(status, 200, "{body}");
    assert!(body.get("result").is_some(), "{body}");

    // The same caller-chosen id in two users' scopes is two threads.
    for user in ["alice", "bob"] {
        let (_, body) = call(
            user,
            "openhuman.threads_upsert",
            json!({ "id": "shared-id", "title": format!("{user}'s"), "created_at": "2026-10-07T00:00:00Z" }),
        );
        assert!(body.get("result").is_some(), "{user} upsert: {body}");
    }

    let (_, alice_list) = call("alice", "openhuman.threads_list", json!({}));
    let (_, bob_list) = call("bob", "openhuman.threads_list", json!({}));
    let alice_ids = thread_ids(&alice_list);
    let bob_ids = thread_ids(&bob_list);
    assert_eq!(alice_ids.len(), 2, "alice: {alice_list}");
    assert_eq!(bob_ids, vec!["shared-id".to_string()], "bob: {bob_list}");
    assert!(bob_list.to_string().contains("bob's"), "{bob_list}");
    assert!(!bob_list.to_string().contains("alice's"), "{bob_list}");
    // And the other way: alice keeps her own `shared-id`, untouched by bob's.
    assert!(
        alice_ids.contains(&"shared-id".to_string()),
        "alice: {alice_list}"
    );
    assert!(alice_list.to_string().contains("alice's"), "{alice_list}");
    assert!(!alice_list.to_string().contains("bob's"), "{alice_list}");

    // A SaaS user cannot point a thread at a host folder.
    let (_, body) = call(
        "alice",
        "openhuman.threads_create_new",
        json!({ "action_dir": "/etc" }),
    );
    assert!(body.get("error").is_some(), "{body}");

    // A hidden method answers unknown-method even with bad params, rather
    // than its parameter errors.
    let (_, body) = call("alice", "openhuman.threads_update_working_dir", json!({}));
    let error = body["error"].to_string();
    assert!(!error.contains("missing"), "{body}");

    // Each user's threads live in their own workspace.
    for (agent, owner) in [(&alice, "alice"), (&bob, "bob")] {
        let threads = d.root.join("agents").join(agent).join("workspace");
        assert!(threads.is_dir(), "{owner}'s workspace");
    }
    // Boot migrations leave an empty index in the operator workspace; no user
    // thread may ever reach it.
    let operator_index = d
        .root
        .join("operator/workspace/memory/conversations/threads.jsonl");
    let operator_threads = std::fs::read_to_string(&operator_index).unwrap_or_default();
    assert!(
        !operator_threads.contains("shared-id") && operator_threads.trim().is_empty(),
        "no user thread lands in the operator workspace: {operator_threads}"
    );

    // Reserved and path-like ids are refused; turn-starting methods are closed.
    for id in ["channel:telegram/1", "../escape"] {
        let (_, body) = call(
            "alice",
            "openhuman.threads_upsert",
            json!({ "id": id, "title": "x", "created_at": "2026-10-07T00:00:00Z" }),
        );
        assert!(body.get("error").is_some(), "{id}: {body}");
    }
    let (_, body) = call("alice", "openhuman.threads_regenerate", json!({}));
    assert!(body.get("error").is_some(), "{body}");
    drop(server);
}

/// Open `/events?client_id=` for `user` and forward each SSE `data:` line.
fn user_events(base: &str, user: &str, client_id: &str) -> std::sync::mpsc::Receiver<String> {
    use openhuman_core::user_agents::gateway::{sign, USER_HEADER, USER_SIG_HEADER};
    use std::io::BufRead;
    let (tx, rx) = std::sync::mpsc::channel();
    let url = format!("{base}/events?client_id={client_id}");
    let user = user.to_string();
    std::thread::spawn(move || {
        let client = reqwest::blocking::Client::builder()
            .timeout(None)
            .build()
            .unwrap();
        let Ok(response) = client
            .get(&url)
            .bearer_auth(BEARER)
            .header(USER_HEADER, &user)
            .header(USER_SIG_HEADER, sign(BEARER, &user, now()))
            .send()
        else {
            return;
        };
        let _ = tx.send(format!("status:{}", response.status().as_u16()));
        for line in std::io::BufReader::new(response).lines() {
            let Ok(line) = line else { break };
            if let Some(data) = line.strip_prefix("data:") {
                if tx.send(data.trim().to_string()).is_err() {
                    break;
                }
            }
        }
    });
    rx
}

#[test]
fn chat_events_reach_only_the_user_whose_turn_produced_them() {
    let d = deployment(true);
    // Point the backend at a closed port so the turn fails fast — the failure
    // is itself an event on the owner's stream, without any real inference.
    let port = free_port();
    let child = core_command(&d, &["--port", &port.to_string()])
        .env("BACKEND_URL", "http://127.0.0.1:9")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn openhuman-core");
    let server = Server(child);
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    while !client
        .get(format!("{base}/health"))
        .send()
        .is_ok_and(|r| r.status().is_success())
    {
        assert!(Instant::now() < deadline, "SaaS core never became healthy");
        std::thread::sleep(Duration::from_millis(250));
    }
    provision(&client, &base, "alice");
    provision(&client, &base, "bob");

    // The operator has no chat stream.
    let status = client
        .get(format!("{base}/events?client_id=c1"))
        .bearer_auth(BEARER)
        .send()
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(status, 404);

    // Both users listen on the same client id.
    let alice_events = user_events(&base, "alice", "c1");
    let bob_events = user_events(&base, "bob", "c1");
    for (who, rx) in [("alice", &alice_events), ("bob", &bob_events)] {
        let first = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(first, "status:200", "{who}'s stream opens");
    }

    // A reserved thread id is refused before any turn starts.
    let (_, body) = user_rpc_with(
        &client,
        &base,
        BEARER,
        "alice",
        None,
        "openhuman.channel_web_chat",
        json!({ "client_id": "c1", "thread_id": "channel:slack:x", "message": "hello" }),
    );
    assert!(body.get("error").is_some(), "{body}");

    let (status, body) = user_rpc_with(
        &client,
        &base,
        BEARER,
        "alice",
        None,
        "openhuman.channel_web_chat",
        json!({ "client_id": "c1", "thread_id": "chat-1", "message": "hello" }),
    );
    assert_eq!(status, 200, "{body}");

    let mut alice_got = Vec::new();
    let until = Instant::now() + Duration::from_secs(30);
    while Instant::now() < until {
        match alice_events.recv_timeout(Duration::from_millis(500)) {
            Ok(data) if data.contains("chat-1") => {
                alice_got.push(data);
                if alice_got
                    .iter()
                    .any(|e| e.contains("error") || e.contains("done"))
                {
                    break;
                }
            }
            Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(e) => panic!("alice's stream closed: {e}"),
        }
    }
    assert!(!alice_got.is_empty(), "alice receives her turn's events");
    assert!(
        alice_got.iter().all(|e| !e.contains("\"agent\"")),
        "the routing stamp is not on the wire: {alice_got:?}"
    );

    std::thread::sleep(Duration::from_secs(1));
    let leaked: Vec<String> = bob_events.try_iter().collect();
    assert!(
        leaked.is_empty(),
        "bob shares the client id but must see none of alice's events: {leaked:?}"
    );
    drop(server);
}

#[test]
fn users_reach_their_memory_but_not_its_configuration() {
    let d = deployment(true);
    let (server, base, client) = start(&d);
    provision(&client, &base, "alice");
    let call = |method: &str, params: Value| {
        user_rpc_with(&client, &base, BEARER, "alice", None, method, params)
    };

    // Reachable: with no backend in this test the engine is off, so recall
    // answers with memory's own error — not "unknown method".
    let (_, body) = call(
        "openhuman.memory_recall",
        json!({ "question": "anything?" }),
    );
    let text = body.to_string();
    assert!(
        !text.contains("unknown method"),
        "memory_recall is on the surface: {text}"
    );

    // Not reachable: anything that changes where memory lives or reads the host.
    // The engine's settings carry its credential: operator-only.
    for method in [
        "openhuman.memory_engine_get",
        "openhuman.memory_engine_set",
        "openhuman.memory_policy_set",
        "openhuman.memory_sources_add",
        "openhuman.memory_import_start",
    ] {
        let (_, body) = call(method, json!({}));
        assert!(
            body.to_string().contains("unknown method"),
            "{method} must be absent: {body}"
        );
    }
    drop(server);
}

#[test]
fn a_duplicate_or_unreadable_user_header_is_refused() {
    use openhuman_core::user_agents::gateway::USER_HEADER;
    let d = deployment(true);
    let (server, base, client) = start(&d);
    let body =
        json!({ "jsonrpc": "2.0", "id": 1, "method": "openhuman.user_agents_list", "params": {} });

    // Two user headers: refused, never run as the operator.
    let status = client
        .post(format!("{base}/rpc"))
        .bearer_auth(BEARER)
        .header(USER_HEADER, "alice")
        .header(USER_HEADER, "bob")
        .json(&body)
        .send()
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(status, 400);

    // A header value that is valid HTTP but not text: refused too.
    let unreadable = reqwest::header::HeaderValue::from_bytes(b"alice\xff").unwrap();
    let status = client
        .post(format!("{base}/rpc"))
        .bearer_auth(BEARER)
        .header(USER_HEADER, unreadable)
        .json(&body)
        .send()
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(status, 400);
    drop(server);
}
