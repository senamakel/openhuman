//! In-process manager for ad-hoc static directory HTTP servers.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use axum::body::Body;
use axum::extract::{Path as AxumPath, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use base64::Engine as _;
use rand::RngExt as _;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::openhuman::config;
use crate::openhuman::credentials::session_support;
use crate::openhuman::http_host::types::{
    HostedDirAuth, HostedDirServerInfo, StartHostedDirParams,
};

const LOG_PREFIX: &str = "[http_host]";

struct HostedDirRuntime {
    info: HostedDirServerInfo,
    shutdown: CancellationToken,
    join_handle: JoinHandle<()>,
}

struct HostedDirRegistry {
    servers: Mutex<HashMap<String, HostedDirRuntime>>,
}

impl HostedDirRegistry {
    fn new() -> Self {
        Self {
            servers: Mutex::new(HashMap::new()),
        }
    }

    fn prune_finished_locked(servers: &mut HashMap<String, HostedDirRuntime>) {
        servers.retain(|server_id, runtime| {
            let keep = !runtime.join_handle.is_finished();
            if !keep {
                log::warn!("{LOG_PREFIX} pruning finished hosted server id={server_id}");
            }
            keep
        });
    }
}

#[derive(Clone)]
struct HostedDirState {
    root_dir: PathBuf,
    auth: HostedDirAuth,
}

static REGISTRY: OnceLock<HostedDirRegistry> = OnceLock::new();
static SHUTDOWN_HOOK_REGISTERED: OnceLock<()> = OnceLock::new();

fn registry() -> &'static HostedDirRegistry {
    REGISTRY.get_or_init(HostedDirRegistry::new)
}

pub async fn start_hosted_dir_server(
    params: StartHostedDirParams,
) -> Result<HostedDirServerInfo, String> {
    register_shutdown_hook_once();

    let root_dir = canonicalize_hosted_directory(&params.directory)?;
    let bind_host = sanitize_bind_host(&params.bind_host)?;
    let server_name = sanitize_optional_label(params.server_name.as_deref());
    let auth = if params.disable_auth {
        HostedDirAuth {
            enabled: false,
            username: None,
            password: None,
        }
    } else {
        let default_username = resolve_default_auth_username().await;
        let username = params
            .username
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or(default_username);
        let username =
            sanitize_basic_auth_username(username).unwrap_or_else(|| "openhuman".to_string());
        let password = generate_password();
        HostedDirAuth {
            enabled: true,
            username: Some(username),
            password: Some(password),
        }
    };

    let bind_target = format!("{bind_host}:{}", params.port);
    log::info!(
        "{LOG_PREFIX} start requested dir={} bind_target={} auth_enabled={} server_name={:?}",
        root_dir.display(),
        bind_target,
        auth.enabled,
        server_name
    );
    let listener = TcpListener::bind(&bind_target)
        .await
        .map_err(|e| format!("failed to bind hosted HTTP server on {bind_target}: {e}"))?;
    let local_addr = listener
        .local_addr()
        .map_err(|e| format!("failed to read hosted HTTP server local addr: {e}"))?;

    let server_id = uuid::Uuid::new_v4().to_string();
    let info = HostedDirServerInfo {
        server_id: server_id.clone(),
        server_name,
        directory: root_dir.display().to_string(),
        bind_host: bind_host.clone(),
        port: local_addr.port(),
        base_url: format!(
            "http://{}:{}/",
            render_host_for_url(&bind_host),
            local_addr.port()
        ),
        local_url: format!("http://127.0.0.1:{}/", local_addr.port()),
        auth: auth.clone(),
    };
    let state = HostedDirState { root_dir, auth };

    let app = Router::new()
        .route("/", get(serve_root).head(serve_root))
        .route("/{*path}", get(serve_path).head(serve_path))
        .with_state(state);
    let shutdown = CancellationToken::new();
    let shutdown_signal = shutdown.clone();
    let server_id_for_task = server_id.clone();
    let join_handle = tokio::spawn(async move {
        log::info!(
            "{LOG_PREFIX} serving hosted directory server_id={} addr={}",
            server_id_for_task,
            local_addr
        );
        if let Err(error) = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown_signal.cancelled().await;
            })
            .await
        {
            log::error!(
                "{LOG_PREFIX} hosted directory server_id={} exited with error: {}",
                server_id_for_task,
                error
            );
        } else {
            log::info!(
                "{LOG_PREFIX} hosted directory server_id={} stopped cleanly",
                server_id_for_task
            );
        }
    });

    let runtime = HostedDirRuntime {
        info: info.clone(),
        shutdown,
        join_handle,
    };

    let registry = registry();
    let mut servers = registry
        .servers
        .lock()
        .expect("hosted-dir registry poisoned");
    HostedDirRegistry::prune_finished_locked(&mut servers);
    if servers.contains_key(&server_id) {
        return Err(format!("hosted HTTP server id collision: {server_id}"));
    }
    if servers
        .values()
        .any(|runtime| runtime.info.bind_host == info.bind_host && runtime.info.port == info.port)
    {
        return Err(format!(
            "a hosted HTTP server is already registered on {}:{}",
            info.bind_host, info.port
        ));
    }
    servers.insert(server_id.clone(), runtime);

    log::info!(
        "{LOG_PREFIX} started hosted directory server_id={} dir={} url={}",
        server_id,
        info.directory,
        info.base_url
    );
    Ok(info)
}

