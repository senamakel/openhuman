//! Profile leases and per-profile turn isolation across real `openhuman-core`
//! processes: one profile is hosted by one core at a time, a profile with a
//! live turn is never evicted from under it, and two users on the same thread
//! id never touch each other's turns.
//!
//! Turns are held mid-flight by a backend that accepts the inference request
//! and never answers ([`hanging_backend`]).

use super::*;
use std::process::Child;

/// A backend that holds every inference request open without answering, so
/// a turn that reaches inference stays in flight until it is cancelled or its
/// process dies. Anything else gets an immediate `500`, so the turn's other
/// backend calls (integrations, model limits) fail fast instead of waiting
/// out their timeouts.
fn hanging_backend() -> u16 {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    return;
                }
                if line.contains("/chat/completions") {
                    // Drain until the peer goes away; never answer.
                    let mut buf = [0u8; 4096];
                    while matches!(reader.read(&mut buf), Ok(n) if n > 0) {}
                    return;
                }
                let mut stream = stream;
                let _ = stream.write_all(
                    b"HTTP/1.1 500 Internal Server Error\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                );
            });
        }
    });
    port
}

/// One core of a deployment.
struct Node {
    name: String,
    server: Server,
    base: String,
    log: PathBuf,
}

impl Node {
    fn log_tail(&self) -> String {
        let log = std::fs::read_to_string(&self.log).unwrap_or_default();
        // The polling RPCs drown everything else out.
        let lines: Vec<&str> = log
            .lines()
            .filter(|line| !line.contains("rpc_handler [rpc]"))
            .collect();
        lines[lines.len().saturating_sub(80)..].join("\n")
    }

    fn kill(&mut self) {
        // SIGKILL on Unix: no shutdown hook runs, no lease is released.
        self.server.0.kill().expect("kill core");
        let _ = self.server.0.wait();
    }
}

/// Write node `name`'s operator file on deployment `d` and start it.
fn start_node(
    d: &Deployment,
    name: &str,
    storage_url: Option<&str>,
    extra: &str,
    backend: Option<u16>,
) -> Node {
    let port = free_port();
    let base = format!("http://127.0.0.1:{port}");
    let mut config = format!(
        "root = {:?}\nnode_id = \"core-{name}\"\noperator_dir = {:?}\nlease_ttl_secs = 3\n",
        d.root.display().to_string(),
        d.tmp
            .path()
            .join(format!("operator-{name}"))
            .display()
            .to_string(),
    );
    if let Some(url) = storage_url {
        config.push_str(&format!(
            "storage_url = {url:?}\nadvertise_url = {base:?}\n"
        ));
    }
    config.push_str(extra);
    let config_path = d.tmp.path().join(format!("operator-{name}.toml"));
    std::fs::write(&config_path, config).unwrap();

    let log = d.tmp.path().join(format!("core-{name}.log"));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_openhuman-core"));
    cmd.args(["run", "--mode", "saas", "--saas-config"])
        .arg(&config_path)
        .args(["--port", &port.to_string()])
        .env("HOME", d.tmp.path())
        .env("USERPROFILE", d.tmp.path())
        .env("RUST_LOG", "info,openhuman::storage::lease=debug")
        .stdout(std::fs::File::create(&log).unwrap())
        .stderr(Stdio::null());
    for var in SCRUBBED_ENV {
        cmd.env_remove(var);
    }
    match backend {
        Some(port) => cmd.env("BACKEND_URL", format!("http://127.0.0.1:{port}")),
        None => cmd.env("BACKEND_URL", "http://127.0.0.1:9"),
    };
    let child: Child = cmd.spawn().expect("spawn openhuman-core");
    let mut node = Node {
        name: name.to_string(),
        server: Server(child),
        base,
        log,
    };
    let client = client();
    let deadline = Instant::now() + Duration::from_secs(120);
    while !client
        .get(format!("{}/health", node.base))
        .send()
        .is_ok_and(|r| r.status().is_success())
    {
        if let Ok(Some(status)) = node.server.0.try_wait() {
            panic!(
                "core {} exited before serving: {status}\n{}",
                node.name,
                node.log_tail()
            );
        }
        assert!(
            Instant::now() < deadline,
            "core {} never became healthy",
            name
        );
        std::thread::sleep(Duration::from_millis(250));
    }
    node
}

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
}

/// POST /rpc for `user` on `node`: status, `X-OpenHuman-Profile-Owner`, body.
fn call(node: &Node, user: &str, method: &str, params: Value) -> (u16, Option<String>, Value) {
    use openhuman_core::profiles::gateway::{sign, USER_HEADER, USER_SIG_HEADER};
    let response = client()
        .post(format!("{}/rpc", node.base))
        .bearer_auth(BEARER)
        .header(USER_HEADER, user)
        .header(USER_SIG_HEADER, sign(BEARER, user, now()))
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
        .send()
        .expect("POST /rpc");
    let status = response.status().as_u16();
    let owner = response
        .headers()
        .get("x-openhuman-profile-owner")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    (status, owner, response.json().unwrap_or(Value::Null))
}

