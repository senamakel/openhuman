//! `skill_search` — find an installed skill without listing them all.
//!
//! # Why this exists
//!
//! Skills reach the model two ways today, and both scale badly with how many
//! are installed. The orchestrator prompt carries a `## Installed Skills`
//! catalogue — one line per skill, on every turn, forever — and `list_workflows`
//! serialises the **whole** [`Workflow`] struct for every skill, frontmatter
//! and resource list included. With a handful of skills neither matters. Now
//! that skills also ship inside the binary ([`super::bundled`]) and a catalogue
//! install is one tool call away, both are a bill that grows without anyone
//! deciding it should.
//!
//! Search is the third way: pay for the one skill that matches, and nothing for
//! the rest. It is the same bargain [`crate::openhuman::tools::implementations::meta::tool_search`]
//! makes for deferred tool schemas, over a different corpus.
//!
//! # What it returns, and what it does not
//!
//! A projection — id, name, a capped description, scope, tags — not the
//! `Workflow`. Returning the struct would reintroduce the cost the search
//! exists to avoid, and the body is one `describe_workflow` away for the one
//! skill the model actually picked.
//!
//! # The seam
//!
//! Ranking is [`crate::openhuman::util::bm25`], which names nothing from this
//! crate. The host-owned half is everything else in this file: which roots are
//! scanned, whether the workspace is trusted, which profile's private skills
//! are in scope, and the per-profile allowlist. That is the split to preserve
//! if this ever becomes a loadable module — a module can rank, but it cannot be
//! the thing that decides whose skills a caller may see.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::ops_discover::{discover_workflows_with_profile, is_workspace_trusted};
use super::ops_types::{Workflow, WorkflowScope};
use super::tools::{skill_allowed, SkillAllowlist};
use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{Tool, ToolResult};
use crate::openhuman::util::bm25::Bm25Index;

/// How many matches a search returns when the caller does not say.
const DEFAULT_LIMIT: usize = 5;
/// Ceiling on `limit`. A search that can return everything is `list_workflows`
/// with extra steps, and would spend exactly what this tool saves.
const MAX_LIMIT: usize = 20;
/// Cap on a returned description.
///
/// Skill descriptions are third-party metadata. The same 240 the orchestrator
/// prompt's catalogue uses — see `render_installed_skills` — so a skill cannot
/// present one size in the catalogue and a different one here.
const MAX_DESCRIPTION: usize = 240;

pub const SKILL_SEARCH_NAME: &str = "skill_search";

/// The text a skill is matched on.
///
/// `dir_name` is included as well as `name` because they diverge — the id the
/// model must eventually type is `dir_name`, and a skill whose frontmatter
/// display name differs would otherwise be unfindable by the name it is
/// actually called by.
fn searchable_text(workflow: &Workflow) -> String {
    format!(
        "{} {} {} {}",
        workflow.dir_name,
        workflow.name,
        workflow.description,
        workflow.tags.join(" ")
    )
}

fn id_of(workflow: &Workflow) -> &str {
    if workflow.dir_name.is_empty() {
        &workflow.name
    } else {
        &workflow.dir_name
    }
}

/// Rank `workflows` against `query`, best first, at most `limit` results.
///
/// Split out from the tool so the ranking is testable without a workspace, a
/// config or a filesystem.
pub fn rank<'a>(workflows: &'a [Workflow], query: &str, limit: usize) -> Vec<&'a Workflow> {
    let texts: Vec<(String, String)> = workflows
        .iter()
        .map(|w| (id_of(w).to_string(), searchable_text(w)))
        .collect();
    let index = Bm25Index::build(texts.iter().map(|(id, text)| (id.as_str(), text.as_str())));
    index
        .search(query, limit)
        .into_iter()
        .map(|i| &workflows[i])
        .collect()
}

/// The compact per-result projection.
fn project(workflow: &Workflow) -> Value {
    json!({
        "id": id_of(workflow),
        "name": workflow.name,
        // Sanitised for the same reason the prompt catalogue sanitises: this is
        // author-controlled text about to be read by a model as if the host had
        // said it. `sanitize_for_llm` strips control characters and instruction
        // fences and caps the length.
        "description": crate::openhuman::util::sanitize::sanitize_for_llm(
            &workflow.description,
            MAX_DESCRIPTION,
        ),
        "tags": workflow.tags,
        "scope": workflow.scope,
    })
}

