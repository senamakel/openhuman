//! What the `shell` tool tells the model about the shell it spawns, and the
//! environment it hands that shell after `env_clear()`.
//!
//! Split from `shell.rs` so both are pure functions of a parameter — the
//! [`ShellFlavor`] and a parent-environment lookup — testable for Windows on
//! any host.

use crate::agent::platform_shell::ShellFlavor;

/// Environment variables safe to pass to shell commands.
/// Only functional variables are included — never API keys or secrets.
pub(super) const SAFE_ENV_VARS: &[&str] = &[
    "PATH",
    "HOME",
    "TERM",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "USER",
    "SHELL",
    "TMPDIR",
    // Windows process creation and child command lookup need these after env_clear().
    "SystemRoot",
    "WINDIR",
    "COMSPEC",
    "PATHEXT",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
];

/// Python's text-encoding defaults for a shell child, unless the parent sets
/// its own. Under `cmd.exe` the console code page is a legacy one (cp1252,
/// cp437), so a script printing anything outside it dies with
/// `UnicodeEncodeError: 'charmap' codec can't encode …`. UTF-8 mode
/// (`PYTHONUTF8`) fixes `open()` and the stdio defaults; `PYTHONIOENCODING`
/// covers interpreters older than 3.7 and stdio explicitly. Both are no-ops
/// for a non-Python command and on a host whose locale is already UTF-8.
pub(super) const PYTHON_UTF8_DEFAULTS: &[(&str, &str)] =
    &[("PYTHONUTF8", "1"), ("PYTHONIOENCODING", "utf-8")];

/// The Python encoding variables for a child: the parent's own value when
/// `lookup` has one, else [`PYTHON_UTF8_DEFAULTS`].
pub(super) fn python_utf8_env(
    lookup: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Vec<(&'static str, std::ffi::OsString)> {
    PYTHON_UTF8_DEFAULTS
        .iter()
        .map(|(name, default)| {
            let value = lookup(name).unwrap_or_else(|| (*default).into());
            (*name, value)
        })
        .collect()
}

/// The environment a native shell child starts from after `env_clear()`:
/// every [`SAFE_ENV_VARS`] name `lookup` (the parent environment) has, plus
/// [`python_utf8_env`].
///
/// `var_os`, not `var`: a value that is not valid Unicode (possible on both
/// Windows and Unix) used to be dropped silently. Lookups through
/// `std::env::var_os` are case-insensitive on Windows, so `USERPROFILE`
/// matches a parent that spells it `UserProfile`.
pub(super) fn shell_child_env(
    lookup: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Vec<(&'static str, std::ffi::OsString)> {
    let mut env: Vec<(&'static str, std::ffi::OsString)> = SAFE_ENV_VARS
        .iter()
        .filter_map(|name| lookup(name).map(|value| (*name, value)))
        .collect();
    env.extend(python_utf8_env(&lookup));
    env
}

/// The `shell` tool description for the shell this host spawns. Models default
/// to POSIX syntax, so on Windows the description says outright that commands
/// run under `cmd.exe` and gives the equivalents of the commands they reach for.
pub(super) fn shell_description(flavor: ShellFlavor) -> &'static str {
    match flavor {
        ShellFlavor::Cmd => {
            "Execute a command under Windows cmd.exe (`cmd /C`), not a POSIX shell: run code, \
             manipulate workspace files, or launch applications (`start \"\" music:`). Use cmd \
             syntax: `cd` with no argument prints the current directory (no `pwd`), `dir` lists \
             (no `ls`), `type` prints a file (no `cat`), `findstr` searches (no `grep`), \
             `del`/`copy`/`move` replace rm/cp/mv, variables are `%VAR%` (not `$VAR`), and \
             commands chain with `&&` (`;` is not a separator). For PowerShell run \
             `powershell -NoProfile -Command \"...\"`. Only stdout/stderr comes back, so a script \
             that computes silently or only writes a file returns nothing — print what you need, \
             or read the file afterwards."
        }
        ShellFlavor::Posix => {
            "Execute a shell command: run code, manipulate workspace files, or launch applications (`open -a Music`, `xdg-open music://`). Only stdout/stderr comes back, so a script that computes silently or only writes a file returns nothing — print what you need, or read the file afterwards."
        }
    }
}

/// The `command` parameter's description, matching [`shell_description`].
pub(super) fn command_param_description(flavor: ShellFlavor) -> &'static str {
    match flavor {
        ShellFlavor::Cmd => "The cmd.exe command to execute (cmd syntax, not POSIX)",
        ShellFlavor::Posix => "The shell command to execute",
    }
}

/// Restores a managed PATH after POSIX login profiles have run. Ordinary
/// commands and cmd.exe retain their existing startup/environment behavior.
pub(super) fn command_with_runtime_path<'a>(
    command: &'a str,
    path: Option<&str>,
    flavor: ShellFlavor,
) -> std::borrow::Cow<'a, str> {
    match (path, flavor) {
        (Some(path), ShellFlavor::Posix) => std::borrow::Cow::Owned(format!(
            "export PATH='{}'\n{command}",
            path.replace('\'', "'\\''")
        )),
        _ => std::borrow::Cow::Borrowed(command),
    }
}
