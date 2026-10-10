//! Agent tools `juice_find`, `juice_extract` and `juice_summarize`: inspect a
//! tool result the TinyJuice module stored behind a handle.
//!
//! With `[tokenjuice] repl_handle_enabled` on, the module replaces a large
//! result with a stats line, a short head and a handle instead of a summary
//! (see `docs/repl-tools.md` in the TinyJuice repository). These tools are how
//! the model queries what is behind that handle without reading it whole.
//!
//! TinyJuice owns the ops and the tool declarations
//! (`tinyjuice::repl::tools::repl_tools`, the `tinytools` feature). Its CCR
//! store, though, lives inside the module behind the bus, so a store handed to
//! `repl_tools` in this process would be empty. Each wrapper here therefore
//! fetches the original through the same `Retrieve` call `juice_retrieve`
//! makes, gives the stock tool a one-entry store holding it, and returns what
//! the stock tool answers. The ops, argument parsing, size caps and
//! read-only/concurrency flags are TinyJuice's, unchanged.
//!
//! Models also pass the `artifact_path` from a `[tool_result_preview]`
//! envelope (an oversized output persisted under
//! `<workspace>/artifacts/tool-results/`) where the handle goes. Such a path is
//! read from that one directory — nowhere else, no traversal, no symlink out,
//! and no larger than `file_read` would open — and queried the same way.
//! Anything else that is not a handle (a tool call id such as `call_…`) gets a
//! message saying what a handle looks like.
//!
//! Read-only, no side effects, no network access. Nothing here logs the
//! handle's content or the query.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tinyjuice::cache::store::{CcrPutResult, CcrStore};
use tinyjuice::repl::ReplLimits;
use tinytools::{PermissionLevel, Tool, ToolResult};

/// The query tools TinyJuice declares. The footer suggests a subset of these.
pub const REPL_TOOL_NAMES: &[&str] = &["juice_find", "juice_extract", "juice_summarize"];

pub fn is_repl_tool(name: &str) -> bool {
    REPL_TOOL_NAMES.contains(&name)
}

/// Longest handle accepted. Real handles are 32 hex characters.
const MAX_HANDLE_LEN: usize = 64;

/// Largest persisted tool-result artifact read through the handle slot: the
/// same limit `file_read` applies, which is also the most the artifact store
/// ever writes.
const MAX_ARTIFACT_BYTES: u64 = tinytools_std::filesystem::FileReadTool::MAX_FILE_SIZE_BYTES;

/// The relative pointer form older stores handed out (`artifacts/tool-results/…`).
const RELATIVE_ARTIFACT_PREFIX: &str = "artifacts/tool-results/";

/// Token the one-entry store files an artifact's content under.
const ARTIFACT_TOKEN: &str = "artifact";

/// Where the original behind a handle comes from.
#[async_trait]
pub(crate) trait OriginalSource: Send + Sync {
    /// `Ok(None)` is an unknown or evicted handle.
    async fn original(&self, handle: &str) -> Result<Option<String>, String>;
}

/// The TinyJuice module's CCR store, over the bus.
struct ModuleSource;

#[async_trait]
impl OriginalSource for ModuleSource {
    async fn original(&self, handle: &str) -> Result<Option<String>, String> {
        super::retrieve(handle.to_string(), None).await
    }
}

/// A [`CcrStore`] holding the one original a call is about.
struct OneEntryStore {
    token: String,
    content: String,
}

impl CcrStore for OneEntryStore {
    fn put(&self, _content: &str) -> CcrPutResult {
        // Read-only ops never store; report "not retained" rather than lie.
        CcrPutResult::new(String::new(), false)
    }

    fn get(&self, token: &str) -> Option<String> {
        (token == self.token).then(|| self.content.clone())
    }
}

struct ModuleReplTool {
    name: String,
    description: String,
    schema: Value,
    cap: Option<usize>,
    source: Arc<dyn OriginalSource>,
    limits: ReplLimits,
    /// `<workspace>/artifacts/tool-results`, when known: the one directory an
    /// `artifact_path` passed as the handle may be read from.
    artifacts_dir: Option<PathBuf>,
}

/// The three REPL tools, reading through the TinyJuice module. Without a
/// workspace they cannot resolve an `artifact_path`; see [`repl_tools_for`].
pub fn repl_tools() -> Vec<Box<dyn Tool>> {
    repl_tools_with(Arc::new(ModuleSource), ReplLimits::default())
}

