//! Detection of leading inline environment-variable assignments on a shell
//! command, and the "dangerous" subset that can redirect execution of an
//! otherwise-allowed command (pagers, editors, loader overrides, …).

/// Environment variable names that can trigger arbitrary command execution
/// when supplied as a leading inline assignment on an otherwise-allowed
/// command. Each name here is either a hook variable that a downstream tool
/// will spawn as a subprocess (`GIT_PAGER`, `GIT_SSH_COMMAND`, `EDITOR`,
/// `LESS`/`LESSOPEN`, `MANPAGER`, `BROWSER`, `BAT_PAGER`), a runtime
/// configuration knob that affects how Python or the shell evaluate user
/// input (`PYTHONSTARTUP`, `BASH_ENV`, `ENV`, `PROMPT_COMMAND`), or a loader
/// override that lets an attacker inject a library into the next process
/// (`LD_PRELOAD`, `LD_LIBRARY_PATH`, `LD_AUDIT`, `DYLD_INSERT_LIBRARIES`,
/// `DYLD_LIBRARY_PATH`, `DYLD_FORCE_FLAT_NAMESPACE`).
///
/// `PATH` and `SHELL` are listed so an inline override cannot redirect
/// resolution of any allowed binary to an attacker-controlled path. `IFS`
/// is listed because the shell uses it for word splitting and a malicious
/// value can hide command boundaries from later parsers.
const DANGEROUS_ENV_PREFIXES: &[&str] = &[
    "BASH_ENV",
    "BAT_PAGER",
    "BROWSER",
    "DYLD_FORCE_FLAT_NAMESPACE",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "EDITOR",
    "ENV",
    "GIT_EDITOR",
    "GIT_EXTERNAL_DIFF",
    "GIT_EXTERNAL_FILTER",
    "GIT_PAGER",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "IFS",
    "LD_AUDIT",
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    "LESS",
    "LESSCLOSE",
    "LESSOPEN",
    "MANOPT",
    "MANPAGER",
    "PAGER",
    "PATH",
    "PROMPT_COMMAND",
    "PS1",
    "PS2",
    "PS3",
    "PS4",
    "PYTHONPATH",
    "PYTHONSTARTUP",
    "SHELL",
    "VISUAL",
];

/// Returns true if `s` starts with one or more inline env assignments and any
/// of the assigned names are in [`DANGEROUS_ENV_PREFIXES`].
///
/// Now superseded by [`has_leading_env_assignment`] for the
/// allowlist check (which rejects ANY leading env assignment), but kept
/// for callers that specifically want the dangerous-only signal —
/// notably tests that pin the old DANGEROUS_ENV_PREFIXES rejection
/// shape.
///
/// The allowlist validation in [`SecurityPolicy::is_command_allowed`] uses
/// [`skip_env_assignments`] to look past the env prefix before matching the
/// command name. That leaves a class of attacks where the bare command (e.g.
/// `git log`) is allowlisted but the env prefix mutates how it executes (e.g.
/// `GIT_PAGER=<cmd> git log` — `git` spawns `<cmd>` as its pager). Because
/// the prefix is stripped before allowlisting and the shell evaluates the
/// prefix at execution time, the bypass lands without ever touching a
/// blocked command name.
///
/// Treating any dangerous prefix as a denial keeps the allowlist
/// semantically meaningful without having to enumerate every shape of every
/// downstream tool's hook surface.
pub(in crate::security::policy) fn has_dangerous_env_prefix(s: &str) -> bool {
    let mut rest = s.trim_start();
    loop {
        let Some(word) = rest.split_whitespace().next() else {
            return false;
        };
        if !word.contains('=') {
            return false;
        }
        if !word
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        {
            return false;
        }
        let (name, _) = word.split_once('=').unwrap_or((word, ""));
        let upper = name.to_ascii_uppercase();
        if DANGEROUS_ENV_PREFIXES.contains(&upper.as_str()) {
            return true;
        }
        rest = rest[word.len()..].trim_start();
    }
}

/// Returns true if `s` starts with at least one inline env-var
/// assignment of the shape `NAME=...`, where `NAME` begins with an ASCII
/// letter or underscore. Catches the entire `GIT_SSH=…`, `SSH_ASKPASS=…`,
/// `LD_PRELOAD=…`, `IFS=…` family — including names we haven't enumerated
/// in [`DANGEROUS_ENV_PREFIXES`] — by treating ANY leading assignment as
/// suspect. The allowlist already names every command we want to permit;
/// nothing in that list needs the operator to set an env var at invoke
/// time, so the broader gate has no false-positive surface on the
/// approved path.
///
/// Used by [`SecurityPolicy::is_command_allowed`] as a structural guard
/// alongside the existing dangerous-prefix check.
pub(in crate::security::policy) fn has_leading_env_assignment(s: &str) -> bool {
    let Some(word) = s.split_whitespace().next() else {
        return false;
    };
    let Some((name, _value)) = word.split_once('=') else {
        return false;
    };
    if name.is_empty() {
        return false;
    }
    // Identifier shape: first char letter or `_`, the rest alphanumeric
    // or `_`. Anything else (e.g. `foo[bar]=`) is not a shell assignment.
    let mut chars = name.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    true
}

/// Skip leading environment variable assignments (e.g. `FOO=bar cmd args`).
/// Returns the remainder starting at the first non-assignment word.
pub(in crate::security::policy) fn skip_env_assignments(s: &str) -> &str {
    let mut rest = s;
    loop {
        let Some(word) = rest.split_whitespace().next() else {
            return rest;
        };
        // Environment assignment: contains '=' and starts with a letter or underscore
        if word.contains('=')
            && word
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        {
            // Advance past this word
            rest = rest[word.len()..].trim_start();
        } else {
            return rest;
        }
    }
}
