//! The host's record of the sync runs it drives, and how a finished run is
//! reported (openhuman#6257).
//!
//! # Why the host keeps a log of its own
//!
//! Brain › Sync › Sync History reads the memory driver's audit log, and the
//! driver writes that log only for the runs it schedules itself: the periodic
//! folder, GitHub, RSS and web loop. Every run this host drives left no row.
//! The Sources Sync button and Apply all reach the driver through
//! `MemorySourceSync::run_source_sync`, which does not audit (tinymemory's own
//! bus test pins that), and every Composio run is read by the connector module
//! and ingested here, so the driver never sees it as a run at all. A user
//! checking a sync saw nothing happen. Those runs are recorded here instead, in
//! the driver's row shape, and the history RPCs read both logs.
//!
//! The driver's file is not appended to from here. Its path, its format pin and
//! its reader sit behind `MemorySourceSync`; a remote or null driver has no such
//! file; and a writer the driver does not know about would be a second owner of
//! its format.
//!
//! # Bounded
//!
//! One line per run, written with a single `write_all` under a process-wide
//! lock, so two runs finishing together cannot interleave half-lines. Past
//! [`COMPACT_AT_BYTES`] an append rewrites the file to its newest [`KEEP_ROWS`]
//! through a temporary file and a rename, so the periodic Composio loop that
//! feeds it cannot grow it without bound. It rewrites again only once the file
//! has doubled since, so long rows cannot turn every append into a rewrite (see
//! [`compaction_trigger`]).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use chrono::{DateTime, Utc};

use crate::config::Config;
use crate::memory::api::provider::sync::SyncAuditEntry;

/// The host-owned directory under the workspace this log lives in, beside the
/// other JSONL files the host writes.
const STATE_DIR: &str = "state";

/// The log's file name — and the only name error messages use: the full path
/// carries the user's home directory, which has no place in a reported error.
const LOG_FILE: &str = "memory_sync_runs.jsonl";

/// Rows a compaction keeps, and the most a read returns.
pub(crate) const KEEP_ROWS: usize = 1_000;

/// Size past which an append compacts the file. A row is a few hundred bytes,
/// so roughly half as many rows again as [`KEEP_ROWS`] accumulate between
/// rewrites — rare enough that an append stays one write.
const COMPACT_AT_BYTES: u64 = 512 * 1024;

/// Serialises appends, compactions and reads of the log within the process,
/// and holds each log's size right after this process last compacted it.
static LOG_STATE: Mutex<Vec<(PathBuf, u64)>> = Mutex::new(Vec::new());

fn log_path(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join(STATE_DIR).join(LOG_FILE)
}

/// One run this host drove, as the host knows it.
///
/// Converted to the driver's [`SyncAuditEntry`] so a history row reads the same
/// whichever log it came from. Two halves of that row stay empty on purpose:
/// the host prices nothing — the inference price is the driver's to state, see
/// `rpc::cost_reporting` — and it has no tree verdict to report, because
/// `SyncRunOutcome` does not carry one.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HostRun {
    pub source_id: String,
    pub source_kind: String,
    pub scope: String,
    pub items: u64,
    pub duration_ms: u64,
    /// Why the run failed; `None` for a run that completed.
    pub error: Option<String>,
    /// Provider actions the run called, when the run reported them.
    pub actions_called: u32,
    /// What the provider charged for those actions, in USD.
    pub provider_cost_usd: f64,
}

impl HostRun {
    /// The audit row for this run, stamped as finishing at `finished_at`.
    pub(crate) fn into_entry(self, finished_at: DateTime<Utc>) -> SyncAuditEntry {
        SyncAuditEntry {
            timestamp: finished_at,
            source_id: self.source_id,
            source_kind: self.source_kind,
            scope: self.scope,
            // Saturating: the row's counter is `u32` on the wire, and a run
            // large enough to pass it is still a run, not a panic.
            items_fetched: u32::try_from(self.items).unwrap_or(u32::MAX),
            batches: 0,
            input_tokens: 0,
            output_tokens: 0,
            estimated_cost_usd: 0.0,
            composio_actions_called: self.actions_called,
            composio_cost_usd: self.provider_cost_usd,
            actual_charged_usd: None,
            duration_ms: self.duration_ms,
            success: self.error.is_none(),
            error: self.error,
            tree_ingest_failures: 0,
            tree_error: None,
        }
    }
}

