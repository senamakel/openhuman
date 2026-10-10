//! Child-process environment carried by one dispatch task.

use std::{collections::BTreeMap, future::Future, sync::Arc};

tokio::task_local! { static ACTIVE: CommandEnvironment; }

/// A complete, turn-owned environment for builtin tool subprocesses.
/// This never mutates the host process environment. Host-spawned tasks must
/// explicitly inherit its scope. Interpreter pools cannot accept scoped work.
#[derive(Clone)]
pub struct CommandEnvironment(Arc<BTreeMap<String, String>>);

impl CommandEnvironment {
    /// Own the exact variables permitted in child processes.
    pub fn new(env: impl IntoIterator<Item = (String, String)>) -> Self {
        Self(Arc::new(env.into_iter().collect()))
    }
    /// Whether this task requires its own child environment.
    pub fn is_active() -> bool {
        ACTIVE.try_with(|_| ()).is_ok()
    }
    /// Run a future using this environment for builtin subprocesses.
    pub async fn scope<F: Future>(&self, future: F) -> F::Output {
        ACTIVE.scope(self.clone(), future).await
    }
    /// Read a functional child variable from the scoped map, falling back to
    /// the process only when no map was supplied.
    pub fn var_os(name: &str) -> Option<std::ffi::OsString> {
        ACTIVE
            .try_with(|environment| environment.0.get(name).map(Into::into))
            .unwrap_or_else(|_| std::env::var_os(name))
    }
    /// UTF-8 counterpart used by managed interpreter command builders.
    pub fn var(name: &str) -> Result<String, std::env::VarError> {
        ACTIVE
            .try_with(|environment| {
                environment
                    .0
                    .get(name)
                    .cloned()
                    .ok_or(std::env::VarError::NotPresent)
            })
            .unwrap_or_else(|_| std::env::var(name))
    }
    pub(super) fn apply(command: &mut tokio::process::Command) {
        let _ = ACTIVE.try_with(|environment| {
            // Explicit runtime/security additions (managed PATH, scratch TMP,
            // Git policy) remain authoritative over the base environment.
            let explicit: Vec<_> = command
                .as_std()
                .get_envs()
                .map(|(key, value)| (key.to_owned(), value.map(ToOwned::to_owned)))
                .collect();
            command.env_clear().envs(environment.0.iter());
            for (key, value) in explicit {
                match value {
                    Some(value) => {
                        command.env(key, value);
                    }
                    None => {
                        command.env_remove(key);
                    }
                }
            }
        });
    }
}
