use super::*;

#[test]
fn install_tool_is_external_effect_so_it_routes_through_approval_gate() {
    let tool = SkillRegistryInstallTool::new(Arc::new(Config::default()));
    assert_eq!(tool.name(), "skill_registry_install");
    // #3993: installs must raise an inline approval card before writing.
    assert!(
        tool.external_effect(),
        "skill_registry_install must declare external_effect so the harness gates it"
    );
    assert!(matches!(tool.permission_level(), PermissionLevel::Write));
}

#[test]
fn read_only_skill_tools_are_not_gated() {
    // Browse/search/sources stay ungated — they only read the catalog.
    assert!(!SkillRegistryBrowseTool.external_effect());
    assert!(!SkillRegistrySearchTool.external_effect());
    assert!(!SkillRegistrySourcesTool.external_effect());
}

async fn search_page(args: serde_json::Value) -> serde_json::Value {
    let result = SkillRegistrySearchTool
        .execute(args)
        .await
        .expect("execute");
    serde_json::from_str(&result.output()).expect("search result is json")
}

#[tokio::test]
async fn search_tool_returns_a_bounded_page_and_where_the_next_one_starts() {
    // #6286: a broad query used to return every match in one result.
    let _env = crate::skills::catalog::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("OPENHUMAN_SKILL_REGISTRY_CACHE_DIR", tmp.path());
    let catalog: Vec<_> = (0..45)
        .map(|i| {
            ops::parse_hermes_entry(&json!({
                "name": format!("review-{i:02}"),
                "description": "x",
                "source": "built-in"
            }))
            .expect("entry")
        })
        .collect();
    crate::skills::catalog::store::save_catalog_cache(&catalog);

    let first = search_page(json!({ "query": "review" })).await;
    assert_eq!(first["total"], 45);
    assert_eq!(
        first["count"], SEARCH_DEFAULT_LIMIT,
        "default page is bounded"
    );
    assert_eq!(
        first["entries"].as_array().map(Vec::len),
        Some(SEARCH_DEFAULT_LIMIT)
    );
    assert_eq!(first["next_offset"], SEARCH_DEFAULT_LIMIT);

    let last = search_page(json!({ "query": "review", "offset": 40, "limit": 10 })).await;
    assert_eq!(last["count"], 5);
    assert_eq!(last["entries"][0]["id"], "review-40");
    assert!(last["next_offset"].is_null(), "no page after the last one");

    let clamped = search_page(json!({ "query": "review", "limit": 0 })).await;
    assert_eq!(clamped["count"], 1, "limit is at least 1");

    crate::skills::catalog::store::clear_cache();
    std::env::remove_var("OPENHUMAN_SKILL_REGISTRY_CACHE_DIR");
}
