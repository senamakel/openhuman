//! Walk a vault's root directory and ingest changed/new files into memory.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::Utc;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::openhuman::config::Config;
use crate::openhuman::memory::ops::{doc_delete, doc_ingest, DeleteDocParams, IngestDocParams};

use super::store;
use super::types::{Vault, VaultFile, VaultFileStatus, VaultSyncReport};

/// Built-in exclude patterns we never traverse. Kept tiny and obvious.
const BUILTIN_EXCLUDE_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".cache",
    ".venv",
    "__pycache__",
    ".DS_Store",
];

/// Max single-file size we read into memory for ingestion (5 MiB).
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// File extensions we currently extract as plain UTF-8.
pub fn supported_extension(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "md" | "mdx"
            | "txt"
            | "rst"
            | "json"
            | "yaml"
            | "yml"
            | "toml"
            | "csv"
            | "html"
            | "htm"
            | "rs"
            | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "py"
            | "go"
            | "java"
            | "rb"
            | "php"
            | "sh"
            | "bash"
            | "zsh"
            | "sql"
            | "css"
            | "scss"
            | "swift"
            | "kt"
            | "c"
            | "cc"
            | "cpp"
            | "h"
            | "hpp"
            | "log"
    )
}

/// Walk `vault.root_path`, ingest new/changed files into memory, delete docs
/// whose source files vanished, and record per-file state in the ledger.
pub async fn sync_vault(config: &Config, vault: &Vault) -> VaultSyncReport {
    let started = Utc::now();
    let mut report = VaultSyncReport {
        vault_id: vault.id.clone(),
        ..Default::default()
    };

    let root = PathBuf::from(&vault.root_path);
    if !root.is_dir() {
        report
            .errors
            .push(format!("root_path is not a directory: {}", vault.root_path));
        report.duration_ms = (Utc::now() - started).num_milliseconds();
        return report;
    }

    // Snapshot existing ledger so we can compute deletions at the end.
    let existing = match store::list_files(config, &vault.id) {
        Ok(rows) => rows,
        Err(err) => {
            report.errors.push(format!("ledger read failed: {err}"));
            return report;
        }
    };
    let mut seen: HashSet<String> = HashSet::new();
    let by_path: std::collections::HashMap<String, VaultFile> = existing
        .iter()
        .map(|f| (f.rel_path.clone(), f.clone()))
        .collect();

    let user_excludes: Vec<String> = vault
        .exclude_globs
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();

    for entry in WalkDir::new(&root).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                report.errors.push(format!("walk error: {err}"));
                continue;
            }
        };

        // Skip builtin-excluded directories at any depth.
        if entry.file_type().is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                if BUILTIN_EXCLUDE_DIRS.contains(&name) {
                    // skip the subtree by checking parent paths later
                    continue;
                }
            }
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let rel_path = match path.strip_prefix(&root) {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => continue,
        };

        if path_is_inside_excluded_dir(path, &root) {
            continue;
        }
        if user_excludes
            .iter()
            .any(|pat| rel_path.to_ascii_lowercase().contains(pat))
        {
            continue;
        }

        report.scanned += 1;

        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if !supported_extension(&ext) {
            report.skipped_unsupported += 1;
            seen.insert(rel_path.clone()); // keep ledger entries pruned
            continue;
        }

        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(err) => {
                report.failed += 1;
                report
                    .errors
                    .push(format!("{rel_path}: stat failed: {err}"));
                continue;
            }
        };
        if metadata.len() > MAX_FILE_BYTES {
            report.skipped_unsupported += 1;
            report.errors.push(format!(
                "{rel_path}: skipped — {} bytes exceeds {}",
                metadata.len(),
                MAX_FILE_BYTES
            ));
            seen.insert(rel_path.clone());
            continue;
        }

        let mtime_ms = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(err) => {
                report.failed += 1;
                report
                    .errors
                    .push(format!("{rel_path}: read failed: {err}"));
                continue;
            }
        };
        let hash = sha256_hex(&content);

        seen.insert(rel_path.clone());

        // Dedup: if hash and mtime unchanged, skip ingest.
        if let Some(prev) = by_path.get(&rel_path) {
            if prev.status == VaultFileStatus::Ok
                && prev.content_hash == hash
                && prev.mtime_ms == mtime_ms
            {
                report.unchanged += 1;
                continue;
            }
        }

        let key = rel_path.clone();
        let title = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&rel_path)
            .to_string();
        let ingest_params = IngestDocParams {
            namespace: vault.namespace.clone(),
            key,
            title,
            content,
            source_type: "vault".to_string(),
            priority: "medium".to_string(),
            tags: vec![format!("vault:{}", vault.id), format!("ext:{ext}")],
            metadata: serde_json::json!({
                "vault_id": vault.id,
                "rel_path": rel_path,
                "mtime_ms": mtime_ms,
                "bytes": metadata.len(),
            }),
            category: "user".to_string(),
            session_id: None,
            document_id: by_path.get(&rel_path).map(|p| p.document_id.clone()),
            config: None,
        };

        match doc_ingest(ingest_params).await {
            Ok(outcome) => {
                let document_id = outcome.value.document_id.clone();
                let file = VaultFile {
                    vault_id: vault.id.clone(),
                    rel_path: rel_path.clone(),
                    document_id,
                    content_hash: hash,
                    mtime_ms,
                    bytes: metadata.len(),
                    ingested_at: Utc::now(),
                    status: VaultFileStatus::Ok,
                };
                if let Err(err) = store::upsert_file(config, &file) {
                    report
                        .errors
                        .push(format!("{rel_path}: ledger write failed: {err}"));
                }
                report.ingested += 1;
            }
            Err(err) => {
                report.failed += 1;
                report
                    .errors
                    .push(format!("{rel_path}: ingest failed: {err}"));
            }
        }
    }

    // Anything in ledger we didn't see this pass is gone — delete it.
    for (path, prev) in by_path.iter() {
        if seen.contains(path) {
            continue;
        }
        let _ = doc_delete(DeleteDocParams {
            namespace: vault.namespace.clone(),
            document_id: prev.document_id.clone(),
        })
        .await;
        let _ = store::delete_file(config, &vault.id, path);
        report.removed += 1;
    }

    let _ = store::touch_last_synced(config, &vault.id, Utc::now());
    report.duration_ms = (Utc::now() - started).num_milliseconds();
    report
}

fn path_is_inside_excluded_dir(path: &Path, root: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    for component in rel.components() {
        if let std::path::Component::Normal(os) = component {
            if let Some(name) = os.to_str() {
                if BUILTIN_EXCLUDE_DIRS.contains(&name) {
                    return true;
                }
            }
        }
    }
    false
}

fn sha256_hex(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest.iter() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
