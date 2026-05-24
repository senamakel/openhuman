//! macOS backend: Seatbelt via `sandbox-exec`.
//!
//! `sandbox-exec` is a built-in macOS binary that takes a Scheme-style
//! profile (the "Seatbelt" / TrustedBSD policy language) and execs the
//! requested command under it. Chromium, iOS simulators and Apple's own
//! tools use the same SPI under the hood. The CLI is technically
//! deprecated but has stayed shipping for a decade and is the only
//! supported way to apply Seatbelt without private framework bindings.

#![cfg(target_os = "macos")]

use std::process::{Child, Command};

use super::jail::{Jail, JailBackend};

pub struct SeatbeltBackend;

impl SeatbeltBackend {
    pub fn new() -> Self {
        Self
    }
}

impl JailBackend for SeatbeltBackend {
    fn name(&self) -> &'static str {
        "seatbelt"
    }

    fn is_available(&self) -> bool {
        std::path::Path::new("/usr/bin/sandbox-exec").exists()
    }

    fn spawn(&self, jail: &Jail, cmd: Command) -> std::io::Result<Child> {
        let profile = render_profile(jail);

        // sandbox-exec only accepts profiles from disk or from `-p`. Inline
        // (`-p`) is simpler and avoids a tempfile lifecycle problem (the
        // child may outlive our parent scope).
        let program = cmd.get_program().to_os_string();
        let args: Vec<_> = cmd.get_args().map(|a| a.to_os_string()).collect();
        let envs: Vec<_> = cmd
            .get_envs()
            .map(|(k, v)| (k.to_os_string(), v.map(|s| s.to_os_string())))
            .collect();
        let cwd = cmd.get_current_dir().map(|p| p.to_path_buf());

        let mut wrapper = Command::new("/usr/bin/sandbox-exec");
        wrapper.arg("-p").arg(profile).arg(program).args(args);
        for (k, v) in envs {
            match v {
                Some(val) => {
                    wrapper.env(k, val);
                }
                None => {
                    wrapper.env_remove(k);
                }
            }
        }
        if let Some(d) = cwd {
            wrapper.current_dir(d);
        }
        // Inherit stdio from the original command intent. `std::process`
        // doesn't expose the original `Stdio`, so we leave the inherited
        // defaults — callers can re-wire by spawning into a pre-set stdio
        // via the returned `Child` is not possible; for now we match the
        // sandbox-exec defaults (inherit). Document this in mod.rs.
        wrapper.spawn()
    }
}

/// Render a Seatbelt profile. Defaults to deny-all; opens just what `jail`
/// allows. Kept conservative — we'd rather break a tool than leak.
fn render_profile(jail: &Jail) -> String {
    let mut out = String::new();
    out.push_str("(version 1)\n(deny default)\n");
    // System reads always required: dyld, frameworks, locale, tz.
    out.push_str("(allow process-fork)\n");
    if jail.allow_subprocess {
        out.push_str("(allow process-exec)\n");
    }
    out.push_str("(allow sysctl-read)\n");
    out.push_str("(allow mach-lookup)\n");
    out.push_str("(allow file-read*\n");
    for sys in [
        "/System", "/usr/lib", "/usr/share", "/Library/Frameworks", "/private/etc",
        "/private/var/db/timezone", "/dev/null", "/dev/random", "/dev/urandom",
    ] {
        out.push_str(&format!("  (subpath \"{sys}\")\n"));
    }
    for ro in &jail.read_only {
        out.push_str(&format!("  (subpath \"{}\")\n", escape(&ro.to_string_lossy())));
    }
    out.push_str(")\n");

    // R/W root.
    out.push_str(&format!(
        "(allow file-read* file-write*\n  (subpath \"{}\")\n)\n",
        escape(&jail.root.to_string_lossy())
    ));
    // /tmp is treated like the root scratchpad. macOS apps assume it.
    out.push_str("(allow file-read* file-write* (subpath \"/private/tmp\"))\n");

    if jail.allow_net {
        out.push_str("(allow network*)\n");
    }
    out
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
