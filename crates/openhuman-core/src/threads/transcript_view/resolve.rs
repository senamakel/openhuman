//! Which files back a thread's transcript view, and in what order.
//!
//! Two things make this more than "every root whose `_meta.thread_id`
//! matches":
//!
//! - **Session generations.** A compaction seals generation `n` and opens
//!   `{stem}.g{n+1}`, which inherits `_meta.created` and opens with the
//!   retained message set rewritten. Ordering by `created` then path put
//!   `X.g1` before `X` (and `.g10` before `.g2`), and concatenating every
//!   generation rendered the retained rows twice. Generations are ordered by
//!   the tinyagents `session_chain` instead, and [`drop_retained_rows`] removes
//!   the rewritten prefix when a successor is projected.
//! - **Adopted legacy roots.** A pre-identity conversation's timestamped roots
//!   are folded into the session file on first resume and left untouched on
//!   disk, so once a session file exists they are duplicates of its head.
//!
//! Sub-agent files are discovered by `_meta.thread_id` as well as by the
//! legacy `{root_stem}__` prefix: a sub-agent's stem chains the parent's
//! *session key* (`{unix}_{agent}`), which for a session-identity thread is not
//! the root file's stem (`{thread}.{agent}` with digests), so the prefix alone
//! found none of them.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use tinyagents_session::transcript::{
    self, DisplayMessage, DisplayRecord, FileTranscriptLocator, SessionRef, TranscriptLocator,
};

const LOG_PREFIX: &str = "[threads][transcript][resolve]";

/// The `_meta` header fields resolution needs, read from a file's first line
/// only (the full reader parses the whole file).
#[derive(Debug, Default, Clone)]
pub(super) struct HeadMeta {
    pub(super) thread_id: Option<String>,
    pub(super) agent_id: Option<String>,
    pub(super) session_id: Option<String>,
}

