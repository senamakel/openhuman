//! Per-profile "home" materialization — hermes-agent-style agent homes.
//!
//! Each profile can own an identity file (`SOUL.md`, re-read on every prompt
//! build), a curated per-profile memory file (`MEMORY.md`), an optional
//! dedicated memory subtree, and an optional agent-writable workspace. The two
//! roots are deliberately split (see [`crate::openhuman::config`]):
//!
//! ```text
//! <workspace>/personalities/<id>/SOUL.md      identity (hot-read each prompt)
//! <workspace>/personalities/<id>/MEMORY.md    curated per-profile memory
//! <action_dir>/profiles/<id>/                 agent-writable workspace
//! ```
//!
//! Identity + memory files live under `workspace_dir` (core-managed reads only —
//! the agent's write tools cannot reach there, enforced fail-closed by
//! `is_workspace_internal_path`). The agent's *writable* working dir lives under
//! `action_dir`, which acting tools (shell/file/git) are allowed to touch.

use std::io;
use std::path::{Path, PathBuf};

use super::types::AgentProfile;

/// Directory holding a profile's core-managed identity + memory files:
/// `<workspace>/personalities/<id>/`.
pub fn profile_home(workspace_dir: &Path, profile_id: &str) -> PathBuf {
    workspace_dir.join("personalities").join(profile_id)
}

/// The agent-writable per-profile workspace: `<action_dir>/profiles/<id>/`.
///
/// Because this lives under `action_dir`, the `SecurityPolicy` already permits
/// acting tools to read/write here — no hardening change is needed.
pub fn profile_action_workspace(action_dir: &Path, profile_id: &str) -> PathBuf {
    action_dir.join("profiles").join(profile_id)
}

/// Validate a profile id against the hermes-style name grammar
/// `^[a-z0-9][a-z0-9_-]{0,63}$`.
///
/// Only enforced when creating a *new* custom profile — legacy / built-in ids
/// (which may not satisfy the grammar) keep loading, so this is never applied on
/// the read/resolve paths.
pub fn validate_profile_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("profile id must not be empty".to_string());
    }
    if id.len() > 64 {
        return Err(format!(
            "profile id '{id}' is too long (max 64 characters)"
        ));
    }
    let mut chars = id.chars();
    let first = chars.next().expect("non-empty checked above");
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err(format!(
            "profile id '{id}' must start with a lowercase letter or digit"
        ));
    }
    for c in id.chars() {
        let ok = c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-';
        if !ok {
            return Err(format!(
                "profile id '{id}' may only contain lowercase letters, digits, '_' or '-'"
            ));
        }
    }
    Ok(())
}

/// Render the default seed persona for a profile lacking an inline `soul_md`.
///
/// Kept intentionally short — a real persona is authored by the user by editing
/// the seeded `SOUL.md`. Never contains PII or secrets.
fn default_soul_template(profile: &AgentProfile) -> String {
    let name = if profile.name.trim().is_empty() {
        profile.id.as_str()
    } else {
        profile.name.trim()
    };
    let description = profile.description.trim();
    let mut out = format!("# {name}\n\n");
    if !description.is_empty() {
        out.push_str(description);
        out.push_str("\n\n");
    }
    out.push_str(&format!(
        "You are {name}. Keep your own voice, working style, and memory distinct \
         from other profiles. Edit this file to shape your identity.\n"
    ));
    out
}

