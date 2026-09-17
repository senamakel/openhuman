use crate::agent::file_state;
use crate::security::SecurityPolicy;
use crate::tools::traits::{Tool, ToolCallOptions, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tinytools::ToolRunContext;

const MAX_FILE_SIZE_BYTES: u64 = 10 * 1024 * 1024;
/// Keep a page below the harness's 16 KiB per-result ceiling so the continuation
/// marker survives the result middleware. Returning the whole remainder and
/// relying on that middleware to truncate it loses the next offset and makes a
/// model repeat the same read forever.
const MAX_PAGE_BYTES: usize = 12 * 1024;
const MAX_TOOL_OUTPUT_BYTES: usize = 16 * 1024;

/// Read file contents with path sandboxing
pub struct FileReadTool {
    security: Arc<SecurityPolicy>,
}

impl FileReadTool {
    /// Largest file this tool will open. Anything that must stay readable
    /// through it (a persisted tool-result artifact) has to fit.
    pub const MAX_FILE_SIZE_BYTES: u64 = MAX_FILE_SIZE_BYTES;

    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file in your working directory (the action sandbox). \
         Relative paths resolve against that directory; paths outside it are blocked. \
         To read a file written by `shell`, confirm its location with `pwd` and use the \
         same relative path. Long files are returned one page at a time; continue with \
         the exact `offset` reported at the end of the page."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path to the file within the workspace"
                },
                "offset": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Byte offset to start reading from. Use the offset a paged read reports to continue it."
                }
            },
            "required": ["path"]
        })
    }

    /// Pure read — safe to fan out across parallel `file_read` calls.
    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        self.execute_in_context(args, None).await
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        _options: ToolCallOptions,
        context: Option<&dyn ToolRunContext>,
    ) -> anyhow::Result<ToolResult> {
        self.execute_in_context(args, context).await
    }
}

impl FileReadTool {
    async fn execute_in_context(
        &self,
        args: serde_json::Value,
        context: Option<&dyn ToolRunContext>,
    ) -> anyhow::Result<ToolResult> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;

        if self.security.is_rate_limited() {
            return Ok(ToolResult::error(
                "Rate limit exceeded: too many actions in the last hour",
            ));
        }

        // Record action BEFORE validation so that every non-trivially-rejected
        // request consumes rate limit budget. This prevents attackers from probing
        // path existence (via canonicalize errors) without rate limit cost.
        if !self.security.record_action() {
            return Ok(ToolResult::error(
                "Rate limit exceeded: action budget exhausted",
            ));
        }

        // Security check: validate path string, resolve symlinks, confirm workspace containment.
        let path_policy = super::security_for_tool_context(&self.security, context, "file_read");
        let resolved_path = match path_policy.validate_path(path).await {
            Ok(p) => p,
            Err(msg) => return Ok(ToolResult::error(msg)),
        };

        // Check file size AFTER canonicalization to prevent TOCTOU symlink bypass
        match tokio::fs::metadata(&resolved_path).await {
            Ok(meta) => {
                if meta.len() > MAX_FILE_SIZE_BYTES {
                    return Ok(ToolResult::error(format!(
                        "File too large: {} bytes (limit: {MAX_FILE_SIZE_BYTES} bytes)",
                        meta.len()
                    )));
                }
            }
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Failed to read file metadata: {e}"
                )));
            }
        }

        match tokio::fs::read_to_string(&resolved_path).await {
            Ok(contents) => {
                if let Some(agent_id) = file_state::current_file_state_agent_id() {
                    let mtime = tokio::fs::metadata(&resolved_path)
                        .await
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    file_state::record_read(&agent_id, resolved_path, mtime, false);
                }
                // An absent or null offset reads from the start; anything else
                // that is not a non-negative integer is rejected, so a
                // malformed continuation never silently re-reads the file from
                // byte 0.
                let offset = match args.get("offset") {
                    None | Some(serde_json::Value::Null) => 0,
                    Some(value) => match value.as_u64().and_then(|o| usize::try_from(o).ok()) {
                        Some(offset) => offset,
                        None => {
                            return Ok(ToolResult::error(format!(
                                "offset must be a non-negative integer byte offset, got {value}"
                            )))
                        }
                    },
                };
                if offset > contents.len() {
                    return Ok(ToolResult::error(format!(
                        "offset {offset} is past the end of the file ({} bytes)",
                        contents.len()
                    )));
                }
                // Rejected rather than snapped to a boundary: a caller paging
                // from `offset` computes the next offset from where it asked to
                // start, so a silent move backwards would skip or repeat bytes.
                if !contents.is_char_boundary(offset) {
                    return Ok(ToolResult::error(format!(
                        "offset {offset} falls inside a multi-byte character; continue from the offset a paged read reports"
                    )));
                }
                Ok(ToolResult::success(page_contents(&contents, path, offset)))
            }
            Err(e) => Ok(ToolResult::error(format!("Failed to read file: {e}"))),
        }
    }
}

fn page_contents(contents: &str, path: &str, offset: usize) -> String {
    let initial_end = offset.saturating_add(MAX_PAGE_BYTES).min(contents.len());
    if initial_end == contents.len() {
        return contents[offset..].to_string();
    }

    let path_json =
        serde_json::to_string(path).unwrap_or_else(|_| "\"<unrenderable path>\"".to_string());
    let marker_with_path = |end: usize| {
        format!(
            "\n\n[file_read page: bytes {offset}..{end} of {}; continue with file_read \
             {{\"path\":{path_json},\"offset\":{end}}}]",
            contents.len()
        )
    };
    let marker_without_path = |end: usize| {
        format!(
            "\n\n[file_read page: bytes {offset}..{end} of {}; continue with file_read at \
             \"offset\":{end}]",
            contents.len()
        )
    };

    // Size against the longest offset this page can report. If the escaped path
    // consumes too much of the middleware budget, omit it: the caller already
    // has the path and the continuation offset is the irreplaceable part.
    let longest_with_path = marker_with_path(initial_end);
    let include_path = longest_with_path.len() + 4 <= MAX_TOOL_OUTPUT_BYTES;
    let marker_len = if include_path {
        longest_with_path.len()
    } else {
        marker_without_path(initial_end).len()
    };
    let content_budget = MAX_PAGE_BYTES.min(MAX_TOOL_OUTPUT_BYTES.saturating_sub(marker_len));
    let mut end = offset.saturating_add(content_budget).min(contents.len());
    while end > offset && !contents.is_char_boundary(end) {
        end -= 1;
    }
    let marker = if include_path {
        marker_with_path(end)
    } else {
        marker_without_path(end)
    };
    let mut page = contents[offset..end].to_string();
    page.push_str(&marker);
    page
}

#[cfg(test)]
#[path = "file_read_tests.rs"]
mod tests;
