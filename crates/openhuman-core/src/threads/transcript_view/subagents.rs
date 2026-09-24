//! Sub-agent trails: project each delegated run's sibling transcript and
//! place it next to the tool call that spawned it.
//!
//! The transcript records no explicit delegation-call → file link: a child's
//! `_meta` carries its own `task_id`, the parent's rows carry only tool-call
//! ids, and neither names the other. So correlation is by evidence, in order:
//!
//! 1. **Turn** — the child's spawn time (the leading unix seconds of its stem
//!    suffix) against the parent turns' commit timestamps
//!    ([`anchor_request_id`]).
//! 2. **Call** — within that turn, the first unclaimed tool call that targets
//!    the child's agent (`delegate_{agent}`, an `agent_id` argument, …), else
//!    the first unclaimed delegation-shaped call.
//!
//! An uncorrelated child lands at the end of its turn (or of the list when
//! there are no turns) instead of after every root item, which is where all
//! sub-agents used to go.

use std::path::{Path, PathBuf};

use tinyagents_session::transcript::{self, DisplayRecord};

use super::project::{parse_native_tool_envelope, project_records};
use super::types::{DisplayItem, SubagentStatus, ToolCallStatus};

const LOG_PREFIX: &str = "[threads][transcript][subagents]";

/// Max sub-agent nesting depth the projection descends; bounded so a worker
/// that itself delegates still surfaces, without unbounded fan-out.
const MAX_SUBAGENT_DEPTH: usize = 3;

/// Prefix the delegation runner puts on a result it gave up on.
const INCOMPLETE_MARKER: &str = "[SUBAGENT_INCOMPLETE]";

/// Prefix of an async spawn's acknowledgement — success of the *spawn*, not
/// of the run, so it says nothing about the child's terminal state.
const ASYNC_ACCEPTED_PREFIX: &str = "Accepted async sub-agent";

/// Argument keys a spawn/delegate tool uses to name its target agent.
const TARGET_ARG_KEYS: &[&str] = &["agent_id", "agent", "subagent", "subagent_type", "target"];

