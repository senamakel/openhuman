//! OpenHuman host adapter for the separately released TinyJuice module.

pub mod config_patch;
pub mod savings;
pub mod schemas;

use crate::core::runtime::DispatchCtx;
use tinyjuice_bus::names::methods;
use tinyjuice_bus::types::{AgentTokenjuiceCompression, CompressorKind};
use tinyjuice_bus::tools::REPL_TOOLS;
use tinyjuice_host::recovery::CompactedToolOutput;
use tinytools::{Tool, ToolRunContext};
use std::sync::Arc;

use tinyjuice_bus::wire::InstallRequest;

pub use tinyjuice_host::recovery::{RECOVERY_TOOL_NAMES, RECOVERY_TOOL_VISIBLE, is_recovery_tool};
pub const LEGACY_RETRIEVE_TOOL_NAME: &str = "retrieve_tool_output";

struct ModuleQuerySource {
    config: Option<Arc<crate::config::Config>>,
}

#[async_trait::async_trait]
impl tinyjuice_host::repl_tools::QuerySource for ModuleQuerySource {
    async fn query(
        &self,
        context: Option<&dyn ToolRunContext>,
        request: tinyjuice_bus::wire::QueryRequest,
    ) -> Result<tinyjuice_bus::wire::QueryResponse, String> {
        match &self.config {
            Some(config) => query_for_config(config, request).await,
            None => {
                let context = crate::agent::tinyagents::host::run_context::dispatch_ctx_for_tool(context);
                query(&context, request).await
            }
        }
    }
}

/// Current REPL tools, with the host's active module configuration and artifact policy.
pub fn repl_tools_for(config: &crate::config::Config) -> Vec<Box<dyn Tool>> {
    if !repl_handle_active(config) {
        return Vec::new();
    }
    tinyjuice_host::repl_tools::repl_tools(
        Arc::new(ModuleQuerySource {
            config: Some(Arc::new(config.clone())),
        }),
        tinyjuice_bus::repl::ReplLimits::default(),
        Some(crate::security::policy::tool_result_artifacts_dir(&config.workspace_dir)),
        tinytools_std::filesystem::FileReadTool::MAX_FILE_SIZE_BYTES,
        None,
    )
}

/// Rebuild recorded REPL declarations verbatim for a resumed session.
pub(crate) fn repl_tools_for_recorded(
    config: Option<&crate::config::Config>,
    workspace_dir: &std::path::Path,
    recorded: &[tinytools::ToolSpec],
) -> Vec<Box<dyn Tool>> {
    let stock = tinyjuice_bus::tools::repl_tool_declarations();
    let declarations = recorded
        .iter()
        .filter(|spec| REPL_TOOLS.contains(&spec.name.as_str()))
        .filter_map(|spec| {
            let mut declaration = stock.iter().find(|item| item.name == spec.name)?.clone();
            declaration.description.clone_from(&spec.description);
            declaration.parameters.clone_from(&spec.parameters);
            Some(declaration)
        })
        .collect();
    tinyjuice_host::repl_tools::repl_tools_with_declarations(
        declarations,
        Arc::new(ModuleQuerySource {
            config: config.cloned().map(Arc::new),
        }),
        tinyjuice_bus::repl::ReplLimits::default(),
        Some(crate::security::policy::tool_result_artifacts_dir(workspace_dir)),
        tinytools_std::filesystem::FileReadTool::MAX_FILE_SIZE_BYTES,
        None,
    )
}

/// Whether the `[components] juice` switch is on. Off means turns truncate
/// tool output only: no compaction, no summary, no `juice_*` / recovery tools,
/// and the module is never loaded for it.
pub fn enabled(config: &crate::config::Config) -> bool {
    config.components.juice
}