/// Materialize a profile's home on disk. Idempotent — existing files are never
/// overwritten, so a user's edited `SOUL.md` / `MEMORY.md` survive re-runs.
///
/// Creates:
/// - `<workspace>/personalities/<id>/` (always),
/// - `SOUL.md` seeded from `profile.soul_md` when non-empty, else a short
///   default persona template (only when the file is absent),
/// - an empty `MEMORY.md` (only when absent),
/// - `<action_dir>/profiles/<id>/` when `profile.dedicated_workspace`.
pub fn ensure_profile_home(
    workspace_dir: &Path,
    action_dir: &Path,
    profile: &AgentProfile,
) -> io::Result<()> {
    let home = profile_home(workspace_dir, &profile.id);
    tracing::debug!(
        profile_id = %profile.id,
        home = %home.display(),
        dedicated_memory = profile.dedicated_memory,
        dedicated_workspace = profile.dedicated_workspace,
        "[profiles][home] ensure_profile_home entry"
    );
    std::fs::create_dir_all(&home).map_err(|e| {
        tracing::debug!(
            profile_id = %profile.id,
            home = %home.display(),
            error = %e,
            "[profiles][home] create_dir_all home failed"
        );
        e
    })?;

    let soul_path = home.join("SOUL.md");
    if soul_path.exists() {
        tracing::debug!(
            profile_id = %profile.id,
            "[profiles][home] SOUL.md already present, not overwriting"
        );
    } else {
        let contents = profile
            .soul_md
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| {
                // Persist inline souls with a trailing newline for tidy files.
                if s.ends_with('\n') {
                    s.to_string()
                } else {
                    format!("{s}\n")
                }
            })
            .unwrap_or_else(|| default_soul_template(profile));
        let seeded_from_inline = profile
            .soul_md
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        std::fs::write(&soul_path, contents.as_bytes()).map_err(|e| {
            tracing::debug!(
                profile_id = %profile.id,
                soul_path = %soul_path.display(),
                error = %e,
                "[profiles][home] seed SOUL.md failed"
            );
            e
        })?;
        tracing::debug!(
            profile_id = %profile.id,
            seeded_from_inline,
            "[profiles][home] SOUL.md seeded"
        );
    }

    let memory_path = home.join("MEMORY.md");
    if memory_path.exists() {
        tracing::debug!(
            profile_id = %profile.id,
            "[profiles][home] MEMORY.md already present, not overwriting"
        );
    } else {
        std::fs::write(&memory_path, b"").map_err(|e| {
            tracing::debug!(
                profile_id = %profile.id,
                memory_path = %memory_path.display(),
                error = %e,
                "[profiles][home] create empty MEMORY.md failed"
            );
            e
        })?;
        tracing::debug!(
            profile_id = %profile.id,
            "[profiles][home] MEMORY.md created (empty)"
        );
    }

    if let Some(ws) = dedicated_workspace_dir(action_dir, profile) {
        std::fs::create_dir_all(&ws).map_err(|e| {
            tracing::debug!(
                profile_id = %profile.id,
                workspace = %ws.display(),
                error = %e,
                "[profiles][home] create dedicated workspace failed"
            );
            e
        })?;
        tracing::debug!(
            profile_id = %profile.id,
            workspace = %ws.display(),
            "[profiles][home] dedicated workspace ensured"
        );
    }

    tracing::debug!(profile_id = %profile.id, "[profiles][home] ensure_profile_home ok");
    Ok(())
}

