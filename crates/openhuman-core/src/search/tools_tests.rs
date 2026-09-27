use super::*;
use crate::config::{SearchProviderSettings, SearchRoute};

fn signed_out_byok_config() -> Config {
    let mut config = Config::default();
    config.search.providers = [
        ("brave".to_string(), SearchProviderSettings::direct()),
        ("tavily".to_string(), SearchProviderSettings::direct()),
    ]
    .into_iter()
    .collect();
    config.search.brave.api_key = Some("b".into());
    config.search.tavily.api_key = Some("t".into());
    config
}

#[test]
fn classified_errors_become_actionable_messages() {
    let balance = user_facing_error("tinysearch.insufficient_balance: 402 from backend");
    assert!(balance.contains("balance is too low"));
    assert!(user_facing_error("tinysearch.rate_limited: slow down").contains("rate limited"));
    assert!(user_facing_error("tinysearch.provider_unavailable: all down").contains("unavailable"));
    assert_eq!(
        user_facing_error(
            "search ExecuteTool failed: tinysearch.invalid_arguments: urls must not be empty"
        ),
        "The search request was rejected: urls must not be empty"
    );
    assert_eq!(user_facing_error("boom"), "Web search failed: boom");
    assert_eq!(
        error_code("x tinysearch.rate_limited: y"),
        Some("rate_limited")
    );
    assert_eq!(error_code("plain"), None);
}

#[test]
fn role_tools_are_built_for_usable_byok_providers() {
    let config = signed_out_byok_config();
    let tools = build_search_tools(&config);
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert!(names.contains(&"web_search_tool"), "{names:?}");
    assert!(names.contains(&"web_contents_tool"), "{names:?}");
    // Nothing usable can answer without a session or a Gemini key.
    assert!(!names.contains(&"web_answer_tool"), "{names:?}");
    let search = tools
        .iter()
        .find(|t| t.name() == "web_search_tool")
        .unwrap();
    assert_eq!(search.category(), ToolCategory::Workflow);
    assert!(search.supports_markdown());
    assert!(search.parameters_schema()["properties"]["query"].is_object());
}

#[test]
fn no_tools_when_search_is_off_or_nothing_is_usable() {
    let mut config = signed_out_byok_config();
    config.search.enabled = Some(false);
    assert!(build_search_tools(&config).is_empty());

    let mut managed_only = Config::default();
    managed_only.search.providers.insert(
        "exa".into(),
        SearchProviderSettings {
            enabled: true,
            route: SearchRoute::Managed,
        },
    );
    // No backend credential in a default test config.
    assert!(build_search_tools(&managed_only).is_empty());
}

#[test]
fn recorded_tools_keep_their_declaration() {
    let spec = ToolSpec {
        name: "web_answer_tool".into(),
        description: "Grounded answers".into(),
        parameters: serde_json::json!({"type": "object"}),
    };
    let tool = TinySearchTool::recorded(spec.clone());
    assert_eq!(tool.name(), "web_answer_tool");
    assert_eq!(tool.spec(), &spec);
    assert_eq!(tool.exposure(), ToolExposure::Direct);
}