/// Whether large results are stored behind a handle (stats, head, handle)
/// instead of being compressed into one blob, and the REPL tools that query
/// them are offered to the model. Needs the router, the CCR store the handle
/// points into, and the compaction switch that lets the module rewrite results
/// at all.
pub fn repl_handle_active(config: &crate::config::Config) -> bool {
    enabled(config)
        && config.context.compaction_enabled
        && config.tokenjuice.router_enabled
        && config.tokenjuice.ccr_enabled
        && config.tokenjuice.repl_handle_enabled
}

/// Whether TinyJuice may summarize this agent's tool output. Only the
/// orchestrator gets a summary model, and a zero threshold turns it off.
pub fn summarizes_tool_output(agent_id: &str, config: &crate::config::Config) -> bool {
    enabled(config)
        && agent_id == "orchestrator"
        && config.context.summarizer_payload_threshold_tokens > 0
}

/// The TinyJuice tools a compacted result can point the model at: the CCR
/// recovery tool while anything can hand out a `⟦tj:…⟧` marker or a summary
/// footer, plus the REPL tools while results are stored behind a handle.
///
/// This is the single list both the session's visible-tool set and the harness
/// allowlist are built from. A name in the former but not the latter is
/// advertised in the prompt and the tool declarations yet answered as an
/// unknown tool at dispatch, so the model burns its failure budget on a tool it
/// was told to call.
pub fn companion_tool_names(agent_id: &str, config: &crate::config::Config) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Vec::new();
    if enabled(config)
        && (config.context.compaction_enabled || summarizes_tool_output(agent_id, config))
    {
        names.extend(RECOVERY_TOOL_VISIBLE.iter().copied());
    }
    if repl_handle_active(config) {
        names.extend(REPL_TOOLS.iter().copied());
    }
    names
}

/// Where the module writes a plain-text copy of each stored original when
/// `[tokenjuice] repl_save_enabled` is on.
pub fn repl_save_dir(workspace_dir: &std::path::Path) -> std::path::PathBuf {
    workspace_dir.join(".tokenjuice").join("repl")
}

/// What the module is told to do, from the resolved configuration.
pub(crate) fn install_request(config: &crate::config::Config) -> InstallRequest {
    let tj = &config.tokenjuice;
    InstallRequest {
        options: tinyjuice_bus::types::CompressOptions {
            router_enabled: tj.router_enabled,
            ccr_enabled: tj.ccr_enabled,
            search_enabled: tj.search_enabled,
            code_enabled: tj.code_enabled,
            html_enabled: tj.html_enabled,
            min_bytes_to_compress: tj.min_bytes_to_compress,
            ccr_min_tokens: tj.ccr_min_tokens,
            // The summary stage still needs a context token per call, which
            // only a turn whose agent carries a summary model supplies — so
            // switching it on here enables it for those turns, not for all.
            llm_summary_enabled: config.context.summarizer_payload_threshold_tokens > 0,
            // Never at ingest: a large result gets deterministic compression
            // and a recovery handle, and the model writes a summary only when
            // the agent asks for one with `juice_summarize` (#6955).
            llm_summary_mode: tinyjuice_bus::types::LlmSummaryMode::OnDemand,
            llm_summary_threshold_tokens: config.context.summarizer_payload_threshold_tokens,
            llm_summary_max_input_tokens: config.context.summarizer_max_payload_tokens,
            repl_handle: repl_handle_active(config),
            repl_save_dir: (repl_handle_active(config) && tj.repl_save_enabled)
                .then(|| repl_save_dir(&config.workspace_dir)),
            ..tinyjuice_bus::types::CompressOptions::default()
        },
        max_cache_entries: tj.max_cache_entries,
        max_cache_bytes: tj.max_cache_bytes,
        ccr_ttl_secs: tj.ccr_ttl_secs,
        disk_tier_root: tj
            .ccr_disk_enabled
            .then(|| config.workspace_dir.join(".tokenjuice").join("ccr"))
            .map(|path| path.to_string_lossy().into_owned()),
    }
}