pub fn list_hosted_dir_servers() -> Result<Vec<HostedDirServerInfo>, String> {
    let registry = registry();
    let mut servers = registry
        .servers
        .lock()
        .expect("hosted-dir registry poisoned");
    HostedDirRegistry::prune_finished_locked(&mut servers);
    let mut out = servers
        .values()
        .map(|runtime| runtime.info.clone())
        .collect::<Vec<_>>();
    out.sort_by(|a, b| a.server_id.cmp(&b.server_id));
    Ok(out)
}

pub fn get_hosted_dir_server(server_id: &str) -> Result<HostedDirServerInfo, String> {
    let registry = registry();
    let mut servers = registry
        .servers
        .lock()
        .expect("hosted-dir registry poisoned");
    HostedDirRegistry::prune_finished_locked(&mut servers);
    servers
        .get(server_id)
        .map(|runtime| runtime.info.clone())
        .ok_or_else(|| format!("hosted HTTP server not found: {server_id}"))
}

pub async fn stop_hosted_dir_server(server_id: &str) -> Result<HostedDirServerInfo, String> {
    let runtime = {
        let registry = registry();
        let mut servers = registry
            .servers
            .lock()
            .expect("hosted-dir registry poisoned");
        HostedDirRegistry::prune_finished_locked(&mut servers);
        servers
            .remove(server_id)
            .ok_or_else(|| format!("hosted HTTP server not found: {server_id}"))?
    };

    log::info!(
        "{LOG_PREFIX} stopping hosted directory server_id={} addr={}:{}",
        runtime.info.server_id,
        runtime.info.bind_host,
        runtime.info.port
    );
    runtime.shutdown.cancel();
    if let Err(error) = runtime.join_handle.await {
        log::warn!(
            "{LOG_PREFIX} hosted directory server join failed server_id={}: {}",
            runtime.info.server_id,
            error
        );
    }
    Ok(runtime.info)
}

pub async fn stop_all_hosted_dir_servers() {
    let runtimes = {
        let registry = registry();
        let mut servers = registry
            .servers
            .lock()
            .expect("hosted-dir registry poisoned");
        std::mem::take(&mut *servers)
    };
    for (server_id, runtime) in runtimes {
        log::info!("{LOG_PREFIX} shutdown hook stopping hosted server id={server_id}");
        runtime.shutdown.cancel();
        let _ = runtime.join_handle.await;
    }
}

fn register_shutdown_hook_once() {
    SHUTDOWN_HOOK_REGISTERED.get_or_init(|| {
        crate::core::shutdown::register(|| async {
            stop_all_hosted_dir_servers().await;
        });
    });
}

