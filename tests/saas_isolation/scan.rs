//! After-the-run audits: what every user's files hold, and which credential
//! carried which user's content to the backend.

use std::path::Path;

use super::mock_llm::Recorded;
use super::world::{owners_in, USERS};
use openhuman_core::storage::{DocumentStoreExt, Scope, StorageBackend};
use tinystoragedrivers_core::Query;

/// Every file under `dir`, recursively.
/// Directories or entries that cannot be read are recorded in `errors`: an
/// unreadable tree is an unaudited tree, not a clean one.
fn files(dir: &Path, out: &mut Vec<std::path::PathBuf>, errors: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            errors.push(format!("could not list {}: {e}", dir.display()));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                errors.push(format!("could not read an entry of {}: {e}", dir.display()));
                continue;
            }
        };
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => files(&path, out, errors),
            Ok(t) if t.is_file() => out.push(path),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => errors.push(format!("could not stat {}: {e}", path.display())),
        }
    }
}

/// Largest file the audit reads whole; a bigger one is itself a violation
/// rather than a silently unaudited file.
const MAX_AUDITED_BYTES: u64 = 512 * 1024 * 1024;

/// The user whose `users/<id>/` tree holds `path`, if any.
fn owner_of(root: &Path, path: &Path) -> Option<usize> {
    let rel = path.strip_prefix(root.join("users")).ok()?;
    let first = rel.components().next()?.as_os_str().to_str()?;
    USERS.iter().position(|u| *u == first)
}

/// Files that hold a canary they must not: another user's under a user's
/// tree, any user's outside every user's tree (the operator plane, shared
/// state, the child's `$HOME`). Returns the violations and, per user, how
/// many of their own files carry their canaries (proof the scan saw data).
pub fn scan_files(root: &Path, home: &Path, storage_db: &Path) -> (Vec<String>, [usize; 3]) {
    let mut violations = Vec::new();
    let mut own = [0usize; 3];
    for base in [root, home] {
        let mut all = Vec::new();
        files(base, &mut all, &mut violations);
        for path in all {
            if is_storage_db_file(&path, storage_db) {
                continue;
            }
            if base == home && path.starts_with(root) {
                continue;
            }
            // The child's own log sits beside the root; it is the operator's.
            if path.extension().is_some_and(|e| e == "log") && !path.starts_with(root) {
                continue;
            }
            let meta = match std::fs::metadata(&path) {
                Ok(meta) => meta,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    violations.push(format!("could not stat {}: {e}", path.display()));
                    continue;
                }
            };
            if meta.len() > MAX_AUDITED_BYTES {
                violations.push(format!(
                    "{} is {} bytes, too large to audit",
                    path.display(),
                    meta.len()
                ));
                continue;
            }
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    violations.push(format!("could not read {}: {e}", path.display()));
                    continue;
                }
            };
            let text = String::from_utf8_lossy(&bytes);
            // The process keyring (the dev file backend in this build) is one
            // store for the whole process: each profile's credential sits in
            // it under `<profile id>:…`. An entry is fine only under its own
            // owner's namespace.
            if path.file_name().is_some_and(|n| n == "dev-keychain.json") {
                violations.extend(keychain_violations(&path, &text));
                continue;
            }
            let found = owners_in(&text);
            if found.is_empty() {
                continue;
            }
            let owner = if base == root {
                owner_of(root, &path)
            } else {
                None
            };
            for user in found {
                if Some(user) == owner {
                    own[user] += 1;
                } else {
                    violations.push(format!(
                        "{} holds {}'s canary (owner: {})",
                        path.display(),
                        USERS[user],
                        owner.map(|o| USERS[o]).unwrap_or("nobody"),
                    ));
                }
            }
        }
    }
    (violations, own)
}

fn is_storage_db_file(path: &Path, db: &Path) -> bool {
    path == db || path == db.with_extension("db-wal") || path == db.with_extension("db-shm")
}

