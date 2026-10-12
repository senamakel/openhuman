use super::*;

#[test]
fn only_a_named_scope_listing_a_spawn_tool_names_one() {
    let named = |names: &[&str]| ToolScope::Named(names.iter().map(|n| (*n).to_string()).collect());
    for spawn in SPAWN_TOOL_NAMES {
        assert!(named(&["memory", spawn]).names_a_spawn_tool(), "{spawn}");
    }
    assert!(!named(&["memory", "cron"]).names_a_spawn_tool());
    assert!(!named(&[]).names_a_spawn_tool());
    assert!(
        !ToolScope::Wildcard.names_a_spawn_tool(),
        "a wildcard names nothing"
    );
}
