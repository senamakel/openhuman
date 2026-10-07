use std::sync::Arc;

use openhuman_embed::{
    Access, Agent, AgentDefinitionSpec, AgentSpec, AgentTurnOrigin, ApiKey, Core, CoreBuilder,
    CoreRuntime, Cron, CronError, DomainSet, GroupMode, Harness, HostKind, JobRun, JobRunRecord,
    JobSchedule, JobSpec, JobTarget, Provider, Runtime, RuntimeBuilder, RuntimeConfig,
    SandboxModeSpec, ScheduledJob, ServiceSet, SystemJobContext, ToolGroups, ToolScopeSpec,
    TrustedAccess, TrustedAutomationSource, Workspace,
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
    // The public-agent contract: lockdown, untrusted access, posture report.
    let _ = AgentSpec::new("public-bot")
        .definition(
            AgentDefinitionSpec::new().tools(ToolScopeSpec::Named(vec!["file_read".into()])),
        )
        .access(Access::public())
        .lockdown();
    async fn posture(agent: &Agent) -> Result<Vec<String>, openhuman_embed::AgentError> {
        agent.effective_tools(None).await
    }
    let _ = posture;
}

#[test]
fn exposes_the_scheduling_contract() {
    fn cron_of(runtime: &Runtime) -> Cron<'_> {
        runtime.cron()
    }
    fn upserts(cron: &Cron<'_>, spec: JobSpec) -> Result<ScheduledJob, CronError> {
        cron.upsert(spec)
    }
    fn lists(cron: &Cron<'_>) -> Result<Vec<ScheduledJob>, CronError> {
        cron.list()
    }
    fn removes(cron: &Cron<'_>) -> Result<bool, CronError> {
        cron.remove("job")
    }
    fn history(cron: &Cron<'_>) -> Result<Vec<JobRunRecord>, CronError> {
        cron.runs("job", 10)
    }
    async fn runs_now(cron: &Cron<'_>) -> Result<JobRun, CronError> {
        cron.run_now("job").await
    }
    fn handles(runtime: &Runtime) -> Result<(), CronError> {
        runtime.on_system_job("digest", |ctx: SystemJobContext| async move {
            let _ = (ctx.job_id, ctx.name);
            Ok(())
        })
    }
    async fn controls_services(runtime: &Runtime) {
        runtime.start_services().await;
        runtime.stop_services();
    }
    let _ = (cron_of, upserts, lists, removes, history, handles);
    let _ = runs_now;
    let _ = controls_services;

    let spec = JobSpec::agent(
        "morning",
        "teeny",
        "Plan the day.",
        JobSchedule::Cron {
            expr: "0 8 * * *".into(),
            tz: None,
        },
    )
    .retries(0)
    .single_flight(true)
    .enabled(true);
    assert_eq!(spec.retries, Some(0));
    let system = JobSpec::system("digest", "digest", JobSchedule::Every { ms: 60_000 });
    assert_eq!(
        system.target,
        JobTarget::System {
            name: "digest".into()
        }
    );
    let _ = JobSchedule::At {
        at: std::time::SystemTime::now(),
    };
}
