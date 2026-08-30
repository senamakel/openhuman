//! Host implementations of the seam traits `tinymemory-core` declares.
//!
//! The extracted memory subsystem reaches back into OpenHuman through nine
//! traits (see `tinymemory_api::host` and the `*_host` modules in
//! `tinymemory_core`). [`super::host`] carries the two that are about *data* —
//! `MemoryHostConfig` and `MemoryEventSink`. This module carries the seven that
//! are about *capability*: building providers, loading config, running spaCy,
//! throttling background work, reporting errors.
//!
//! # They are process-globals, installed once
//!
//! Every one is reached through a `set_*` installer that
//! [`install_memory_host_seams`] calls during startup wiring, before any memory
//! work begins. That mirrors the shape the subsystem had before the extraction,
//! when these were free functions it called directly.
//!
//! # Why several of them capture an `Arc<Config>`
//!
//! Four of the seams take a config on the seam side but delegate to a host
//! function whose signature does not (`resolve_api_key`, `ollama_base_url`,
//! `api_key`). Those impls hold the config the installer was given. It is the
//! startup config: a mid-session settings change is *not* reflected, which
//! matches how the pre-extraction call sites behaved — they read the same
//! ambient config — but is worth knowing before adding a seam method that
//! should be live. Seams that must be live (`ComposioHost`, `ConfigLoader`)
//! take a `&Config` argument and re-read instead.

use std::sync::Arc;

use async_trait::async_trait;
use tinyagents::harness::model::{ChatModel, ModelResponse};
use tinymemory_api::host::{EmbeddingHost, EmbeddingProvider, ErrorReporter, UsageInfo};
use tinymemory_core::chat_host::ChatHost;
use tinymemory_core::composio_host::{ComposioConnection, ComposioExecuteResponse, ComposioHost};
use tinymemory_core::config_loader::ConfigLoader;
use tinymemory_core::nlp_host::{NlpHost, SpacyResponse};
use tinymemory_core::scheduler_gate::{Policy, SchedulerGate};
use tinymemory_core::shutdown::{ShutdownHook, ShutdownHost};
use tokio::sync::Notify;

use crate::openhuman::config::Config;

/// Type alias for the seam's config trait object, to keep signatures readable.
type SeamConfig = tinymemory_core::Config;

// ── Embeddings ──────────────────────────────────────────────────────────────

/// Builds embedding providers for the memory subsystem.
#[derive(Debug)]
pub struct OpenHumanEmbeddingHost {
    config: Arc<Config>,
}

impl EmbeddingHost for OpenHumanEmbeddingHost {
    fn resolve_api_key(&self, provider: &str) -> Option<String> {
        let key = crate::openhuman::inference::embeddings::resolve_api_key(&self.config, provider);
        // The host returns "" for "no credential stored"; the seam distinguishes
        // absence from an empty key so callers can report the difference.
        (!key.is_empty()).then_some(key)
    }

    fn ollama_base_url(&self) -> String {
        crate::openhuman::inference::local::ollama_base_url_from_config(&self.config)
    }

    fn default_embedding_provider(&self) -> Arc<dyn EmbeddingProvider> {
        // Scope the managed embedder to THIS host's config credential store, not
        // the keyless `default_state_dir()` hardcode. The memory client caches
        // this provider for the process lifetime, so a keyless scope that misses
        // the signed-in user's `app-session` token makes every ingested
        // document persist vector-less while "Test connection" (config-scoped)
        // still passes — #5501.
        crate::openhuman::inference::embeddings::default_embedding_provider_with_config(
            &self.config,
        )
    }

    fn create_embedding_provider_with_credentials(
        &self,
        provider: &str,
        model: &str,
        dims: usize,
        api_key: &str,
        custom_endpoint: Option<&str>,
    ) -> Result<Box<dyn EmbeddingProvider>, String> {
        crate::openhuman::inference::embeddings::create_embedding_provider_with_credentials(
            provider,
            model,
            dims,
            api_key,
            custom_endpoint,
        )
        .map_err(|e| format!("{e:#}"))
    }

    fn model_supports_dimensions(&self, model: &str) -> bool {
        crate::openhuman::inference::embeddings::model_supports_dimensions(model)
    }

    fn cloud_embedding_provider(
        &self,
        model: &str,
        dims: usize,
    ) -> Result<Box<dyn EmbeddingProvider>, String> {
        Ok(Box::new(
            crate::openhuman::inference::embeddings::cloud::OpenHumanCloudEmbedding::new(
                None,
                self.config
                    .config_path
                    .parent()
                    .map(std::path::PathBuf::from),
                self.config.secrets.encrypt,
                model,
                dims,
            ),
        ))
    }

