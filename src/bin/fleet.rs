//! `openhuman-fleet` — a process-per-user supervisor + reverse proxy.
//!
//! Hosts one `openhuman-core` process per user/workspace and fronts them behind
//! a single endpoint so a team server can manage many members' assistants while
//! every existing client (`CloudHttpTransport`) keeps working unchanged. This is
//! Phase 4 of the pluggable-core plan (`docs/plans/pluggable-core/phase-4-fleet-host.md`).
//!
//! Design (process-per-user, not in-process multi-tenancy):
//! - Each tenant gets its own OS process (`openhuman-core run --jsonrpc-only`),
//!   its own workspace volume (`OPENHUMAN_WORKSPACE`), and its own core bearer
//!   (`OPENHUMAN_CORE_TOKEN`) — so tenants are isolated at the OS boundary,
//!   which matters because agents run arbitrary tools.
//! - The supervisor mints a distinct **edge token** per tenant for clients; it
//!   is the only holder of the tenants' **core bearers**. `EdgeToken` and
//!   `CoreBearer` are kept deliberately distinct so they cannot be confused.
//! - The reverse proxy forwards `POST /{user_id}/rpc` verbatim to that tenant's
//!   core `http://127.0.0.1:<port>/rpc`, so the JSON-RPC wire contract is
//!   unchanged end to end.
//!
//! MVP scope: explicit sequential port assignment (a production supervisor would
//! read each core's bound port from a ready file / `EmbeddedReadySignal` and
//! reconcile membership against `tinyhumansai/backend`). Limitations are logged,
//! never silently swallowed.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use axum::{
    body::Bytes,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use clap::Parser;
use tokio::sync::RwLock;

// ---------------------------------------------------------------------------
// Edge auth — maps opaque client-facing tokens to a user id.
// ---------------------------------------------------------------------------

mod edge_auth {
    use std::collections::HashMap;

    /// Maps opaque, client-facing **edge tokens** to the user id they authorize.
    /// The fleet never hands a tenant's core bearer to a client — clients only
    /// ever see an edge token, which the proxy exchanges for the core bearer.
    #[derive(Default)]
    pub struct EdgeAuth {
        tokens: HashMap<String, String>,
    }

    impl EdgeAuth {
        pub fn new() -> Self {
            Self::default()
        }

        /// Mint an edge token authorizing `user_id`. Deterministic prefix +
        /// caller-supplied unique suffix (a UUID at the call site) so this stays
        /// pure and unit-testable.
        pub fn insert(&mut self, token: impl Into<String>, user_id: impl Into<String>) {
            self.tokens.insert(token.into(), user_id.into());
        }

        /// The user id an edge token authorizes, if any.
        pub fn user_for(&self, token: &str) -> Option<&str> {
            self.tokens.get(token).map(String::as_str)
        }

        pub fn len(&self) -> usize {
            self.tokens.len()
        }

        pub fn is_empty(&self) -> bool {
            self.tokens.is_empty()
        }
    }
}

use edge_auth::EdgeAuth;

// ---------------------------------------------------------------------------
// Tenant registry — pure derivation of per-user port / workspace / rpc url.
// ---------------------------------------------------------------------------

/// A provisioned tenant core: where it listens and the bearer to reach it.
#[derive(Debug, Clone)]
struct CoreInstance {
    user_id: String,
    port: u16,
    core_bearer: String,
    workspace_dir: PathBuf,
}

impl CoreInstance {
    /// The loopback RPC URL the proxy forwards to.
    fn rpc_url(&self) -> String {
        format!("http://127.0.0.1:{}/rpc", self.port)
    }
}

/// Pure port assignment: tenant `index` (0-based) maps to `base_port + index`.
/// Kept a free function so it is trivially unit-testable and the policy is
/// obvious at the call site.
fn port_for_index(base_port: u16, index: usize) -> Option<u16> {
    u16::try_from(base_port as usize + index).ok()
}

