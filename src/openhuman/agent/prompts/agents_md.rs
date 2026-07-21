//! AGENTS.md instruction loading — OpenHuman's analog of Claude Code's
//! `CLAUDE.md` / Codex's `AGENTS.md`.
//!
//! Loads configurable instruction text from `AGENTS.md` files at two layers:
//!
//! 1. **Global** — `<workspace_dir>/AGENTS.md`: the user's OpenHuman workspace
//!    (where `SOUL.md` / `USER.md` already live). Applies to every run.
//! 2. **Local / project** — `<effective action_dir>/AGENTS.md`: the folder the
//!    agent is actually operating in. For sub-agent runs with a
//!    `worktree_action_dir` override, that override dir is the local layer.
//!
//! The loaded strings are threaded into
//! [`crate::openhuman::context::prompt::PromptContext`] and rendered by
//! `AgentsInstructionsSection` **once at system-prompt build time** — never
//! re-read per turn — so the frozen system-prompt prefix / KV-cache contract is
//! preserved. Missing, unreadable, or empty files are silently skipped. The
//! renderer caps each layer at
//! [`crate::openhuman::context::prompt::BOOTSTRAP_MAX_CHARS`] with a
//! `[... truncated]` marker.

use super::types::BOOTSTRAP_MAX_CHARS;
use std::io::Read;
use std::path::Path;

/// The instruction file name loaded at each layer.
pub const AGENTS_MD_FILENAME: &str = "AGENTS.md";

/// Hard cap on the number of bytes read from an `AGENTS.md` before the
/// renderer's per-layer character cap ([`BOOTSTRAP_MAX_CHARS`]) applies.
///
/// `AGENTS.md` is an untrusted, user/project-controlled file. Reading it whole
/// via `read_to_string` would allocate an arbitrarily large buffer — a
/// multi-megabyte file could stall prompt construction or exhaust memory
/// **before** the renderer ever gets a chance to truncate. Bounding the read
/// here fixes that at the root. At UTF-8's worst case of 4 bytes per character
/// this budget still yields at least `BOOTSTRAP_MAX_CHARS` characters (plus
/// slack), so the renderer's cap + `[... truncated]` marker still fire exactly
/// as before for any file large enough to matter — the read bound is invisible
/// to well-formed files and only clamps pathological ones.
const MAX_AGENTS_MD_READ_BYTES: u64 = (BOOTSTRAP_MAX_CHARS as u64) * 4 + 1024;

/// Pre-loaded `AGENTS.md` contents for the global + local layers.
///
/// Both fields are `None` when the corresponding file is absent / empty /
/// unreadable, or (for `local`) when it deduplicates against the global layer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentsMdContent {
    /// `<workspace_dir>/AGENTS.md` — the workspace-global layer.
    pub global: Option<String>,
    /// `<effective action_dir>/AGENTS.md` — the project layer. `None` when the
    /// local dir resolves to the same path as the workspace dir (the file is
    /// then loaded once, into [`Self::global`]).
    pub local: Option<String>,
}

impl AgentsMdContent {
    /// Whether either layer carries content.
    pub fn is_empty(&self) -> bool {
        self.global.is_none() && self.local.is_none()
    }
}

/// Load a single `AGENTS.md` from `dir`.
///
/// Returns `Some(trimmed_content)` when the file exists, is readable, and is
/// non-empty after trimming. Missing / unreadable / empty files yield `None`
/// (silently skipped) — callers inject the result unconditionally without a
/// noisy placeholder. Never logs the file contents, only paths and sizes.
pub fn load_agents_md(dir: &Path) -> Option<String> {
    let path = dir.join(AGENTS_MD_FILENAME);
    // Bounded read: never slurp an arbitrarily large untrusted file whole into
    // memory. We open + `take(MAX_AGENTS_MD_READ_BYTES)` rather than
    // `read_to_string`, so a pathological multi-MB AGENTS.md can't stall prompt
    // construction or exhaust memory before the renderer's char cap applies.
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            match e.kind() {
                std::io::ErrorKind::NotFound => {
                    log::debug!("[agents_md] no AGENTS.md at {}", path.display());
                }
                _ => {
                    log::debug!("[agents_md] failed to open {}: {e}", path.display());
                }
            }
            return None;
        }
    };
    let mut buf = Vec::new();
    if let Err(e) = file.take(MAX_AGENTS_MD_READ_BYTES).read_to_end(&mut buf) {
        log::debug!("[agents_md] failed to read {}: {e}", path.display());
        return None;
    }
    let hit_read_bound = buf.len() as u64 >= MAX_AGENTS_MD_READ_BYTES;
    // `from_utf8_lossy` tolerates a multi-byte char clipped at the read bound by
    // substituting the replacement character for the trailing partial bytes.
    let content = String::from_utf8_lossy(&buf);
    let trimmed = content.trim();
    if trimmed.is_empty() {
        log::debug!("[agents_md] skipped empty {}", path.display());
        return None;
    }
    if hit_read_bound {
        log::debug!(
            "[agents_md] {} exceeds {} bytes; read bounded before render-time cap",
            path.display(),
            MAX_AGENTS_MD_READ_BYTES
        );
    }
    log::debug!(
        "[agents_md] loaded {} ({} chars)",
        path.display(),
        trimmed.chars().count()
    );
    Some(trimmed.to_string())
}

