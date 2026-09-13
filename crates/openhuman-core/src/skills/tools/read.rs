//! Read-only workflow tools: list, describe, read a bundled resource, list
//! recent runs, and read a run log.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::config::Config;
use crate::tools::traits::{Tool, ToolResult};

use super::super::ops_discover::{
    discover_workflows_with_profile, is_workspace_trusted, profile_local_skill_ids,
    read_workflow_resource_with_profile,
};
use super::super::ops_types::WorkflowScope;
use super::super::registry::get_workflow_with_profile;
use super::super::run_log::{read_run_log_slice, scan_runs};
use super::helpers::{
    read_required_str, read_workflow_id, skill_allowed, skill_allowed_including_profile,
    SkillAllowlist,
};

/// List installed skills.
pub struct WorkflowListTool {
    workspace_dir: PathBuf,
    skill_allowlist: SkillAllowlist,
    /// 2a — the active profile's private skills root
    /// (`<workspace>/personalities/<id>/skills/`). `None` for the profile-less
    /// session and other profiles, so the listed set is byte-identical to today.
    profile_skills_root: Option<PathBuf>,
    /// User-scope root to scan, when it must not be the real `$HOME`.
    ///
    /// `None` — production — resolves `dirs::home_dir()` at call time, exactly
    /// as before. See [`Self::with_home_dir`].
    home_dir: Option<PathBuf>,
}

impl WorkflowListTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            workspace_dir: config.workspace_dir.clone(),
            skill_allowlist: None,
            profile_skills_root: None,
            home_dir: None,
        }
    }

    /// Scan `home` as the user scope instead of `dirs::home_dir()`.
    ///
    /// Every other layer of discovery already takes the home directory as a
    /// parameter — `discover_workflows_inner`, `discover_workflows_with_profile`
    /// and `install_workflow_from_url_with_home` all do — and this tool was the
    /// one place that resolved it internally, which made it the one place a test
    /// could not isolate.
    ///
    /// That is not a hypothetical: a test seeding one project-scope workflow
    /// into a tempdir also picked up every skill the developer had installed
    /// under their real `~/.openhuman/skills` and `~/.agents/skills`. The result
    /// is one JSON blob, the harness caps a tool result at 16 KiB
    /// (`ContextConfig::tool_result_budget_bytes`), and fifteen real bundles
    /// push past that — so the seeded workflow was discovered correctly and then
    /// truncated back out before the assertion could see it. The failure looked
    /// like broken discovery and was a non-hermetic fixture.
    #[must_use]
    pub fn with_home_dir(mut self, home: Option<PathBuf>) -> Self {
        self.home_dir = home;
        self
    }

    /// Scope the listed workflows to a per-profile allowlist of `dir_name`
    /// slugs. `None` leaves all workflows visible.
    pub fn with_skill_allowlist(mut self, allowlist: SkillAllowlist) -> Self {
        self.skill_allowlist = allowlist;
        self
    }

    /// Surface the active profile's private skills
    /// (`<workspace>/personalities/<id>/skills/`) in this list. Profile-local
    /// skills are implicitly allowed for their owner (they bypass the
    /// `skill_allowlist`) and win same-name collisions against global skills.
    pub fn with_profile_skills_root(mut self, root: Option<PathBuf>) -> Self {
        self.profile_skills_root = root;
        self
    }
}

#[async_trait]
impl Tool for WorkflowListTool {
    fn name(&self) -> &str {
        "list_workflows"
    }

