use super::*;

fn tool() -> MemoryTool {
    MemoryTool::new(
        Arc::new(Config::default()),
        Arc::new(SecurityPolicy::default()),
    )
}

#[test]
fn every_member_is_hidden_so_the_collapse_actually_saves_something() {
    // `all_actions`, not `actions`: capability filtering could hide a
    // still-advertised member from this check in some environments, and
    // the property being pinned holds regardless of capabilities.
    for entry in tool().all_actions() {
        assert_eq!(
            entry.tool.exposure(),
            ToolExposure::Hidden,
            "`{}` is still advertised alongside the collapsed `memory` tool",
            entry.tool.name()
        );
    }
}

#[test]
fn the_schema_advertises_every_action() {
    let schema = tool().parameters_schema();
    let listed = schema["properties"]["action"]["enum"]
        .as_array()
        .expect("enum")
        .len();
    assert_eq!(listed, 11);
}

#[test]
fn each_action_resolves_to_exactly_its_members_level() {
    // The real contract, and the one that keeps collapsing honest: whatever
    // a member declares, the collapsed tool reports for that action.
    //
    // Note this family currently declares one level between them (see the
    // module docs): `memory_store` and `memory_forget` never override
    // `permission_level`, so they inherit the `ReadOnly` default. That is
    // pre-existing and deliberately not changed here — raising them is a
    // behaviour change to the approval gate, not a token optimisation. An
    // inequality assertion would therefore be asserting a bug.
    let tool = tool();
    for entry in tool.actions() {
        let args = serde_json::json!({"action": entry.action});
        assert_eq!(
            tool.permission_level_with_args(&args),
            entry.tool.permission_level_with_args(&args),
            "action `{}` must report what `{}` reports",
            entry.action,
            entry.tool.name()
        );
    }
}

#[test]
fn collapsing_never_lowers_the_argument_free_level() {
    // The safety property that does not depend on what the members happen
    // to declare today: a caller that cannot pass arguments is never told
    // a level below any member's.
    let tool = tool();
    let floor = tool.permission_level();
    for entry in tool.actions() {
        assert!(
            entry.tool.permission_level() <= floor,
            "`{}` requires more than the collapsed tool advertises",
            entry.tool.name()
        );
    }
}

#[test]
fn an_unknown_action_falls_back_to_the_strictest_level() {
    let tool = tool();
    assert_eq!(
        tool.permission_level_with_args(&serde_json::json!({"action": "nope"})),
        tool.permission_level()
    );
}

#[tokio::test]
async fn an_unknown_action_is_an_error_result_naming_the_valid_ones() {
    let result = tool()
        .execute(serde_json::json!({"action": "recal"}))
        .await
        .expect("dispatch does not fail the call");
    assert!(result.is_error);
    let text = format!("{result:?}");
    assert!(text.contains("recal"));
    assert!(text.contains("recall|store|forget"));
}

#[test]
fn the_memory_tree_tool_is_not_a_member() {
    // Pinning the decision in the module docs: `memory_tree` dispatches on
    // its own `mode`, and folding it in would make this two-level.
    assert!(
        !tool()
            .all_actions()
            .iter()
            .any(|e| e.tool.name() == "memory_tree"),
        "memory_tree stays a separate tool"
    );
}
