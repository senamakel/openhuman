//! Whether a tool call can change anything outside the conversation, read
//! from the tool's own declarations where it makes them.
//!
//! The failure breaker needs this to decide what a timeout means. A read that
//! timed out can simply be retried; an action that timed out may already have
//! happened, so retrying it can repeat an effect the agent cannot observe. It
//! used to answer from a five-name allowlist, so `use_skill` →
//! `composio_list_tools` and every other read outside that list halted the
//! turn on its first timeout.
//!
//! Sources, strongest first:
//!
//! 1. [`tinytools::Tool::external_effect_with_args`]: the approval gate's own
//!    signal for an outbound message, payment, or remote write. Always wins.
//! 2. A classified [`tinytools::ToolPolicy`] declaring `read_only`, or a write,
//!    destructive, install, or payment side effect.
//! 3. The operation verb in the tool's name (or, for a dispatcher, its
//!    target's): a documented list in [`name_effect`]. A mutating verb wins
//!    over a reading one, so `list_and_send` is an action.
//! 4. [`tinytools::Tool::permission_level_with_args`] at `ReadOnly` or below,
//!    when the name carries no verb either way.
//!
//! Anything left over is [`CallEffect::Unknown`], which the breaker treats as
//! side-effecting.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use tinytools::{PermissionLevel, Tool};

/// What a failed call may have done to the world.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CallEffect {
    /// Reads, lists, searches: safe to retry.
    ReadOnly,
    /// May have changed state outside the conversation.
    SideEffecting,
    /// Nothing says either way; treated as side-effecting.
    #[default]
    Unknown,
}

/// What a registered tool declares about one call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ToolEffectFacts {
    /// From a classified `ToolPolicy`, when its side effects decide the
    /// question (`None` for an unclassified tool, or one declaring only
    /// network access).
    pub(crate) classified: Option<CallEffect>,
    /// `external_effect_with_args`: the approval gate's signal.
    pub(crate) external: bool,
    /// `permission_level_with_args` above `ReadOnly`.
    pub(crate) elevated: bool,
}

/// Looks up the declarations of a registered tool by name, for these args.
pub(crate) type ToolFactsLookup =
    Arc<dyn Fn(&str, &Value) -> Option<ToolEffectFacts> + Send + Sync>;

/// The facts `tool` declares for a call with `args`.
pub(crate) fn facts_for_tool(tool: &dyn Tool, args: &Value) -> ToolEffectFacts {
    let policy = tool.policy();
    let effects = &policy.side_effects;
    let classified = if !policy.classified {
        None
    } else if effects.read_only {
        Some(CallEffect::ReadOnly)
    } else if effects.writes_files
        || effects.installs_dependencies
        || effects.destructive
        || effects.payment
    {
        Some(CallEffect::SideEffecting)
    } else {
        None
    };
    ToolEffectFacts {
        classified,
        external: tool.external_effect_with_args(args),
        elevated: tool.permission_level_with_args(args) > PermissionLevel::ReadOnly,
    }
}

/// A lookup over the turn's registered tool sets.
pub(crate) fn tool_sets_lookup(tool_sets: Vec<Arc<Vec<Box<dyn Tool>>>>) -> ToolFactsLookup {
    let mut index: HashMap<String, (usize, usize)> = HashMap::new();
    for (set_idx, set) in tool_sets.iter().enumerate() {
        for (tool_idx, tool) in set.iter().enumerate() {
            index
                .entry(tool.name().to_string())
                .or_insert((set_idx, tool_idx));
        }
    }
    Arc::new(move |name: &str, args: &Value| {
        let (set_idx, tool_idx) = *index.get(name)?;
        let tool = tool_sets.get(set_idx)?.get(tool_idx)?;
        Some(facts_for_tool(tool.as_ref(), args))
    })
}

/// The tool a dispatcher call actually reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DispatchTarget<'a> {
    /// The skill (tool pack) named by the call, when the dispatcher has one.
    pub(crate) skill: Option<&'a str>,
    /// The target tool or action name.
    pub(crate) tool: &'a str,
    /// The arguments passed through to the target.
    pub(crate) args: Option<&'a Value>,
}

/// The connector dispatcher whose `tool` argument is a Composio action slug.
const COMPOSIO_EXECUTE: &str = "composio_execute";

