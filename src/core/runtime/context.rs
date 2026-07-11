//! Workspace-bound store initialization.
//!
//! Extracted verbatim (Phase 0 — pure motion) from the inline block that used
//! to live in `run_server_inner` (`src/core/jsonrpc.rs`, ~lines 1788-1893). It
//! initializes the process-global stores that are bound to a single resolved
//! workspace directory: memory, image attachments, WhatsApp data, and people —
//! plus the boot-time Sentry user binding.
//!
//! Keeping this as a standalone step is the first move toward a `CoreContext`
//! that *owns* store init order (Phase 2). For now it preserves the exact
//! behavior and ordering of the original inline block, including the
//! deliberate wrong-workspace guard (never seed against a `Config::default`
//! fallback — Sentry OPENHUMAN-CORE-48 / TAURI-RUST-8NM).

/// Initialize the global `MemoryClient` and the other workspace-bound stores so
/// composio providers (gmail/slack/notion) can persist their `sync_state`, and
/// so any subsystem that calls `memory::global::client_if_ready()` gets a live
/// handle.
///
/// A `Config::load_or_init` failure here is operator-visible and serious
/// (corrupt toml, bad permissions, missing/unwritable `OPENHUMAN_WORKSPACE` —
/// common on headless/containerised deploys with no writable `$HOME`).
/// Previously the fallback to `Config::default()` initialised the memory +
/// whatsapp_data stores against the *wrong* workspace dir, silently causing
/// chunk loss / cross-workspace bleed-over while the app looked healthy (Sentry
/// OPENHUMAN-CORE-48). Instead: skip the workspace-bound init entirely so
/// memory stays explicitly *uninitialised* — callers then get a clear "memory
/// client not ready" error rather than reading/writing the wrong workspace. The
/// server still comes up; the operator sees the loud error and fixes their
/// config or sets `OPENHUMAN_WORKSPACE` to a writable path, then restarts.
pub async fn init_stores() {
    match crate::openhuman::config::Config::load_or_init().await {
        Ok(cfg) => {
            let keyring_dir = crate::openhuman::keyring::store::workspace_dir_for_file_backend();
            log::info!(
                "[boot] paths: config={} workspace={} keyring_dir={} keyring_backend={}",
                cfg.config_path.display(),
                cfg.workspace_dir.display(),
                keyring_dir.display(),
                crate::openhuman::keyring::backend_name(),
            );
            match crate::openhuman::memory::global::init(cfg.workspace_dir.clone()) {
                Ok(_) => log::info!(
                    "[boot] memory::global initialized (workspace={})",
                    cfg.workspace_dir.display()
                ),
                Err(e) => log::warn!("[boot] memory::global init failed: {e}"),
            }
            // Install the on-disk image-attachment sidecar dir so inbound
            // image markers persist under <workspace>/attachments/ instead
            // of an in-memory FIFO (survives restarts + delegation hops).
            // Also fires a best-effort stale-file sweep.
            crate::openhuman::agent::multimodal::init_attachments_dir(
                cfg.workspace_dir.join("attachments"),
            );
            log::info!(
                "[boot] image attachments sidecar dir = {}",
                cfg.workspace_dir.join("attachments").display()
            );
            // Initialize the WhatsApp data store so scanner ingest calls
            // can write data without requiring a lazy-init fallback.
            match crate::openhuman::whatsapp_data::global::init(cfg.workspace_dir.clone()) {
                Ok(_) => log::info!(
                    "[boot] whatsapp_data::global initialized (workspace={})",
                    cfg.workspace_dir.display()
                ),
                Err(e) => log::warn!("[boot] whatsapp_data::global init failed: {e}"),
            }
            // Seed the people store so people controllers + `people_*`
            // tools can read/write. Without this the process-global stays
            // empty and every call fails with "people store not
            // initialised" (Sentry TAURI-RUST-8NM). Sits inside this
            // Ok(cfg) arm so it inherits the wrong-workspace guard above
            // (never seed against a Config::default fallback).
            match crate::openhuman::people::store::init_from_workspace(&cfg.workspace_dir) {
                Ok(_) => log::info!(
                    "[boot] people::store initialized (workspace={})",
                    cfg.workspace_dir.display()
                ),
                Err(e) => log::warn!("[boot] people::store init failed: {e}"),
            }
            // Prune legacy bundled skills (dev-workflow / github-issue-crusher
            // / pr-review-shepherd) that older builds seeded into
            // <workspace>/skills/. OpenHuman no longer ships bundled defaults;
            // this removes the stale dirs on upgrade. Idempotent.
            crate::openhuman::skills::registry::prune_legacy_default_workflows(&cfg.workspace_dir);
            // Boot-time Sentry user binding — issue #3135. If the user is
            // already signed in (typical desktop restart), the auth-profile
            // store has their `user_id` *now*, before any background loop
            // (Composio sync tick, heartbeat, etc.) fires its first event.
            // Reading from the store here means subsequent events carry
            // `user.id` even when no `app_state_snapshot` RPC has run yet.
            match crate::openhuman::credentials::session_support::build_session_state(&cfg) {
                Ok(state) => {
                    if let Some(uid) = state.user_id.as_deref() {
                        crate::openhuman::credentials::sentry_scope::bind(uid);
                    }
                }
                Err(e) => log::debug!(
                    "[boot] sentry scope user bind skipped — build_session_state failed: {e}"
                ),
            }
        }
        Err(e) => {
            log::error!(
                "[boot] memory::global + whatsapp_data init SKIPPED — \
                 Config::load_or_init failed ({e:#}). Memory persistence is \
                 DISABLED for this run; no silent fallback to the default \
                 workspace (which would cause chunk loss / cross-workspace \
                 bleed-over). Fix config.toml or set OPENHUMAN_WORKSPACE to a \
                 writable path, then restart."
            );
        }
    }
}
