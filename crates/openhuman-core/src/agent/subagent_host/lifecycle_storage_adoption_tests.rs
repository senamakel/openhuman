use super::*;
use crate::core::runtime::{ContextOverlay, CoreContext, DomainSet};
use tinyagents_orchestration::subagent::{
    PersistedSubagentPause, SubagentOutcomeKind, SubagentPause, SubagentPersistence,
    SubagentResume, SubagentTerminalPersistenceDisposition,
};

fn task() -> SubagentTaskKey {
    SubagentTaskKey {
        root_run_id: "storage-root".into(),
        parent_run_id: "storage-parent".into(),
        thread_id: Some("same-thread".into()),
        task_id: "same-child".into(),
    }
}

fn paused(key: &SubagentTaskKey) -> PersistedSubagentPause {
    let mut outcome = SubagentOutcome::cancelled(&key.task_id);
    outcome.output = "durable partial".into();
    outcome.status = SubagentOutcomeKind::AwaitingInput(SubagentPause {
        reason: "which source?".into(),
        resume: SubagentResume::default(),
    });
    PersistedSubagentPause {
        key: key.clone(),
        outcome,
        replaces: None,
    }
}

fn actor(workspace: &Path, profile: &str) -> std::sync::Arc<CoreContext> {
    let mut config = crate::config::Config::default();
    config.workspace_dir = workspace.to_path_buf();
    config.config_path = workspace.join("config.toml");
    CoreContext::for_test(DomainSet::full(), None).derive_with(
        ContextOverlay::new(config, DomainSet::full(), Default::default())
            .profile(profile)
            .session_agent("same-agent"),
    )
}

#[tokio::test]
async fn production_lifecycle_persistence_follows_the_explicit_actor_scope_on_reopen() {
    let workspace = tempfile::tempdir().unwrap();
    let checkpoints = workspace.path().join(".openhuman/subagent_checkpoints");
    let key = task();
    let alice = actor(workspace.path(), "lifecycle-alice");
    let bob = actor(workspace.path(), "lifecycle-bob");
    CoreContext::scope(alice.clone(), async {
        let adapter = super::super::persistence::OpenHumanPersistence::new(checkpoints.clone());
        adapter.save_pause(paused(&key)).await.unwrap();
        assert!(
            !checkpoints.join("lifecycle").exists(),
            "native storage creates no file-era persistence"
        );
        assert_eq!(
            adapter.checkpoint_for_task(&key.task_id).unwrap().output,
            "durable partial"
        );
    })
    .await;
    CoreContext::scope(bob, async {
        let adapter = super::super::persistence::OpenHumanPersistence::new(checkpoints.clone());
        assert!(adapter.load_pause(&key).await.unwrap().is_none());
        assert!(adapter.checkpoint_for_task(&key.task_id).is_err());
    })
    .await;
    CoreContext::scope(alice, async {
        let reopened = super::super::persistence::OpenHumanPersistence::new(checkpoints.clone());
        let durable = reopened.load_pause(&key).await.unwrap().unwrap();
        assert_eq!(durable.output, "durable partial");
        let SubagentOutcomeKind::AwaitingInput(pause) = durable.status else {
            panic!("durable pause")
        };
        let mut terminal = SubagentOutcome::cancelled(&key.task_id);
        terminal.output = "terminal winner".into();
        terminal
            .extensions
            .insert("agent_id".into(), serde_json::json!("researcher"));
        terminal
            .extensions
            .insert("elapsed_ms".into(), serde_json::json!(97));
        assert_eq!(
            reopened
                .record_terminal(&key, &terminal, Some(&pause.resume))
                .await
                .unwrap(),
            SubagentTerminalPersistenceDisposition::Inserted
        );
        let restarted = super::super::persistence::OpenHumanPersistence::new(checkpoints.clone());
        assert!(restarted.load_pause(&key).await.unwrap().is_none());
        // A stale legacy copy may still exist after read-only migration.
        let stale_path = restarted.legacy_pause_path(&key);
        std::fs::create_dir_all(stale_path.parent().unwrap()).unwrap();
        let stale =
            super::super::persistence::OpenHumanPersistence::checkpoint_from_persisted_pause(
                paused(&key),
            )
            .unwrap();
        std::fs::write(&stale_path, serde_json::to_vec(&stale).unwrap()).unwrap();
        assert!(restarted.checkpoint_for_task(&key.task_id).is_err());
        assert!(super::super::load_subagent_checkpoint(&checkpoints, &key.task_id).is_err());
        let terminal = restarted.load_terminal(&key).await.unwrap().unwrap();
        assert_eq!(terminal.output, "terminal winner");
        assert_eq!(terminal.extensions["agent_id"], "researcher");
        assert_eq!(terminal.extensions["elapsed_ms"], 97);
    })
    .await;
}