    fn description(&self) -> &str {
        "List installed workflows (reusable, packaged agent procedures — a goal \
         plus the procedure to reach it). Returns each workflow's name, dir, \
         description, tags, tool hints, scope, and any warnings. Use to find a \
         workflow to inspect (`describe_workflow`) or run (`run_workflow`)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][workflows] list invoked");
        let home = self.home_dir.clone().or_else(dirs::home_dir);
        let trusted = is_workspace_trusted(&self.workspace_dir);
        let mut workflows = discover_workflows_with_profile(
            home.as_deref(),
            Some(&self.workspace_dir),
            self.profile_skills_root.as_deref(),
            trusted,
        );
        if self.skill_allowlist.is_some() {
            let before = workflows.len();
            // Profile-local skills are implicitly allowed for their owner — they
            // bypass the `allowed_skills` allowlist (which scopes only global
            // skills). Keep any skill whose scope is `Profile`.
            workflows.retain(|w| {
                w.scope == WorkflowScope::Builtin
                    || w.scope == WorkflowScope::Profile
                    || skill_allowed(&self.skill_allowlist, &w.dir_name)
            });
            log::debug!(
                "[profiles] list_workflows scoped to profile allowlist: before={before} after={}",
                workflows.len()
            );
        }
        Ok(ToolResult::success(serde_json::to_string(&json!({
            "count": workflows.len(),
            "workflows": workflows,
        }))?))
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }
}

/// Describe one skill (definition + declared inputs).
pub struct WorkflowDescribeTool {
    workspace_dir: PathBuf,
    skill_allowlist: SkillAllowlist,
    /// Active profile's private skills root — resolves + implicitly allows the
    /// owner's profile-local skills. `None` = byte-identical to today.
    profile_skills_root: Option<PathBuf>,
}

impl WorkflowDescribeTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            workspace_dir: config.workspace_dir.clone(),
            skill_allowlist: None,
            profile_skills_root: None,
        }
    }

    /// Scope describe access to a per-profile allowlist of `dir_name` slugs.
    pub fn with_skill_allowlist(mut self, allowlist: SkillAllowlist) -> Self {
        self.skill_allowlist = allowlist;
        self
    }

    /// Resolve (and implicitly allow) the active profile's private skills.
    pub fn with_profile_skills_root(mut self, root: Option<PathBuf>) -> Self {
        self.profile_skills_root = root;
        self
    }
}

#[async_trait]
impl Tool for WorkflowDescribeTool {
    fn name(&self) -> &str {
        "describe_workflow"
    }

    fn description(&self) -> &str {
        "Describe one workflow by `workflow_id`: its agent definition (id, \
         display name, when-to-use) and the inputs it declares (name, \
         description, required, type). Use before running a workflow to learn \
         which inputs to supply."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": { "workflow_id": { "type": "string", "description": "Workflow id (directory name)." } },
            "required": ["workflow_id"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][workflows] describe invoked");
        let skill_id = read_workflow_id(&args)?;
        let profile_local = profile_local_skill_ids(self.profile_skills_root.as_deref());
        if !skill_allowed_including_profile(
            &self.skill_allowlist,
            &profile_local,
            &self.workspace_dir,
            self.profile_skills_root.as_deref(),
            &skill_id,
        ) {
            log::debug!("[profiles] describe_workflow blocked by profile allowlist: {skill_id}");
            return Ok(ToolResult::error(format!(
                "describe_workflow: workflow `{skill_id}` is not available to the active agent profile"
            )));
        }
        let def = get_workflow_with_profile(
            &self.workspace_dir,
            &skill_id,
            self.profile_skills_root.as_deref(),
        )
        .ok_or_else(|| anyhow::anyhow!("describe_workflow: workflow `{skill_id}` not found"))?;
        Ok(ToolResult::success(serde_json::to_string(&json!({
            "definition": def.definition,
            "inputs": def.inputs,
            "github_gated": def.github.is_some(),
        }))?))
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }
}

/// Read a bundled resource file from a skill.
pub struct WorkflowReadResourceTool {
    workspace_dir: PathBuf,
    skill_allowlist: SkillAllowlist,
    /// Active profile's private skills root — resolves + implicitly allows the
    /// owner's profile-local skills. `None` = byte-identical to today.
    profile_skills_root: Option<PathBuf>,
}

