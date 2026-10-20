//! Workspace integrity: [`check_workspace`] and its disk-space and probe-path
//! helpers.

use std::io::Write;
use std::path::Path;

use crate::config::Config;

use super::types::DiagnosticItem;

pub(super) fn check_workspace(config: &Config, items: &mut Vec<DiagnosticItem>) {
    let cat = "workspace";
    let ws = &config.workspace_dir;

    if ws.exists() {
        items.push(DiagnosticItem::ok(
            cat,
            format!("directory exists: {}", ws.display()),
        ));
    } else {
        items.push(DiagnosticItem::error(
            cat,
            format!("directory missing: {}", ws.display()),
        ));
        return;
    }

    // Writable check
    let probe = workspace_probe_path(ws);
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(mut probe_file) => {
            let write_result = probe_file.write_all(b"probe");
            drop(probe_file);
            let _ = std::fs::remove_file(&probe);
            match write_result {
                Ok(()) => items.push(DiagnosticItem::ok(cat, "directory is writable")),
                Err(e) => items.push(DiagnosticItem::error(
                    cat,
                    format!("directory write probe failed: {e}"),
                )),
            }
        }
        Err(e) => {
            items.push(DiagnosticItem::error(
                cat,
                format!("directory is not writable: {e}"),
            ));
        }
    }

    // Minimal workspace folders
    let mem_dir = ws.join("memory");
    if mem_dir.exists() {
        items.push(DiagnosticItem::ok(
            cat,
            format!("memory directory: {}", mem_dir.display()),
        ));
    } else {
        items.push(DiagnosticItem::warn(
            cat,
            format!("memory directory missing: {}", mem_dir.display()),
        ));
    }

    // Check for config templates or docs
    let prompt = ws.join("SYSTEM.md");
    if prompt.exists() {
        items.push(DiagnosticItem::ok(
            cat,
            format!("SYSTEM prompt: {}", prompt.display()),
        ));
    } else {
        items.push(DiagnosticItem::warn(
            cat,
            format!("SYSTEM prompt missing: {}", prompt.display()),
        ));
    }

    // Disk space warning (best-effort)
    if let Some(avail_mb) = available_disk_space_mb(ws) {
        if avail_mb < 512 {
            items.push(DiagnosticItem::warn(
                cat,
                format!("low disk space: {avail_mb} MB free"),
            ));
        } else {
            items.push(DiagnosticItem::ok(
                cat,
                format!("disk space OK: {avail_mb} MB free"),
            ));
        }
    }
}

fn available_disk_space_mb(path: &Path) -> Option<u64> {
    #[cfg(target_os = "windows")]
    {
        available_disk_space_mb_windows(path)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let output = std::process::Command::new("df")
            .arg("-m")
            .arg(path)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_df_available_mb(&stdout)
    }
}

#[cfg(not(target_os = "windows"))]
fn parse_df_available_mb(stdout: &str) -> Option<u64> {
    let line = stdout.lines().rev().find(|line| !line.trim().is_empty())?;
    let avail = line.split_whitespace().nth(3)?;
    avail.parse::<u64>().ok()
}

#[cfg(target_os = "windows")]
fn available_disk_space_mb_windows(path: &Path) -> Option<u64> {
    use std::path::{Component, Prefix};

    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let letter = canonical.components().find_map(|c| match c {
        Component::Prefix(pc) => match pc.kind() {
            Prefix::Disk(b) | Prefix::VerbatimDisk(b) => Some((b as char).to_ascii_uppercase()),
            _ => None,
        },
        _ => None,
    })?;

    // PowerShell is ubiquitous on supported Windows; `Get-PSDrive` needs no admin
    // and returns free bytes as a single integer line.
    let script = format!("(Get-PSDrive -Name {letter} -ErrorAction Stop).Free");
    let mut cmd = std::process::Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &script,
    ]);
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let bytes: u64 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .ok()?;
    Some(bytes / (1024 * 1024))
}

fn workspace_probe_path(workspace_dir: &Path) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    workspace_dir.join(format!(
        ".openhuman_doctor_probe_{}_{}",
        std::process::id(),
        nanos
    ))
}
