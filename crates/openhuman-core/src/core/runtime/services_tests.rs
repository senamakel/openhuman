use super::*;

#[tokio::test]
async fn legacy_migrations_run_for_each_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let mut first = Config::default();
    first.workspace_dir = tmp.path().join("first");
    let mut second = Config::default();
    second.workspace_dir = tmp.path().join("second");

    for config in [first, second] {
        run_legacy_migrations(&config).await;
    }
}

/// desktop() must enable every bootstrap job — proves the un-bundling kept
/// the desktop job set byte-identical.
#[test]
fn desktop_plan_enables_every_job() {
    let plan = bootstrap_job_plan(&ServiceSet::desktop());
    assert_eq!(
        plan,
        BootstrapJobPlan {
            memory_queue: true,
            composio_integration_sync: true,
            workspace_memory_sync: true,
            proactive_task_pollers: true,
            module_preload: true,
        }
    );
}

/// none() / headless_api() run no bootstrap job at all.
#[test]
fn job_free_presets_enable_nothing() {
    let empty = BootstrapJobPlan {
        memory_queue: false,
        composio_integration_sync: false,
        workspace_memory_sync: false,
        proactive_task_pollers: false,
        module_preload: false,
    };
    assert_eq!(bootstrap_job_plan(&ServiceSet::none()), empty);
    assert_eq!(bootstrap_job_plan(&ServiceSet::headless_api()), empty);
}

/// From none(), flipping exactly one concern flag enables exactly its job
/// and nothing else.
#[test]
fn each_concern_flag_enables_exactly_its_job() {
    let mut integrations = ServiceSet::none();
    integrations.integrations = true;
    let plan = bootstrap_job_plan(&integrations);
    assert!(plan.composio_integration_sync);
    assert!(!plan.workspace_memory_sync);
    assert!(!plan.memory_queue);
    assert!(!plan.proactive_task_pollers);

    let mut memory_sync = ServiceSet::none();
    memory_sync.memory_sync = true;
    let plan = bootstrap_job_plan(&memory_sync);
    assert!(plan.workspace_memory_sync);
    assert!(!plan.composio_integration_sync);
    assert!(!plan.module_preload);

    // The module preload is memory background work: it follows the queue
    // flag and no other.
    let mut memory_queue = ServiceSet::none();
    memory_queue.memory_queue = true;
    let plan = bootstrap_job_plan(&memory_queue);
    assert!(plan.module_preload);
    assert!(plan.memory_queue);
    assert!(!plan.workspace_memory_sync);
    assert!(!plan.composio_integration_sync);
    assert!(!plan.proactive_task_pollers);
}

/// From desktop(), disabling exactly one concern flag disables only its job.
#[test]
fn disabling_one_concern_disables_only_its_job() {
    let mut services = ServiceSet::desktop();
    services.integrations = false;
    let plan = bootstrap_job_plan(&services);
    assert!(!plan.composio_integration_sync);
    assert!(plan.workspace_memory_sync);
    assert!(plan.memory_queue);
    assert!(plan.proactive_task_pollers);

    let mut services = ServiceSet::desktop();
    services.memory_sync = false;
    let plan = bootstrap_job_plan(&services);
    assert!(!plan.workspace_memory_sync);
    assert!(plan.composio_integration_sync);
}

/// The #5028 regression: `channels` gates NO bootstrap job. Turning channels
/// on by itself must enable zero sync jobs, and turning channels off while
/// the new flags stay on must lose nothing.
#[test]
fn channels_flag_gates_no_bootstrap_job() {
    // channels=true alone → zero sync jobs.
    let mut channels_only = ServiceSet::none();
    channels_only.channels = true;
    let plan = bootstrap_job_plan(&channels_only);
    assert_eq!(
        plan,
        bootstrap_job_plan(&ServiceSet::none()),
        "channels alone must enable no bootstrap job"
    );

    // channels=false with every new flag on → identical to desktop's plan.
    let mut channels_off = ServiceSet::desktop();
    channels_off.channels = false;
    assert_eq!(
        bootstrap_job_plan(&channels_off),
        bootstrap_job_plan(&ServiceSet::desktop()),
        "dropping channels must not drop any bootstrap job"
    );
}