#[test]
fn legacy_host_kv_pause_is_converted_without_rewriting_the_source_record() {
    let workspace = tempfile::tempdir().unwrap();
    let store = LifecycleStore::open(workspace.path()).unwrap();
    let key = task();
    let mut checkpoint =
        super::super::persistence::OpenHumanPersistence::checkpoint_from_persisted_pause(paused(
            &key,
        ))
        .unwrap();
    checkpoint.agent_id = "researcher".into();
    checkpoint.elapsed_ms = 73;
    let record = LifecycleRecord {
        pause: Some(checkpoint),
        terminal: None,
    };
    store
        .kv
        .put(&LifecycleStore::record_name(&key), &record)
        .unwrap();
    let before = serde_json::to_value(store.record(&key).unwrap()).unwrap();
    let owner = store.neutral_store().unwrap();
    let durable = owner.pause_outcome(&key).unwrap().unwrap();
    assert_eq!(
        durable.extensions["openhuman_checkpoint"]["agent_id"],
        "researcher"
    );
    assert_eq!(durable.extensions["openhuman_checkpoint"]["elapsed_ms"], 73);
    assert_eq!(
        owner.keys_for_task(&key.task_id).unwrap(),
        vec![key.clone()]
    );
    assert_eq!(
        serde_json::to_value(store.record(&key).unwrap()).unwrap(),
        before
    );
}

#[test]
fn owner_file_pause_is_imported_and_the_original_bytes_stay_unchanged() {
    let workspace = tempfile::tempdir().unwrap();
    let key = task();
    let adapter =
        super::super::persistence::OpenHumanPersistence::new(workspace.path().to_path_buf());
    let file = adapter.legacy_pause_path(&key);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let bytes = serde_json::to_vec(&paused(&key)).unwrap();
    std::fs::write(&file, &bytes).unwrap();
    let store = LifecycleStore::open(workspace.path()).unwrap();
    let owner = store.neutral_store().unwrap();
    assert_eq!(
        owner.pause_outcome(&key).unwrap().unwrap().output,
        "durable partial"
    );
    assert_eq!(std::fs::read(file).unwrap(), bytes);
}

#[tokio::test]
async fn a_detached_driver_keeps_its_carried_actor_for_lifecycle_reads_and_commits() {
    use super::super::driver_adapter::{HostInvocation, HostRequest};
    use crate::agent::tinyagents::host::OpenHumanRunContext;
    use tinyagents_harness::context::{RunConfig, RunContext};

    struct CompleteTool;
    #[async_trait::async_trait]
    impl tinytools::Tool for CompleteTool {
        fn name(&self) -> &str {
            "complete_fixture"
        }
        fn description(&self) -> &str {
            "Finish without a provider"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type":"object"})
        }
        async fn execute(&self, _: serde_json::Value) -> anyhow::Result<tinytools::ToolResult> {
            Ok(tinytools::ToolResult::success("detached actor result"))
        }
    }

    let workspace = tempfile::tempdir().unwrap();
    let checkpoints = workspace.path().join(".openhuman/subagent_checkpoints");
    let alice = actor(workspace.path(), "detached-lifecycle-alice");
    let bob = actor(workspace.path(), "detached-lifecycle-bob");
    let parent = RunContext::new(
        RunConfig::new("detached-lifecycle-root").with_thread("detached-lifecycle-thread"),
        OpenHumanRunContext::new().with_core(Some(alice.clone())),
    );
    let task_id = "detached-actor-child";
    let key = SubagentTaskKey::from_context(&parent, task_id, None);
    let options = crate::agent::subagent_host::SubagentRunOptions {
        task_id: Some(task_id.into()),
        checkpoint_dir: Some(checkpoints.clone()),
        run_context: parent.data.clone(),
        ..Default::default()
    };
    let request = HostRequest {
        invocation: HostInvocation::ConfiguredTool {
            agent_id: "same-agent".into(),
            tool: std::sync::Arc::new(CompleteTool),
            arguments: serde_json::json!({}),
        },
        options,
    };
    let (done, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let completion: super::super::HostCompletion = std::sync::Arc::new(move |_, result, _| {
        let done = done.clone();
        Box::pin(async move {
            done.send(result).unwrap();
        })
    });
    // No ambient CoreContext is installed here. The actor is carried only
    // by the explicit parent, through the generic tool's real tokio::spawn.
    let (_, bundle) = super::super::invoke_driver_tool_with_request(
        &parent,
        request,
        "finish".into(),
        "background",
        Some(completion),
        Some(tinyagents_orchestration::subagent::SubAgentJobRegistry::default()),
        None,
    )
    .await
    .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), receiver.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    CoreContext::scope(alice, async {
        let restarted = super::super::persistence::OpenHumanPersistence::new(checkpoints.clone());
        let durable = restarted
            .load_terminal(&key)
            .await
            .unwrap()
            .expect("the detached driver committed in its carried actor scope");
        assert_eq!(durable.output, "detached actor result");
    })
    .await;
    CoreContext::scope(bob, async {
        let other = super::super::persistence::OpenHumanPersistence::new(checkpoints);
        assert!(other.load_terminal(&key).await.unwrap().is_none());
    })
    .await;
    // Keep the production bundle alive until its detached completion finished.
    drop(bundle);
}