/// Audit the backend by its actual scope and record enumeration APIs. This
/// keeps the SaaS isolation check meaningful when a configured backend stores
/// every tenant in one database file.
pub async fn scan_storage(backend: &dyn StorageBackend) -> Result<[usize; 3], String> {
    let mut violations = Vec::new();
    let mut own = [0usize; 3];
    let scopes = backend
        .scopes("")
        .await
        .map_err(|error| error.to_string())?;
    for scope in scopes {
        let owner = USERS.iter().position(|user| {
            let tenant = openhuman_core::storage::scope_for_profile(user);
            scope == tenant || scope.as_str().starts_with(&format!("{}/", tenant.as_str()))
        });
        let scoped = backend
            .for_scope(&scope)
            .map_err(|error| format!("open scope {}: {error}", scope.as_str()))?;

        for collection in scoped
            .documents()
            .collections("")
            .await
            .map_err(|error| format!("list collections in {}: {error}", scope.as_str()))?
        {
            let docs = scoped
                .documents()
                .query_all(&collection, &Query::all())
                .await
                .map_err(|error| format!("read {collection} in {}: {error}", scope.as_str()))?;
            for doc in docs {
                let text = serde_json::to_string(&doc.doc)
                    .map_err(|error| format!("serialize {collection}: {error}"))?;
                if text.len() as u64 > MAX_AUDITED_BYTES {
                    return Err(format!(
                        "document in {collection} at {} exceeds audit size limit",
                        scope.as_str()
                    ));
                }
                record_scope_content(&scope, owner, &text, &mut own, &mut violations);
            }
        }

        for stream in scoped
            .streams()
            .streams("")
            .await
            .map_err(|error| format!("list streams in {}: {error}", scope.as_str()))?
        {
            let mut offset = 0;
            let mut total = 0_u64;
            loop {
                let entries = scoped
                    .streams()
                    .read_window(&stream, offset, 128)
                    .await
                    .map_err(|error| format!("read {stream} in {}: {error}", scope.as_str()))?;
                if entries.is_empty() {
                    break;
                }
                for entry in entries {
                    let text = serde_json::to_string(&entry.value)
                        .map_err(|error| format!("serialize stream {stream}: {error}"))?;
                    total = total.saturating_add(text.len() as u64);
                    if total > MAX_AUDITED_BYTES {
                        return Err(format!(
                            "stream {stream} in {} exceeds audit size limit",
                            scope.as_str()
                        ));
                    }
                    record_scope_content(&scope, owner, &text, &mut own, &mut violations);
                    offset = entry.offset.saturating_add(1);
                }
            }
        }

        let blobs = scoped
            .blobs()
            .list("")
            .await
            .map_err(|error| format!("list blobs in {}: {error}", scope.as_str()))?;
        for meta in blobs {
            if meta.len > MAX_AUDITED_BYTES {
                return Err(format!(
                    "blob {} in {} exceeds audit size limit",
                    meta.key,
                    scope.as_str()
                ));
            }
            let blob = scoped
                .blobs()
                .get(&meta.key)
                .await
                .map_err(|error| format!("read blob {} in {}: {error}", meta.key, scope.as_str()))?
                .ok_or_else(|| {
                    format!(
                        "blob {} in {} disappeared during audit",
                        meta.key,
                        scope.as_str()
                    )
                })?;
            let text = String::from_utf8_lossy(&blob.bytes);
            record_scope_content(&scope, owner, &text, &mut own, &mut violations);
        }
    }
    if !violations.is_empty() {
        return Err(violations.join("\n"));
    }
    Ok(own)
}

fn record_scope_content(
    scope: &Scope,
    owner: Option<usize>,
    text: &str,
    own: &mut [usize; 3],
    violations: &mut Vec<String>,
) {
    for user in owners_in(text) {
        if Some(user) == owner {
            own[user] += 1;
        } else {
            violations.push(format!(
                "scope {} holds {}'s canary (owner: {})",
                scope.as_str(),
                USERS[user],
                owner.map(|index| USERS[index]).unwrap_or("nobody"),
            ));
        }
    }
}

#[path = "scan_tests.rs"]
mod tests;

/// Keyring entries holding a user's canary outside that user's namespace.
fn keychain_violations(path: &Path, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let map: std::collections::HashMap<String, String> = match serde_json::from_str(text) {
        Ok(map) => map,
        Err(e) => {
            out.push(format!("{}: malformed keychain data: {e}", path.display()));
            return out;
        }
    };
    for (key, value) in &map {
        let namespace = key.split(':').next().unwrap_or_default();
        for user in owners_in(value) {
            if namespace != USERS[user] {
                out.push(format!(
                    "{}: entry `{key}` holds {}'s secret",
                    path.display(),
                    USERS[user]
                ));
            }
        }
    }
    out
}

/// Requests whose content and credential disagree: any request carrying a
/// user's canary must carry that user's credential and no one else's
/// content, and every inference request must carry exactly one user's
/// credential. Returns the violations and inference requests per user.
pub fn audit_backend(requests: &[Recorded]) -> (Vec<String>, [usize; 3]) {
    let mut violations = Vec::new();
    let mut per_user = [0usize; 3];
    for r in requests {
        let by = owners_in(&r.auth);
        let content = owners_in(&r.body);
        if by.len() > 1 {
            violations.push(format!("{} carried several credentials: {:?}", r.path, by));
            continue;
        }
        let by = by.first().copied();
        if r.is_inference() {
            match by {
                Some(user) => per_user[user] += 1,
                None => violations.push(format!(
                    "inference request without a user credential (content of {:?}, auth `{}`)",
                    content.iter().map(|&u| USERS[u]).collect::<Vec<_>>(),
                    redact(&r.auth),
                )),
            }
        }
        for user in content {
            if Some(user) != by {
                violations.push(format!(
                    "{} carried {}'s content under {}'s credential",
                    r.path,
                    USERS[user],
                    by.map(|b| USERS[b]).unwrap_or("no user"),
                ));
            }
        }
    }
    (violations, per_user)
}

fn redact(auth: &str) -> String {
    auth.chars().take(12).collect::<String>() + "…"
}
