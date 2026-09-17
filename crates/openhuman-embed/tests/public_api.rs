use std::sync::Arc;

use openhuman_embed::{
    set_product_identity, Access, Agent, AgentDefinitionSpec, AgentSpec, AgentTurnOrigin, ApiKey,
    Core, CoreBuilder, CoreRuntime, DomainSet, GroupMode, Harness, HostKind, ProductIdentity,
    Provider, Runtime, RuntimeBuilder, RuntimeConfig, SandboxModeSpec, ServiceSet, ToolGroups,
    ToolScopeSpec, TrustedAccess, TrustedAutomationSource, Workspace,
};

#[test]
fn exposes_the_host_facing_embedding_contract() {
    fn accepts_core(_: Core) {}
    fn accepts_builder(_: CoreBuilder) {}
    fn accepts_runtime(_: Arc<CoreRuntime>) {}
    fn accepts_harness(_: Harness) {}
    fn accepts_access(_: Access) {}
    fn accepts_provider(_: Provider) {}
    fn accepts_workspace(_: Workspace) {}
    fn applies_turn_origin(
        turn: openhuman_embed::Turn,
        origin: AgentTurnOrigin,
    ) -> openhuman_embed::Turn {
        turn.origin(origin)
    }
    fn accepts_runtime_handle(_: Runtime) {}
    fn accepts_runtime_builder(_: RuntimeBuilder) {}
    fn accepts_agent(_: Agent) {}
    fn accepts_agent_spec(_: AgentSpec) {}
    fn accepts_api_key(_: ApiKey) {}

    let _ = accepts_core;
    let _ = accepts_builder;
    let _ = accepts_runtime;
    let _ = accepts_harness;
    let _ = accepts_access;
    let _ = accepts_provider;
    let _ = accepts_workspace;
    let _ = applies_turn_origin;
    let _ = accepts_runtime_handle;
    let _ = accepts_runtime_builder;
    let _ = accepts_agent;
    let _ = accepts_agent_spec;
    let _ = accepts_api_key;
    let _ = Runtime::builder()
        .api_key("th_public_api")
        .backend_url("https://backend.example");
    let _ = AgentSpec::new("public-api")
        .system_prompt("You are a test.")
        .definition(
            AgentDefinitionSpec::new()
                .tools(ToolScopeSpec::Named(vec!["read_file".into()]))
                .sandbox(SandboxModeSpec::ReadOnly),
        )
        .access(Access::readonly())
        .action_dir("/tmp/embed-public-api")
        .include_user_skills(false)
        .dedicated_memory(false)
        .config(|_config| {});
    let _ = DomainSet::embedded;
    let _ = ServiceSet::none;
    let _ = HostKind::Library;
    let runtime_config = RuntimeConfig::default();
    let _ = CoreBuilder::new(HostKind::Library).config(runtime_config.clone());
    let _ = Harness::builder().config(runtime_config);
    let _ = ToolGroups::none().with("documents", GroupMode::Advertised);
    let automation = AgentTurnOrigin::TrustedAutomation {
        job_id: "embed-public-api".to_string(),
        source: TrustedAutomationSource::Cron,
    };
    let _ = Access::full()
        .trust("/tmp/embed-public-api", TrustedAccess::ReadWrite)
        .origin(automation);
    let _ = set_product_identity;
    assert!(ProductIdentity::new("opencompany").is_some());
}
