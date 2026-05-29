//! Branch coverage for the workflows controller schemas.

use super::*;

#[test]
fn all_controller_schemas_covers_every_function() {
    let names: Vec<_> = all_controller_schemas()
        .into_iter()
        .map(|s| s.function)
        .collect();
    assert_eq!(names, vec!["list", "read", "create", "uninstall", "phase"]);
}

#[test]
fn all_registered_controllers_has_handler_per_schema() {
    let controllers = all_registered_controllers();
    assert_eq!(controllers.len(), 5);
    let names: Vec<_> = controllers.iter().map(|c| c.schema.function).collect();
    assert_eq!(names, vec!["list", "read", "create", "uninstall", "phase"]);
}

#[test]
fn schemas_have_workflows_namespace() {
    for f in ["list", "read", "create", "uninstall", "phase"] {
        assert_eq!(schemas(f).namespace, "workflows");
    }
}

#[test]
fn schemas_list_has_no_inputs_and_workflows_output() {
    let s = schemas("list");
    assert!(s.inputs.is_empty());
    assert_eq!(s.outputs[0].name, "workflows");
}

#[test]
fn schemas_create_requires_name_and_description() {
    let s = schemas("create");
    let required: Vec<_> = s
        .inputs
        .iter()
        .filter(|f| f.required)
        .map(|f| f.name)
        .collect();
    assert!(required.contains(&"name"));
    assert!(required.contains(&"description"));
}

#[test]
fn schemas_phase_requires_workflow_id_and_phase() {
    let s = schemas("phase");
    let names: Vec<_> = s.inputs.iter().map(|f| f.name).collect();
    assert!(names.contains(&"workflow_id"));
    assert!(names.contains(&"phase"));
}

#[test]
fn schemas_unknown_function_returns_placeholder() {
    let s = schemas("does-not-exist");
    assert_eq!(s.function, "unknown");
    assert_eq!(s.outputs[0].name, "error");
}