/// Milliseconds since `started`, saturating.
pub(crate) fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// The `completed` stage's detail string.
///
/// A parse contract, not prose: the Sources UI extracts the count with
/// `/ingested\s+(\d+)\s+item/i` and falls back to a generic "up to date"
/// when it cannot (#3295). Pinned by a unit test against that exact pattern.
///
/// `note` is what the run said about stopping short — today's request budget
/// being spent, above all. It rides *after* the count, never inside it, so the
/// regex keeps matching and everything past the count is free text the UI can
/// show. Without it a spent budget wrote zero items and read back as "Up to
/// date", the opposite of what happened.
///
/// Owned here, beside the history row, because every kind of run reports its
/// finish through it: the Composio loop and the driver-backed Sync button both
/// (openhuman#6257).
pub(crate) fn completed_sync_detail(
    total_written: u64,
    more_pending: bool,
    note: Option<&str>,
) -> String {
    let mut detail = if more_pending {
        format!("ingested {total_written} item(s), more pending — Sync again to continue")
    } else {
        format!("ingested {total_written} item(s)")
    };
    if let Some(note) = note.map(str::trim).filter(|note| !note.is_empty()) {
        detail.push_str("; ");
        detail.push_str(note);
    }
    detail
}

/// Record one finished run in `config`'s workspace.
///
/// Never fails the run it describes: the run's items are already committed,
/// and a row that could not be written is a gap in the history, not a failed
/// sync. It is still an unexpected I/O failure on the host's own state
/// directory, so it is reported, not only logged.
pub(crate) fn record_run(config: &Config, entry: SyncAuditEntry) {
    match append_run(&config.workspace_dir, &entry) {
        Ok(()) => tracing::debug!(
            source_id = %entry.source_id,
            source_kind = %entry.source_kind,
            success = entry.success,
            items = entry.items_fetched,
            "[memory_sources:history] sync run recorded"
        ),
        Err(error) => {
            tracing::warn!(
                source_id = %entry.source_id,
                error = %error,
                "[memory_sources:history] could not record a sync run; the run itself is unaffected"
            );
            crate::core::observability::report_error(
                error.as_str(),
                "memory_sources",
                "record_sync_run",
                &[("source_kind", entry.source_kind.as_str())],
            );
        }
    }
}

/// Append `entry` to the log under `workspace_dir`, compacting past the size
/// ceiling.
///
/// # Errors
///
/// When the state directory cannot be created or the row cannot be written. A
/// compaction that fails after the row landed is not an error here — the row
/// is recorded — and is reported separately.
pub(crate) fn append_run(workspace_dir: &Path, entry: &SyncAuditEntry) -> Result<(), String> {
    append_run_compacting_at(workspace_dir, entry, COMPACT_AT_BYTES)
}

fn append_run_compacting_at(
    workspace_dir: &Path,
    entry: &SyncAuditEntry,
    compact_at_bytes: u64,
) -> Result<(), String> {
    let mut line = serde_json::to_vec(entry)
        .map_err(|error| format!("encode a sync run for {LOG_FILE}: {error}"))?;
    line.push(b'\n');

    let path = log_path(workspace_dir);
    let mut compacted = LOG_STATE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)
            .map_err(|error| format!("create the directory for {LOG_FILE}: {error}"))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("open {LOG_FILE}: {error}"))?;
    // One buffer, one write: on an append handle the whole line lands at once.
    file.write_all(&line)
        .map_err(|error| format!("append to {LOG_FILE}: {error}"))?;

    let trigger = compaction_trigger(&compacted, &path, compact_at_bytes);
    match file.metadata() {
        Ok(metadata) if metadata.len() > trigger => {
            drop(file);
            match compact(&path) {
                Ok(size) => remember_compaction(&mut compacted, &path, size),
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "[memory_sources:history] {LOG_FILE} compaction failed; the next append retries it"
                    );
                    crate::core::observability::report_error(
                        error.as_str(),
                        "memory_sources",
                        "compact_sync_run_log",
                        &[],
                    );
                }
            }
        }
        Ok(_) => {}
        Err(error) => tracing::warn!(
            error = %error,
            "[memory_sources:history] could not size {LOG_FILE}; compaction skipped this time"
        ),
    }
    Ok(())
}

