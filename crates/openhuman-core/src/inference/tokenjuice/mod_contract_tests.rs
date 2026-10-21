use tinyjuice_bus::types::ContentKind;
use tinyjuice_bus::types::{CompressOptions, ContentHint};

#[test]
fn model_facing_tool_schemas_match_the_pre_migration_fixture() {
    use tinytools::Tool;

    let retrieve = tinyjuice_host::retrieve_tool::RetrieveToolOutputTool::new(
        std::sync::Arc::new(|_, _| Box::pin(async { Ok::<_, String>(None) })),
    );
    let mut declarations = vec![serde_json::json!({
            "name": retrieve.name(),
            "description": retrieve.description(),
            "parameters": retrieve.parameters_schema(),
        })];
    struct UnusedQuerySource;
    #[async_trait::async_trait]
    impl tinyjuice_host::repl_tools::QuerySource for UnusedQuerySource {
        async fn query(
            &self,
            _: Option<&dyn tinytools::ToolRunContext>,
            _: tinyjuice_bus::wire::QueryRequest,
        ) -> Result<tinyjuice_bus::wire::QueryResponse, String> {
            Err("schema fixture does not execute queries".into())
        }
    }
    let tools = tinyjuice_host::repl_tools::repl_tools(
        std::sync::Arc::new(UnusedQuerySource),
        tinyjuice_bus::repl::ReplLimits::default(),
        None,
        tinytools_std::filesystem::FileReadTool::MAX_FILE_SIZE_BYTES,
        None,
    );
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
    assert_eq!(declarations, fixture);
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