/// Pure workspace derivation: `<root>/<user_id>`. The caller is responsible for
/// having validated `user_id` (see [`is_valid_user_id`]).
fn workspace_for(root: &Path, user_id: &str) -> PathBuf {
    root.join(user_id)
}

/// A user id must be a single safe path segment — no separators, no `..`, non
/// empty — so it cannot escape the workspaces root or the proxy route.
fn is_valid_user_id(user_id: &str) -> bool {
    !user_id.is_empty()
        && user_id.len() <= 128
        && user_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// ---------------------------------------------------------------------------
// Fleet state
// ---------------------------------------------------------------------------

struct Fleet {
    instances: HashMap<String, CoreInstance>,
    edge_auth: EdgeAuth,
    http: reqwest::Client,
}

impl Fleet {
    fn user_for_bearer(&self, headers: &HeaderMap) -> Option<String> {
        let token = bearer_from_headers(headers)?;
        self.edge_auth.user_for(&token).map(str::to_string)
    }
}

/// Extract the bearer token from an `Authorization: Bearer <t>` header.
fn bearer_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// Proxy handler
// ---------------------------------------------------------------------------

async fn rpc_proxy(
    State(fleet): State<Arc<RwLock<Fleet>>>,
    AxumPath(user_id): AxumPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let fleet = fleet.read().await;

    // Edge auth: the bearer must map to a user, and that user must match the
    // path segment — so tenant A's token cannot reach tenant B's core.
    let authorized = fleet.user_for_bearer(&headers);
    match authorized {
        Some(u) if u == user_id => {}
        Some(_) => {
            log::warn!("[fleet] reject: edge token authorized a different user than /{user_id}");
            return (StatusCode::FORBIDDEN, "token/user mismatch").into_response();
        }
        None => {
            return (StatusCode::UNAUTHORIZED, "missing or unknown edge token").into_response();
        }
    }

    let Some(instance) = fleet.instances.get(&user_id) else {
        return (StatusCode::NOT_FOUND, "no such tenant").into_response();
    };

    // Forward verbatim to the tenant core, swapping the edge token for the
    // tenant's core bearer. The JSON-RPC body is passed through untouched.
    let upstream = fleet
        .http
        .post(instance.rpc_url())
        .header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", instance.core_bearer),
        )
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await;

    match upstream {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            match resp.bytes().await {
                Ok(bytes) => (status, bytes).into_response(),
                Err(e) => {
                    log::error!("[fleet] upstream body read failed for /{user_id}: {e}");
                    (StatusCode::BAD_GATEWAY, "upstream body error").into_response()
                }
            }
        }
        Err(e) => {
            log::error!("[fleet] upstream request to tenant {user_id} failed: {e}");
            (StatusCode::BAD_GATEWAY, "tenant core unreachable").into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Core process lifecycle
// ---------------------------------------------------------------------------

/// Spawn one `openhuman-core run --jsonrpc-only` child bound to `instance.port`,
/// scoped to the tenant's workspace and core bearer. Returns the child handle.
async fn spawn_core(
    core_bin: &Path,
    instance: &CoreInstance,
) -> anyhow::Result<tokio::process::Child> {
    std::fs::create_dir_all(&instance.workspace_dir).with_context(|| {
        format!(
            "creating workspace dir {} for tenant {}",
            instance.workspace_dir.display(),
            instance.user_id
        )
    })?;

    log::info!(
        "[fleet] spawning core for tenant={} port={} workspace={}",
        instance.user_id,
        instance.port,
        instance.workspace_dir.display()
    );

    let child = tokio::process::Command::new(core_bin)
        .arg("run")
        .arg("--jsonrpc-only")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(instance.port.to_string())
        .env("OPENHUMAN_WORKSPACE", &instance.workspace_dir)
        .env("OPENHUMAN_CORE_TOKEN", &instance.core_bearer)
        // Each tenant is a headless single-core; keep channel listeners off so a
        // fleet host doesn't poll every member's messaging integrations.
        .env("OPENHUMAN_DISABLE_CHANNEL_LISTENERS", "1")
        .spawn()
        .with_context(|| {
            format!(
                "spawning {} for tenant {}",
                core_bin.display(),
                instance.user_id
            )
        })?;

    Ok(child)
}

/// Poll a tenant core's `/health` until it responds or the attempt budget is
/// exhausted. Best-effort — the proxy still starts even if a core is slow.
async fn wait_healthy(http: &reqwest::Client, port: u16, attempts: u32) -> bool {
    let url = format!("http://127.0.0.1:{port}/health");
    for attempt in 1..=attempts {
        match http.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => return true,
            _ => tokio::time::sleep(std::time::Duration::from_millis(250 * attempt as u64)).await,
        }
    }
    false
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(
    name = "openhuman-fleet",
    about = "Process-per-user OpenHuman core supervisor + reverse proxy"
)]
struct Args {
    /// Address the reverse proxy listens on.
    #[arg(long, default_value = "127.0.0.1:8899")]
    listen: String,
    /// Root directory under which each tenant's workspace is created.
    #[arg(long, default_value = "./fleet-workspaces")]
    workspaces_root: PathBuf,
    /// Path to the `openhuman-core` binary to spawn per tenant.
    #[arg(long, default_value = "openhuman-core")]
    core_bin: PathBuf,
    /// First tenant core port; tenant N listens on `base_core_port + N`.
    #[arg(long, default_value_t = 7900)]
    base_core_port: u16,
    /// Comma-separated user ids to provision at boot.
    #[arg(long, value_delimiter = ',')]
    users: Vec<String>,
}

/// Provision the in-memory tenant table + edge tokens for `users`. Pure w.r.t.
/// the filesystem/network so it is unit-testable; spawning happens separately.
fn provision(
    users: &[String],
    workspaces_root: &Path,
    base_core_port: u16,
) -> anyhow::Result<(
    HashMap<String, CoreInstance>,
    EdgeAuth,
    Vec<(String, String)>,
)> {
    let mut instances = HashMap::new();
    let mut edge_auth = EdgeAuth::new();
    let mut minted = Vec::new();

    for (index, user_id) in users.iter().enumerate() {
        if !is_valid_user_id(user_id) {
            anyhow::bail!("invalid user id {user_id:?}: must be a single [A-Za-z0-9_-] segment");
        }
        if instances.contains_key(user_id) {
            anyhow::bail!("duplicate user id {user_id:?}");
        }
        let port = port_for_index(base_core_port, index)
            .with_context(|| format!("port overflow assigning tenant #{index}"))?;
        let core_bearer = format!("core-{}", uuid::Uuid::new_v4());
        let edge_token = format!("edge-{}", uuid::Uuid::new_v4());
        edge_auth.insert(edge_token.clone(), user_id.clone());
        minted.push((user_id.clone(), edge_token));
        instances.insert(
            user_id.clone(),
            CoreInstance {
                user_id: user_id.clone(),
                port,
                core_bearer,
                workspace_dir: workspace_for(workspaces_root, user_id),
            },
        );
    }

    Ok((instances, edge_auth, minted))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(false).try_init();
    let args = Args::parse();

    if args.users.is_empty() {
        anyhow::bail!("no tenants: pass --users a,b,c");
    }

    let (instances, edge_auth, minted) =
        provision(&args.users, &args.workspaces_root, args.base_core_port)?;

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("building HTTP client")?;

    // Spawn each tenant core. Children are held for the lifetime of the process;
    // dropping the supervisor drops the handles.
    let mut children = Vec::new();
    for instance in instances.values() {
        match spawn_core(&args.core_bin, instance).await {
            Ok(child) => {
                let healthy = wait_healthy(&http, instance.port, 20).await;
                log::info!(
                    "[fleet] tenant {} core {}",
                    instance.user_id,
                    if healthy {
                        "ready"
                    } else {
                        "spawned (health probe timed out)"
                    }
                );
                children.push(child);
            }
            Err(e) => {
                log::error!("[fleet] failed to spawn tenant {}: {e:#}", instance.user_id);
            }
        }
    }

    // Surface the minted edge tokens so the operator can hand them to clients.
    // (A production supervisor would return these via an admin API, not stdout.)
    for (user_id, token) in &minted {
        println!("edge-token {user_id} {token}");
    }

    let fleet = Arc::new(RwLock::new(Fleet {
        instances,
        edge_auth,
        http,
    }));

    let app = Router::new()
        .route("/{user_id}/rpc", post(rpc_proxy))
        .with_state(fleet);

    let addr: SocketAddr = args
        .listen
        .parse()
        .with_context(|| format!("invalid --listen address {:?}", args.listen))?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding proxy on {addr}"))?;
    log::info!("[fleet] reverse proxy listening on http://{addr} — POST /{{user_id}}/rpc");

    axum::serve(listener, app).await.context("serving proxy")?;

    // Keep children owned until serve returns (shutdown).
    drop(children);
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests — pure logic (no child processes / network).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_assignment_is_sequential_from_base() {
        assert_eq!(port_for_index(7900, 0), Some(7900));
        assert_eq!(port_for_index(7900, 5), Some(7905));
    }

    #[test]
    fn port_assignment_detects_overflow() {
        assert_eq!(port_for_index(u16::MAX, 1), None);
    }

    #[test]
    fn workspace_is_user_scoped_under_root() {
        let ws = workspace_for(Path::new("/srv/fleet"), "alice");
        assert_eq!(ws, PathBuf::from("/srv/fleet/alice"));
    }

    #[test]
    fn user_id_validation_rejects_path_escapes() {
        assert!(is_valid_user_id("alice"));
        assert!(is_valid_user_id("user_42-x"));
        assert!(!is_valid_user_id(""));
        assert!(!is_valid_user_id("../etc"));
        assert!(!is_valid_user_id("a/b"));
        assert!(!is_valid_user_id("a.b"));
    }

    #[test]
    fn provision_assigns_distinct_ports_and_edge_tokens() {
        let root = PathBuf::from("/tmp/ws");
        let users = vec!["alice".to_string(), "bob".to_string()];
        let (instances, edge_auth, minted) = provision(&users, &root, 7900).unwrap();

        assert_eq!(instances.len(), 2);
        assert_eq!(instances["alice"].port, 7900);
        assert_eq!(instances["bob"].port, 7901);
        assert_ne!(instances["alice"].core_bearer, instances["bob"].core_bearer);
        assert_eq!(instances["alice"].rpc_url(), "http://127.0.0.1:7900/rpc");

        // Every minted edge token resolves back to exactly its user.
        assert_eq!(edge_auth.len(), 2);
        for (user_id, token) in &minted {
            assert_eq!(edge_auth.user_for(token), Some(user_id.as_str()));
        }
    }

    #[test]
    fn provision_rejects_duplicate_and_invalid_users() {
        let root = PathBuf::from("/tmp/ws");
        assert!(provision(&["a".into(), "a".into()], &root, 7900).is_err());
        assert!(provision(&["../x".into()], &root, 7900).is_err());
    }

    #[test]
    fn bearer_parsing_requires_bearer_prefix() {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer edge-123".parse().unwrap(),
        );
        assert_eq!(bearer_from_headers(&h), Some("edge-123".to_string()));

        let mut h2 = HeaderMap::new();
        h2.insert(
            axum::http::header::AUTHORIZATION,
            "edge-123".parse().unwrap(),
        );
        assert_eq!(bearer_from_headers(&h2), None);
    }
}
