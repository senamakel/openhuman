//! Test-only driver construction for handlers that read through a family the
//! null driver does not serve.
//!
//! Lives in a `test_support` directory because that is what the memory-guard
//! bypass scanner skips by path (`bypass_allowlist_tests::is_test_path`). The
//! alternative was an allowlist entry, and the allowlist's own rule is that it
//! may shrink but never grow — a test fixture is not the kind of bypass that
//! list exists to track.

use super::binding::install_for_test;
use crate::openhuman::memory::api::provider::MemoryProvider;
use std::sync::Arc;

/// Bind the in-process TinyCortex driver over a whole `Config`'s workspace.
///
/// The shorthand for a test whose handler reads through a family the null
/// driver does not serve — `Chunks`, `Documents`, `Retrieval` — and which
/// proves itself by writing rows and reading them back. `FixedDiagnostics`
/// cannot serve those: it answers `Maintenance` and delegates the rest to
/// null.
///
/// This is the driver the loadable module wraps, so a test binding it exercises
/// the same engine production reaches over the bus. It is not the bus itself,
/// and cannot be: a `dlopen`'ed module is a process singleton, and two tests
/// loading one in the same process hang rather than fail.
pub(crate) fn install_tinycortex_for_test(config: &crate::openhuman::config::Config) {
    crate::openhuman::memory::host_impls::install_for_tests();
    let client = Arc::new(
        tinymemory_core::store::MemoryClient::from_workspace_dir(config.workspace_dir.clone())
            .expect("open the workspace store"),
    );
    // Registered in the process-global slot as well as handed to the driver.
    // The engine's Composio sync pipeline opens with `global::client_if_ready`
    // and refuses with "memory client is not ready" without it — the module
    // path calls `global::bind` for exactly this reason (tinymemory#100), and a
    // fixture that builds a client owes the same registration. `bind` rather
    // than `init` so the driver and the slot are the SAME client: `init` would
    // construct a second one over the same SQLite file, which is two ingestion
    // workers and the hazard `global.rs` documents at length.
    let _ = tinymemory_core::global::bind(config.workspace_dir.clone(), Arc::clone(&client));
    let engine_config = tinymemory_tinycortex::engine::EngineRuntimeConfig {
        workspace_dir: config.workspace_dir.clone(),
        config_path: config.workspace_dir.join("config.toml"),
        memory: config.memory.clone(),
        memory_tree: config.memory_tree.clone(),
        scheduler_gate: config.scheduler_gate.clone(),
        local_ai: config.local_ai.clone(),
        embeddings_provider: config.embeddings_provider.clone(),
        memory_provider: None,
        // Added by tinymemory#100, which moved the periodic sync loops into the
        // module. A test fixture wants the same "no cadence configured" default
        // the module answers for an older host that sends nothing.
        // Carried from the host config rather than blanked: the engine's
        // `composio_config` branches on this, so an empty mode sends every
        // fixture down the proxied path whether or not that is what the test
        // configured.
        memory_sync_interval_secs: config.memory_sync_interval_secs,
        composio_mode: config.composio.mode.clone(),
        composio_entity_id: config.composio.entity_id.clone(),
        // Added by tinymemory#103: proxied Composio addresses the backend with
        // this. Empty means the host named none, and the request then fails in the
        // HTTP client rather than falling back to a guessed host.
        backend_api_url: crate::api::config::effective_backend_api_url(&config.api_url),
        default_model: None,
        default_temperature: 0.2,
        output_language: None,
        memory_sources: serde_json::Value::Null,
    };
    let provider: Arc<dyn MemoryProvider> =
        Arc::new(tinymemory_tinycortex::engine::TinycortexProvider::new(
            "tinycortex".to_string(),
            engine_config,
            client,
        ));
    install_for_test(&config.workspace_dir, &config.subsystems.memory, provider);
}
