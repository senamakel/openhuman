//! Cross-scope name-collision resolution for discovered workflows: which
//! scope wins on a same-name (or same-`dir_name`) hit, and the warning left
//! on the survivor / shadowed entries.

use std::collections::HashMap;

use crate::skills::ops_types::{Workflow, WorkflowScope};

pub(super) fn absorb(by_name: &mut HashMap<String, Workflow>, incoming: Vec<Workflow>) {
    for mut skill in incoming {
        let key = skill.name.clone();
        // A workflow's runnable identity is `dir_name`, while `name` is only
        // display metadata. Collapse on either so a profile-local `foo/` also
        // shadows a global `foo/` whose frontmatter happens to use a different
        // display name. Otherwise registry lookup by slug could nondeterministically
        // select the global copy.
        let collision_keys: Vec<String> = by_name
            .iter()
            .filter(|(existing_name, existing)| {
                existing_name.as_str() == key || existing.dir_name == skill.dir_name
            })
            .map(|(existing_name, _)| existing_name.clone())
            .collect();

        if let Some((_, highest_name, highest_scope)) = collision_keys
            .iter()
            .filter_map(|collision_key| by_name.get(collision_key))
            .map(|existing| {
                (
                    precedence(existing.scope),
                    existing.name.clone(),
                    existing.scope,
                )
            })
            .max_by_key(|(rank, _, _)| *rank)
        {
            if precedence(skill.scope) < precedence(highest_scope) {
                if let Some(kept) = by_name.get_mut(&highest_name) {
                    kept.warnings.push(format!(
                        "workflow id '{}' or name '{}' also declared in {:?} scope at {} (ignored)",
                        skill.dir_name,
                        skill.name,
                        skill.scope,
                        skill
                            .location
                            .as_deref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "<unknown>".to_string())
                    ));
                }
                continue;
            }
        }

        for collision_key in collision_keys {
            if let Some(loser) = by_name.remove(&collision_key) {
                skill.warnings.push(format!(
                    "shadowed {:?}-scope skill '{}' (workflow id '{}') at {}",
                    loser.scope,
                    loser.name,
                    loser.dir_name,
                    loser
                        .location
                        .as_deref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "<unknown>".to_string())
                ));
            }
        }
        by_name.insert(key, skill);
    }
}

pub(super) fn precedence(scope: WorkflowScope) -> u8 {
    match scope {
        // Builtin sits below everything, including Legacy: a bundle that ships
        // with the binary must never shadow something the user installed or
        // wrote. Adding a builtin skill is then a change that cannot take a
        // name away from an existing workspace.
        WorkflowScope::Builtin => 0,
        WorkflowScope::Legacy => 1,
        WorkflowScope::User => 2,
        WorkflowScope::Project => 3,
        // Profile-local skills win against every global scope for their owner.
        WorkflowScope::Profile => 4,
        // Flows are never discovered by this scanner, so they never take part
        // in a name collision resolved here. Ranked above everything so that
        // if one ever reaches this function the answer is deterministic rather
        // than accidental.
        WorkflowScope::Flow => 5,
    }
}
