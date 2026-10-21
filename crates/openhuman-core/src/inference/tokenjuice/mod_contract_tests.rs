use tinyjuice_bus::types::ContentKind;
use tinyjuice_bus::types::{CompressOptions, ContentHint};

#[test]
fn model_facing_tool_schemas_match_the_pre_migration_fixture() {
    use tinytools::Tool;

    let retrieve =
        tinyjuice_host::retrieve_tool::RetrieveToolOutputTool::new(std::sync::Arc::new(|_, _| {
            Box::pin(async { Ok::<_, String>(None) })
        }));
    let mut declarations = vec![serde_json::json!({
        "name": retrieve.name(),
        "description": retrieve.description(),
        "parameters": retrieve.parameters_schema(),
    })];
    let config = crate::config::Config::default();
    let tools = super::repl_tools_for(&config);
    for tool in tools {
        declarations.push(serde_json::json!({
            "name": tool.name(),
            "description": tool.description(),
            "parameters": tool.parameters_schema(),
        }));
    }

    let fixture: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("tool_schema_fixtures.json"))
            .expect("pre-migration declarations are valid JSON");
    assert_eq!(
        declarations
            .iter()
            .map(|declaration| declaration["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "juice_retrieve",
            "juice_find",
            "juice_extract",
            "juice_summarize"
        ]
    );
    assert_eq!(
        serde_json::to_vec(&declarations).expect("serialize current declarations"),
        serde_json::to_vec(&fixture).expect("serialize captured declarations"),
        "all four names, descriptions and schemas stay byte-identical"
    );
}

#[test]
fn resumed_repl_tools_keep_the_recorded_declarations() {
    use tinytools::Tool;

    let config = crate::config::Config::default();
    let live = super::repl_tools_for(&config);
    assert_eq!(live.len(), 3);
    let recorded = live
        .iter()
        .map(|tool| tinytools::ToolSpec {
            name: tool.name().to_owned(),
            description: format!("frozen declaration for {}", tool.name()),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "recorded_schema_marker": { "type": "string" } },
                "required": ["recorded_schema_marker"]
            }),
        })
        .collect::<Vec<_>>();
    let rebuilt = super::repl_tools_for_recorded(Some(&config), &config.workspace_dir, &recorded);
    let rebuilt = rebuilt
        .iter()
        .map(|tool| tinytools::ToolSpec {
            name: tool.name().to_owned(),
            description: tool.description().to_owned(),
            parameters: tool.parameters_schema(),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        serde_json::to_vec(&rebuilt).expect("serialize rebuilt declarations"),
        serde_json::to_vec(&recorded).expect("serialize recorded declarations"),
    );
}

#[test]
fn compress_options_accept_omitted_fields() {
    let options: CompressOptions = serde_json::from_value(serde_json::json!({
        "routerEnabled": false
    }))
    .expect("partial options remain forward-compatible");
    assert!(!options.router_enabled);
    assert!(options.ccr_enabled);
    assert_eq!(options.min_bytes_to_compress, 2048);
}

#[test]
fn content_hint_matches_the_module_wire_shape() {
    let hint = ContentHint {
        source_tool: Some("shell".to_string()),
        explicit: Some(ContentKind::PlainText),
        ..ContentHint::default()
    };
    assert_eq!(
        serde_json::to_value(hint).expect("serialize hint"),
        serde_json::json!({
            "mime": null,
            "extension": null,
            "sourceTool": "shell",
            "query": null,
            "explicit": "plainText",
            // Added to the contract alongside the source-stub work. `Exact`
            // is the default and the shape the module expects; pinning it
            // here keeps this test doing its job — catching a wire change
            // rather than absorbing one.
            "readIntent": "Exact"
        })
    );
}
