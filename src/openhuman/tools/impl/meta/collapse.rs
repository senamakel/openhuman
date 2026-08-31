//! Building blocks for collapsing a family of tools into one action-dispatched
//! tool.
//!
//! # Why collapse
//!
//! Every tool on the wire costs its name, its description and its full
//! parameter schema on every request. A family of six CRUD tools over one
//! resource pays that six times to say almost the same thing: `cron_list`,
//! `cron_add`, `cron_update`, `cron_remove`, `cron_run` and `cron_runs` were
//! 3,938 bytes between them, and four of the six are a `job_id` and nothing
//! else. Hermes reaches the same conclusion from the other direction — its
//! whole scheduler surface is a single `cronjob` tool, its whole memory surface
//! a single `memory`.
//!
//! # The two rules that make this safe
//!
//! Collapsing merges tools that the security layer had been judging
//! separately, and getting that wrong is how a token optimisation becomes a
//! privilege bug. So:
//!
//! 1. **The parameter schema is merged from the members, never retyped.** A
//!    hand-written union drifts the moment a member gains a field, and the
//!    drift is silent: the model is told about a parameter the implementation
//!    ignores, or not told about one it needs. [`merge_action_schemas`] derives
//!    it from the same `parameters_schema()` the members serve.
//! 2. **Permission is per action, and the argument-free answer is the
//!    strictest.** [`Tool::permission_level`] has no arguments, so a collapsed
//!    tool cannot answer it honestly; it returns the strictest level any member
//!    requires, and [`Tool::permission_level_with_args`] gives the exact one
//!    once the action is known. A caller that ignores the arguments therefore
//!    over-restricts rather than under-restricts.
//!
//! The same reasoning applies to [`Tool::external_effect`], which has no
//! argument-aware variant at all: a collapsed tool reports `true` if *any*
//! member does.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::openhuman::tools::{PermissionLevel, Tool};

/// One member of a collapsed family: the action name the model passes, and the
/// tool that serves it.
pub struct CollapsedAction<'a> {
    pub action: &'static str,
    pub tool: &'a dyn Tool,
}