    fn default_cloud_embedding_model(&self) -> &str {
        crate::openhuman::inference::embeddings::DEFAULT_CLOUD_EMBEDDING_MODEL
    }

    fn default_cloud_embedding_dimensions(&self) -> usize {
        crate::openhuman::inference::embeddings::DEFAULT_CLOUD_EMBEDDING_DIMENSIONS
    }

    fn ollama_embedding_provider(
        &self,
        base_url: &str,
        model: &str,
        dims: usize,
    ) -> Result<Box<dyn EmbeddingProvider>, String> {
        self.create_embedding_provider_with_credentials("ollama", model, dims, "", Some(base_url))
    }
}

// ── Chat models ─────────────────────────────────────────────────────────────

/// Builds chat models for summarisation and the memory chat helper.
///
/// Routing reads BYOK fallbacks, per-role routes and credentials, so it needs
/// the host's own `Config` — recovered from the trait object with
/// [`host_config`]. The captured config is only the fallback for a caller that
/// handed us somebody else's implementation.
#[derive(Debug)]
pub struct OpenHumanChatHost {
    config: Arc<Config>,
}

impl ChatHost for OpenHumanChatHost {
    fn provider_for_role(&self, role: &str, config: &SeamConfig) -> String {
        crate::openhuman::inference::provider::provider_for_role(
            role,
            host_config(config, &self.config),
        )
    }

    fn create_chat_model_with_model_id(
        &self,
        role: &str,
        config: &SeamConfig,
        temperature: f64,
    ) -> Result<(Arc<dyn ChatModel<()>>, String), String> {
        crate::openhuman::inference::provider::create_chat_model_with_model_id(
            role,
            host_config(config, &self.config),
            temperature,
        )
        .map_err(|e| format!("{e:#}"))
    }

    fn usage_from_response(&self, response: &ModelResponse) -> Option<UsageInfo> {
        crate::openhuman::agent::tinyagents::model::usage_info_from_response(response)
    }

    fn summarizer_available(&self, config: &SeamConfig) -> (bool, &'static str) {
        crate::openhuman::memory::tree::tree_runtime::ops::summarizer_available(host_config(
            config,
            &self.config,
        ))
    }
}

// ── Composio ────────────────────────────────────────────────────────────────

/// Runs Composio calls for the memory sync pipelines.
///
/// The two async methods re-read the config from disk, because an OAuth
/// completion or a `set_api_key` RPC between ticks has to take effect
/// immediately. The two sync ones recover the caller's config with
/// [`host_config`], which costs nothing and is still current as of the call.
#[derive(Debug)]
pub struct OpenHumanComposioHost {
    config: Arc<Config>,
}

#[async_trait]
impl ComposioHost for OpenHumanComposioHost {
    async fn list_connections(
        &self,
        config: &SeamConfig,
    ) -> Result<Vec<ComposioConnection>, String> {
        use crate::openhuman::integrations::composio::client::{
            create_composio_client, direct_list_connections, ComposioClientKind,
        };
        let config = live_config(config).await?;
        let response = match create_composio_client(&config)
            .map_err(|e| format!("create_composio_client: {e:#}"))?
        {
            ComposioClientKind::Backend(client) => client
                .list_connections()
                .await
                .map_err(|e| format!("list_connections (backend): {e:#}"))?,
            ComposioClientKind::Direct(direct) => {
                direct_list_connections(&direct).await.map_err(|e| {
                    // [#1166 / Sentry TAURI-RUST-X9] The v3 `/connected_accounts`
                    // 401 shape has to reach the observability classifier, and it
                    // only fires on a message carrying the `[composio-direct]`
                    // anchor. Render it here, where the direct client lives.
                    let rendered = format!("[composio-direct] list_connections (direct): {e:#}");
                    crate::openhuman::integrations::composio::ops::report_composio_op_error(
                        "list_connections",
                        &rendered,
                    );
                    rendered
                })?
            }
        };
        // Across the seam into `tinymemory`'s parallel copy — see
        // `integrations::composio::types::reencode`. Removed with the trait in
        // phase 4 of the connector extraction.
        crate::openhuman::integrations::composio::types::reencode(&response.connections)
    }

