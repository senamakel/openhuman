//! LLM-callable wrappers over the `vault` domain.
//!
//! Vaults are user-registered local markdown folders that the agent can
//! enumerate, sync into memory, and audit. Each tool is a thin shim over the
//! async functions in [`crate::openhuman::vault::ops`], which return
//! `RpcOutcome<T>`; the wrapper emits the inner value as JSON.
//!
//! NOTE: the markdown writer already ships as `VaultWriteMarkdownTool`
//! (registered separately in `tools::ops`), so it is intentionally not
//! duplicated here.
//!
//! Read/observe + bounded-write tools (`list` / `get` / `files` / `create` /
//! `sync` / `sync_status`) are default-enabled. `vault_remove` unregisters a
//! vault and can purge its memory chunks — it is `Dangerous` and ships
//! default-OFF via `tools/user_filter.rs`.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolResult};

use super::ops;

fn read_required_str(args: &serde_json::Value, key: &str) -> anyhow::Result<String> {
    args.get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("missing required string argument `{key}`"))
}

fn opt_str_vec(args: &serde_json::Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(serde_json::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

macro_rules! emit {
    ($outcome:expr, $name:literal) => {{
        let outcome = $outcome.map_err(|e| anyhow::anyhow!(concat!($name, ": {}"), e))?;
        Ok(ToolResult::success(serde_json::to_string(&outcome.value)?))
    }};
}

/// List registered vaults.
pub struct VaultListTool {
    config: Arc<Config>,
}

impl VaultListTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for VaultListTool {
    fn name(&self) -> &str {
        "vault_list"
    }

    fn description(&self) -> &str {
        "List the user's registered local vaults (markdown folders synced into \
         memory). Each entry carries `id`, `name`, `root_path`, `namespace`, \
         `file_count`, and last-sync time. Use to find a vault `id` before \
         syncing, auditing files, or removing it."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][vault] list invoked");
        emit!(ops::vault_list(&self.config).await, "vault_list")
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }
}

/// Read one vault by id.
pub struct VaultGetTool {
    config: Arc<Config>,
}

impl VaultGetTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for VaultGetTool {
    fn name(&self) -> &str {
        "vault_get"
    }

    fn description(&self) -> &str {
        "Get one vault by `id`, returning its full record (name, root path, \
         namespace, file count, sync state)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": { "id": { "type": "string", "description": "Vault id." } },
            "required": ["id"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][vault] get invoked");
        let id = read_required_str(&args, "id")?;
        emit!(ops::vault_get(&self.config, &id).await, "vault_get")
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }
}

/// List the per-file ingestion ledger for a vault.
pub struct VaultFilesTool {
    config: Arc<Config>,
}

impl VaultFilesTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for VaultFilesTool {
    fn name(&self) -> &str {
        "vault_files"
    }

    fn description(&self) -> &str {
        "List the per-file ingestion ledger for a vault (by `id`): every file's \
         relative path, content hash, size, ingest time, and status. Use to \
         audit exactly what was synced into memory from a vault."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": { "id": { "type": "string", "description": "Vault id." } },
            "required": ["id"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][vault] files invoked");
        let id = read_required_str(&args, "id")?;
        emit!(ops::vault_files(&self.config, &id).await, "vault_files")
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }
}

/// Register a new vault.
pub struct VaultCreateTool {
    config: Arc<Config>,
}

impl VaultCreateTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for VaultCreateTool {
    fn name(&self) -> &str {
        "vault_create"
    }

    fn description(&self) -> &str {
        "Register a new vault from an absolute `root_path` and a `name`, with \
         optional `include_globs` / `exclude_globs` to scope which files sync. \
         Creating a vault does not sync it — call `vault_sync` afterwards."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Display name (required)." },
                "root_path": { "type": "string", "description": "Absolute path to the vault folder (required)." },
                "include_globs": { "type": "array", "items": { "type": "string" } },
                "exclude_globs": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["name", "root_path"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Write
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][vault] create invoked");
        let name = read_required_str(&args, "name")?;
        let root_path = read_required_str(&args, "root_path")?;
        let include = opt_str_vec(&args, "include_globs");
        let exclude = opt_str_vec(&args, "exclude_globs");
        emit!(
            ops::vault_create(&self.config, &name, &root_path, include, exclude).await,
            "vault_create"
        )
    }
}

/// Trigger a background sync crawl for a vault.
pub struct VaultSyncTool {
    config: Arc<Config>,
}

impl VaultSyncTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for VaultSyncTool {
    fn name(&self) -> &str {
        "vault_sync"
    }

    fn description(&self) -> &str {
        "Start a background sync crawl for a vault (by `id`): scans the folder \
         and ingests new/changed markdown into memory. Returns immediately; \
         poll `vault_sync_status` for progress."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": { "id": { "type": "string", "description": "Vault id." } },
            "required": ["id"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Write
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][vault] sync invoked");
        let id = read_required_str(&args, "id")?;
        emit!(ops::vault_sync(&self.config, &id).await, "vault_sync")
    }
}

/// Poll the sync progress for a vault.
pub struct VaultSyncStatusTool;

#[async_trait]
impl Tool for VaultSyncStatusTool {
    fn name(&self) -> &str {
        "vault_sync_status"
    }

    fn description(&self) -> &str {
        "Poll the current sync state for a vault (by `id`): status, counts of \
         scanned/ingested/unchanged/removed/failed files, and any errors. \
         Returns an idle state if no sync has run."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": { "id": { "type": "string", "description": "Vault id." } },
            "required": ["id"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][vault] sync_status invoked");
        let id = read_required_str(&args, "id")?;
        emit!(ops::vault_sync_status(&id).await, "vault_sync_status")
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }
}

/// Unregister a vault, optionally purging its memory. **Destructive** —
/// default-OFF.
pub struct VaultRemoveTool {
    config: Arc<Config>,
}

impl VaultRemoveTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for VaultRemoveTool {
    fn name(&self) -> &str {
        "vault_remove"
    }

    fn description(&self) -> &str {
        "Unregister a vault by `id`. When `purge_memory` is true, also deletes \
         every memory chunk ingested from that vault. Irreversible. Only use \
         when the user wants the vault (and optionally its indexed contents) \
         gone."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Vault id to remove." },
                "purge_memory": { "type": "boolean", "description": "Also delete ingested memory chunks (default false)." }
            },
            "required": ["id"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Dangerous
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][vault] remove invoked");
        let id = read_required_str(&args, "id")?;
        let purge = args
            .get("purge_memory")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        emit!(
            ops::vault_remove(&self.config, &id, purge).await,
            "vault_remove"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::tools::traits::ToolScope;

    fn cfg() -> Arc<Config> {
        Arc::new(Config::default())
    }

    #[test]
    fn names_and_levels() {
        let c = cfg();
        assert_eq!(VaultListTool::new(c.clone()).name(), "vault_list");
        assert_eq!(
            VaultListTool::new(c.clone()).permission_level(),
            PermissionLevel::ReadOnly
        );
        assert_eq!(
            VaultCreateTool::new(c.clone()).permission_level(),
            PermissionLevel::Write
        );
        assert_eq!(
            VaultSyncTool::new(c.clone()).permission_level(),
            PermissionLevel::Write
        );
        assert_eq!(
            VaultRemoveTool::new(c.clone()).permission_level(),
            PermissionLevel::Dangerous
        );
        assert_eq!(
            VaultSyncStatusTool.permission_level(),
            PermissionLevel::ReadOnly
        );
        assert_eq!(VaultListTool::new(c).scope(), ToolScope::All);
    }

    #[tokio::test]
    async fn get_requires_id() {
        let err = VaultGetTool::new(cfg())
            .execute(json!({}))
            .await
            .expect_err("missing id");
        assert!(err.to_string().contains("id"));
    }

    #[tokio::test]
    async fn remove_requires_id() {
        let err = VaultRemoveTool::new(cfg())
            .execute(json!({ "purge_memory": true }))
            .await
            .expect_err("missing id");
        assert!(err.to_string().contains("id"));
    }

    #[tokio::test]
    async fn create_requires_name_and_root_path() {
        let err = VaultCreateTool::new(cfg())
            .execute(json!({ "name": "notes" }))
            .await
            .expect_err("missing root_path");
        assert!(err.to_string().contains("root_path"));
    }
}
