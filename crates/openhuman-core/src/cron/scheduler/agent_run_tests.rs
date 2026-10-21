use super::*;

/// Run an agent job under a fresh run id for focused scheduler tests.
pub(crate) async fn run_agent_job(
    config: &Config,
    job: &CronJob,
) -> (bool, String, Option<String>) {
    let run_id = uuid::Uuid::new_v4().to_string();
    run_agent_job_for_run(
        &crate::core::runtime::DispatchCtx::host_default(),
        config,
        job,
        &run_id,
    )
    .await
}
