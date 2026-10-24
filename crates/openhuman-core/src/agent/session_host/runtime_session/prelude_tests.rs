use super::*;
use std::{collections::HashSet, sync::Arc};
use tinyagents_harness::tool::ToolSnapshot;
use tinytools::ToolSpec;

fn spec(name: &str, description: &str) -> Arc<ToolSpec> {
    Arc::new(ToolSpec {
        name: name.into(),
        description: description.into(),
        parameters: serde_json::json!({"type": "object"}),
    })
}

#[test]
fn a_deferred_host_tool_shadowing_a_builtin_is_granted_once_as_the_host_tool() {
    // `HostTurnTools::merge_into` splices host tools ahead of the
    // config-derived belt, so the registry holds the host's `file_read`
    // first and the built-in second.
    let registered = vec![
        spec("file_read", "host: read a project file"),
        spec("shell", "built-in shell"),
        spec("file_read", "built-in: read any file"),
    ];
    let visible = vec![spec("shell", "built-in shell")];
    let deferred: HashSet<String> = ["file_read".to_string()].into();

    let granted = granted_tool_specs(&visible, &registered, &deferred, false);

    let reads: Vec<_> = granted.iter().filter(|s| s.name == "file_read").collect();
    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0].description, "host: read a project file");
    let snapshot = ToolSnapshot::new(granted).expect("name-unique declarations freeze");
    assert_eq!(snapshot.specs().len(), 2);
}

#[test]
fn a_tool_both_advertised_and_deferred_keeps_the_advertised_declaration() {
    let registered = vec![spec("lookup", "deferred copy")];
    let visible = vec![spec("lookup", "advertised copy")];
    let deferred: HashSet<String> = ["lookup".to_string()].into();

    let granted = granted_tool_specs(&visible, &registered, &deferred, true);

    assert_eq!(granted.len(), 1);
    assert_eq!(granted[0].description, "advertised copy");
}