async fn serve_root(State(state): State<HostedDirState>, headers: HeaderMap) -> Response {
    serve_relative_path(state, headers, "").await
}

async fn serve_path(
    AxumPath(path): AxumPath<String>,
    State(state): State<HostedDirState>,
    headers: HeaderMap,
) -> Response {
    serve_relative_path(state, headers, &path).await
}

async fn serve_relative_path(state: HostedDirState, headers: HeaderMap, path: &str) -> Response {
    if let Err(response) = ensure_authorized(&headers, &state.auth) {
        return response;
    }

    let resolved = match resolve_request_path(&state.root_dir, path) {
        Ok(path) => path,
        Err(error) => {
            log::warn!("{LOG_PREFIX} rejected path='{}': {}", path, error);
            return (StatusCode::BAD_REQUEST, error).into_response();
        }
    };

    match tokio::fs::metadata(&resolved).await {
        Ok(metadata) if metadata.is_dir() => {
            serve_directory(&state.root_dir, &resolved, path).await
        }
        Ok(metadata) if metadata.is_file() => serve_file(&resolved).await,
        Ok(_) => (StatusCode::NOT_FOUND, "not found").into_response(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (StatusCode::NOT_FOUND, "not found").into_response()
        }
        Err(error) => {
            log::warn!(
                "{LOG_PREFIX} metadata failed path={} err={}",
                resolved.display(),
                error
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to read hosted directory entry",
            )
                .into_response()
        }
    }
}

async fn serve_file(path: &Path) -> Response {
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let mut response = Response::new(Body::from(bytes));
            *response.status_mut() = StatusCode::OK;
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(content_type_for_path(path)),
            );
            response
        }
        Err(error) => {
            log::warn!(
                "{LOG_PREFIX} read file failed path={} err={}",
                path.display(),
                error
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to read hosted file",
            )
                .into_response()
        }
    }
}

async fn serve_directory(root_dir: &Path, dir: &Path, requested_path: &str) -> Response {
    let index_path = dir.join("index.html");
    if tokio::fs::metadata(&index_path)
        .await
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
    {
        return serve_file(&index_path).await;
    }

    match tokio::fs::read_dir(dir).await {
        Ok(mut entries) => {
            let mut rows = Vec::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                let file_type = match entry.file_type().await {
                    Ok(file_type) => file_type,
                    Err(_) => continue,
                };
                let suffix = if file_type.is_dir() { "/" } else { "" };
                rows.push((name, suffix.to_string()));
            }
            rows.sort_by(|a, b| a.0.cmp(&b.0));

            let title = if requested_path.is_empty() {
                "/".to_string()
            } else {
                format!("/{}", requested_path.trim_matches('/'))
            };
            let mut html = String::from("<!doctype html><html><head><meta charset=\"utf-8\"><title>OpenHuman Directory Listing</title></head><body>");
            html.push_str(&format!(
                "<h1>Directory listing for {}</h1><ul>",
                escape_html(&title)
            ));
            if dir != root_dir {
                let parent_href = parent_href_for(requested_path);
                html.push_str(&format!("<li><a href=\"{}\">..</a></li>", parent_href));
            }
            for (name, suffix) in rows {
                let href = child_href_for(requested_path, &name, suffix.as_str());
                html.push_str(&format!(
                    "<li><a href=\"{}\">{}{}</a></li>",
                    href,
                    escape_html(&name),
                    suffix
                ));
            }
            html.push_str("</ul></body></html>");
            Html(html).into_response()
        }
        Err(error) => {
            log::warn!(
                "{LOG_PREFIX} read_dir failed path={} err={}",
                dir.display(),
                error
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to enumerate hosted directory",
            )
                .into_response()
        }
    }
}

