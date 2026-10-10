use super::*;
use serde_json::json;

use serde_json::{Map, Value};
// ── schemas() coverage ────────────────────────────────────────────────────

#[test]
fn schemas_registry_search_has_no_required_inputs() {
    let s = schemas("registry_search");
    assert_eq!(s.namespace, "mcp_clients");
    assert!(s.inputs.iter().all(|f| !f.required));
}

#[test]
fn schemas_registry_get_requires_qualified_name() {
    let s = schemas("registry_get");
    let qn = s
        .inputs
        .iter()
        .find(|f| f.name == "qualified_name")
        .unwrap();
    assert!(qn.required);
}

#[test]
fn schemas_config_set_takes_the_document_root() {
    let s = schemas("config_set");
    assert_eq!(s.inputs.len(), 1);
    assert_eq!(s.inputs[0].name, "mcpServers");
    assert!(s.inputs[0].required);
    let outputs: Vec<_> = s.outputs.iter().map(|f| f.name).collect();
    assert_eq!(outputs, ["mcpServers", "added", "updated", "removed"]);
}

#[test]
fn schemas_config_get_has_no_inputs() {
    let s = schemas("config_get");
    assert!(s.inputs.is_empty());
    assert_eq!(s.outputs[0].name, "mcpServers");
}

#[test]
fn there_is_no_install_from_the_catalog_any_more() {
    // Installing is declaring a server in mcp.json; the catalog has no install RPC.
    assert_eq!(schemas("install").function, "unknown");
    assert_eq!(schemas("config_assist").function, "unknown");
}

#[test]
fn schemas_connect_requires_server_id() {
    let s = schemas("connect");
    let si = s.inputs.iter().find(|f| f.name == "server_id").unwrap();
    assert!(si.required);
}

#[test]
fn schemas_tool_call_requires_three_fields() {
    let s = schemas("tool_call");
    let required: Vec<_> = s.inputs.iter().filter(|f| f.required).collect();
    assert_eq!(required.len(), 3);
}

// ── all_controller_schemas / all_registered_controllers ────────────────────

#[test]
fn all_controller_schemas_covers_expected_methods() {
    let schemas = all_controller_schemas();
    // 17 mcp_clients: the catalog install and the configuration assistant went
    // with the setup agent; config_get / config_set (mcp.json) and list_tools
    // took their place.
    assert_eq!(schemas.len(), 17);
    assert!(schemas.iter().all(|s| s.namespace == "mcp_clients"));
    let functions: Vec<_> = schemas.iter().map(|s| s.function).collect();
    assert!(functions.contains(&"config_get"));
    assert!(functions.contains(&"config_set"));
    assert!(functions.contains(&"list_tools"));
    assert!(!functions.contains(&"install"));
    assert!(!functions.contains(&"config_assist"));
    // The #3039 + #3196 additions are present.
    assert!(functions.contains(&"update_env"));
    assert!(functions.contains(&"registry_settings_get"));
    assert!(functions.contains(&"registry_settings_set"));
    assert!(functions.contains(&"set_enabled"));
    // The #3495 OAuth/auth-detection additions are present.
    assert!(functions.contains(&"detect_auth"));
    assert!(functions.contains(&"oauth_begin"));
}

#[test]
fn all_registered_controllers_use_expected_namespaces() {
    for c in all_registered_controllers() {
        assert_eq!(
            c.schema.namespace, "mcp_clients",
            "unexpected namespace {}",
            c.schema.namespace
        );
    }
}

// ── read_required ─────────────────────────────────────────────────────────

#[test]
fn read_required_returns_value_for_present_key() {
    let mut params = Map::new();
    params.insert("server_id".into(), json!("srv-1"));
    let got: String = read_required(&params, "server_id").unwrap();
    assert_eq!(got, "srv-1");
}

#[test]
fn read_required_errors_on_missing_key() {
    let err = read_required::<String>(&Map::new(), "server_id").unwrap_err();
    assert!(err.contains("missing required param 'server_id'"));
}

// ── read_optional_u32 ─────────────────────────────────────────────────────

#[test]
fn read_optional_u32_absent_is_none() {
    assert_eq!(read_optional_u32(&Map::new(), "page").unwrap(), None);
}

#[test]
fn read_optional_u32_valid_number() {
    let mut p = Map::new();
    p.insert("page".into(), json!(2));
    assert_eq!(read_optional_u32(&p, "page").unwrap(), Some(2));
}

#[test]
fn read_optional_u32_rejects_negative() {
    let mut p = Map::new();
    p.insert("page".into(), json!(-1));
    assert!(read_optional_u32(&p, "page").is_err());
}

// ── type_name ─────────────────────────────────────────────────────────────

#[test]
fn type_name_covers_all_variants() {
    assert_eq!(type_name(&Value::Null), "null");
    assert_eq!(type_name(&json!(true)), "bool");
    assert_eq!(type_name(&json!(1)), "number");
    assert_eq!(type_name(&json!("s")), "string");
    assert_eq!(type_name(&json!([])), "array");
    assert_eq!(type_name(&json!({})), "object");
}
