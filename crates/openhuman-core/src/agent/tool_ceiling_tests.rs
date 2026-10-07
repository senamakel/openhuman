use super::*;

#[test]
fn allows_only_named_tools() {
    let ceiling = ToolCeiling::new(["file_read", "use_skill"]);
    assert!(ceiling.allows("file_read"));
    assert!(!ceiling.allows("shell"));
    assert_eq!(ceiling.names().collect::<Vec<_>>(), ["file_read", "use_skill"]);
}

#[test]
fn narrowing_is_an_intersection_and_never_widens() {
    let parent = ToolCeiling::new(["file_read", "run_workflow"]);
    let child = ToolCeiling::new(["file_read", "shell"]);
    assert_eq!(parent.narrow(Some(&child)), ToolCeiling::new(["file_read"]));
    assert_eq!(parent.narrow(None), parent);
}

#[test]
fn impose_on_narrows_an_existing_config_ceiling() {
    let mut config = crate::config::AgentConfig::default();
    assert!(ToolCeiling::from_config(&config).is_none());
    ToolCeiling::new(["file_read", "run_workflow"]).impose_on(&mut config);
    assert_eq!(
        config.tool_ceiling.as_deref(),
        Some(&["file_read".to_string(), "run_workflow".to_string()][..])
    );
    ToolCeiling::new(["file_read", "shell"]).impose_on(&mut config);
    assert_eq!(
        ToolCeiling::from_config(&config),
        Some(ToolCeiling::new(["file_read"]))
    );
}

#[test]
fn missing_lists_names_outside_the_ceiling() {
    let ceiling = ToolCeiling::new(["file_read"]);
    assert_eq!(ceiling.missing(["file_read", "shell", "edit"]), ["shell", "edit"]);
}

struct Named(&'static str);

#[async_trait::async_trait]
impl Tool for Named {
    fn name(&self) -> &str {
        self.0
    }
    fn description(&self) -> &str {
        "fixture"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(&self, _: serde_json::Value) -> anyhow::Result<tinytools::ToolResult> {
        Ok(tinytools::ToolResult::success("ok"))
    }
}

#[test]
fn retain_tools_drops_everything_outside() {
    let mut tools: Vec<Box<dyn Tool>> = vec![Box::new(Named("file_read")), Box::new(Named("shell"))];
    let dropped = ToolCeiling::new(["file_read"]).retain_tools(&mut tools);
    assert_eq!(dropped, 1);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name(), "file_read");
}

#[test]
fn scheduled_jobs_are_checked_against_the_ceiling() {
    assert!(check_scheduled_job(None, "cron_add", true).is_none());
    let without_shell = ToolCeiling::new(["cron_add"]);
    let refused = check_scheduled_job(Some(&without_shell), "cron_add", true).unwrap();
    assert!(refused.contains(CEILING_REFUSAL) && refused.contains("shell"), "{refused}");
    let with_shell = ToolCeiling::new(["cron_add", "shell"]);
    assert!(check_scheduled_job(Some(&with_shell), "cron_add", true).is_none());
    let agent_job = check_scheduled_job(Some(&with_shell), "schedule", false).unwrap();
    assert!(agent_job.contains("scheduled agent job"), "{agent_job}");
}

#[test]
fn refusal_names_the_tool_and_what_is_missing() {
    let text = refusal("run_workflow", "workflow `x`", &["shell"]);
    assert!(text.starts_with("run_workflow:"));
    assert!(text.contains("shell") && text.contains(CEILING_REFUSAL));
}