/// A projected child run awaiting placement.
struct ChildRun {
    /// Unix seconds the child was spawned at, from its stem.
    spawn_unix: Option<i64>,
    agent_id: Option<String>,
    item: DisplayItem,
    /// The child's own terminal evidence, before the spawning call is known.
    own_state: OwnState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OwnState {
    Completed,
    Interrupted,
    Unknown,
}

/// Place every direct child of the root (`__`-once stems) into `items`.
/// `segments` are the root turns' `(request_id, commit unix)` pairs.
pub(super) fn attach(
    items: &mut Vec<DisplayItem>,
    sub_paths: &[PathBuf],
    segments: &[(String, i64)],
) {
    let children = build_children(sub_paths, None, 0);
    place(items, children, segments);
}

/// Project the direct children of `parent_stem` (or of the roots, when
/// `None`), recursing into their own children.
fn build_children(sub_paths: &[PathBuf], parent_stem: Option<&str>, depth: usize) -> Vec<ChildRun> {
    if depth >= MAX_SUBAGENT_DEPTH {
        return Vec::new();
    }
    let mut children = Vec::new();
    for path in sub_paths {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let suffix = match parent_stem {
            Some(parent) => match stem.strip_prefix(parent).and_then(|r| r.strip_prefix("__")) {
                Some(rest) if !rest.contains("__") => rest,
                _ => continue,
            },
            // A root's direct child has exactly one `__` separator.
            None => match stem.split_once("__") {
                Some((_, rest)) if !rest.contains("__") => rest,
                _ => continue,
            },
        };
        if let Some(child) = build_child(path, stem, suffix, sub_paths, depth) {
            children.push(child);
        }
    }
    children.sort_by_key(|child| child.spawn_unix);
    children
}

fn build_child(
    path: &Path,
    stem: &str,
    suffix: &str,
    sub_paths: &[PathBuf],
    depth: usize,
) -> Option<ChildRun> {
    let display = match transcript::read_transcript_display(path) {
        Ok(display) => display,
        Err(err) => {
            log::warn!(
                "{LOG_PREFIX} failed to read sub-agent transcript {}: {err}",
                path.display()
            );
            return None;
        }
    };
    let own_state = own_state(&display.records);
    let mut items = project_records(&display.records);
    let grandchildren = build_children(sub_paths, Some(stem), depth + 1);
    place(&mut items, grandchildren, &turn_segments(&display.records));

    let task_id = display.meta.task_id.clone().filter(|id| !id.is_empty());
    let agent_id = display
        .meta
        .agent_id
        .clone()
        .or_else(|| Some(display.meta.agent_name.clone()))
        .filter(|id| !id.is_empty());
    let id = task_id.clone().unwrap_or_else(|| suffix.to_string());
    Some(ChildRun {
        spawn_unix: child_spawn_unix(suffix),
        agent_id: agent_id.clone(),
        item: DisplayItem::Subagent {
            id,
            agent_id,
            task_id,
            call_id: None,
            status: SubagentStatus::Running,
            request_id: None,
            items,
        },
        own_state,
    })
}

/// What the child's own transcript says about how it ended.
fn own_state(records: &[DisplayRecord]) -> OwnState {
    let last = records.iter().rev().find_map(|record| match record {
        DisplayRecord::Message(msg) if msg.message.role != "system" => Some(msg),
        _ => None,
    });
    match last {
        Some(msg) if msg.interrupted => OwnState::Interrupted,
        Some(msg)
            if msg.message.role == "assistant"
                && parse_native_tool_envelope(&msg.message.content)
                    .is_none_or(|(_, calls)| calls.is_empty()) =>
        {
            OwnState::Completed
        }
        _ => OwnState::Unknown,
    }
}

/// Insert `children` into `items`, each after its correlated spawning call
/// (claimed at most once), else at the end of its anchored turn.
fn place(items: &mut Vec<DisplayItem>, children: Vec<ChildRun>, segments: &[(String, i64)]) {
    if children.is_empty() {
        return;
    }
    let mut claimed = vec![false; items.len()];
    // (insert position, order) — applied back-to-front afterwards.
    let mut inserts: Vec<(usize, usize, DisplayItem)> = Vec::new();
    for (order, mut child) in children.into_iter().enumerate() {
        let request_id = anchor_request_id(child.spawn_unix, segments);
        let (start, end) = turn_range(items, request_id.as_deref());
        let pick = find_spawning_call(items, &claimed, start, end, child.agent_id.as_deref());
        let (position, call) = match pick {
            Some(index) => {
                claimed[index] = true;
                (index + 1, Some(index))
            }
            None => (end, None),
        };
        let (call_id, call_status, call_result) = match call.and_then(|i| items.get(i)) {
            Some(DisplayItem::ToolCall {
                call_id,
                status,
                result,
                ..
            }) => (Some(call_id.clone()), Some(*status), result.clone()),
            _ => (None, None, None),
        };
        let status = derive_status(child.own_state, call_status, call_result.as_deref());
        if let DisplayItem::Subagent {
            id,
            call_id: call_slot,
            status: status_slot,
            request_id: request_slot,
            ..
        } = &mut child.item
        {
            log::debug!(
                "{LOG_PREFIX} subagent id={id} agent={:?} request_id={request_id:?} call_id={call_id:?} status={status:?}",
                child.agent_id
            );
            *call_slot = call_id;
            *status_slot = status;
            *request_slot = request_id;
        }
        inserts.push((position, order, child.item));
    }
    // Back-to-front keeps earlier positions valid; for one position, the
    // later child is inserted first so spawn order is preserved.
    inserts.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    for (position, _, item) in inserts {
        items.insert(position.min(items.len()), item);
    }
}

/// Terminal state from the spawning call's outcome and the child's own
/// transcript. A failed or incomplete delegation wins; then the child's own
/// ending; then a settled synchronous call.
fn derive_status(
    own: OwnState,
    call_status: Option<ToolCallStatus>,
    call_result: Option<&str>,
) -> SubagentStatus {
    let result = call_result.map(str::trim_start).unwrap_or_default();
    if call_status == Some(ToolCallStatus::Error) || result.starts_with(INCOMPLETE_MARKER) {
        return SubagentStatus::Failed;
    }
    match own {
        OwnState::Interrupted => SubagentStatus::Interrupted,
        OwnState::Completed => SubagentStatus::Completed,
        OwnState::Unknown
            if call_status == Some(ToolCallStatus::Success)
                && !result.starts_with(ASYNC_ACCEPTED_PREFIX) =>
        {
            SubagentStatus::Completed
        }
        OwnState::Unknown => SubagentStatus::Running,
    }
}

/// `[start, end)` of `request_id`'s items (after its boundary, up to the next
/// one); the whole list when the turn is unknown.
fn turn_range(items: &[DisplayItem], request_id: Option<&str>) -> (usize, usize) {
    let Some(request_id) = request_id else {
        return (0, items.len());
    };
    let Some(boundary) = items.iter().position(
        |item| matches!(item, DisplayItem::TurnBoundary { request_id: rid } if rid == request_id),
    ) else {
        return (0, items.len());
    };
    let end = items[boundary + 1..]
        .iter()
        .position(|item| matches!(item, DisplayItem::TurnBoundary { .. }))
        .map_or(items.len(), |offset| boundary + 1 + offset);
    (boundary + 1, end)
}

fn find_spawning_call(
    items: &[DisplayItem],
    claimed: &[bool],
    start: usize,
    end: usize,
    agent_id: Option<&str>,
) -> Option<usize> {
    let candidates = || {
        (start..end).filter_map(|index| match &items[index] {
            DisplayItem::ToolCall { name, args, .. } if !claimed[index] => {
                Some((index, name.as_str(), args.as_ref()))
            }
            _ => None,
        })
    };
    if let Some(agent_id) = agent_id {
        if let Some((index, ..)) =
            candidates().find(|(_, name, args)| call_targets_agent(name, *args, agent_id))
        {
            return Some(index);
        }
    }
    candidates()
        .find(|(_, name, _)| is_delegation_tool(name))
        .map(|(index, ..)| index)
}

/// Whether a tool call names `agent_id` as its delegation target.
fn call_targets_agent(name: &str, args: Option<&serde_json::Value>, agent_id: &str) -> bool {
    let agent = agent_id.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    if name == format!("delegate_{agent}") || name == format!("delegate_to_{agent}") {
        return true;
    }
    let named_in_args = args
        .and_then(serde_json::Value::as_object)
        .is_some_and(|args| {
            TARGET_ARG_KEYS.iter().any(|key| {
                args.get(*key)
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|value| value.eq_ignore_ascii_case(&agent))
            })
        });
    if named_in_args {
        return true;
    }
    // Alias tools such as `research` → `researcher`. The length floor keeps a
    // short generic tool name from matching an agent by accident.
    let stripped = name
        .strip_prefix("delegate_to_")
        .or_else(|| name.strip_prefix("delegate_"))
        .unwrap_or(&name);
    stripped.len() >= 5 && agent.starts_with(stripped)
}

