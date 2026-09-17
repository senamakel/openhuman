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