/// Load the global + local `AGENTS.md` layers given the workspace dir and the
/// effective local (action) dir.
///
/// Global renders first, local second (project instructions layered after).
/// **Dedupe:** when the two dirs resolve to the same path the file is loaded
/// once, into [`AgentsMdContent::global`], and `local` stays `None` so the same
/// instructions are never injected twice.
pub fn load_agents_md_layers(workspace_dir: &Path, local_dir: &Path) -> AgentsMdContent {
    let global = load_agents_md(workspace_dir);
    let same_dir = paths_equal(workspace_dir, local_dir);
    let local = if same_dir {
        log::debug!(
            "[agents_md] local dir {} == workspace dir; loaded once (deduped)",
            local_dir.display()
        );
        None
    } else {
        load_agents_md(local_dir)
    };
    AgentsMdContent { global, local }
}

/// Compare two directory paths for identity, canonicalizing when possible so
/// `./foo` and `foo` (or symlinked equivalents) dedupe. Falls back to literal
/// equality when canonicalization fails (e.g. a dir that does not exist yet).
fn paths_equal(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir().join(format!(
            "openhuman-agents-md-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn missing_file_returns_none() {
        let dir = tmp();
        assert_eq!(load_agents_md(&dir), None);
    }

    #[test]
    fn present_file_returns_trimmed_content() {
        let dir = tmp();
        fs::write(dir.join(AGENTS_MD_FILENAME), "\n\n  hello world  \n\n").unwrap();
        assert_eq!(load_agents_md(&dir), Some("hello world".to_string()));
    }

    #[test]
    fn empty_file_is_skipped() {
        let dir = tmp();
        fs::write(dir.join(AGENTS_MD_FILENAME), "   \n\t\n  ").unwrap();
        assert_eq!(load_agents_md(&dir), None);
    }

    #[test]
    fn layers_load_both_when_dirs_differ() {
        let ws = tmp();
        let local = tmp();
        fs::write(ws.join(AGENTS_MD_FILENAME), "global rules").unwrap();
        fs::write(local.join(AGENTS_MD_FILENAME), "project rules").unwrap();
        let content = load_agents_md_layers(&ws, &local);
        assert_eq!(content.global.as_deref(), Some("global rules"));
        assert_eq!(content.local.as_deref(), Some("project rules"));
        assert!(!content.is_empty());
    }

    #[test]
    fn same_dir_dedupes_to_global_only() {
        let ws = tmp();
        fs::write(ws.join(AGENTS_MD_FILENAME), "shared rules").unwrap();
        // Pass the same dir as both workspace and local.
        let content = load_agents_md_layers(&ws, &ws);
        assert_eq!(content.global.as_deref(), Some("shared rules"));
        assert_eq!(content.local, None, "same-dir local must dedupe to None");
    }

    #[test]
    fn same_dir_dedupes_even_with_non_canonical_paths() {
        let ws = tmp();
        fs::write(ws.join(AGENTS_MD_FILENAME), "shared rules").unwrap();
        // A `./` prefixed variant must canonicalize to the same path.
        let dotted = ws.join(".").join("");
        let content = load_agents_md_layers(&ws, &dotted);
        assert_eq!(content.local, None, "canonicalized same-dir must dedupe");
    }

    #[test]
    fn oversized_file_is_bounded_at_read_time() {
        let dir = tmp();
        // A file far larger than the read bound must not be slurped whole:
        // the loader returns bounded content (never the full body) so a
        // pathological AGENTS.md can't exhaust memory before rendering.
        let oversized = "a".repeat((MAX_AGENTS_MD_READ_BYTES as usize) * 3);
        fs::write(dir.join(AGENTS_MD_FILENAME), &oversized).unwrap();
        let loaded = load_agents_md(&dir).expect("non-empty file loads");
        assert!(
            (loaded.len() as u64) <= MAX_AGENTS_MD_READ_BYTES,
            "loader must bound the read to MAX_AGENTS_MD_READ_BYTES, got {} bytes",
            loaded.len()
        );
        assert!(
            loaded.len() < oversized.len(),
            "bounded content must be shorter than the on-disk file"
        );
    }

    #[test]
    fn both_missing_is_empty() {
        let ws = tmp();
        let local = tmp();
        let content = load_agents_md_layers(&ws, &local);
        assert!(content.is_empty());
    }
}