fn operator(node: &Node, method: &str, params: Value) -> Value {
    let (_, body) = rpc_with(&client(), &node.base, Some(BEARER), method, params);
    body
}

/// Provision `user` (through `node`) with a session credential, so their
/// turns reach inference.
fn provision_with_credential(node: &Node, user: &str) -> String {
    let profile = provision(&client(), &node.base, user);
    let body = operator(
        node,
        "openhuman.profiles_set_credential",
        json!({ "profile_id": profile, "kind": "session", "token": format!("{user}-jwt") }),
    );
    assert!(body.get("result").is_some(), "{body}");
    profile
}

fn start_turn(node: &Node, user: &str, thread: &str) -> Value {
    let (status, _, body) = call(
        node,
        user,
        "openhuman.channel_web_chat",
        json!({ "client_id": "c1", "thread_id": thread, "message": "hello" }),
    );
    assert_eq!(status, 200, "{user} starts a turn on {thread}: {body}");
    assert!(body.get("result").is_some(), "{body}");
    body
}

/// Whether `user`'s turn on `thread` is in flight on `node`.
fn active(node: &Node, user: &str, thread: &str) -> bool {
    let (status, _, body) = call(
        node,
        user,
        "openhuman.channel_web_queue_status",
        json!({ "thread_id": thread }),
    );
    status == 200 && body.to_string().contains("\"active\":true")
}