pub async fn install_from_config(config: &crate::config::Config) -> Result<(), String> {
    if !enabled(config) {
        log::debug!("[tokenjuice] component disabled; module install skipped");
        return Ok(());
    }
    if let Some(model) = config.default_model.clone().or_else(|| {
        crate::inference::registry::provider_default_model(
            crate::inference::provider::factory::PROVIDER_OPENHUMAN,
        )
    }) {
        savings::configure(model);
    }
    // Totals the old build kept in a snapshot file move into the module's
    // storage first (once per file, best effort).
    crate::modules::import_legacy_tinyjuice_savings(
        &crate::core::runtime::DispatchCtx::ambient(),
        &client(config),
        &config.workspace_dir,
    )
    .await;
    let request = install_request(config);
    // The module keeps the durable tier on its blob storage; originals the old
    // disk tier wrote move across first (once per directory, best effort).
    if let Some(root) = request.disk_tier_root.as_deref() {
        crate::modules::import_legacy_tinyjuice(
            &crate::core::runtime::DispatchCtx::ambient(),
            std::path::Path::new(root),
            request.ccr_ttl_secs.map(std::time::Duration::from_secs),
        )
        .await;
    }
    let fingerprint = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    static INSTALLED: std::sync::OnceLock<tokio::sync::Mutex<Option<Vec<u8>>>> =
        std::sync::OnceLock::new();
    let mut installed = INSTALLED
        .get_or_init(|| tokio::sync::Mutex::new(None))
        .lock()
        .await;
    if installed.as_ref() == Some(&fingerprint) {
        return Ok(());
    }
    client(config)
        .call::<()>("tinyjuice", methods::INSTALL, (request,))
        .await
        .map_err(|e| e.to_string())?;
    *installed = Some(fingerprint);
    Ok(())
}

/// Construct explicit access over the shared process-wide loader.
pub(super) fn client(config: &crate::config::Config) -> tinybus::module::ModuleClient {
    #[cfg(test)]
    if let Some(client) = module_stub::rpc_client(config) {
        return client;
    }
    crate::modules::client(config.clone())
}

/// Everything the module considers about one tool result.
pub struct ToolOutputCompaction<'a> {
    pub content: String,
    pub tool_name: &'a str,
    pub enabled: bool,
    pub profile: AgentTokenjuiceCompression,
    pub runtime_config: Option<&'a std::sync::Arc<crate::config::Config>>,
    /// The tool call's JSON arguments, for command, extension and query hints.
    pub arguments: Option<serde_json::Value>,
    /// What the calling model said it needs from this result.
    pub focus: Option<String>,
    /// A [`generate::GenerateTicket`] token. `None` keeps the module from
    /// asking for a summary.
    pub context_token: Option<String>,
    /// Scopes summary reuse and the failure breaker, usually the thread id.
    pub scope: Option<String>,
}

/// Deadline for `CompactWith`. It may wait on a summary model call, which the
/// module itself bounds at 300s; this sits just past that so the module's own
/// timeout is the one that fires.
const COMPACT_WITH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(330);