/// The target of a dispatcher tool call: `use_skill` (`skill`, `tool`,
/// `args`) or `composio_execute` (`tool`, `arguments`). `None` for any other
/// tool, or when the call names no target.
pub(crate) fn dispatch_target<'a>(tool: &str, args: &'a Value) -> Option<DispatchTarget<'a>> {
    let (args_field, skill) = if tool == tinyagents_harness::tool::packs::USE_SKILL {
        ("args", args.get("skill").and_then(Value::as_str))
    } else if tool == COMPOSIO_EXECUTE {
        ("arguments", None)
    } else {
        return None;
    };
    let target = args
        .get("tool")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())?;
    Some(DispatchTarget {
        skill: skill.map(str::trim).filter(|s| !s.is_empty()),
        tool: target,
        args: args.get(args_field),
    })
}

/// Operation verbs that read without changing anything.
const READING_VERBS: &[&str] = &[
    "browse", "cat", "check", "count", "describe", "dig", "fetch", "find", "get", "grep", "info",
    "inspect", "list", "lookup", "ls", "peek", "poll", "preview", "query", "read", "recall",
    "retrieve", "search", "show", "stat", "status", "view", "whois",
];

/// Operation verbs that change state. One of these anywhere in the name makes
/// the call an action, even beside a reading verb.
const MUTATING_VERBS: &[&str] = &[
    "add",
    "apply",
    "approve",
    "archive",
    "authorize",
    "book",
    "bridge",
    "buy",
    "cancel",
    "clear",
    "close",
    "commit",
    "connect",
    "copy",
    "create",
    "delete",
    "deploy",
    "disable",
    "disconnect",
    "edit",
    "enable",
    "exec",
    "execute",
    "follow",
    "forward",
    "insert",
    "install",
    "invite",
    "kill",
    "like",
    "mark",
    "merge",
    "mint",
    "modify",
    "move",
    "open",
    "patch",
    "pay",
    "post",
    "publish",
    "purchase",
    "push",
    "put",
    "remove",
    "rename",
    "replace",
    "reply",
    "reset",
    "restart",
    "run",
    "save",
    "schedule",
    "send",
    "set",
    "share",
    "sign",
    "star",
    "start",
    "stop",
    "store",
    "submit",
    "swap",
    "transfer",
    "trash",
    "uninstall",
    "update",
    "upload",
    "upsert",
    "write",
];

/// The effect the operation verbs in `name` imply: `Unknown` when it carries
/// none. Names are split on anything that is not a letter or digit, so
/// `composio_list_tools` and `GMAIL_FETCH_EMAILS` both read as reads.
pub(crate) fn name_effect(name: &str) -> CallEffect {
    let mut reads = false;
    for token in name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
    {
        let token = token.to_ascii_lowercase();
        if MUTATING_VERBS.contains(&token.as_str()) {
            return CallEffect::SideEffecting;
        }
        reads |= READING_VERBS.contains(&token.as_str());
    }
    if reads {
        CallEffect::ReadOnly
    } else {
        CallEffect::Unknown
    }
}

/// The effect of one tool (not a dispatcher) from its facts and its name.
fn leaf_effect(facts: Option<ToolEffectFacts>, name: &str) -> CallEffect {
    if let Some(facts) = facts {
        if facts.external {
            return CallEffect::SideEffecting;
        }
        if let Some(classified) = facts.classified {
            return classified;
        }
    }
    match name_effect(name) {
        CallEffect::Unknown => match facts {
            Some(facts) if !facts.elevated => CallEffect::ReadOnly,
            _ => CallEffect::Unknown,
        },
        known => known,
    }
}

/// What a call to `tool` with `args` may have done. `lookup` supplies the
/// registered tools' declarations; without it only names are read.
///
/// A dispatcher is judged by its target, unless the dispatcher itself already
/// declares an external effect for these arguments (`composio_execute` does
/// for a mutating action slug).
pub(crate) fn call_effect(
    lookup: Option<&ToolFactsLookup>,
    tool: &str,
    args: &Value,
) -> CallEffect {
    let facts = lookup.and_then(|lookup| lookup(tool, args));
    let Some(target) = dispatch_target(tool, args) else {
        return leaf_effect(facts, tool);
    };
    if facts.is_some_and(|f| f.external || f.classified == Some(CallEffect::SideEffecting)) {
        return CallEffect::SideEffecting;
    }
    let inner_args = target.args.unwrap_or(&Value::Null);
    let inner_facts = lookup.and_then(|lookup| lookup(target.tool, inner_args));
    // A target the lookup does not know (a Composio slug, a packed tool) is
    // judged by its name alone: being reached through a read-only-declared
    // dispatcher says nothing about it.
    leaf_effect(inner_facts, target.tool)
}
