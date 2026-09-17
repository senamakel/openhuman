//! Resolution helpers for reading resources from discovered skills.

use crate::skills::ops_types::Workflow;

pub(super) fn resolve_workflow_for_resource(
    workflows: Vec<Workflow>,
    skill_id: &str,
) -> Result<Workflow, String> {
    let mut dir_match: Option<Workflow> = None;
    let mut name_match: Option<Workflow> = None;

    for workflow in workflows {
        if workflow.dir_name == skill_id {
            if dir_match.is_some() {
                return Err(format!(
                    "skill id '{skill_id}' is ambiguous across multiple skill directories"
                ));
            }
            dir_match = Some(workflow);
            continue;
        }
        if workflow.name == skill_id {
            if name_match.is_some() {
                return Err(format!(
                    "skill name '{skill_id}' is ambiguous; use the directory id"
                ));
            }
            name_match = Some(workflow);
        }
    }
    match (dir_match, name_match) {
        (Some(dir_skill), Some(name_skill)) if dir_skill.location == name_skill.location => {
            Ok(dir_skill)
        }
        (Some(_), Some(_)) => Err(format!(
            "skill id '{skill_id}' matches both a directory id and a different skill name"
        )),
        (Some(skill), None) | (None, Some(skill)) => Ok(skill),
        (None, None) => Err(format!("skill '{skill_id}' not found")),
    }
}