/// Compact one tool result through the module, passing it through unchanged on
/// any failure.
pub async fn compact_tool_output(call: ToolOutputCompaction<'_>) -> CompactedToolOutput {
    let ToolOutputCompaction {
        content,
        tool_name,
        enabled,
        profile,
        runtime_config,
        arguments,
        focus,
        context_token,
        scope,
    } = call;
    // A summary call is registered, so the result qualified for one. Any path
    // below that ends without a module answer says so, rather than handing
    // the model a raw dump it will re-run the tool to get summarized.
    let wants_summary = context_token.is_some();
    // This agent bypasses TinyJuice's router entirely. The middleware caller
    // never registers a summary ticket for an Off profile, but if one somehow
    // is, disclose it the same way every other no-answer path does rather
    // than silently dropping it in an `unchanged` return.
    if profile == AgentTokenjuiceCompression::Off {
        return CompactedToolOutput::passthrough(content, wants_summary);
    }
    // Nothing to ask the module for: the router is off and no summary call is
    // registered.
    if !enabled && !wants_summary {
        return CompactedToolOutput::unchanged(content);
    }
    #[cfg(test)]
    if let Ok(stub) = module_stub::STUB.try_with(std::sync::Arc::clone) {
        let response = stub(tinyjuice_bus::wire::CompactRequest {
            content,
            tool_name: tool_name.to_string(),
            enabled,
            profile,
            arguments,
            focus,
            context_token,
            scope,
        })
        .await;
        return compacted_from(response);
    }
    let config = match runtime_config {
        Some(config) => std::sync::Arc::clone(config),
        None => match crate::config::Config::load_or_init().await {
            Ok(config) => std::sync::Arc::new(config),
            Err(error) => {
                log::debug!("[tokenjuice] config unavailable, passing through: {error}");
                return CompactedToolOutput::passthrough(content, wants_summary);
            }
        },
    };
    #[cfg(test)]
    // Only the config loaded from disk is replaced: a caller-supplied one is
    // what the test is asking about.
    let config =
        if runtime_config.is_none() && crate::modules::local_artifact("tinyjuice").is_some() {
            // The released-module regression must not inherit an operator's
            // persisted compression thresholds or disabled router flags.
            std::sync::Arc::new(crate::config::Config::default())
        } else {
            config
        };
    if let Err(error) = install_from_config(&config).await {
        log::debug!("[tokenjuice] module configuration failed, passing through: {error}");
        return CompactedToolOutput::passthrough(content, wants_summary);
    }
    let request = tinyjuice_bus::wire::CompactRequest {
        content: content.clone(),
        tool_name: tool_name.to_string(),
        enabled,
        profile,
        arguments,
        focus,
        context_token,
        scope,
    };
    let response = match client(&config)
        .with_timeout(COMPACT_WITH_TIMEOUT)
        .call::<tinyjuice_bus::wire::CompactResponse>(
            "tinyjuice",
            methods::COMPACT_WITH,
            (request,),
        )
        .await
    {
        Ok(response) => response,
        Err(error) => {
            log::debug!("[tokenjuice] module compaction unavailable: {error}");
            return CompactedToolOutput::passthrough(content, wants_summary);
        }
    };
    record_savings(&client(&config), &response).await;
    compacted_from(response)
}

fn compacted_from(response: tinyjuice_bus::wire::CompactResponse) -> CompactedToolOutput {
    let summarized_from_bytes = (response.compressor == CompressorKind::LlmSummary.as_str())
        .then_some(response.original_bytes);
    CompactedToolOutput {
        text: response.text,
        notice: response.notice,
        summarized_from_bytes,
    }
}

async fn record_savings(
    client: &tinybus::module::ModuleClient,
    response: &tinyjuice_bus::wire::CompactResponse,
) {
    use std::str::FromStr as _;
    let compressor = CompressorKind::from_str(&response.compressor).unwrap_or(CompressorKind::None);
    savings::record(
        client,
        compressor,
        response.original_tokens,
        response.compacted_tokens,
    )
    .await;
}

pub async fn detect(
    content: String,
    hint: tinyjuice_bus::types::ContentHint,
) -> Result<String, String> {
    let config = crate::config::Config::load_or_init()
        .await
        .map_err(|error| error.to_string())?;
    detect_with_config(&config, content, hint).await
}

async fn detect_with_config(
    config: &crate::config::Config,
    content: String,
    hint: tinyjuice_bus::types::ContentHint,
) -> Result<String, String> {
    client(config)
        .call("tinyjuice", methods::DETECT, (content, hint))
        .await
        .map_err(|error| error.to_string())
}