/// The REPL tools, or none while large results are not stored behind a handle
/// (compaction, router, CCR or `repl_handle_enabled` off): without a handle to
/// name there is nothing to query, and the schemas would be dead weight on
/// every turn.
pub fn repl_tools_for(config: &crate::config::Config) -> Vec<Box<dyn Tool>> {
    if !super::repl_handle_active(config) {
        return Vec::new();
    }
    log::debug!(
        "[tokenjuice][repl] registering {}",
        REPL_TOOL_NAMES.join(", ")
    );
    repl_tools_with_artifacts(
        Arc::new(ModuleSource),
        ReplLimits::default(),
        Some(crate::security::policy::tool_result_artifacts_dir(
            &config.workspace_dir,
        )),
    )
}

pub(crate) fn repl_tools_with(
    source: Arc<dyn OriginalSource>,
    limits: ReplLimits,
) -> Vec<Box<dyn Tool>> {
    repl_tools_with_artifacts(source, limits, None)
}

pub(crate) fn repl_tools_with_artifacts(
    source: Arc<dyn OriginalSource>,
    limits: ReplLimits,
    artifacts_dir: Option<PathBuf>,
) -> Vec<Box<dyn Tool>> {
    // The declarations come from TinyJuice; the probe store is never read.
    let probe: Arc<dyn CcrStore> = Arc::new(OneEntryStore {
        token: String::new(),
        content: String::new(),
    });
    tinyjuice::repl::tools::repl_tools(probe, limits)
        .into_iter()
        .map(|inner| {
            Box::new(ModuleReplTool {
                name: inner.name().to_string(),
                description: inner.description().to_string(),
                schema: inner.parameters_schema(),
                cap: inner.max_result_size_chars(),
                source: Arc::clone(&source),
                limits,
                artifacts_dir: artifacts_dir.clone(),
            }) as Box<dyn Tool>
        })
        .collect()
}

/// Accept a bare handle, or the `⟦tj:<handle>⟧` marker form.
fn normalize_handle(raw: &str) -> Option<&str> {
    let handle = raw
        .trim()
        .trim_start_matches("⟦tj:")
        .trim_end_matches('⟧')
        .trim();
    let valid = !handle.is_empty()
        && handle.len() <= MAX_HANDLE_LEN
        && handle.chars().all(|c| c.is_ascii_alphanumeric());
    valid.then_some(handle)
}

/// What the model passed in the `handle` slot.
enum HandleArg {
    /// A CCR handle (bare or marker form), normalized.
    Handle(String),
    /// Something path-shaped: possibly a persisted tool-result artifact.
    Path(String),
    /// Neither: a tool call id, prose, an over-long token.
    Invalid,
}

fn classify_handle_arg(raw: &str) -> HandleArg {
    let trimmed = raw.trim();
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.ends_with(".txt") {
        return HandleArg::Path(trimmed.to_string());
    }
    match normalize_handle(trimmed) {
        Some(handle) => HandleArg::Handle(handle.to_string()),
        None => HandleArg::Invalid,
    }
}

fn not_a_handle_message() -> &'static str {
    "juice: that is not a juice handle. A handle is the 32-character hex token named in a \
     stored-output footer (or its `⟦tj:<handle>⟧` marker); a tool call id such as `call_…` \
     is not one. For a `[tool_result_preview]` envelope, pass its `artifact_path` as the \
     handle, or read that file with file_read."
}

fn artifact_refused(reason: &str) -> String {
    format!(
        "juice: {reason}. Only an `artifact_path` under the workspace's \
         artifacts/tool-results directory can stand in for a handle; read other files with \
         file_read."
    )
}