    async fn execute(
        &self,
        config: &SeamConfig,
        tool: &str,
        arguments: Option<serde_json::Value>,
        entity_id: &str,
        connection_id: Option<&str>,
    ) -> Result<ComposioExecuteResponse, String> {
        use crate::openhuman::integrations::composio::client::{
            create_composio_client, direct_execute, ComposioClientKind,
        };
        let config = live_config(config).await?;
        let response = match create_composio_client(&config).map_err(|e| format!("{e:#}"))? {
            ComposioClientKind::Backend(client) => client
                .execute_tool(tool, arguments)
                .await
                .map_err(|e| format!("{e:#}"))?,
            ComposioClientKind::Direct(direct) => {
                direct_execute(&direct, tool, arguments, entity_id, connection_id)
                    .await
                    .map_err(|e| format!("{e:#}"))?
            }
        };
        // Across the seam into `tinymemory`'s parallel copy — see
        // `integrations::composio::types::reencode`.
        crate::openhuman::integrations::composio::types::reencode(&response)
    }

    fn api_key(&self, config: &SeamConfig) -> Option<String> {
        crate::openhuman::security::credentials::get_composio_api_key(host_config(
            config,
            &self.config,
        ))
        .ok()
        .flatten()
    }

    /// The app-session bearer proxied Composio mode authenticates with.
    ///
    /// The trait defaults this to `None`, which is right for a host that has no
    /// session concept — and wrong here, where the whole point of the member is
    /// that the engine can reach a live one. Left on the default, the engine's
    /// proxied branch falls back to `Config::session_token`, which works only
    /// while the engine runs in THIS process and answers nothing once it moves
    /// behind the bus.
    ///
    /// Resolved through `host_config` for the same reason every other member
    /// here does: the seam is handed a `dyn MemoryHostConfig` that may be the
    /// module's snapshot rather than this host's own config.
    fn session_bearer(&self, config: &SeamConfig) -> Option<String> {
        crate::api::jwt::get_session_token(host_config(config, &self.config))
            .ok()
            .flatten()
    }

    fn is_available(&self, config: &SeamConfig) -> bool {
        use crate::openhuman::integrations::composio::client::create_composio_client;
        create_composio_client(host_config(config, &self.config)).is_ok()
    }
}

// ── Config loading ──────────────────────────────────────────────────────────

/// Loads host configs for the memory subsystem's background loops.
#[derive(Debug)]
pub struct OpenHumanConfigLoader;

#[async_trait]
impl ConfigLoader for OpenHumanConfigLoader {
    async fn load(&self) -> Result<Box<SeamConfig>, String> {
        Ok(Box::new(
            crate::openhuman::config::rpc::load_config_with_timeout().await?,
        ))
    }

    async fn reload_snapshot(&self, snapshot: &SeamConfig) -> Result<Arc<SeamConfig>, String> {
        // Addressed by path, not by the whole config: the caller holds the
        // seam's trait object and cannot hand us a concrete `Config`.
        let config = crate::openhuman::config::rpc::reload_config_from_paths(
            snapshot.config_path(),
            snapshot.workspace_dir(),
        )
        .await?;
        Ok(Arc::new(config))
    }
}

// ── spaCy ───────────────────────────────────────────────────────────────────

/// Runs spaCy extraction through the host's Python runtime.
#[derive(Debug)]
pub struct OpenHumanNlpHost;

#[async_trait]
impl NlpHost for OpenHumanNlpHost {
    async fn extract_spacy(
        &self,
        config: &SeamConfig,
        text: &str,
    ) -> Result<SpacyResponse, String> {
        let config = live_config(config).await?;
        crate::openhuman::runtime::python_server::extract_spacy(&config, text)
            .await
            .map_err(|e| format!("{e:#}"))
    }
}

// ── Scheduler gate ──────────────────────────────────────────────────────────

/// Exposes the host's background-AI throttle.
#[derive(Debug)]
pub struct OpenHumanSchedulerGate;

#[async_trait]
impl SchedulerGate for OpenHumanSchedulerGate {
    fn current_policy(&self) -> Policy {
        crate::openhuman::cron::scheduler_gate::gate::current_policy()
    }

    fn resume_notify(&self) -> Arc<Notify> {
        crate::openhuman::cron::scheduler_gate::gate::resume_notify()
    }

    async fn wait_for_capacity(&self) -> Option<Box<dyn Send>> {
        crate::openhuman::cron::scheduler_gate::wait_for_capacity()
            .await
            .map(|permit| Box::new(permit) as Box<dyn Send>)
    }
}

// ── Shutdown ────────────────────────────────────────────────────────────────