pub async fn compress(
    content: String,
    hint: tinyjuice_bus::types::ContentHint,
) -> Result<tinyjuice_bus::types::CompressedOutput, String> {
    let config = crate::config::Config::load_or_init()
        .await
        .map_err(|error| error.to_string())?;
    compress_with_config(&config, content, hint).await
}

async fn compress_with_config(
    config: &crate::config::Config,
    content: String,
    hint: tinyjuice_bus::types::ContentHint,
) -> Result<tinyjuice_bus::types::CompressedOutput, String> {
    install_from_config(config).await?;
    let response: tinyjuice_bus::types::CompressedOutput = client(config)
        .call("tinyjuice", methods::COMPRESS, (content, hint))
        .await
        .map_err(|error| error.to_string())?;
    savings::record(
        &client(config),
        response.compressor,
        (response.original_bytes as u64).div_ceil(4),
        (response.compacted_bytes as u64).div_ceil(4),
    )
    .await;
    Ok(response)
}

pub async fn retrieve(
    token: String,
    range: Option<tinyjuice_bus::wire::RetrieveRange>,
) -> Result<Option<String>, String> {
    let config = crate::config::Config::load_or_init()
        .await
        .map_err(|error| error.to_string())?;
    retrieve_with_config(&config, token, range).await
}

async fn retrieve_with_config(
    config: &crate::config::Config,
    token: String,
    range: Option<tinyjuice_bus::wire::RetrieveRange>,
) -> Result<Option<String>, String> {
    install_from_config(config).await?;
    client(config)
        .call("tinyjuice", methods::RETRIEVE, (token, range))
        .await
        .map_err(|error| error.to_string())
}

pub async fn cache_stats() -> Result<tinyjuice_bus::wire::CacheStats, String> {
    let config = crate::config::Config::load_or_init()
        .await
        .map_err(|error| error.to_string())?;
    cache_stats_with_config(&config).await
}

async fn cache_stats_with_config(
    config: &crate::config::Config,
) -> Result<tinyjuice_bus::wire::CacheStats, String> {
    install_from_config(config).await?;
    client(config)
        .call("tinyjuice", methods::CACHE_STATS, ())
        .await
        .map_err(|error| error.to_string())
}

pub fn all_tokenjuice_registered_controllers() -> Vec<crate::core::all::RegisteredController> {
    schemas::all_registered_controllers()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "module_stub_tests.rs"]
pub(crate) mod module_stub;

#[cfg(test)]
#[path = "mod_repl_module_tests.rs"]
mod repl_module_tests;

#[cfg(test)]
#[path = "rpc_scope_tests.rs"]
mod rpc_scope_tests;

/// Run a typed query inside TinyJuice; cached originals stay inside the module.
pub async fn query(
    ctx: &DispatchCtx,
    request: tinyjuice_bus::wire::QueryRequest,
) -> Result<tinyjuice_bus::wire::QueryResponse, String> {
    let config = ctx.load_config().await?;
    query_for_config(&config, request).await
}

pub(crate) async fn query_for_config(
    config: &crate::config::Config,
    request: tinyjuice_bus::wire::QueryRequest,
) -> Result<tinyjuice_bus::wire::QueryResponse, String> {
    install_from_config(config).await?;
    client(config)
        .call("tinyjuice", methods::QUERY, (request,))
        .await
        .map_err(|e| e.to_string())
}

/// Extract HTML through the compiled module independently of compression settings.
pub async fn extract_html(ctx: &DispatchCtx, html: String) -> Result<String, String> {
    let config = ctx.load_config().await?;
    let response: tinyjuice_bus::wire::HtmlResponse = client(&config)
        .call("tinyjuice", methods::EXTRACT_HTML, (html,))
        .await
        .map_err(|e| e.to_string())?;
    response.map_err(|_| "juice: HTML input exceeds module limit".into())
}

#[cfg(test)]
#[path = "mod_contract_tests.rs"]
mod contract_tests;