async fn assert_legacy_terminal_fixture(format: &str) {
    use super::super::persistence::OpenHumanPersistence;
    let workspace = tempfile::tempdir().unwrap();
    let key = task();
    let mut outcome = SubagentOutcome::cancelled(&key.task_id);
    outcome.output = "legacy terminal".into();
    let file = workspace
        .path()
        .join("lifecycle")
        .join(format!("{}.terminal.json", super::key_component(&key)));
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let record = super::TerminalRecord {
        key: Some(key.clone()),
        outcome,
        agent_id: String::new(),
        elapsed_ms: 0,
        extensions: Default::default(),
    };
    let mut record = serde_json::to_value(record).unwrap();
    if format == "bare" {
        record = record["outcome"].clone();
        record["agent_id"] = serde_json::json!("legacy-owner");
        record["elapsed_ms"] = serde_json::json!(41);
        record["legacy_extra"] = serde_json::json!({"kept": true});
    } else {
        if format == "keyless" {
            record.as_object_mut().unwrap().remove("key");
        }
        record["agent_id"] = serde_json::json!("legacy-owner");
        record["elapsed_ms"] = serde_json::json!(41);
        record["legacy_extra"] = serde_json::json!({"kept": true});
    }
    let bytes = serde_json::to_vec_pretty(&record).unwrap();
    std::fs::write(&file, &bytes).unwrap();
    let adapter = OpenHumanPersistence::new(workspace.path().to_path_buf());
    let stale_path = adapter.legacy_pause_path(&key);
    let stale = OpenHumanPersistence::checkpoint_from_persisted_pause(paused(&key)).unwrap();
    let stale_bytes = serde_json::to_vec(&stale).unwrap();
    std::fs::write(&stale_path, &stale_bytes).unwrap();

    assert!(super::super::load_subagent_checkpoint(workspace.path(), &key.task_id).is_err());
    let terminal = adapter
        .load_terminal(&key)
        .await
        .unwrap()
        .expect("legacy terminal remains visible");
    assert_eq!(terminal.output, "legacy terminal");
    assert_eq!(terminal.extensions["agent_id"], "legacy-owner");
    assert_eq!(terminal.extensions["elapsed_ms"], 41);
    assert_eq!(terminal.extensions["legacy_extra"]["kept"], true);
    assert!(adapter.load_pause(&key).await.unwrap().is_none());
    assert!(super::super::load_subagent_checkpoint(workspace.path(), &key.task_id).is_err());
    let reopened = OpenHumanPersistence::new(workspace.path().to_path_buf());
    assert_eq!(
        reopened.load_terminal(&key).await.unwrap().unwrap(),
        terminal
    );
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    assert_eq!(std::fs::read(&stale_path).unwrap(), stale_bytes);
}

#[tokio::test]
async fn legacy_owner_terminal_key_bearing_keeps_flattened_extensions() {
    assert_legacy_terminal_fixture("current").await;
}

#[tokio::test]
async fn legacy_owner_terminal_keyless_keeps_host_metadata() {
    assert_legacy_terminal_fixture("keyless").await;
}

#[tokio::test]
async fn legacy_owner_terminal_bare_outcome_keeps_extensions() {
    assert_legacy_terminal_fixture("bare").await;
}
