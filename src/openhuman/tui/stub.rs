//! Disabled-`tui` facade for [`super`] (the tabbed terminal UI).
//!
//! Compiled only when the `tui` Cargo feature is OFF (see the gate in
//! [`super`]). It mirrors the one public symbol always-compiled callers reach —
//! [`run_from_cli`] — with a disabled-error body.
//!
//! The signature MUST match the real one exactly (`&[String] -> anyhow::Result<()>`).
//! The disabled build
//! (`cargo check --no-default-features --features tokenjuice-treesitter`) is the
//! only thing that catches drift.

/// Error text returned by the disabled path. Shared so callers / log-greps see
/// one stable string, and asserted by the disabled-build CLI tests.
const DISABLED_MSG: &str = "tui feature disabled at compile time";

/// Fails with a build-fact diagnostic instead of opening the terminal UI.
///
/// This is deliberately a stub rather than a `#[cfg]` on the `"tui" | "chat"`
/// match arm in `src/core/cli.rs`. Deleting the arm is the naive move and is
/// WRONG: the `tui` / `chat` token would fall through to generic namespace
/// resolution and die with `unknown namespace: tui`, which reads like the user
/// typo'd a command rather than like a deliberate property of this build.
/// Keeping the arm and failing here means the user gets a non-zero exit and a
/// one-line stderr diagnostic naming the fix, and `cli.rs` needs no `#[cfg]` at
/// all — the gate stays invisible to the transport layer.
///
/// Banner suppression in `cli.rs` is a `matches!` on the raw string, so it
/// keeps working here without touching a gated symbol.
pub fn run_from_cli(_args: &[String]) -> anyhow::Result<()> {
    log::warn!(
        "[tui] {DISABLED_MSG} — `openhuman tui`/`chat` rejected; rebuild with `--features tui`"
    );
    anyhow::bail!(
        "{DISABLED_MSG}: this build was compiled without the `tui` feature, so the terminal \
         chat UI is unavailable. Rebuild with `--features tui`."
    )
}