/// Search installed skills by capability.
pub struct SkillSearchTool {
    workspace_dir: PathBuf,
    skill_allowlist: SkillAllowlist,
    profile_skills_root: Option<PathBuf>,
}

impl SkillSearchTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            workspace_dir: config.workspace_dir.clone(),
            skill_allowlist: None,
            profile_skills_root: None,
        }
    }

    /// Scope results to a per-profile allowlist of `dir_name` slugs.
    pub fn with_skill_allowlist(mut self, allowlist: SkillAllowlist) -> Self {
        self.skill_allowlist = allowlist;
        self
    }

    /// Include the active profile's private skills.
    pub fn with_profile_skills_root(mut self, root: Option<PathBuf>) -> Self {
        self.profile_skills_root = root;
        self
    }

    /// The visible corpus for this caller.
    ///
    /// Identical filtering to `list_workflows` — deliberately, and this is the
    /// part that must not drift: a search that saw one skill more than the list
    /// would be a way to discover a skill the profile was scoped away from.
    fn visible(&self) -> Vec<Workflow> {
        let trusted = is_workspace_trusted(&self.workspace_dir);
        let mut workflows = discover_workflows_with_profile(
            dirs::home_dir().as_deref(),
            Some(&self.workspace_dir),
            self.profile_skills_root.as_deref(),
            trusted,
        );
        if self.skill_allowlist.is_some() {
            workflows.retain(|w| {
                // Builtin and profile-local scopes bypass the allowlist — see
                // `tools::is_builtin_skill`. Search must apply exactly the
                // filter `list_workflows` applies; a search that saw one skill
                // more than the list would be a way around the scoping.
                w.scope == WorkflowScope::Builtin
                    || w.scope == WorkflowScope::Profile
                    || skill_allowed(&self.skill_allowlist, &w.dir_name)
            });
        }
        workflows
    }
}

#[async_trait]
impl Tool for SkillSearchTool {
    fn name(&self) -> &str {
        SKILL_SEARCH_NAME
    }

    fn description(&self) -> &str {
        "Find an installed skill by what it does. Give a plain-language query \
         (\"turn a repo into a changelog\", \"post to discord\") and get back the \
         best-matching skills with their id and description. Prefer this over \
         `list_workflows` when you know the capability you want but not the \
         name — it returns only what matched. Then `describe_workflow` for the \
         details, or `run_skill` to run it. Searches only what is installed \
         locally; use `skill_registry_search` to find skills to install."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What you want done, in plain language."
                },
                "limit": {
                    "type": "integer",
                    "description": format!(
                        "Maximum results (default {DEFAULT_LIMIT}, max {MAX_LIMIT})."
                    ),
                }
            },
            "required": ["query"]
        })
    }

    fn is_concurrency_safe(&self, _args: &Value) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if query.trim().is_empty() {
            return Ok(ToolResult::error(
                "skill_search needs a `query` describing what you want done.".to_string(),
            ));
        }
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| (n as usize).clamp(1, MAX_LIMIT))
            .unwrap_or(DEFAULT_LIMIT);

        let workflows = self.visible();
        let matches = rank(&workflows, query, limit);
        tracing::debug!(
            installed = workflows.len(),
            matched = matches.len(),
            limit,
            "[tool][skill_search] ranked"
        );

        Ok(ToolResult::success(serde_json::to_string(&render(
            &matches,
            workflows.len(),
        ))?))
    }
}

/// Build the tool's reply.
///
/// Split out from `execute` so it is testable without a workspace: the tool
/// itself discovers through `dirs::home_dir()`, so any test that went through
/// `execute` would rank against whatever skills the developer running it
/// happens to have installed. That is the same non-hermetic trap the prompt
/// budget lane hit — a test whose corpus is the machine it runs on.
fn render(matches: &[&Workflow], installed: usize) -> Value {
    if matches.is_empty() {
        // An explicit miss, not an empty list. "No skill matched" and "you have
        // no skills installed" call for different next moves, and a bare `[]`
        // does not distinguish them.
        return json!({
            "matched": 0,
            "installed": installed,
            "skills": [],
            "hint": "No installed skill matched. Try `skill_registry_search` to \
                     find one to install, or just do the task directly.",
        });
    }
    json!({
        "matched": matches.len(),
        "installed": installed,
        "skills": matches.iter().map(|w| project(w)).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod search_tests;