/// Registers memory shutdown hooks with the host's shutdown sequencer.
#[derive(Debug)]
pub struct OpenHumanShutdownHost;

impl ShutdownHost for OpenHumanShutdownHost {
    fn register(&self, hook: ShutdownHook) {
        let hook = Arc::new(hook);
        crate::core::shutdown::register(move || {
            let hook = Arc::clone(&hook);
            async move { hook().await }
        });
    }
}

// ── Error reporting ─────────────────────────────────────────────────────────

/// Routes memory error reports into the host's observability pipeline.
#[derive(Debug)]
pub struct OpenHumanErrorReporter;

impl ErrorReporter for OpenHumanErrorReporter {
    fn report_error(&self, rendered: &str, domain: &str, operation: &str, tags: &[(&str, &str)]) {
        crate::core::observability::report_error(rendered, domain, operation, tags);
    }

    fn report_error_or_expected(
        &self,
        rendered: &str,
        domain: &str,
        operation: &str,
        tags: &[(&str, &str)],
    ) {
        crate::core::observability::report_error_or_expected(rendered, domain, operation, tags);
    }
}

// ── Wiring ──────────────────────────────────────────────────────────────────

/// Recover the host's concrete `Config` from the seam's trait object.
///
/// Returns `fallback` when the config is some other implementor — a test
/// double, say. That is not an error: it means there is no host config to
/// recover, and the one the seam was installed with is the best answer.
fn host_config<'a>(config: &'a SeamConfig, fallback: &'a Config) -> &'a Config {
    config.as_any().downcast_ref::<Config>().unwrap_or(fallback)
}

/// Re-read the host's concrete `Config` from the paths the seam points at.
///
/// The seam is `dyn MemoryHostConfig` and the host functions these impls
/// delegate to want `&Config`. Recovering one is a file read, so it is only
/// available to the **async** seam methods; the sync ones use the `Arc<Config>`
/// captured at install time instead, and say so on their impl.
///
/// Deliberately not a downcast: `TestHostConfig` and any future implementor are
/// not the host's `Config`, and a downcast would turn them into a silent `None`
/// rather than an honest re-read.
///
/// # Errors
///
/// Returns `Err` when the config file cannot be read.
async fn live_config(config: &SeamConfig) -> Result<Config, String> {
    crate::openhuman::config::rpc::reload_config_from_paths(
        config.config_path(),
        config.workspace_dir(),
    )
    .await
}

/// Install every host seam into `tinymemory-core`.
///
/// Call once during startup wiring, **before any memory work begins** — the
/// embedding, chat, Composio and config seams all fail loudly when unwired, by
/// design, because degrading quietly would corrupt an embedding space or make a
/// sync run look empty rather than broken.
pub fn install_memory_host_seams(config: Arc<Config>) {
    tinymemory_core::embedding_host::set_embedding_host(Arc::new(OpenHumanEmbeddingHost {
        config: Arc::clone(&config),
    }));
    tinymemory_core::chat_host::set_chat_host(Arc::new(OpenHumanChatHost {
        config: Arc::clone(&config),
    }));
    tinymemory_core::composio_host::set_composio_host(Arc::new(OpenHumanComposioHost {
        config: Arc::clone(&config),
    }));
    tinymemory_core::config_loader::set_config_loader(Arc::new(OpenHumanConfigLoader));
    tinymemory_core::nlp_host::set_nlp_host(Arc::new(OpenHumanNlpHost));
    tinymemory_core::scheduler_gate::set_scheduler_gate(Arc::new(OpenHumanSchedulerGate));
    tinymemory_core::shutdown::set_shutdown_host(Arc::new(OpenHumanShutdownHost));
    tinymemory_core::observability::set_error_reporter(Arc::new(OpenHumanErrorReporter));
    super::host::install_memory_event_sink();
    log::debug!("[memory:host] all seam implementations installed");
}

