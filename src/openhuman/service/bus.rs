use async_trait::async_trait;

use crate::openhuman::event_bus::{DomainEvent, EventHandler};

/// Long-lived event-bus subscriber that turns restart requests into a real
/// process respawn.
///
/// This subscriber is registered during core JSON-RPC bootstrap so any restart
/// request published from RPC, CLI, or another internal component goes through
/// the same execution path.
pub struct RestartSubscriber;

#[async_trait]
impl EventHandler for RestartSubscriber {
    fn name(&self) -> &str {
        "service::restart"
    }

    fn domains(&self) -> Option<&[&str]> {
        Some(&["system"])
    }

    async fn handle(&self, event: &DomainEvent) {
        let DomainEvent::SystemRestartRequested { source, reason } = event else {
            return;
        };

        log::warn!(
            "[service:restart] executing restart request source={} reason={}",
            source,
            reason
        );

        match crate::openhuman::service::restart::trigger_self_restart_now(source, reason) {
            Ok(child_pid) => {
                log::warn!(
                    "[service:restart] replacement pid={} spawned; exiting current process",
                    child_pid
                );
                // Brief 150ms grace period before exit: allows in-flight log
                // flushes and the replacement process to bind its listener before
                // this process terminates. Empirically tuned — increase if logs
                // are truncated on shutdown.
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                    std::process::exit(0);
                });
            }
            Err(err) => {
                log::error!("[service:restart] failed to restart current process: {err}");
            }
        }
    }
}
