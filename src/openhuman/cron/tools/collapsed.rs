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
use crate::openhuman::config::Config;
use crate::openhuman::security::policy::SecurityPolicy;
use crate::openhuman::tools::implementations::meta::collapse::{
    any_external_effect, args_without_action, merge_action_schemas, resolve, strictest_permission,
    unknown_action_message, CollapsedAction,
};
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCallOptions, ToolResult};

#[cfg(test)]
use crate::openhuman::tools::traits::ToolExposure;

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
mod tests {
    use super::*;

    fn tool() -> CronTool {
        CronTool::new(
            Arc::new(Config::default()),
            Arc::new(SecurityPolicy::default()),
        )
    }

    #[test]
    fn the_schema_advertises_every_action() {
        let schema = tool().parameters_schema();
        let actions = schema["properties"]["action"]["enum"]
            .as_array()
            .expect("enum")
            .iter()
            .map(|v| v.as_str().unwrap_or_default().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            actions,
            vec!["list", "add", "update", "remove", "run", "runs"]
        );
    }

    #[test]
    fn the_schema_carries_the_members_parameters() {
        // `job_id` comes from four members and `patch` only from `update`.
        // Their presence is what proves the merge read the members rather than
        // a hand-written union that could drift from them.
        let schema = tool().parameters_schema();
        let props = schema["properties"].as_object().expect("properties");
        assert!(props.contains_key("job_id"));
        assert!(props.contains_key("patch"));
    }

    #[test]
    fn a_read_only_action_is_not_reported_as_execute() {
        // The whole point of `permission_level_with_args`: collapsing must not
        // silently promote `list` to the privilege `add` needs.
        let tool = tool();
        let listing = serde_json::json!({"action": "list"});
        assert!(
            tool.permission_level_with_args(&listing) < tool.permission_level(),
            "list must resolve below the family's strictest level"
        );
    }

    #[test]
    fn an_unknown_action_falls_back_to_the_strictest_level() {
        let tool = tool();
        let nonsense = serde_json::json!({"action": "definitely_not_an_action"});
        assert_eq!(
            tool.permission_level_with_args(&nonsense),
            tool.permission_level()
        );
    }

    #[test]
    fn a_missing_action_falls_back_to_the_strictest_level() {
        let tool = tool();
        assert_eq!(
            tool.permission_level_with_args(&serde_json::json!({})),
            tool.permission_level()
        );
    }

    #[tokio::test]
    async fn an_unknown_action_is_an_error_result_naming_the_valid_ones() {
        let result = tool()
            .execute(serde_json::json!({"action": "nope"}))
            .await
            .expect("dispatch does not fail the call");
        assert!(result.is_error);
        let text = format!("{result:?}");
        assert!(text.contains("nope"), "names what was passed: {text}");
        assert!(text.contains("list|add"), "names the valid actions: {text}");
    }

    #[test]
    fn every_member_is_hidden_so_the_collapse_actually_saves_something() {
        // The load-bearing assertion. Adding an action to the table while
        // leaving its member `Direct` would ship both surfaces and save
        // nothing, and nothing else in the build would notice.
        for entry in tool().actions() {
            assert_eq!(
                entry.tool.exposure(),
                ToolExposure::Hidden,
                "`{}` is still advertised alongside the collapsed `cron` tool",
                entry.tool.name()
            );
        }
    }
}