/// Install the seams for this crate's own tests.
///
/// Before the extraction, memory code called `inference::embeddings` and the
/// provider factory directly, so any test that built a memory client got the
/// real implementations for free. The seams made that wiring explicit — which
/// is the point at runtime, but it means a test that builds a client now has to
/// say so. This installs exactly what used to be implicit: the real host impls,
/// over a default config.
///
/// Idempotent, and safe to call from many test threads.
///
/// # Why the thread
///
/// `Config` is a large struct, and `Config::default()` materialises one on the
/// caller's stack before it reaches the `Arc`. Most callers here are
/// `#[tokio::test]` async fns whose futures are already deep; adding it inline
/// overflows the 2 MiB test-thread stack. Building it on a thread with a stack
/// of its own keeps the cost off the caller entirely, and `Once` means it
/// happens exactly one time per test binary.
/// Drop this process's cached handle to the chunk store and reopen it.
///
/// The loaded TinyMemory module and this process each embed their own copy of
/// the engine, with their own connection cache. When the *module* quarantines a
/// corrupt `chunks.db` it renames the file and rebuilds an empty one — but this
/// process's cached connection still points at the old inode (now the
/// `.corrupt-<ts>` copy), so every in-process read (`sources::status`, the
/// session builder's recall) keeps failing with `database disk image is
/// malformed` until restart. Seen live on openhuman#5820's fix: the module
/// recovered, the host's status rows stayed red.
///
/// Delegates to the engine's own recovery entry point, which takes the
/// per-path init lock, drops the cached connection, re-runs `quick_check` on
/// what is on disk now (the rebuilt file, so no second quarantine), and
/// reopens. Best-effort: a failure is logged, never propagated — the module's
/// quarantine already happened and the notice is already on its way.
pub(crate) fn reset_in_process_chunk_store(config: &Config) {
    let engine = tinymemory_core::engine::memory_config_from(config, config.workspace_dir.clone());
    match tinymemory_core::engine::backend::chunks::recover_corrupt_db(&engine) {
        Ok(false) => log::info!(
            "[memory:host] dropped the in-process chunk-store handle and reopened the \
             rebuilt file after the module's quarantine"
        ),
        Ok(true) => log::warn!(
            "[memory:host] the in-process chunk-store reset found the file corrupt as \
             well and quarantined it"
        ),
        Err(error) => log::warn!(
            "[memory:host] could not reset the in-process chunk-store handle after the \
             module's quarantine; in-process reads may keep failing until restart: {error:#}"
        ),
    }
}

#[cfg(test)]
pub(crate) fn install_for_tests() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        std::thread::Builder::new()
            .name("memory-seam-install".to_string())
            .stack_size(8 * 1024 * 1024)
            .spawn(|| install_memory_host_seams(Arc::new(Config::default())))
            .expect("spawn seam installer")
            .join()
            .expect("seam installer panicked");
    });
}

#[cfg(test)]
mod boot_seam_tests {
    use super::*;

    /// Installing the seams must satisfy the engine's `require_embedding_host`.
    ///
    /// This is the regression for the outage #5560 shipped and had to take back:
    /// the seam install was removed from all three boot sites on the reasoning
    /// that "this process embeds no engine, so there is nothing to call back".
    /// It does embed one — `tinymemory-core` is a normal dependency, and
    /// `session::builder::factory` reaches `store::factories::
    /// create_session_memory_with_local_ai`, which calls
    /// `require_embedding_host()`. Every chat turn then died with
    ///
    ///   no EmbeddingHost installed — the host must call
    ///   memory::embedding_host::set_embedding_host during startup wiring
    ///
    /// The loaded module installing its own seams does not cover this: a
    /// `cdylib` has its own statics, so what it sets is invisible in this
    /// process. Nothing in a build or a type check said so, which is why the
    /// assertion is here.
    ///
    /// It asserts the engine's own accessor rather than a local flag, so it
    /// keeps testing the thing the engine actually reads.
    #[test]
    fn installing_the_seams_satisfies_the_engines_embedding_host() {
        install_for_tests();

        assert!(
            tinymemory_core::embedding_host::embedding_host().is_some(),
            "boot installed no EmbeddingHost; the session memory factory on the \
             chat path calls require_embedding_host() and will fail every turn"
        );
        assert!(
            tinymemory_core::embedding_host::require_embedding_host().is_ok(),
            "require_embedding_host must succeed once the boot seams are in"
        );
    }
}

#[cfg(test)]
mod chunk_store_reset_tests {
    use super::*;

    /// The reset must be safe to run against a healthy (or absent) store: it
    /// drops the cached handle and reopens without quarantining anything.
    #[test]
    fn reset_reopens_a_healthy_or_absent_store_without_quarantining() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.workspace_dir = tmp.path().join("workspace");
        std::fs::create_dir_all(config.workspace_dir.join("memory_tree")).unwrap();

        reset_in_process_chunk_store(&config);

        let entries: Vec<String> = std::fs::read_dir(config.workspace_dir.join("memory_tree"))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            !entries.iter().any(|name| name.contains(".corrupt-")),
            "a healthy or absent store must never be quarantined by the reset: {entries:?}"
        );
    }
}