/// The size past which an append to `path` compacts it: the ceiling, or twice
/// the size this process last compacted the file to, whichever is larger.
///
/// A failed run's row carries its error text, so the newest [`KEEP_ROWS`] can
/// outweigh the ceiling on their own. A ceiling-only rule would then rewrite
/// the whole file on every append; the doubling keeps a rewrite to one per
/// file's worth of new rows.
fn compaction_trigger(compacted: &[(PathBuf, u64)], path: &Path, compact_at_bytes: u64) -> u64 {
    compacted
        .iter()
        .find(|(logged, _)| logged.as_path() == path)
        .map_or(compact_at_bytes, |(_, size)| {
            compact_at_bytes.max(size.saturating_mul(2))
        })
}

/// Note that `path` was just compacted to `size` bytes.
fn remember_compaction(compacted: &mut Vec<(PathBuf, u64)>, path: &Path, size: u64) {
    match compacted
        .iter_mut()
        .find(|(logged, _)| logged.as_path() == path)
    {
        Some((_, last)) => *last = size,
        None => compacted.push((path.to_path_buf(), size)),
    }
}

/// Rewrite the log to its newest [`KEEP_ROWS`] parseable lines, answering the
/// rewritten file's size.
///
/// Through a temporary file and a rename, so a crash mid-compaction leaves the
/// old file whole rather than a truncated one. The caller holds [`LOG_STATE`].
fn compact(path: &Path) -> Result<u64, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("read {LOG_FILE} to compact it: {error}"))?;
    let rows: Vec<&str> = content
        .lines()
        .filter(|line| serde_json::from_str::<SyncAuditEntry>(line).is_ok())
        .collect();
    let kept = &rows[rows.len().saturating_sub(KEEP_ROWS)..];
    let mut rewritten = kept.join("\n");
    rewritten.push('\n');
    let size = u64::try_from(rewritten.len()).unwrap_or(u64::MAX);

    let staging = path.with_extension("jsonl.tmp");
    std::fs::write(&staging, rewritten)
        .map_err(|error| format!("write the compacted {LOG_FILE}: {error}"))?;
    std::fs::rename(&staging, path)
        .map_err(|error| format!("replace {LOG_FILE} with its compacted copy: {error}"))?;
    tracing::debug!(
        kept = kept.len(),
        dropped = rows.len() - kept.len(),
        "[memory_sources:history] {LOG_FILE} compacted"
    );
    Ok(size)
}

/// The runs recorded under `workspace_dir`, newest first, at most `limit`.
///
/// A missing file is an empty log. A line that does not parse is skipped with
/// a warning: the file is append-only across crashes, so a torn last line must
/// not hide every row before it.
///
/// # Errors
///
/// Only when the file exists and cannot be read. An unreadable log is not
/// reported as "no runs", the one answer a caller cannot tell from the truth.
pub(crate) fn read_runs(workspace_dir: &Path, limit: usize) -> Result<Vec<SyncAuditEntry>, String> {
    let path = log_path(workspace_dir);
    let content = {
        let _guard = LOG_STATE.lock().unwrap_or_else(PoisonError::into_inner);
        match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(format!("read {LOG_FILE}: {error}")),
        }
    };

    let mut malformed = 0usize;
    let mut rows: Vec<SyncAuditEntry> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| match serde_json::from_str(line) {
            Ok(row) => Some(row),
            Err(_) => {
                malformed += 1;
                None
            }
        })
        .collect();
    if malformed > 0 {
        tracing::warn!(
            malformed,
            "[memory_sources:history] skipped malformed lines in {LOG_FILE}"
        );
    }
    rows.reverse();
    rows.truncate(limit);
    Ok(rows)
}

/// Two newest-first logs as one newest-first list of at most `cap` rows.
///
/// A stable sort, so rows stamped with the same instant keep the driver's rows
/// ahead of the host's, as passed.
pub(crate) fn merge_newest_first(
    mut driver: Vec<SyncAuditEntry>,
    host: Vec<SyncAuditEntry>,
    cap: usize,
) -> Vec<SyncAuditEntry> {
    driver.extend(host);
    driver.sort_by_key(|entry| std::cmp::Reverse(entry.timestamp));
    driver.truncate(cap);
    driver
}

#[cfg(test)]
#[path = "run_history_tests.rs"]
mod tests;
