//! `cron` — the whole scheduler surface as one action-dispatched tool.
//!
//! Replaces six advertised schemas (`cron_add`, `cron_list`, `cron_update`,
//! `cron_remove`, `cron_run`, `cron_runs`) with one. Four of the six took a
//! `job_id` and nothing else, so most of what they cost was their own name and
//! description repeated six times.
//!
//! # It delegates; it does not reimplement
//!
//! Each action forwards to the tool that already served it. That is deliberate
//! and not merely convenient: `cron_add` carries schedule parsing, timezone
//! resolution and a `SecurityPolicy` check, and a second copy of any of that
//! would be a place for the two to disagree about what is allowed. The old
//! tools stay registered as [`ToolExposure::Hidden`] so a replayed transcript
//! or a saved skill that names `cron_add` still works — they are simply off
//! the wire.
//!
//! # Permissions
//!
//! The six members do not share a permission level: `cron_list` is read-only
//! while `cron_add` is `Execute` (it persists a command that will later run on
//! the host). `permission_level_with_args` resolves the real one once the
//! action is known; the argument-free `permission_level` reports the strictest,
//! so a caller that does not pass arguments over-restricts rather than under-.
//! See `tools::implementations::meta::collapse` for the reasoning.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::{
    add::CronAddTool, list::CronListTool, remove::CronRemoveTool, run::CronRunTool,
    runs::CronRunsTool, update::CronUpdateTool,
};
use crate::config::Config;
use crate::security::policy::SecurityPolicy;
use crate::tools::implementations::meta::collapse::{
    any_external_effect, args_without_action, merge_action_schemas, resolve, strictest_permission,
    unknown_action_message, CollapsedAction,
};
use crate::tools::traits::{PermissionLevel, Tool, ToolCallOptions, ToolResult};

#[cfg(test)]
use crate::tools::traits::ToolExposure;

/// The advertised name. A constant so the registration site, the legacy-alias
/// carve-out and the tests cannot disagree.
pub const CRON_TOOL_NAME: &str = "cron";

pub struct CronTool {
    add: CronAddTool,
    list: CronListTool,
    update: CronUpdateTool,
    remove: CronRemoveTool,
    run: CronRunTool,
    runs: CronRunsTool,
}

impl CronTool {
    pub fn new(config: Arc<Config>, security: Arc<SecurityPolicy>) -> Self {
        Self {
            add: CronAddTool::new(Arc::clone(&config), Arc::clone(&security)),
            list: CronListTool::new(Arc::clone(&config)),
            update: CronUpdateTool::new(Arc::clone(&config), security),
            remove: CronRemoveTool::new(Arc::clone(&config)),
            run: CronRunTool::new(Arc::clone(&config)),
            runs: CronRunsTool::new(config),
        }
    }

    /// The action table, in the order it is advertised.
    ///
    /// Rebuilt per call rather than stored because `CollapsedAction` borrows
    /// the members; the cost is six pointer copies and it keeps the type free
    /// of a self-referential field.
    fn actions(&self) -> Vec<CollapsedAction<'_>> {
        vec![
            CollapsedAction {
                action: "list",
                tool: &self.list,
            },
            CollapsedAction {
                action: "add",
                tool: &self.add,
            },
            CollapsedAction {
                action: "update",
                tool: &self.update,
            },
            CollapsedAction {
                action: "remove",
                tool: &self.remove,
            },
            CollapsedAction {
                action: "run",
                tool: &self.run,
            },
            CollapsedAction {
                action: "runs",
                tool: &self.runs,
            },
        ]
    }
}

#[async_trait]
impl Tool for CronTool {
    fn name(&self) -> &str {
        CRON_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Manage scheduled jobs. `action`: `list` (all jobs), `add` (create a \
         shell or agent job on a cron/at/every schedule), `update` (patch one), \
         `remove`, `run` (force-run now), `runs` (recent run history). \
         Schedules use the device-local timezone unless `tz` is set; the \
         scheduler polls on an interval and does not catch up missed runs. \
         For agent jobs, when the current turn carries a `[Channel context]` \
         block, set `delivery` to `{\"mode\": \"announce\", \"channel\": <channel>, \
         \"to\": <reply target>}` so the reminder returns to that chat rather \
         than the desktop."
    }

    fn parameters_schema(&self) -> Value {
        merge_action_schemas(&self.actions())
    }

    fn permission_level(&self) -> PermissionLevel {
        strictest_permission(&self.actions())
    }

    fn permission_level_with_args(&self, args: &Value) -> PermissionLevel {
        // The honest answer, once the action is known. Falls back to the
        // strictest when the action is missing or unrecognised — such a call is
        // about to be rejected anyway, and answering `None` for it would let an
        // unparseable call past a gate that a parseable one would not clear.
        let actions = self.actions();
        args.get("action")
            .and_then(Value::as_str)
            .and_then(|action| resolve(&actions, action))
            .map(|entry| entry.tool.permission_level_with_args(args))
            .unwrap_or_else(|| strictest_permission(&actions))
    }

    fn external_effect(&self) -> bool {
        any_external_effect(&self.actions())
    }

    fn external_effect_with_args(&self, args: &Value) -> bool {
        let actions = self.actions();
        args.get("action")
            .and_then(Value::as_str)
            .and_then(|action| resolve(&actions, action))
            .map(|entry| entry.tool.external_effect_with_args(args))
            .unwrap_or(true)
    }

    fn supports_markdown(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.execute_with_options(args, ToolCallOptions::default())
            .await
    }

    async fn execute_with_options(
        &self,
        args: Value,
        options: ToolCallOptions,
    ) -> anyhow::Result<ToolResult> {
        let actions = self.actions();
        let requested = args.get("action").and_then(Value::as_str);
        let Some(entry) = requested.and_then(|action| resolve(&actions, action)) else {
            return Ok(ToolResult::error(unknown_action_message(
                &actions, requested,
            )));
        };
        tracing::debug!(action = %entry.action, "[tool][cron] dispatch");
        // `execute_with_options` rather than `execute`, so an action whose
        // member honours `prefer_markdown` keeps doing so through the collapse.
        entry
            .tool
            .execute_with_options(args_without_action(&args), options)
            .await
    }
}

#[cfg(test)]
#[path = "collapsed_tests.rs"]
mod tests;