/// Build the collapsed `parameters_schema` from the members' own schemas.
///
/// The result is an object with `action` (a required enum over the member
/// names) plus the union of every member's properties. Property descriptions
/// are prefixed with the action they belong to — the convention `memory_tree`
/// and `todo` already use — so the model can tell which fields apply to the
/// action it picked.
///
/// Nothing is `required` beyond `action`. A union cannot express "required for
/// this action only", and marking a field required because one action needs it
/// would make every other action's call invalid. The members already validate
/// their own required arguments and return a useful error, so the check lives
/// where it can be specific rather than in a schema that has to be vague.
pub fn merge_action_schemas(actions: &[CollapsedAction<'_>]) -> Value {
    let mut properties: BTreeMap<String, Value> = BTreeMap::new();
    // Track which actions mentioned each property so a shared field reads as
    // shared rather than as belonging to whichever action happened to be first.
    let mut owners: BTreeMap<String, Vec<&str>> = BTreeMap::new();

    for entry in actions {
        let schema = entry.tool.parameters_schema();
        let Some(props) = schema.get("properties").and_then(Value::as_object) else {
            continue;
        };
        for (name, spec) in props {
            owners
                .entry(name.clone())
                .or_default()
                .push(entry.action);
            properties.entry(name.clone()).or_insert_with(|| spec.clone());
        }
    }

    // Rewrite each description to name its actions. Done in a second pass so
    // the prefix can list every owner, which the first pass does not yet know.
    for (name, spec) in properties.iter_mut() {
        let Some(object) = spec.as_object_mut() else {
            continue;
        };
        let owned_by = owners.get(name).map(Vec::as_slice).unwrap_or(&[]);
        // A property every action takes needs no prefix — saying so would be
        // noise on every line.
        if owned_by.len() == actions.len() || owned_by.is_empty() {
            continue;
        }
        let prefix = owned_by.join("/");
        let existing = object
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let described = if existing.is_empty() {
            prefix
        } else {
            format!("{prefix}: {existing}")
        };
        object.insert("description".to_string(), Value::String(described));
    }

    let enum_values: Vec<Value> = actions
        .iter()
        .map(|entry| Value::String(entry.action.to_string()))
        .collect();

    let mut merged = Map::new();
    merged.insert(
        "action".to_string(),
        json!({
            "type": "string",
            "enum": enum_values,
            "description": "Which operation to run."
        }),
    );
    for (name, spec) in properties {
        merged.insert(name, spec);
    }

    json!({
        "type": "object",
        "properties": Value::Object(merged),
        "required": ["action"]
    })
}

/// The strictest permission level any member requires.
///
/// Used for the argument-free [`Tool::permission_level`], which cannot know
/// which action is coming. Over-restricting is the only safe direction.
pub fn strictest_permission(actions: &[CollapsedAction<'_>]) -> PermissionLevel {
    actions
        .iter()
        .map(|entry| entry.tool.permission_level())
        .max_by_key(permission_rank)
        .unwrap_or(PermissionLevel::None)
}

/// `true` when any member has an external effect.
pub fn any_external_effect(actions: &[CollapsedAction<'_>]) -> bool {
    actions.iter().any(|entry| entry.tool.external_effect())
}

/// Order the permission levels from least to most privileged.
///
/// `PermissionLevel` does derive `Ord` over explicit discriminants, so `.max()`
/// would work today. This exhaustive match is here for the day it gains a
/// variant: a new level would compile fine against `.max()` and silently take
/// whatever rank its discriminant implied, whereas here it is a compile error
/// until someone decides where it sits. Getting that wrong under-restricts a
/// collapsed tool, which is the failure this module exists to avoid.
fn permission_rank(level: &PermissionLevel) -> u8 {
    match level {
        PermissionLevel::None => 0,
        PermissionLevel::ReadOnly => 1,
        PermissionLevel::Write => 2,
        PermissionLevel::Execute => 3,
        PermissionLevel::Dangerous => 4,
    }
}

/// Find the member serving `action`.
pub fn resolve<'a>(
    actions: &'a [CollapsedAction<'a>],
    action: &str,
) -> Option<&'a CollapsedAction<'a>> {
    actions.iter().find(|entry| entry.action == action)
}

/// The error a collapsed tool returns for an unknown or missing action.
///
/// Lists the valid actions, because the model's next move after this message is
/// to guess, and a guess against a printed list is far more likely to be right.
pub fn unknown_action_message(actions: &[CollapsedAction<'_>], got: Option<&str>) -> String {
    let valid = actions
        .iter()
        .map(|entry| entry.action)
        .collect::<Vec<_>>()
        .join("|");
    match got {
        Some(other) => format!("unknown action '{other}' (expected {valid})"),
        None => format!("missing required field `action` (expected {valid})"),
    }
}

/// Strip the dispatch key before forwarding to the member.
///
/// The members are the same tools that serve the legacy names, and several of
/// them set `"additionalProperties": false`; leaving `action` in the object
/// would be rejected by any validation they do.
pub fn args_without_action(args: &Value) -> Value {
    match args.as_object() {
        Some(object) => {
            let mut cloned = object.clone();
            cloned.remove("action");
            Value::Object(cloned)
        }
        None => args.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::tools::ToolResult;
    use async_trait::async_trait;

    struct Stub {
        name: &'static str,
        schema: Value,
        permission: PermissionLevel,
        external: bool,
    }

    #[async_trait]
    impl Tool for Stub {
        fn name(&self) -> &str {
            self.name
        }
        fn description(&self) -> &str {
            "stub"
        }
        fn parameters_schema(&self) -> Value {
            self.schema.clone()
        }
        fn permission_level(&self) -> PermissionLevel {
            self.permission
        }
        fn external_effect(&self) -> bool {
            self.external
        }
        async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
            Ok(ToolResult::success("ok"))
        }
    }

    fn stub(name: &'static str, schema: Value, permission: PermissionLevel, external: bool) -> Stub {
        Stub {
            name,
            schema,
            permission,
            external,
        }
    }

    #[test]
    fn the_union_carries_every_members_properties() {
        let list = stub("list", json!({"type": "object", "properties": {}}), PermissionLevel::ReadOnly, false);
        let runs = stub(
            "runs",
            json!({"type": "object", "properties": {
                "job_id": {"type": "string"},
                "limit": {"type": "integer", "description": "How many."}
            }}),
            PermissionLevel::Read,
            false,
        );
        let actions = vec![
            CollapsedAction { action: "list", tool: &list },
            CollapsedAction { action: "runs", tool: &runs },
        ];
        let merged = merge_action_schemas(&actions);
        let props = merged["properties"].as_object().expect("properties");
        assert!(props.contains_key("action"));
        assert!(props.contains_key("job_id"));
        assert!(props.contains_key("limit"));
        assert_eq!(merged["required"], json!(["action"]));
    }

    #[test]
    fn only_action_is_required_because_a_union_cannot_say_otherwise() {
        // `job_id` is required for `runs` and meaningless for `list`. Marking
        // it required here would make every `list` call invalid.
        let list = stub("list", json!({"type": "object", "properties": {}}), PermissionLevel::ReadOnly, false);
        let runs = stub(
            "runs",
            json!({"type": "object", "properties": {"job_id": {"type": "string"}}, "required": ["job_id"]}),
            PermissionLevel::Read,
            false,
        );
        let actions = vec![
            CollapsedAction { action: "list", tool: &list },
            CollapsedAction { action: "runs", tool: &runs },
        ];
        assert_eq!(merge_action_schemas(&actions)["required"], json!(["action"]));
    }

    #[test]
    fn a_property_only_some_actions_take_is_labelled_with_them() {
        let a = stub("a", json!({"type": "object", "properties": {"shared": {"type": "string"}}}), PermissionLevel::ReadOnly, false);
        let b = stub(
            "b",
            json!({"type": "object", "properties": {
                "shared": {"type": "string"},
                "only_b": {"type": "string", "description": "B's field."}
            }}),
            PermissionLevel::Read,
            false,
        );
        let actions = vec![
            CollapsedAction { action: "a", tool: &a },
            CollapsedAction { action: "b", tool: &b },
        ];
        let merged = merge_action_schemas(&actions);
        let props = &merged["properties"];
        assert_eq!(props["only_b"]["description"], json!("b: B's field."));
        // Taken by every action, so no prefix — it would be noise.
        assert!(props["shared"].get("description").is_none());
    }

    #[test]
    fn permission_is_the_strictest_member_not_the_first() {
        let read = stub("r", json!({}), PermissionLevel::ReadOnly, false);
        let execute = stub("x", json!({}), PermissionLevel::Execute, false);
        let write = stub("w", json!({}), PermissionLevel::Write, false);
        let actions = vec![
            CollapsedAction { action: "r", tool: &read },
            CollapsedAction { action: "x", tool: &execute },
            CollapsedAction { action: "w", tool: &write },
        ];
        assert_eq!(strictest_permission(&actions), PermissionLevel::Execute);
    }

    #[test]
    fn external_effect_is_true_when_any_member_has_one() {
        let clean = stub("c", json!({}), PermissionLevel::ReadOnly, false);
        let dirty = stub("d", json!({}), PermissionLevel::ReadOnly, true);
        assert!(!any_external_effect(&[CollapsedAction { action: "c", tool: &clean }]));
        assert!(any_external_effect(&[
            CollapsedAction { action: "c", tool: &clean },
            CollapsedAction { action: "d", tool: &dirty },
        ]));
    }

    #[test]
    fn the_dispatch_key_does_not_reach_the_member() {
        // Several members set `additionalProperties: false`.
        let args = json!({"action": "runs", "job_id": "j1"});
        assert_eq!(args_without_action(&args), json!({"job_id": "j1"}));
    }

    #[test]
    fn an_unknown_action_names_the_valid_ones() {
        let a = stub("a", json!({}), PermissionLevel::ReadOnly, false);
        let actions = vec![CollapsedAction { action: "add", tool: &a }];
        assert_eq!(
            unknown_action_message(&actions, Some("addd")),
            "unknown action 'addd' (expected add)"
        );
        assert_eq!(
            unknown_action_message(&actions, None),
            "missing required field `action` (expected add)"
        );
    }
}