fn is_delegation_tool(name: &str) -> bool {
    name.starts_with("delegate") || name.starts_with("spawn_")
}

/// The turns' `(request_id, commit unix)` pairs, in file order: the last
/// parseable timestamp of each `request_id` run.
///
/// Every stamped row of a turn carries the turn's *commit* time (the writer
/// stamps it when the turn is appended), so this is when the turn ended, not
/// when it began.
pub(super) fn turn_segments(records: &[DisplayRecord]) -> Vec<(String, i64)> {
    let mut segments: Vec<(String, i64)> = Vec::new();
    for record in records {
        let DisplayRecord::Message(msg) = record else {
            continue;
        };
        let (Some(rid), Some(ts)) = (msg.request_id.as_deref(), msg.ts.as_deref()) else {
            continue;
        };
        let Some(unix) = parse_rfc3339_unix(ts) else {
            continue;
        };
        match segments.last_mut() {
            Some((last, end)) if last == rid => *end = (*end).max(unix),
            _ => segments.push((rid.to_string(), unix)),
        }
    }
    segments
}

/// Extract a sub-agent's spawn unix timestamp (seconds) from its stem suffix
/// (`{unix}_{nanos}_{agent}…`). `None` for non-numeric legacy stems.
fn child_spawn_unix(stem_suffix: &str) -> Option<i64> {
    stem_suffix
        .split('_')
        .next()
        .and_then(|s| s.parse::<i64>().ok())
}

fn parse_rfc3339_unix(ts: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|dt| dt.timestamp())
}

/// Anchor a sub-agent to the turn that was running at `child_unix`: the first
/// turn whose commit time is at or after the spawn.
///
/// Fallbacks: no segments → `None` (unanchored); unknown spawn time, or a
/// spawn after every recorded commit (a turn still in flight) → the newest
/// turn.
fn anchor_request_id(child_unix: Option<i64>, segments: &[(String, i64)]) -> Option<String> {
    let last = segments.last()?;
    let Some(child_unix) = child_unix else {
        return Some(last.0.clone());
    };
    segments
        .iter()
        .find(|(_, end)| *end >= child_unix)
        .or(Some(last))
        .map(|(rid, _)| rid.clone())
}