fn ensure_authorized(headers: &HeaderMap, auth: &HostedDirAuth) -> Result<(), Response> {
    if !auth.enabled {
        return Ok(());
    }
    let Some(expected_user) = auth.username.as_deref() else {
        return Err(unauthorized_response());
    };
    let Some(expected_pass) = auth.password.as_deref() else {
        return Err(unauthorized_response());
    };
    let Some(header_value) = headers.get(header::AUTHORIZATION) else {
        return Err(unauthorized_response());
    };
    let Ok(auth_value) = header_value.to_str() else {
        return Err(unauthorized_response());
    };
    let Some(encoded) = auth_value.strip_prefix("Basic ") else {
        return Err(unauthorized_response());
    };
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded.as_bytes()) else {
        return Err(unauthorized_response());
    };
    let Ok(rendered) = String::from_utf8(decoded) else {
        return Err(unauthorized_response());
    };
    let Some((username, password)) = rendered.split_once(':') else {
        return Err(unauthorized_response());
    };
    if username == expected_user && password == expected_pass {
        Ok(())
    } else {
        Err(unauthorized_response())
    }
}

fn unauthorized_response() -> Response {
    let mut response = (StatusCode::UNAUTHORIZED, "basic auth required").into_response();
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Basic realm=\"OpenHuman Hosted Directory\""),
    );
    response
}

fn canonicalize_hosted_directory(input: &str) -> Result<PathBuf, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("directory must not be empty".to_string());
    }
    let path = PathBuf::from(trimmed);
    let canonical = std::fs::canonicalize(&path).map_err(|e| {
        format!(
            "failed to resolve hosted directory '{}': {e}",
            path.display()
        )
    })?;
    let metadata = std::fs::metadata(&canonical).map_err(|e| {
        format!(
            "failed to read hosted directory '{}': {e}",
            canonical.display()
        )
    })?;
    if !metadata.is_dir() {
        return Err(format!(
            "hosted path is not a directory: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn sanitize_bind_host(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("bind_host must not be empty".to_string());
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err("bind_host must be a hostname or IP address, not a path".to_string());
    }
    Ok(trimmed.to_string())
}

fn sanitize_optional_label(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(120).collect())
}

async fn resolve_default_auth_username() -> Option<String> {
    let config = match config::load_config_with_timeout().await {
        Ok(config) => config,
        Err(error) => {
            log::debug!("{LOG_PREFIX} default auth username config load failed: {error}");
            return fallback_env_username();
        }
    };

    match session_support::build_session_state(&config) {
        Ok(state) => state
            .user
            .as_ref()
            .and_then(resolve_default_auth_username_from_user_value)
            .or(state.user_id)
            .and_then(|value| sanitize_basic_auth_username(Some(value))),
        Err(error) => {
            log::debug!("{LOG_PREFIX} session state lookup failed for auth username: {error}");
            fallback_env_username()
        }
    }
}

fn fallback_env_username() -> Option<String> {
    sanitize_basic_auth_username(
        std::env::var("USER")
            .ok()
            .or_else(|| std::env::var("USERNAME").ok()),
    )
}

fn resolve_default_auth_username_from_user_value(user: &serde_json::Value) -> Option<String> {
    let object = user.as_object()?;
    [
        "username",
        "userName",
        "handle",
        "slug",
        "name",
        "displayName",
        "display_name",
        "email",
        "user_id",
        "userId",
        "id",
    ]
    .iter()
    .find_map(|key| object.get(*key).and_then(|value| value.as_str()))
    .map(str::to_string)
}

