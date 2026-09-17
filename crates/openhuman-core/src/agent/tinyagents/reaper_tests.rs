use super::*;

use tinyagents_harness::ids::ComponentId;

use crate::agent::tinyagents::journal::mint_run_id;

/// Build a fresh status in the given non-terminal state and persist it.
async fn seed_status(store: &FileStatusStore, status_kind: ExecutionStatus) -> String {
    let run_id = mint_run_id();
    let mut status =
        HarnessRunStatus::new(run_id.clone(), ComponentId::new("mock-model".to_string()));
    match status_kind {
        ExecutionStatus::Pending => { /* fresh status is already Pending */ }
        ExecutionStatus::Running => status.mark_running(HarnessPhase::Model),
        ExecutionStatus::Interrupted => status.mark_interrupted(),
        other => panic!("seed_status only seeds non-terminal states, got {other:?}"),
    }
    store.put_status(status).await.unwrap();
    run_id.as_str().to_string()
}

/// The sweep reaps every non-terminal run to `Cancelled` with the reason,
/// leaves terminal runs untouched, and empties the active listing.
#[tokio::test]
async fn reap_cancels_every_active_run_and_spares_terminal_ones() {
    let tmp = std::env::temp_dir().join(format!("oh-reaper-{}", uuid::Uuid::new_v4()));
    let store = FileStatusStore::new(open_session_stores(&tmp).kv);

    let pending = seed_status(&store, ExecutionStatus::Pending).await;
    let running = seed_status(&store, ExecutionStatus::Running).await;
    let interrupted = seed_status(&store, ExecutionStatus::Interrupted).await;

    // A run that already finished must survive the sweep unchanged.
    let done = mint_run_id();
    let mut done_status =
        HarnessRunStatus::new(done.clone(), ComponentId::new("mock-model".to_string()));
    done_status.mark_running(HarnessPhase::Model);
    done_status.mark_completed();
    store.put_status(done_status).await.unwrap();

    let reaped = reap_orphaned_runs(&tmp).await;
    assert_eq!(reaped, 3, "the three non-terminal runs were reaped");

    // Every orphan is now terminal-cancelled with the stable reason.
    for run_id in [&pending, &running, &interrupted] {
        let status = store
            .get_status(run_id)
            .await
            .unwrap()
            .expect("status present");
        assert_eq!(status.status, ExecutionStatus::Cancelled);
        assert_eq!(status.current_phase, HarnessPhase::Done);
        assert_eq!(status.error.as_deref(), Some(ORPHAN_REAP_REASON));
        assert!(status.ended_at.is_some(), "reaped run has an end time");
    }

    // The completed run is left exactly as it was.
    let done_after = store
        .get_status(done.as_str())
        .await
        .unwrap()
        .expect("done present");
    assert_eq!(done_after.status, ExecutionStatus::Completed);
    assert!(done_after.error.is_none());

    // The active listing is now empty — a second sweep is a no-op.
    assert!(store.list_active().await.unwrap().is_empty());
    assert_eq!(
        reap_orphaned_runs(&tmp).await,
        0,
        "idempotent: nothing left to reap"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

/// A workspace that never hosted a run reaps nothing and does not error.
#[tokio::test]
async fn reap_on_empty_workspace_is_a_noop() {
    let tmp = std::env::temp_dir().join(format!("oh-reaper-empty-{}", uuid::Uuid::new_v4()));
    assert_eq!(reap_orphaned_runs(&tmp).await, 0);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// The sweep has to happen on the path a **build-only** embedder takes.
///
/// `CoreRuntime::invoke` dispatches `openhuman.agent_runs_active` the moment
/// `build()` returns, with no transport and no background services in
/// between, so a sweep that lived with the other boot-once jobs (which run
/// from `serve()`) would leave that caller reading the previous process's
/// graveyard. This pins the sweep to the build path with every optional
/// service off.
///
/// The build itself runs in a **child process** (this same test binary,
/// re-invoked with `--exact` on this test and `REAPER_BOOT_CHILD` set).
/// `CoreBuilder::build` → `CoreContext::init` → `bootstrap_core_runtime`
/// installs process-wide `OnceLock` singletons that can never be unset — the
/// default `CoreContext` (here with `DomainSet::harness()` and a throw-away
/// workspace), the global `ApprovalGate`, the socket manager, native-bus
/// handlers. Booting in the lib test binary leaked all of them into every
/// test that ran afterwards under `--test-threads=1`: `all_tools` lost its
/// acting tool groups, the tinyflows HTTP adapter hit the origin-label gate
/// instead of the SSRF guard, memory-binding tests saw a workspace they never
/// set up. The parent seeds the store and verifies the sweep; only the child
/// touches the singletons, and it exits with them.
#[tokio::test]
async fn a_build_only_runtime_is_swept_before_it_can_be_invoked() {
    if std::env::var_os(REAPER_BOOT_CHILD_ENV).is_some() {
        boot_child().await;
        return;
    }

    let tmp = std::env::temp_dir().join(format!("oh-reaper-boot-{}", uuid::Uuid::new_v4()));
    // `OPENHUMAN_WORKSPACE` names the root; the resolved workspace is the
    // `workspace` directory under it, which is what the sweep will read.
    let workspace = tmp.join("workspace");
    let store = FileStatusStore::new(open_session_stores(&workspace).kv);
    let orphan = seed_status(&store, ExecutionStatus::Running).await;
    assert_eq!(store.list_active().await.unwrap().len(), 1);

    let exe = std::env::current_exe().expect("test binary path");
    let output = std::process::Command::new(exe)
        .args([
            "--exact",
            "agent::tinyagents::reaper::tests::\
             a_build_only_runtime_is_swept_before_it_can_be_invoked",
            "--test-threads=1",
            "--nocapture",
        ])
        .env(REAPER_BOOT_CHILD_ENV, "1")
        .env("OPENHUMAN_WORKSPACE", &tmp)
        .output()
        .expect("spawn the boot child");
    assert!(
        output.status.success(),
        "the boot child must build a core and pass its own assertions\n\
         --- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // The child writes the durable store in a separate process. Reopen it for
    // verification instead of reading through the parent's handle, whose
    // refactored KV layer may still serve the status it cached while seeding.
    let after_store = FileStatusStore::new(open_session_stores(&workspace).kv);
    let after = after_store
        .get_status(&orphan)
        .await
        .unwrap()
        .expect("the seeded run is still readable");
    assert_eq!(
        after.status,
        ExecutionStatus::Cancelled,
        "build() must reap before any RPC can be dispatched"
    );
    assert_eq!(after.error.as_deref(), Some(ORPHAN_REAP_REASON));
    assert!(after_store.list_active().await.unwrap().is_empty());

    let _ = std::fs::remove_dir_all(&tmp);
}

/// Set on the re-invoked test binary so the test body knows it is the boot
/// child rather than the seeding/verifying parent.
const REAPER_BOOT_CHILD_ENV: &str = "OPENHUMAN_TEST_REAPER_BOOT_CHILD";

/// The child half of `a_build_only_runtime_is_swept_before_it_can_be_invoked`:
/// build a runtime with no transport and no services — the shape
/// `examples/embed_headless.rs` documents — against the `OPENHUMAN_WORKSPACE`
/// the parent exported, and check the build resolved that workspace. The
/// parent reads the sweep's effect out of the store afterwards.
async fn boot_child() {
    use crate::core::runtime::{CoreBuilder, DomainSet, ServiceSet};
    use crate::core::types::HostKind;

    let root = std::path::PathBuf::from(
        std::env::var_os("OPENHUMAN_WORKSPACE").expect("parent exports OPENHUMAN_WORKSPACE"),
    );
    let built = CoreBuilder::new(HostKind::Cli)
        .services(ServiceSet::none())
        .domains(DomainSet::harness())
        .build()
        .await
        .expect("a headless build succeeds");
    assert_eq!(
        built.context().workspace_dir().ok(),
        Some(root.join("workspace")),
        "the build must have resolved the workspace the parent seeded"
    );
}
