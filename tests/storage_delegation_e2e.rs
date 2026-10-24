//! The delegation graph's durable checkpointer (`DriverCheckpointer` behind
//! `open_delegation_checkpointer`) on a configured backend: a checkpoint is
//! written and resumed, resumed again from a reopened backend, and two
//! agents' scopes keep their checkpoints apart.
//!
//! Its own test binary because it installs a backend into the process-wide
//! storage slot.

use std::sync::Arc;

use openhuman_core::agent::orchestration::open_delegation_checkpointer;
use openhuman_core::config::Config;
use openhuman_core::core::runtime::{
    AgentContextRegistry, ContextOverlay, CoreBuilder, CoreContext, DomainSet, ServiceSet,
};
use openhuman_core::HostKind;
use tinyagents_graph::checkpoint::{Checkpoint, Checkpointer};
use tinyagents_graph::delegation::DelegationState;

#[path = "support/tinyhumans_boot.rs"]
mod tinyhumans_boot;

#[path = "support/storage_case.rs"]
mod storage_case;

use storage_case::Case;

fn state(plan: &str) -> DelegationState {
    DelegationState {
        plan: Some(plan.to_string()),
        ..DelegationState::default()
    }
}

fn checkpoint(thread: &str, id: &str, plan: &str) -> Checkpoint<DelegationState> {
    Checkpoint::new(state(plan), Vec::new())
        .with_thread_id(thread)
        .with_checkpoint_id(id)
}

async fn plan_of(
    checkpointer: &Arc<dyn Checkpointer<DelegationState>>,
    thread: &str,
) -> Option<String> {
    checkpointer
        .get(thread, None)
        .await
        .unwrap()
        .and_then(|checkpoint| checkpoint.state.plan)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_delegation_checkpointer_resumes_and_keeps_scopes_apart() {
    let case = Case::open();
    let workspace = tempfile::tempdir().unwrap();
    let config = Config {
        workspace_dir: workspace.path().join("workspace"),
        config_path: workspace.path().join("config.toml"),
        ..Config::default()
    };
    tinyhumans_boot::boot();
    let _runtime = CoreBuilder::new(HostKind::Library)
        .config(config.clone())
        .services(ServiceSet::none())
        .domains(DomainSet::none())
        .build()
        .await
        .unwrap();
    case.install();

    let derive = |agent: &str| {
        let context = CoreContext::current().unwrap().derive_with(
            ContextOverlay::new(config.clone(), DomainSet::none(), Default::default())
                .session_agent(agent),
        );
        AgentContextRegistry::register(agent, &context);
        context
    };
    let alpha = derive("agent-alpha");
    let beta = derive("agent-beta");

    // Checkpoint as A, and as B under the very same thread id.
    CoreContext::scope(Arc::clone(&alpha), async {
        let checkpointer = open_delegation_checkpointer(&config).unwrap();
        checkpointer
            .put(checkpoint("run-1", "c1", "alpha plan, step 1"))
            .await
            .unwrap();
        checkpointer
            .put(checkpoint("run-1", "c2", "alpha plan, step 2"))
            .await
            .unwrap();
    })
    .await;
    CoreContext::scope(Arc::clone(&beta), async {
        let checkpointer = open_delegation_checkpointer(&config).unwrap();
        assert_eq!(
            plan_of(&checkpointer, "run-1").await,
            None,
            "B sees A's checkpoint"
        );
        checkpointer
            .put(checkpoint("run-1", "c1", "beta plan"))
            .await
            .unwrap();
    })
    .await;

    // Resume: a fresh handle reads the latest checkpoint of the thread, in
    // its own scope.
    for (context, expected, latest, named_c1) in [
        (&alpha, "alpha plan, step 2", "c2", "alpha plan, step 1"),
        (&beta, "beta plan", "c1", "beta plan"),
    ] {
        CoreContext::scope(Arc::clone(context), async {
            let checkpointer = open_delegation_checkpointer(&config).unwrap();
            let resumed = checkpointer.get("run-1", None).await.unwrap().unwrap();
            assert_eq!(resumed.checkpoint_id, latest);
            assert_eq!(resumed.state.plan.as_deref(), Some(expected));
            // A named checkpoint resolves in the same scope only.
            let first = checkpointer.get("run-1", Some("c1")).await.unwrap();
            assert_eq!(
                first
                    .and_then(|checkpoint| checkpoint.state.plan)
                    .as_deref(),
                Some(named_c1),
                "the named checkpoint is this agent's own"
            );
            assert_eq!(
                checkpointer.list_threads().await.unwrap(),
                vec!["run-1".to_string()]
            );
        })
        .await;
    }

    // Local (no acting agent) holds neither.
    let local = open_delegation_checkpointer(&config).unwrap();
    assert!(local.list_threads().await.unwrap().is_empty());
    assert_eq!(plan_of(&local, "run-1").await, None);

    // The checkpoint resumes after a durable backend is reopened, as a
    // restarted process would. The memory driver is intentionally volatile:
    // opening it again creates a new, empty backend, so it cannot exercise
    // restart persistence.
    if case.url != "memory" {
        let url = case.url.clone();
        let reopened =
            openhuman_core::storage::block_on(
                async move { openhuman_core::storage::open(&url).await },
            )
            .unwrap();
        openhuman_core::storage::install(reopened);
        CoreContext::scope(Arc::clone(&alpha), async {
            let checkpointer = open_delegation_checkpointer(&config).unwrap();
            assert_eq!(
                plan_of(&checkpointer, "run-1").await.as_deref(),
                Some("alpha plan, step 2"),
                "the checkpoint survived reopening the backend"
            );
        })
        .await;
    }

    assert!(AgentContextRegistry::deregister("agent-alpha", &alpha));
    assert!(AgentContextRegistry::deregister("agent-beta", &beta));
}