fn sanitize_basic_auth_username(value: Option<String>) -> Option<String> {
    let raw = value?;
    let mut out = String::new();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    for ch in trimmed.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '_' | '-' | '@' => out.push(ch),
            ' ' => out.push('-'),
            ':' => {}
            _ => {}
        }
        if out.len() >= 64 {
            break;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn generate_password() -> String {
    let mut bytes = [0u8; 18];
    rand::rng().fill(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn render_host_for_url(bind_host: &str) -> String {
    if bind_host.contains(':') && !bind_host.starts_with('[') {
        format!("[{bind_host}]")
    } else {
        bind_host.to_string()
    }
}

fn resolve_request_path(root_dir: &Path, requested_path: &str) -> Result<PathBuf, String> {
    let mut candidate = root_dir.to_path_buf();
    let trimmed = requested_path.trim_matches('/');
    if trimmed.is_empty() {
        return Ok(candidate);
    }

    for segment in trimmed.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        let decoded = urlencoding::decode(segment)
            .map_err(|e| format!("invalid URL path segment '{}': {e}", segment))?;
        if decoded.contains('/') || decoded.contains('\\') || decoded.contains('\0') {
            return Err(format!("invalid path segment '{}'", decoded));
        }
        let path_component = Path::new(decoded.as_ref());
        if path_component.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(format!("path traversal is not allowed: '{}'", decoded));
        }
        candidate.push(path_component);
    }

    if candidate.exists() {
        let canonical = std::fs::canonicalize(&candidate).map_err(|e| {
            format!(
                "failed to resolve requested path '{}': {e}",
                candidate.display()
            )
        })?;
        if !canonical.starts_with(root_dir) {
            return Err("requested path escapes hosted directory".to_string());
        }
        Ok(canonical)
    } else {
        Ok(candidate)
    }
}

fn parent_href_for(requested_path: &str) -> String {
    let trimmed = requested_path.trim_matches('/');
    let mut parts = trimmed
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let _ = parts.pop();
    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}/", parts.join("/"))
    }
}

fn child_href_for(requested_path: &str, child_name: &str, suffix: &str) -> String {
    let encoded = urlencoding::encode(child_name);
    let trimmed = requested_path.trim_matches('/');
    if trimmed.is_empty() {
        format!("/{encoded}{suffix}")
    } else {
        format!("/{trimmed}/{encoded}{suffix}")
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn content_type_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "application/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("txt") | Some("log") | Some("md") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("pdf") => "application/pdf",
        Some("csv") => "text/csv; charset=utf-8",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn sanitize_basic_auth_username_normalizes_and_limits() {
        let username = sanitize_basic_auth_username(Some(" Jane Doe:admin ".to_string())).unwrap();
        assert_eq!(username, "Jane-Doeadmin");
    }

    #[test]
    fn resolve_user_name_prefers_username_like_fields() {
        let user = serde_json::json!({
            "displayName": "Display Name",
            "username": "primary-user"
        });
        assert_eq!(
            resolve_default_auth_username_from_user_value(&user).as_deref(),
            Some("primary-user")
        );
    }

    #[test]
    fn resolve_request_path_rejects_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let err = resolve_request_path(&root, "../secret").unwrap_err();
        assert!(err.contains("path traversal"));
    }

    #[tokio::test]
    async fn start_serves_files_with_basic_auth() {
        let _guard = TEST_MUTEX.lock().unwrap();
        stop_all_hosted_dir_servers().await;

        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("hello.txt");
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "hello from hosted dir").unwrap();

        let server = start_hosted_dir_server(StartHostedDirParams {
            directory: tmp.path().display().to_string(),
            port: 0,
            bind_host: "127.0.0.1".to_string(),
            server_name: Some("test".to_string()),
            disable_auth: false,
            username: Some("tester".to_string()),
        })
        .await
        .unwrap();

        let client = reqwest::Client::builder().build().unwrap();
        let unauthorized = client
            .get(format!("{}hello.txt", server.local_url))
            .send()
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let auth_user = server.auth.username.clone().unwrap();
        let auth_pass = server.auth.password.clone().unwrap();
        let authorized = client
            .get(format!("{}hello.txt", server.local_url))
            .basic_auth(auth_user, Some(auth_pass))
            .send()
            .await
            .unwrap();
        assert_eq!(authorized.status(), StatusCode::OK);
        assert!(authorized
            .text()
            .await
            .unwrap()
            .contains("hello from hosted dir"));

        let listed = list_hosted_dir_servers().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].server_id, server.server_id);

        let stopped = stop_hosted_dir_server(&server.server_id).await.unwrap();
        assert_eq!(stopped.server_id, server.server_id);
        assert!(list_hosted_dir_servers().unwrap().is_empty());
    }
}
