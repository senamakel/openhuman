//! Tauri commands for exporting agent-generated artifacts (#2779).
//!
//! One export path, fed by the frontend resolving an artifact's absolute
//! source path via the `openhuman.ai_get_artifact` core RPC:
//! [`download_artifact_to_downloads`] copies the artifact into the user's
//! Downloads directory with a non-colliding name and returns the dest path
//! so the UI can offer "Reveal in Finder". Cross-platform.
//!
//! **The native Save-As dialog (#3162) was removed.** It was one call into
//! `rfd`, and `rfd` carried 13 packages — the xdg-desktop-portal client
//! (`ashpd`), `zbus`, and the `async-io`/`polling` executor stack — into a
//! binary that already reaches D-Bus through other paths. The frontend's
//! `saveArtifactViaDialog` had a Downloads fallback for hosts with no
//! portal from the day it landed, so that fallback is simply the only path
//! now; the user still gets the file plus "Reveal in Finder", one dialog
//! fewer. If a real Save-As is wanted again, prefer the destination-picking
//! surface Tauri itself already links over re-adding a second dialog stack.
//!
//! It validates that the source is an existing file inside the OpenHuman
//! data dir's `artifacts/` tree, and sanitizes the filename hint, so the
//! renderer can never copy an arbitrary local file out nor write outside
//! the Downloads directory.

use std::path::{Path, PathBuf};

/// Validate a renderer-supplied source path: must be a non-empty,
/// absolute path that exists on disk AND resolve inside the OpenHuman
/// data directory's `artifacts/` tree. The path always originates from
/// the core `ai_get_artifact` RPC's `absolute_path`, but the command is
/// reachable by the renderer directly, so we re-validate the trust
/// boundary here — without the artifacts-root check a compromised
/// renderer could copy any readable local file out through the Save-As
/// dialog under an artifact-looking name (Codex P2).
fn validate_source(source_path: &str) -> Result<PathBuf, String> {
    if source_path.trim().is_empty() {
        return Err("source_path must not be empty".to_string());
    }
    let source = PathBuf::from(source_path);
    if !source.is_absolute() {
        return Err(format!(
            "source_path must be absolute (came from ai_get_artifact): {source_path:?}"
        ));
    }
    if !source.is_file() {
        return Err(format!(
            "artifact source not present on disk: {source_path}"
        ));
    }
    let root = crate::file_logging::resolve_data_dir();
    assert_artifact_source(&source, &root)?;
    Ok(source)
}

/// Confirm `source` resolves inside `root` (the OpenHuman data dir) and
/// carries an `artifacts` path component — i.e. it is a workspace
/// artifact, not an arbitrary local file. Canonicalizes both sides so
/// symlink trickery can't escape the root. Isolated for unit testing
/// without touching the real home directory.
fn assert_artifact_source(source: &Path, root: &Path) -> Result<(), String> {
    let canon_source = source
        .canonicalize()
        .map_err(|e| format!("cannot resolve source path: {e}"))?;
    let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if !canon_source.starts_with(&canon_root) {
        return Err("source must be inside the OpenHuman data directory".to_string());
    }
    if !canon_source
        .components()
        .any(|c| c.as_os_str() == "artifacts")
    {
        return Err("source must be a workspace artifact file".to_string());
    }
    Ok(())
}

/// Copy `source` to `dest`, returning the byte count. Isolated so it is
/// unit-testable without touching the real Downloads directory.
async fn copy_to_path(source: &Path, dest: &Path) -> Result<u64, String> {
    tokio::fs::copy(source, dest)
        .await
        .map_err(|e| format!("failed to copy artifact to {:?}: {e}", dest))
}

/// Maximum number of `(N)` suffixes we'll append when picking a
/// non-colliding filename. After 1000 we give up and append a UUID
/// suffix instead so the download never silently overwrites.
const MAX_COLLISION_SUFFIX: u32 = 1000;

#[tauri::command]
pub async fn download_artifact_to_downloads(
    source_path: String,
    filename: String,
) -> Result<String, String> {
    let source = validate_source(&source_path)?;
    if filename.trim().is_empty() {
        return Err("filename must not be empty".to_string());
    }
    let sanitized = sanitize_filename(&filename)?;

    let downloads = directories::UserDirs::new()
        .and_then(|u| u.download_dir().map(|p| p.to_path_buf()))
        .ok_or_else(|| "OS Downloads directory not resolvable".to_string())?;
    tokio::fs::create_dir_all(&downloads)
        .await
        .map_err(|e| format!("failed to ensure Downloads dir {:?}: {e}", downloads))?;

    let dest = pick_unique_path(&downloads, &sanitized);
    let bytes = copy_to_path(&source, &dest).await?;

    log::info!(
        "[artifact_commands] download_artifact_to_downloads bytes={bytes} dest={}",
        dest.display()
    );
    Ok(dest.display().to_string())
}

/// Strip path-traversal characters from a filename hint. The
/// renderer is expected to pass something like `"My Deck.pptx"`;
/// reject anything that contains a separator or null byte so a
/// malicious `ai_get_artifact` response can never escape the chosen dir.
fn sanitize_filename(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("filename must not be empty after trim".to_string());
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err(format!(
            "filename must not contain path separators: {trimmed:?}"
        ));
    }
    if trimmed.contains('\0') {
        return Err(format!("filename must not contain NUL bytes: {trimmed:?}"));
    }
    if trimmed == "." || trimmed == ".." {
        return Err(format!("filename must not be '.' or '..': {trimmed:?}"));
    }
    Ok(trimmed.to_string())
}

/// Pick a destination path under `dir` that does not exist yet.
/// Inserts ` (N)` between the stem and the extension. Falls back to
/// a UUID suffix after [`MAX_COLLISION_SUFFIX`] tries.
fn pick_unique_path(dir: &Path, filename: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = split_stem_ext(filename);
    for n in 1..=MAX_COLLISION_SUFFIX {
        let nth = if ext.is_empty() {
            format!("{stem} ({n})")
        } else {
            format!("{stem} ({n}).{ext}")
        };
        let path = dir.join(&nth);
        if !path.exists() {
            return path;
        }
    }
    // 1000 collisions is implausible in practice; if we hit it, fall
    // back to a monotonic nanosecond suffix so the copy still succeeds
    // without overwriting anything. Reaches for the OS clock instead of
    // pulling in `uuid` as a Tauri-shell dep just for this corner.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let with_uniq = if ext.is_empty() {
        format!("{stem}-{nanos}")
    } else {
        format!("{stem}-{nanos}.{ext}")
    };
    dir.join(with_uniq)
}

fn split_stem_ext(filename: &str) -> (String, String) {
    if let Some(idx) = filename.rfind('.') {
        // Reject leading-dot files (`.hidden`) — treat as having no extension.
        if idx > 0 && idx < filename.len() - 1 {
            return (filename[..idx].to_string(), filename[idx + 1..].to_string());
        }
    }
    (filename.to_string(), String::new())
}

#[cfg(test)]
#[path = "artifact_commands_tests.rs"]
mod tests;
