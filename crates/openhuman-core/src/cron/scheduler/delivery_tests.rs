use super::*;

/// Deliver under a fresh run id and discard the delivery status.
pub(crate) async fn deliver_if_configured(
    ctx: &crate::core::runtime::DispatchCtx,
    config: &Config,
    job: &CronJob,
    output: &str,
    success: bool,
) -> Result<()> {
    let run_id = uuid::Uuid::new_v4().to_string();
    deliver_run(ctx, config, job, &run_id, output, success)
        .await
        .map(|_| ())
}