/// Read a persisted tool-result artifact named by `raw`, which must resolve
/// inside `dir` (the absolute pointer a detached store hands out, or the
/// legacy `artifacts/tool-results/…` relative form). Refuses `..`, a symlink
/// or anything else that resolves outside `dir`, a non-file, and a file larger
/// than `max_bytes`. Errors never quote the file's content.
pub(crate) fn read_tool_result_artifact(
    dir: &Path,
    raw: &str,
    max_bytes: u64,
) -> Result<String, String> {
    let raw = raw.trim();
    let path = Path::new(raw);
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(artifact_refused("that path contains `..`"));
    }
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let forward = raw.replace('\\', "/");
        match forward
            .trim_start_matches("./")
            .strip_prefix(RELATIVE_ARTIFACT_PREFIX)
        {
            Some(rest) if !rest.is_empty() => dir.join(rest),
            _ => return Err(artifact_refused("that path is not a tool-result artifact")),
        }
    };
    if !resolved.starts_with(dir) {
        return Err(artifact_refused("that path is not a tool-result artifact"));
    }
    let not_found = |_| artifact_refused("that artifact no longer exists");
    let canonical_dir = std::fs::canonicalize(dir).map_err(not_found)?;
    let canonical = std::fs::canonicalize(&resolved).map_err(not_found)?;
    if !canonical.starts_with(&canonical_dir) {
        return Err(artifact_refused(
            "that path resolves outside the tool-results directory",
        ));
    }
    let meta = std::fs::metadata(&canonical).map_err(not_found)?;
    if !meta.is_file() {
        return Err(artifact_refused("that path is not a file"));
    }
    if meta.len() > max_bytes {
        return Err(artifact_refused(&format!(
            "that artifact is {} bytes, over the {max_bytes}-byte limit",
            meta.len()
        )));
    }
    let bytes = std::fs::read(&canonical).map_err(not_found)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn miss_message() -> &'static str {
    "juice: that handle is no longer stored (evicted, or from an earlier session). \
     Do NOT re-run the same tool call to regenerate it: the result would be stored \
     again under a new handle. Work from the preview already shown, or re-run with \
     narrower arguments so the result is small enough to keep in full."
}

impl ModuleReplTool {
    /// Run TinyJuice's own tool over `content`, filed under `token`.
    async fn run_stock(
        &self,
        token: String,
        content: String,
        mut args: Value,
    ) -> anyhow::Result<ToolResult> {
        if let Some(object) = args.as_object_mut() {
            object.insert("handle".into(), Value::String(token.clone()));
        }
        let store: Arc<dyn CcrStore> = Arc::new(OneEntryStore { token, content });
        let Some(tool) = tinyjuice::repl::tools::repl_tools(store, self.limits)
            .into_iter()
            .find(|tool| tool.name() == self.name)
        else {
            return Ok(ToolResult::error("juice: unknown repl tool"));
        };
        tool.execute(args).await
    }

    /// The handle slot held a path: query the persisted artifact it names.
    async fn execute_on_artifact(&self, path: String, args: Value) -> anyhow::Result<ToolResult> {
        let Some(dir) = self.artifacts_dir.clone() else {
            log::debug!(
                "[tokenjuice][repl] {} artifact path with no workspace",
                self.name
            );
            return Ok(ToolResult::error(not_a_handle_message()));
        };
        let read = crate::core::runtime::spawn_blocking_scoped(move || {
            read_tool_result_artifact(&dir, &path, MAX_ARTIFACT_BYTES)
        })
        .await
        .map_err(|e| anyhow::anyhow!("juice: artifact read task failed: {e}"))?;
        match read {
            Ok(content) => {
                log::debug!(
                    "[tokenjuice][repl] {} artifact bytes={}",
                    self.name,
                    content.len()
                );
                self.run_stock(ARTIFACT_TOKEN.to_string(), content, args)
                    .await
            }
            Err(message) => {
                log::debug!("[tokenjuice][repl] {} artifact refused", self.name);
                Ok(ToolResult::error(message))
            }
        }
    }
}

#[async_trait]
impl Tool for ModuleReplTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> Value {
        self.schema.clone()
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let Some(raw) = args.get("handle").and_then(Value::as_str) else {
            return Ok(ToolResult::error("missing required argument: handle"));
        };
        let handle = match classify_handle_arg(raw) {
            HandleArg::Handle(handle) => handle,
            HandleArg::Path(path) => return self.execute_on_artifact(path, args).await,
            HandleArg::Invalid => {
                log::debug!("[tokenjuice][repl] {} rejected a non-handle", self.name);
                return Ok(ToolResult::error(not_a_handle_message()));
            }
        };
        let content = match self.source.original(&handle).await {
            Ok(Some(content)) => content,
            Ok(None) => {
                log::debug!("[tokenjuice][repl] {} handle miss", self.name);
                return Ok(ToolResult::failed(miss_message()));
            }
            Err(error) => {
                log::debug!("[tokenjuice][repl] {} source error: {error}", self.name);
                return Ok(ToolResult::error(format!("juice: {error}")));
            }
        };
        log::debug!(
            "[tokenjuice][repl] {} handle={handle} bytes={}",
            self.name,
            content.len()
        );
        self.run_stock(handle, content, args).await
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn is_concurrency_safe(&self, _args: &Value) -> bool {
        true
    }

    fn external_effect(&self) -> bool {
        false
    }

    fn max_result_size_chars(&self) -> Option<usize> {
        self.cap
    }
}

#[cfg(test)]
#[path = "repl_tools_tests.rs"]
mod tests;
