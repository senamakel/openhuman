//! The session tool ceiling: the set of tool names a session — and every run
//! it starts — may ever reach.
//!
//! A ceiling is set on the session's config (`[agent] tool_ceiling`) by the
//! host. It is applied when the session is built
//! ([`OpenHumanSessionHost::build_session_agent_inner`]): every registered tool
//! and every synthesised delegation tool outside it is dropped, and the
//! surviving names become the sub-agent ceiling that
//! `subagent_host::ops::runner` intersects every child belt with.
//!
//! The tools that start work outside the session read it from the same config
//! they were built from, so the ceiling travels with the tool object rather
//! than through ambient state:
//!
//! * `run_workflow` refuses a workflow that declares a tool outside the
//!   ceiling and builds the run it starts with the same ceiling;
//! * `cron_add` / `schedule` refuse a shell job unless `shell` is inside it,
//!   and refuse an agent job outright, because a scheduled run is built later
//!   from the stored job and cannot carry the ceiling;
//! * `run_flow` / `resume_flow_run` refuse, because a flow's tool nodes are
//!   dispatched outside the session.
//!
//! Nesting can only narrow: a nested session's ceiling is the intersection of
//! its own config's and the one it inherits ([`ToolCeiling::narrow`]).
//!
//! [`OpenHumanSessionHost::build_session_agent_inner`]: crate::agent::OpenHumanSessionHost

use std::collections::BTreeSet;
use std::sync::Arc;

use tinytools::Tool;

/// Phrase every ceiling refusal carries, so a model and a host test can tell a
/// ceiling refusal from any other tool error.
pub const CEILING_REFUSAL: &str = "tool ceiling";

/// An immutable set of tool names. Cheap to clone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCeiling {
    names: Arc<BTreeSet<String>>,
}

impl ToolCeiling {
    /// A ceiling admitting exactly `names`.
    pub fn new<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            names: Arc::new(names.into_iter().map(Into::into).collect()),
        }
    }

    /// The ceiling `config` carries, if any.
    pub fn from_config(config: &crate::config::AgentConfig) -> Option<Self> {
        config.tool_ceiling.as_ref().map(Self::new)
    }

    /// Whether `name` is inside the ceiling.
    pub fn allows(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// The admitted names, sorted.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }

    /// The intersection with `other`. `None` leaves this ceiling unchanged:
    /// an absent ceiling never widens a present one.
    #[must_use]
    pub fn narrow(&self, other: Option<&ToolCeiling>) -> ToolCeiling {
        match other {
            Some(other) => Self::new(self.names().filter(|name| other.allows(name))),
            None => self.clone(),
        }
    }

    /// Drop every tool outside the ceiling, returning how many were dropped.
    pub fn retain_tools(&self, tools: &mut Vec<Box<dyn Tool>>) -> usize {
        let before = tools.len();
        tools.retain(|tool| self.allows(tool.name()));
        before - tools.len()
    }

    /// The names in `wanted` this ceiling does not admit, in order.
    pub fn missing<'a, I>(&self, wanted: I) -> Vec<&'a str>
    where
        I: IntoIterator<Item = &'a str>,
    {
        wanted.into_iter().filter(|name| !self.allows(name)).collect()
    }

    /// Narrow `config`'s ceiling by this one. A run built from the result
    /// reaches no tool this ceiling refuses.
    pub fn impose_on(&self, config: &mut crate::config::AgentConfig) {
        let narrowed = self.narrow(ToolCeiling::from_config(config).as_ref());
        config.tool_ceiling = Some(narrowed.names().map(str::to_string).collect());
    }
}

/// The refusal a nested-dispatch tool returns when starting `what` would
/// reach `missing` outside the session's ceiling.
pub fn refusal(tool: &str, what: &str, missing: &[&str]) -> String {
    if missing.is_empty() {
        format!(
            "{tool}: refused by this session's {CEILING_REFUSAL}: {what} cannot inherit the \
             ceiling, so it cannot be started from this session."
        )
    } else {
        format!(
            "{tool}: refused by this session's {CEILING_REFUSAL}: {what} needs {} outside the \
             tools this session may reach.",
            missing.join(", ")
        )
    }
}

/// The ceiling check for a job a scheduling tool is about to create.
///
/// `shell_command` is `true` for a job that runs a shell command and `false`
/// for one that runs an agent prompt. Returns the refusal, if any.
pub fn check_scheduled_job(
    ceiling: Option<&ToolCeiling>,
    tool: &str,
    shell_command: bool,
) -> Option<String> {
    let ceiling = ceiling?;
    let message = if shell_command {
        let missing = ceiling.missing(["shell"]);
        if missing.is_empty() {
            return None;
        }
        refusal(tool, "a scheduled shell job", &missing)
    } else {
        refusal(tool, "a scheduled agent job", &[])
    };
    log::debug!("[tool_ceiling] {tool} refused a scheduled job (shell={shell_command})");
    Some(message)
}

#[cfg(test)]
#[path = "tool_ceiling_tests.rs"]
mod tests;