/// Resolve the agent-writable workspace directory for a profile *iff* it opts
/// into a dedicated workspace and its id passes [`validate_profile_id`].
///
/// Returns `None` for shared-workspace profiles (the common case) and for
/// legacy ids that would fail id validation — in both cases the caller falls
/// back to the shared `action_dir`. This is the seam the session builder uses to
/// derive a per-profile default cwd (section D): a `WorkspaceDescriptor` rooted
/// at this path is threaded into the top-level chat turn.
pub fn dedicated_workspace_dir(action_dir: &Path, profile: &AgentProfile) -> Option<PathBuf> {
    if !profile.dedicated_workspace {
        return None;
    }
    if let Err(e) = validate_profile_id(&profile.id) {
        tracing::warn!(
            profile_id = %profile.id,
            error = %e,
            "[profiles][home] dedicated_workspace requested but id fails validation, \
             falling back to shared action_dir"
        );
        return None;
    }
    Some(profile_action_workspace(action_dir, &profile.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_profile(id: &str) -> AgentProfile {
        let mut p = crate::openhuman::profiles::store::built_in_default_profile();
        p.id = id.to_string();
        p.name = id.to_string();
        p.built_in = false;
        p.is_master = false;
        p.memory_dir_suffix = None;
        p.soul_md = None;
        p.dedicated_memory = false;
        p.dedicated_workspace = false;
        p
    }

    #[test]
    fn profile_home_and_action_workspace_paths() {
        let ws = Path::new("/tmp/ws");
        let action = Path::new("/tmp/act");
        assert_eq!(
            profile_home(ws, "alice"),
            Path::new("/tmp/ws/personalities/alice")
        );
        assert_eq!(
            profile_action_workspace(action, "alice"),
            Path::new("/tmp/act/profiles/alice")
        );
    }

    #[test]
    fn validate_profile_id_matrix() {
        // Valid.
        let max_len = "x".repeat(64);
        for id in [
            "a",
            "a1",
            "alice",
            "alice-bob",
            "alice_bob",
            "0",
            max_len.as_str(),
        ] {
            assert!(validate_profile_id(id).is_ok(), "expected ok: {id}");
        }
        // Invalid.
        for id in [
            "",                      // empty
            "-alice",                // leading dash
            "_alice",                // leading underscore
            "Alice",                 // uppercase
            "alice bob",             // space
            "alice.bob",             // dot
            "alice/bob",             // slash
            "über",                  // non-ascii
        ] {
            assert!(validate_profile_id(id).is_err(), "expected err: {id}");
        }
        // Too long (65).
        assert!(validate_profile_id(&"a".repeat(65)).is_err());
    }

    #[test]
    fn ensure_profile_home_creates_and_seeds() {
        let ws = TempDir::new().unwrap();
        let action = TempDir::new().unwrap();
        let mut profile = test_profile("alice");
        profile.description = "A tidy writer.".to_string();

        ensure_profile_home(ws.path(), action.path(), &profile).expect("ensure");

        let home = profile_home(ws.path(), "alice");
        assert!(home.is_dir());
        let soul = std::fs::read_to_string(home.join("SOUL.md")).unwrap();
        // Default template used (no inline soul_md).
        assert!(soul.contains("alice"));
        assert!(soul.contains("A tidy writer."));
        assert!(home.join("MEMORY.md").exists());
        // No dedicated workspace requested.
        assert!(!profile_action_workspace(action.path(), "alice").exists());
    }

    #[test]
    fn ensure_profile_home_seeds_soul_from_inline() {
        let ws = TempDir::new().unwrap();
        let action = TempDir::new().unwrap();
        let mut profile = test_profile("bob");
        profile.soul_md = Some("I am Bob, terse and exact.".to_string());

        ensure_profile_home(ws.path(), action.path(), &profile).expect("ensure");
        let soul =
            std::fs::read_to_string(profile_home(ws.path(), "bob").join("SOUL.md")).unwrap();
        assert_eq!(soul, "I am Bob, terse and exact.\n");
    }

    #[test]
    fn ensure_profile_home_is_idempotent_and_preserves_edits() {
        let ws = TempDir::new().unwrap();
        let action = TempDir::new().unwrap();
        let profile = test_profile("carol");

        ensure_profile_home(ws.path(), action.path(), &profile).expect("ensure 1");
        let home = profile_home(ws.path(), "carol");
        // User edits both files.
        std::fs::write(home.join("SOUL.md"), "EDITED SOUL").unwrap();
        std::fs::write(home.join("MEMORY.md"), "EDITED MEMORY").unwrap();

        // Second run must not clobber.
        ensure_profile_home(ws.path(), action.path(), &profile).expect("ensure 2");
        assert_eq!(
            std::fs::read_to_string(home.join("SOUL.md")).unwrap(),
            "EDITED SOUL"
        );
        assert_eq!(
            std::fs::read_to_string(home.join("MEMORY.md")).unwrap(),
            "EDITED MEMORY"
        );
    }

    #[test]
    fn ensure_profile_home_creates_dedicated_workspace_when_opted_in() {
        let ws = TempDir::new().unwrap();
        let action = TempDir::new().unwrap();
        let mut profile = test_profile("dave");
        profile.dedicated_workspace = true;

        ensure_profile_home(ws.path(), action.path(), &profile).expect("ensure");
        assert!(profile_action_workspace(action.path(), "dave").is_dir());
    }

    #[test]
    fn dedicated_workspace_dir_gates_on_flag_and_id() {
        let action = Path::new("/tmp/act");
        let mut shared = test_profile("eve");
        assert_eq!(dedicated_workspace_dir(action, &shared), None);

        shared.dedicated_workspace = true;
        assert_eq!(
            dedicated_workspace_dir(action, &shared),
            Some(Path::new("/tmp/act/profiles/eve").to_path_buf())
        );

        // Legacy invalid id + dedicated_workspace → None (falls back to shared).
        let mut legacy = test_profile("Legacy Id");
        legacy.id = "Legacy Id".to_string();
        legacy.dedicated_workspace = true;
        assert_eq!(dedicated_workspace_dir(action, &legacy), None);
    }
}