fn wait_until(what: &str, node: &Node, timeout: Duration, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !done() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; core {} log:\n{}",
            node.name,
            node.log_tail()
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The lifecycle of `user`'s latest turn snapshot on `thread`, if any.
fn turn_state(node: &Node, user: &str, thread: &str) -> Option<String> {
    let (status, _, body) = call(
        node,
        user,
        "openhuman.threads_turn_state_get",
        json!({ "thread_id": thread }),
    );
    assert_eq!(status, 200, "{body}");
    body.pointer("/result/data/turnState/lifecycle")
        .or_else(|| body.pointer("/result/result/data/turnState/lifecycle"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The lease scenario on one storage configuration: `storage_url` shared by
/// both cores (cluster leases), or `None` (file locks on the shared root).
fn one_core_at_a_time(storage_url: Option<&str>) {
    let d = deployment(true);
    let storage_url =
        storage_url.map(|url| url.replace("{tmp}", &d.tmp.path().display().to_string()));
    let backend = hanging_backend();
    let one = start_node(&d, "1", storage_url.as_deref(), "", Some(backend));
    let mut two = start_node(&d, "2", storage_url.as_deref(), "", Some(backend));
    provision_with_credential(&one, "alice");

    // Core 1 opens alice; core 2 is told who has her.
    let (status, _, body) = call(&one, "alice", "core.ping", json!({}));
    assert_eq!(status, 200, "{body}");
    let (status, owner, body) = call(&two, "alice", "core.ping", json!({}));
    assert_eq!(status, 409, "{body}");
    assert_eq!(owner.as_deref(), Some("core-1"));
    assert_eq!(body["error"], "profile_held");
    assert_eq!(body["owner"], "core-1");
    if storage_url.is_some() {
        assert_eq!(body["endpoint"], one.base.as_str(), "{body}");
        assert!(
            body["retry_after_ms"].as_u64().is_some_and(|ms| ms > 0),
            "{body}"
        );
    }

    // An operator release on core 1 hands alice over at once.
    let body = operator(
        &one,
        "openhuman.profiles_release",
        json!({ "profile_id": "alice" }),
    );
    assert!(body.to_string().contains("\"released\":true"), "{body}");
    let (status, _, body) = call(&two, "alice", "core.ping", json!({}));
    assert_eq!(
        status, 200,
        "core 2 hosts alice right after the release: {body}"
    );
    let (status, owner, _) = call(&one, "alice", "core.ping", json!({}));
    assert_eq!((status, owner.as_deref()), (409, Some("core-2")));

    // Core 2 dies mid-turn, holding the lease. (The credential is installed
    // through the core that runs the turn: each node keeps its own keyring.)
    let body = operator(
        &two,
        "openhuman.profiles_set_credential",
        json!({ "profile_id": "alice", "kind": "session", "token": "alice-jwt" }),
    );
    assert!(body.get("result").is_some(), "{body}");
    start_turn(&two, "alice", "t-kill");
    // In flight, and far enough that its snapshot is on disk.
    wait_until(
        "alice's turn in flight on core 2",
        &two,
        Duration::from_secs(90),
        || active(&two, "alice", "t-kill") && turn_state(&two, "alice", "t-kill").is_some(),
    );
    let live = turn_state(&two, "alice", "t-kill");
    assert!(
        matches!(live.as_deref(), Some("started" | "streaming")),
        "{live:?}"
    );
    two.kill();

    // Core 1 takes alice over once the lease lapses (at once for a file
    // lock, which dies with its process), and the turn reads interrupted.
    wait_until(
        "core 1 to take alice over",
        &one,
        Duration::from_secs(30),
        || call(&one, "alice", "core.ping", json!({})).0 == 200,
    );
    assert_eq!(
        turn_state(&one, "alice", "t-kill").as_deref(),
        Some("interrupted"),
        "core 1 log:\n{}",
        one.log_tail()
    );
}

#[test]
fn a_profile_is_hosted_by_one_core_at_a_time() {
    one_core_at_a_time(None);
}

/// The same over a shared SQLite backend: the profile registry and the
/// leases live in it, and a dead holder's lease is taken over only after its
/// TTL. Needs the `storage-sqlite` feature, which CI's product set leaves
/// out: `cargo test -p openhuman-cli --test saas_mode_e2e --features storage-sqlite`.
#[cfg(feature = "storage-sqlite")]
#[test]
fn a_profile_is_hosted_by_one_core_at_a_time_over_a_shared_backend() {
    one_core_at_a_time(Some("sqlite:{tmp}/shared.db"));
}

#[test]
fn a_profile_in_use_is_not_evicted_mid_turn() {
    let d = deployment(true);
    let backend = hanging_backend();
    let node = start_node(
        &d,
        "1",
        None,
        "max_profiles_open = 1\nidle_evict_secs = 0\n",
        Some(backend),
    );
    provision_with_credential(&node, "alice");
    provision_with_credential(&node, "bob");

    // Alice's request returns at once; her turn keeps running.
    start_turn(&node, "alice", "t1");
    wait_until(
        "alice's turn in flight",
        &node,
        Duration::from_secs(60),
        || active(&node, "alice", "t1"),
    );

    // The only slot is busy with a live turn, though no request holds it.
    let (status, _, body) = call(&node, "bob", "core.ping", json!({}));
    assert_eq!(status, 503, "bob must not evict alice mid-turn: {body}");

    // Alice's turn tables survived: her cancel finds the turn.
    let (_, _, body) = call(
        &node,
        "alice",
        "openhuman.channel_web_cancel",
        json!({ "client_id": "c1", "thread_id": "t1" }),
    );
    assert!(body.to_string().contains("\"cancelled\":true"), "{body}");

    // Once the turn is gone alice is idle, and bob gets the slot.
    wait_until(
        "bob to get the slot",
        &node,
        Duration::from_secs(30),
        || call(&node, "bob", "core.ping", json!({})).0 == 200,
    );
}

#[test]
fn two_users_with_the_same_thread_id_stay_apart() {
    let d = deployment(true);
    let backend = hanging_backend();
    let node = start_node(&d, "1", None, "", Some(backend));
    provision_with_credential(&node, "alice");
    provision_with_credential(&node, "bob");

    // Concurrent turns on one thread id.
    let (alice_turn, bob_turn) = std::thread::scope(|s| {
        let a = s.spawn(|| start_turn(&node, "alice", "t1"));
        let b = s.spawn(|| start_turn(&node, "bob", "t1"));
        (a.join().unwrap(), b.join().unwrap())
    });
    wait_until(
        "both turns in flight",
        &node,
        Duration::from_secs(60),
        || active(&node, "alice", "t1") && active(&node, "bob", "t1"),
    );
    let request_id = |body: &Value| {
        find_key(body, "request_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| panic!("no request_id in {body}"))
    };
    let alice_request = request_id(&alice_turn);

    // Bob naming alice's request on the shared thread id cancels nothing.
    let (_, _, body) = call(
        &node,
        "bob",
        "openhuman.channel_web_cancel",
        json!({ "client_id": "c1", "thread_id": "t1", "request_id": alice_request }),
    );
    assert!(body.to_string().contains("\"cancelled\":false"), "{body}");
    assert!(active(&node, "alice", "t1") && active(&node, "bob", "t1"));

    // Bob's unscoped stop takes down his own turn only.
    let (_, _, body) = call(
        &node,
        "bob",
        "openhuman.channel_web_cancel",
        json!({ "client_id": "c1", "thread_id": "t1" }),
    );
    assert!(body.to_string().contains("\"cancelled\":true"), "{body}");
    assert!(!body.to_string().contains(alice_request.as_str()), "{body}");
    assert!(body.to_string().contains(&request_id(&bob_turn)), "{body}");
    wait_until("bob's turn to stop", &node, Duration::from_secs(15), || {
        !active(&node, "bob", "t1")
    });
    assert!(
        active(&node, "alice", "t1"),
        "alice's turn on the same thread id is untouched"
    );
}

/// The first value under `key` anywhere in `value`.
fn find_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => map
            .get(key)
            .or_else(|| map.values().find_map(|v| find_key(v, key))),
        Value::Array(items) => items.iter().find_map(|v| find_key(v, key)),
        _ => None,
    }
}
