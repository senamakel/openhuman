use super::*;
use crate::memory::test_fixtures::config_in;

#[test]
fn the_state_file_round_trips_through_the_atomic_store() {
    let tmp = tempfile::tempdir().unwrap();
    let store = FileCheckpointStore::new(tmp.path());
    assert_eq!(
        store.load(),
        ImportFile::default(),
        "no file reads as fresh"
    );
    let mut file = ImportFile::default();
    file.state.phase = ImportPhase::Done;
    file.state.imported = 7;
    file.listed_unconfirmed = true;
    store.save(&file).unwrap();
    assert_eq!(store.load(), file);
    assert!(listed_unconfirmed(tmp.path()));
    assert!(tmp.path().join("memory/import_state.json").is_file());
}

#[test]
fn a_damaged_state_file_reads_as_a_fresh_start() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("memory/import_state.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"{ not json").unwrap();
    assert_eq!(read_file(tmp.path()), ImportFile::default());
}

#[test]
fn a_running_state_with_no_live_run_reports_interrupted() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config_in(&tmp);
    let mut file = ImportFile::default();
    file.state.phase = ImportPhase::Running;
    FileCheckpointStore::new(&config.workspace_dir)
        .save(&file)
        .unwrap();
    let state = status(&config);
    assert_eq!(state.phase, ImportPhase::Error);
    assert!(state.error.unwrap().contains("interrupted"));
    assert!(
        !in_progress(&config),
        "a stale persisted Running phase is not a live import task"
    );
}

#[test]
fn a_credits_pause_blocks_organizing_until_resumed() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config_in(&tmp);
    let mut file = ImportFile::default();
    file.state.phase = ImportPhase::Error;
    file.paused_for_credits = true;
    FileCheckpointStore::new(&config.workspace_dir)
        .save(&file)
        .unwrap();
    assert!(in_progress(&config));
}

#[tokio::test]
async fn starting_without_consent_is_refused_before_anything_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config_in(&tmp);
    let error = start(
        &crate::core::runtime::DispatchCtx::ambient(),
        &config,
        false,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("consent"), "{error}");
    assert!(!is_running(&config.workspace_dir));
    assert_eq!(status(&config).phase, ImportPhase::Idle);
}

#[tokio::test]
async fn retrying_with_nothing_failed_is_refused_and_releases_the_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config_in(&tmp);
    // Either no engine is bound here or there is nothing to retry; either
    // way the call fails and the workspace must not stay claimed.
    assert!(
        retry_failed(&crate::core::runtime::DispatchCtx::ambient(), &config)
            .await
            .is_err()
    );
    assert!(!is_running(&config.workspace_dir));
}

#[tokio::test]
async fn nothing_resumes_when_no_import_was_started() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config_in(&tmp);
    let resumed = resume_interrupted_with(
        &crate::core::runtime::DispatchCtx::ambient(),
        &config,
        Arc::new(|| false),
        Arc::new(|_: &Config| true),
    )
    .await;
    assert!(!resumed);
}

#[tokio::test]
async fn a_paused_scheduler_leaves_an_interrupted_import_for_later() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config_in(&tmp);
    let mut file = ImportFile::default();
    file.state.phase = ImportPhase::Running;
    FileCheckpointStore::new(&config.workspace_dir)
        .save(&file)
        .unwrap();
    let resumed = resume_interrupted_with(
        &crate::core::runtime::DispatchCtx::ambient(),
        &config,
        Arc::new(|| true),
        Arc::new(|_: &Config| true),
    )
    .await;
    assert!(!resumed);
    assert_eq!(
        read_file(&config.workspace_dir).state.phase,
        ImportPhase::Running
    );
}

#[tokio::test]
async fn a_live_import_blocks_migration_and_does_not_make_a_stale_migration_look_live() {
    use crate::memory::layout_migration::{self, Phase, Trigger, state, test_host::FakeHost};

    let tmp = tempfile::tempdir().unwrap();
    let config = config_in(&tmp);
    let host = Arc::new(FakeHost::with(2, &config).await);
    let workspace = config.workspace_dir.clone();
    let _reservation = crate::memory::run_slots::reserve(
        &crate::memory::run_slots::IMPORT,
        &workspace,
        &[&crate::memory::run_slots::BACKFILL, &crate::memory::run_slots::MIGRATION],
    )
    .expect("the test owns a live import slot");

    let mut stale = state::load(&workspace).unwrap();
    stale.phase = Phase::Copying;
    state::save(&workspace, &stale).unwrap();

    let migration_status = layout_migration::status(&config).unwrap();
    assert!(!migration_status.running);
    assert!(migration_status.interrupted);
    assert!(
        !layout_migration::start(
            &crate::core::runtime::DispatchCtx::ambient(),
            config.clone(),
            host,
            Trigger::Auto,
            Arc::new(|| false),
        ),
        "an active import prevents migration from reserving the workspace"
    );
    assert!(!layout_migration::ops::is_running(&config));
}

#[tokio::test]
async fn a_credit_paused_import_waits_for_billing_and_records_a_resume_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config_in(&tmp);
    let mut file = ImportFile::default();
    file.state.phase = ImportPhase::Error;
    file.paused_for_credits = true;
    FileCheckpointStore::new(&config.workspace_dir)
        .save(&file)
        .unwrap();

    assert!(
        !resume_interrupted_with(
            &crate::core::runtime::DispatchCtx::ambient(),
            &config,
            Arc::new(|| false),
            Arc::new(|_: &Config| false),
        )
        .await
    );
    assert_eq!(read_file(&config.workspace_dir), file);

    assert!(
        !resume_interrupted_with(
            &crate::core::runtime::DispatchCtx::ambient(),
            &config,
            Arc::new(|| false),
            Arc::new(|_: &Config| true),
        )
        .await,
        "a permitted resume still reports failure when no engine can be bound"
    );
    let stopped = read_file(&config.workspace_dir);
    assert_eq!(stopped.state.phase, ImportPhase::Error);
    assert!(
        stopped
            .state
            .error
            .as_deref()
            .unwrap()
            .contains("could not resume")
    );
    assert!(!stopped.paused_for_credits);
    assert!(!is_running(&config.workspace_dir));
}

#[test]
fn a_store_with_no_legacy_files_counts_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(tinymemory_integrations::import::count_legacy(tmp.path()).is_none());
}
