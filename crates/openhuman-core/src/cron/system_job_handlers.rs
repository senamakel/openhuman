//! In-process handlers for system cron jobs.
//!
//! A system job (see [`crate::cron::system_jobs`]) normally *announces* itself:
//! the scheduler publishes `DomainEvent::CronSystemJobDue` and records the run
//! as `ok` the moment it is published, because the bus is fire-and-forget and
//! nothing comes back. A host that wants the run's real outcome in the job's
//! history registers a handler here instead. When the job comes due the
//! scheduler still publishes the event (observers keep working), then awaits
//! the handler and records **its** result: `ok`, or `error` with the handler's
//! message, subject to the job's retry budget like any other run.
//!
//! One handler per name; a later registration replaces an earlier one, and
//! dropping a [`SystemJobRegistration`] removes its handler only while it is
//! still the registered one.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use futures::future::BoxFuture;

/// The job a handler is asked to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemJobContext {
    /// The cron row's id.
    pub job_id: String,
    /// The system job's name: the `<name>` in its `system:<name>` command.
    pub name: String,
}

/// A handler for one system job. `Err` is recorded as the run's failure.
pub type SystemJobHandler =
    Arc<dyn Fn(SystemJobContext) -> BoxFuture<'static, Result<(), String>> + Send + Sync>;

static HANDLERS: LazyLock<RwLock<HashMap<String, SystemJobHandler>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Keeps a handler registered while held.
#[must_use = "the handler is unregistered when the registration drops"]
pub struct SystemJobRegistration {
    name: String,
    handler: SystemJobHandler,
}

impl Drop for SystemJobRegistration {
    fn drop(&mut self) {
        let mut handlers = HANDLERS.write().unwrap_or_else(PoisonError::into_inner);
        if handlers
            .get(&self.name)
            .is_some_and(|current| Arc::ptr_eq(current, &self.handler))
        {
            handlers.remove(&self.name);
            tracing::debug!(job = %self.name, "[cron:system_handlers] handler unregistered");
        }
    }
}

impl std::fmt::Debug for SystemJobRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemJobRegistration")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Register `handler` for system job `name`, replacing any earlier handler.
pub fn register(name: &str, handler: SystemJobHandler) -> SystemJobRegistration {
    HANDLERS
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(name.to_string(), Arc::clone(&handler));
    tracing::debug!(job = %name, "[cron:system_handlers] handler registered");
    SystemJobRegistration {
        name: name.to_string(),
        handler,
    }
}

/// Whether `name` has a handler.
pub fn has_handler(name: &str) -> bool {
    HANDLERS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .contains_key(name)
}

/// Run the handler registered for `ctx.name`. `None` when there is none.
pub async fn dispatch(ctx: SystemJobContext) -> Option<Result<(), String>> {
    let handler = HANDLERS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&ctx.name)
        .cloned()?;
    tracing::debug!(job = %ctx.name, job_id = %ctx.job_id, "[cron:system_handlers] dispatching");
    let result = handler(ctx.clone()).await;
    tracing::debug!(
        job = %ctx.name,
        ok = result.is_ok(),
        "[cron:system_handlers] handler finished"
    );
    Some(result)
}

#[cfg(test)]
#[path = "system_job_handlers_tests.rs"]
mod tests;