impl WorkflowReadResourceTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            workspace_dir: config.workspace_dir.clone(),
            skill_allowlist: None,
            profile_skills_root: None,
        }
    }

    /// Scope resource reads to a per-profile allowlist of `dir_name` slugs, so a
    /// restricted profile can't exfiltrate the bundled scripts/docs of a
    /// workflow outside its skill set.
    pub fn with_skill_allowlist(mut self, allowlist: SkillAllowlist) -> Self {
        self.skill_allowlist = allowlist;
        self
    }

    /// Resolve (and implicitly allow) the active profile's private skills.
    pub fn with_profile_skills_root(mut self, root: Option<PathBuf>) -> Self {
        self.profile_skills_root = root;
        self
    }
}

#[async_trait]
impl Tool for WorkflowReadResourceTool {
    fn name(&self) -> &str {
        "read_workflow_resource"
    }

    fn description(&self) -> &str {
        "Read a bundled resource file from a workflow (`workflow_id` + \
         `relative_path` under the workflow directory, e.g. `scripts/run.sh` or \
         `references/spec.md`). Path-hardened and size-capped. Use to inspect a \
         workflow's helper scripts or reference docs."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "workflow_id": { "type": "string", "description": "Workflow id (directory name)." },
                "relative_path": { "type": "string", "description": "Path relative to the workflow directory." }
            },
            "required": ["workflow_id", "relative_path"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][workflows] read_resource invoked");
        let skill_id = read_workflow_id(&args)?;
        let profile_local = profile_local_skill_ids(self.profile_skills_root.as_deref());
        if !skill_allowed_including_profile(
            &self.skill_allowlist,
            &profile_local,
            &self.workspace_dir,
            self.profile_skills_root.as_deref(),
            &skill_id,
        ) {
            log::debug!(
                "[profiles] read_workflow_resource blocked by profile allowlist: {skill_id}"
            );
            return Ok(ToolResult::error(format!(
                "read_workflow_resource: workflow `{skill_id}` is not available to the active agent profile"
            )));
        }
        let relative_path = read_required_str(&args, "relative_path")?;
        let content = read_workflow_resource_with_profile(
            &self.workspace_dir,
            &skill_id,
            Path::new(&relative_path),
            self.profile_skills_root.as_deref(),
        )
        .map_err(|e| anyhow::anyhow!("read_workflow_resource: {e}"))?;
        Ok(ToolResult::success(serde_json::to_string(&json!({
            "workflow_id": skill_id,
            "relative_path": relative_path,
            "content": content,
        }))?))
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }
}

/// List recent skill runs.
pub struct WorkflowRecentRunsTool {
    workspace_dir: PathBuf,
    active_profile_id: Option<String>,
    skill_allowlist: SkillAllowlist,
    profile_skills_root: Option<PathBuf>,
}

impl WorkflowRecentRunsTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            workspace_dir: config.workspace_dir.clone(),
            active_profile_id: None,
            skill_allowlist: None,
            profile_skills_root: None,
        }
    }

    pub fn with_active_profile(
        mut self,
        profile: Option<crate::agent::profiles::AgentProfile>,
    ) -> Self {
        self.active_profile_id = profile.map(|profile| profile.id);
        self
    }

    pub fn with_skill_allowlist(mut self, allowlist: SkillAllowlist) -> Self {
        self.skill_allowlist = allowlist;
        self
    }

    pub fn with_profile_skills_root(mut self, root: Option<PathBuf>) -> Self {
        self.profile_skills_root = root;
        self
    }
}

#[async_trait]
impl Tool for WorkflowRecentRunsTool {
    fn name(&self) -> &str {
        "list_workflow_runs"
    }