/// Read the first-line `_meta` header of a transcript. `None` when the file
/// cannot be read or does not start with a meta line.
pub(super) fn read_head_meta(path: &Path) -> Option<HeadMeta> {
    let file = fs::File::open(path).ok()?;
    let mut first = String::new();
    BufReader::new(file).read_line(&mut first).ok()?;
    let value: serde_json::Value = serde_json::from_str(first.trim()).ok()?;
    let meta = value.get("_meta")?;
    let field = |key: &str| {
        meta.get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    Some(HeadMeta {
        thread_id: field("thread_id"),
        agent_id: field("agent_id"),
        session_id: field("session_id"),
    })
}

/// Resolve the root generations and sub-agent siblings backing `thread_id`.
/// `None` when the thread has no root transcript yet.
pub fn resolve_files(
    workspace_dir: &Path,
    thread_id: &str,
) -> Option<(Vec<PathBuf>, Vec<PathBuf>)> {
    let found = transcript::find_root_transcripts_for_thread(workspace_dir, thread_id);
    if found.is_empty() {
        return None;
    }
    let raw_dir = found[0].parent()?.to_path_buf();
    let roots = order_root_files(workspace_dir, thread_id, found);
    let subs = discover_subagent_files(&raw_dir, thread_id, &roots);
    log::debug!(
        "{LOG_PREFIX} thread={thread_id} roots={} subagent_files={}",
        roots.len(),
        subs.len()
    );
    Some((roots, subs))
}

fn file_name(path: &Path) -> Option<&std::ffi::OsStr> {
    path.file_name()
}

/// Order the thread's roots: each session's generations oldest-first (from
/// `session_chain`), legacy (pre-identity) roots dropped once a session file
/// exists. A thread with no session file keeps the scan's order unchanged.
fn order_root_files(workspace_dir: &Path, thread_id: &str, roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let metas: Vec<(PathBuf, HeadMeta)> = roots
        .into_iter()
        .map(|path| {
            let meta = read_head_meta(&path).unwrap_or_default();
            (path, meta)
        })
        .collect();
    if !metas.iter().any(|(_, meta)| meta.session_id.is_some()) {
        return metas.into_iter().map(|(path, _)| path).collect();
    }

    let locator = FileTranscriptLocator::new(workspace_dir);
    let mut seen: HashSet<std::ffi::OsString> = HashSet::new();
    let mut ordered = Vec::new();
    let mut dropped_legacy = 0usize;
    for (path, meta) in &metas {
        if meta.session_id.is_none() {
            dropped_legacy += 1;
            continue;
        }
        let Some(name) = file_name(path) else {
            continue;
        };
        if seen.contains(name) {
            continue;
        }
        let chain: Vec<PathBuf> = meta
            .agent_id
            .as_deref()
            .map(|agent_id| {
                let session = SessionRef::scoped(thread_id, agent_id);
                locator
                    .session_chain(&session)
                    .iter()
                    .filter_map(|generation| {
                        transcript::resolve_keyed_transcript_path(
                            workspace_dir,
                            &transcript::session_stem(generation),
                        )
                        .ok()
                    })
                    .collect()
            })
            .unwrap_or_default();
        if chain.iter().any(|link| file_name(link) == Some(name)) {
            for link in chain {
                if let Some(link_name) = file_name(&link) {
                    if seen.insert(link_name.to_os_string()) {
                        ordered.push(link);
                    }
                }
            }
        } else {
            // A session file this thread's own identity does not derive (a
            // different key scheme): keep it where the scan put it.
            seen.insert(name.to_os_string());
            ordered.push(path.clone());
        }
    }
    if dropped_legacy > 0 {
        log::debug!(
            "{LOG_PREFIX} thread={thread_id} dropped {dropped_legacy} adopted legacy root(s) \
             in favour of the session chain"
        );
    }
    ordered
}

/// Every sub-agent (`__`) transcript of this thread in `raw_dir`: those whose
/// stem extends a root stem (legacy layout), plus those whose `_meta.thread_id`
/// names the thread (session-identity layout). Sorted by path.
fn discover_subagent_files(raw_dir: &Path, thread_id: &str, roots: &[PathBuf]) -> Vec<PathBuf> {
    let prefixes: Vec<String> = roots
        .iter()
        .filter_map(|root| root.file_stem().and_then(|stem| stem.to_str()))
        .map(|stem| format!("{stem}__"))
        .collect();
    let entries = match fs::read_dir(raw_dir) {
        Ok(entries) => entries,
        Err(error) => {
            log::debug!(
                "{LOG_PREFIX} subagent discovery read_dir failed dir={} error={error}",
                raw_dir.display()
            );
            return Vec::new();
        }
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
        .filter(|path| {
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                return false;
            };
            if !stem.contains("__") {
                return false;
            }
            prefixes.iter().any(|prefix| stem.starts_with(prefix))
                || read_head_meta(path)
                    .and_then(|meta| meta.thread_id)
                    .is_some_and(|id| id == thread_id)
        })
        .collect();
    paths.sort();
    paths
}

/// Identity of one message row for cross-generation de-duplication.
type RowKey = (String, String, Option<String>, Option<String>);

fn row_key(msg: &DisplayMessage) -> RowKey {
    (
        msg.message.role.clone(),
        msg.message.content.clone(),
        msg.message.id.clone(),
        msg.request_id.clone(),
    )
}

/// Multiset of the message rows of one generation, for [`drop_retained_rows`].
pub(super) fn generation_rows(records: &[DisplayRecord]) -> HashMap<RowKey, usize> {
    let mut rows = HashMap::new();
    for record in records {
        if let DisplayRecord::Message(msg) = record {
            *rows.entry(row_key(msg)).or_insert(0) += 1;
        }
    }
    rows
}

/// Split a successor generation's records into `(new records, retained rows)`.
///
/// A successor opens with the retained set rewritten verbatim (same role,
/// content, id and — because retained rows keep their own correlation id —
/// the same `request_id`). Each such row consumes one occurrence from the
/// predecessor's multiset, so a genuinely repeated row beyond what the
/// predecessor held still renders.
pub(super) fn drop_retained_rows(
    records: &[DisplayRecord],
    mut predecessor: HashMap<RowKey, usize>,
) -> (Vec<DisplayRecord>, Vec<DisplayMessage>) {
    let mut kept = Vec::with_capacity(records.len());
    let mut retained = Vec::new();
    for record in records {
        if let DisplayRecord::Message(msg) = record {
            if let Some(count) = predecessor.get_mut(&row_key(msg)) {
                if *count > 0 {
                    *count -= 1;
                    retained.push((**msg).clone());
                    continue;
                }
            }
        }
        kept.push(record.clone());
    }
    (kept, retained)
}