    fn description(&self) -> &str {
        "List recent workflow runs (optionally filtered by `workflow_id`), \
         newest first. Each carries `run_id`, `workflow_id`, start time, status, \
         and duration. Use to find a `run_id` for `read_workflow_run_log` or \
         `await_workflow`."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "workflow_id": { "type": "string", "description": "Filter to one workflow (optional)." },
                "limit": { "type": "integer", "minimum": 1, "description": "Max runs (default 20)." }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][workflows] recent_runs invoked");
        let skill_id = args
            .get("workflow_id")
            .or_else(|| args.get("skill_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let limit = args
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map(|v| v as usize)
            .unwrap_or(20);
        let profile_local = profile_local_skill_ids(self.profile_skills_root.as_deref());
        let runs = scan_runs(&self.workspace_dir, skill_id, usize::MAX)
            .into_iter()
            .filter(|run| {
                run.profile_id.as_deref() == self.active_profile_id.as_deref()
                    && skill_allowed_including_profile(
                        &self.skill_allowlist,
                        &profile_local,
                        &self.workspace_dir,
                        self.profile_skills_root.as_deref(),
                        &run.workflow_id,
                    )
            })
            .take(limit)
            .collect::<Vec<_>>();
        Ok(ToolResult::success(serde_json::to_string(&json!({
            "count": runs.len(),
            "runs": runs,
        }))?))
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }
}

/// Read a slice of a run log.
pub struct WorkflowReadRunLogTool {
    workspace_dir: PathBuf,
    active_profile_id: Option<String>,
    skill_allowlist: SkillAllowlist,
    profile_skills_root: Option<PathBuf>,
}

impl WorkflowReadRunLogTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            workspace_dir: config.workspace_dir.clone(),
            active_profile_id: None,
            skill_allowlist: None,
            profile_skills_root: None,
        }
    }

    pub fn with_active_profile(
        mut self,
        profile: Option<crate::agent::profiles::AgentProfile>,
    ) -> Self {
        self.active_profile_id = profile.map(|profile| profile.id);
        self
    }

    pub fn with_skill_allowlist(mut self, allowlist: SkillAllowlist) -> Self {
        self.skill_allowlist = allowlist;
        self
    }

    pub fn with_profile_skills_root(mut self, root: Option<PathBuf>) -> Self {
        self.profile_skills_root = root;
        self
    }
}

#[async_trait]
impl Tool for WorkflowReadRunLogTool {
    fn name(&self) -> &str {
        "read_workflow_run_log"
    }

    fn description(&self) -> &str {
        "Read a slice of a workflow run's log by `run_id`, from `offset` bytes \
         up to `max_bytes`. Returns the content plus the next offset and an \
         `eof` flag so you can stream a long log. Use `list_workflow_runs` to \
         find a run id."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "run_id": { "type": "string", "description": "Workflow run id." },
                "offset": { "type": "integer", "minimum": 0, "description": "Byte offset to start at (default 0)." },
                "max_bytes": { "type": "integer", "minimum": 1, "description": "Max bytes to read (default 65536)." }
            },
            "required": ["run_id"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][skills] read_run_log invoked");
        let run_id = read_required_str(&args, "run_id")?;
        let offset = args
            .get("offset")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let max_bytes = args
            .get("max_bytes")
            .and_then(serde_json::Value::as_u64)
            .map(|v| v as usize)
            .unwrap_or(65536);
        let profile_local = profile_local_skill_ids(self.profile_skills_root.as_deref());
        let run = scan_runs(&self.workspace_dir, None, usize::MAX)
            .into_iter()
            .find(|run| {
                run.run_id == run_id
                    && run.profile_id.as_deref() == self.active_profile_id.as_deref()
                    && skill_allowed_including_profile(
                        &self.skill_allowlist,
                        &profile_local,
                        &self.workspace_dir,
                        self.profile_skills_root.as_deref(),
                        &run.workflow_id,
                    )
            })
            .ok_or_else(|| anyhow::anyhow!("read_workflow_run_log: run `{run_id}` not found"))?;
        let path = PathBuf::from(run.log_path);
        let slice = read_run_log_slice(&path, offset, max_bytes)
            .map_err(|e| anyhow::anyhow!("read_workflow_run_log: {e}"))?;
        Ok(ToolResult::success(serde_json::to_string(&slice)?))
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }
}
