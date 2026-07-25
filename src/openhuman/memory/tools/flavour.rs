//! Agent tool: read a compiled persona flavour profile (issue #5172).
//!
//! Persona ingestion (`src/openhuman/tinycortex/persona.rs`) distills a
//! person's coding-agent history into seven [`PersonaFacet`] flavoured trees
//! (communication, coding style, stack, workflow, environment, directives,
//! anti-preferences), each compiled into a small prompt-ready markdown
//! profile via [`compile_flavoured_root`]. Until this tool, nothing surfaced
//! those compiled profiles to the agent loop — the ingested data sat unread.
//! `memory_flavour` lets an agent pull one facet's profile on demand.
//!
//! Strictly read-only: it never ingests, seals, or otherwise creates persona
//! evidence. The only disk write it can trigger is `compile_flavoured_root`
//! re-staging the fixed-path compiled artifact — a pure, idempotent
//! projection of the tree's existing root node (see
//! `vendor/tinycortex/src/memory/tree/flavoured.rs`), not new memory content.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use tinycortex::memory::persona::PersonaFacet;
use tinycortex::memory::tree::store::{get_tree_by_scope, TreeKind};
use tinycortex::memory::tree::{compile_flavoured_root, flavoured_root_abs_path};

use crate::openhuman::config::Config;
use crate::openhuman::tinycortex::memory_config_from;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolResult};

/// The seven valid `flavour` slugs, for error messages.
const VALID_FLAVOURS: &str =
    "communication, coding_style, stack, workflow, environment, directives, anti_preferences";

/// Let the agent read the compiled persona profile for one facet.
pub struct MemoryFlavourTool {
    config: Arc<Config>,
}

impl MemoryFlavourTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

/// Strip the YAML front matter written by [`compile_flavoured_root`]
/// (`---\n...\n---\n<body>`) and return just the body. Front-matter field
/// values are single-line (`yaml_quote` collapses interior newlines), so the
/// first `\n---\n` after the opening delimiter is always the closing one.
fn body_after_front_matter(content: &str) -> &str {
    match content.strip_prefix("---\n") {
        Some(rest) => match rest.find("\n---\n") {
            Some(pos) => &rest[pos + "\n---\n".len()..],
            // Malformed front matter (opener present but no closer): fall
            // back to everything after the opening delimiter rather than
            // the raw content, so the opener itself is never leaked.
            None => rest,
        },
        None => content,
    }
}

#[async_trait]
impl Tool for MemoryFlavourTool {
    fn name(&self) -> &str {
        "memory_flavour"
    }

    fn description(&self) -> &str {
        "Read the compiled persona profile for one distillation facet, built from this \
         person's coding-agent history. Valid `flavour` values: communication (tone, \
         verbosity, feedback style), coding_style (naming, structure, testing habits), \
         stack (languages, frameworks, architecture), workflow (branching, PR habits, \
         parallelism), environment (editors, harnesses, CLIs, OS), directives (explicit \
         standing rules), anti_preferences (things to never do). Returns markdown prose, or \
         a clear message if no profile has been built yet. Read-only."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "flavour": {
                    "type": "string",
                    "description": "Which persona facet to read: communication, coding_style, \
                        stack, workflow, environment, directives, or anti_preferences."
                }
            },
            "required": ["flavour"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        // Genuinely read-only: this tool never ingests, seals, or writes
        // memory content. Overridden explicitly (not relying on the trait
        // default) so a future default change can't silently loosen this.
        PermissionLevel::ReadOnly
    }

    fn permission_level_with_args(&self, _args: &serde_json::Value) -> PermissionLevel {
        // No arg combination for this tool escalates past ReadOnly.
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let flavour_raw = args
            .get("flavour")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'flavour' parameter"))?
            .trim();
        if flavour_raw.is_empty() {
            return Err(anyhow::anyhow!("'flavour' cannot be empty"));
        }

        let facet = PersonaFacet::parse_loose(flavour_raw).ok_or_else(|| {
            anyhow::anyhow!("Unknown flavour '{flavour_raw}'. Valid flavours: {VALID_FLAVOURS}")
        })?;

        let mc = memory_config_from(&self.config, self.config.workspace_dir.clone());
        let scope = facet.tree_scope();
        let heading = facet.heading();

        tracing::debug!(
            target: "memory_flavour",
            flavour = flavour_raw,
            facet = ?facet,
            "[memory_flavour] entry"
        );

        // Fast path: the compiled artifact already exists on disk with a
        // non-empty body — read it directly without touching the tree store.
        let abs_path = flavoured_root_abs_path(&mc, &scope);
        if abs_path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&abs_path) {
                let body = body_after_front_matter(&content);
                if !body.trim().is_empty() {
                    tracing::debug!(
                        target: "memory_flavour",
                        flavour = flavour_raw,
                        body_len = body.len(),
                        "[memory_flavour] fast path hit: returning stripped body from disk"
                    );
                    return Ok(ToolResult::success(body.to_string()));
                }
            }
        }

        tracing::debug!(
            target: "memory_flavour",
            flavour = flavour_raw,
            "[memory_flavour] fast path missed or empty, falling to tree lookup"
        );

        // Slow path: look up the flavoured tree and (re)compile its root.
        match get_tree_by_scope(&mc, TreeKind::Flavoured, &scope) {
            Ok(None) => {
                tracing::debug!(
                    target: "memory_flavour",
                    flavour = flavour_raw,
                    "[memory_flavour] no flavoured tree exists yet"
                );
                Ok(ToolResult::success(format!(
                    "No profile built yet for {heading}. Run persona ingestion first, then try \
                     again."
                )))
            }
            Ok(Some(tree)) => {
                tracing::debug!(
                    target: "memory_flavour",
                    flavour = flavour_raw,
                    tree_id = %tree.id,
                    "[memory_flavour] tree found, compiling root"
                );
                match compile_flavoured_root(&mc, &tree.id) {
                    Ok(markdown) => {
                        let body = body_after_front_matter(&markdown);
                        if body.trim().is_empty() {
                            Ok(ToolResult::success(format!(
                                "No profile built yet for {heading}. Run persona ingestion \
                                 first, then try again."
                            )))
                        } else {
                            tracing::debug!(
                                target: "memory_flavour",
                                flavour = flavour_raw,
                                body_len = body.len(),
                                "[memory_flavour] compiled profile returned"
                            );
                            Ok(ToolResult::success(body.to_string()))
                        }
                    }
                    Err(err) => {
                        tracing::warn!(
                            %err,
                            flavour = flavour_raw,
                            "[memory_flavour] failed to compile flavoured profile"
                        );
                        Ok(ToolResult::error(format!(
                            "Failed to compile the {heading} profile: {err}"
                        )))
                    }
                }
            }
            Err(err) => {
                tracing::warn!(
                    %err,
                    flavour = flavour_raw,
                    "[memory_flavour] failed to look up flavoured tree"
                );
                Ok(ToolResult::error(format!(
                    "Failed to look up the {heading} profile: {err}"
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_config() -> (TempDir, Arc<Config>) {
        let tmp = TempDir::new().unwrap();
        let mut cfg = Config::default();
        cfg.workspace_dir = tmp.path().to_path_buf();
        (tmp, Arc::new(cfg))
    }

    #[test]
    fn name_and_schema() {
        let (_tmp, cfg) = test_config();
        let tool = MemoryFlavourTool::new(cfg);
        assert_eq!(tool.name(), "memory_flavour");
        assert_eq!(tool.parameters_schema()["required"], json!(["flavour"]));
        assert!(tool.parameters_schema()["properties"]["flavour"].is_object());
    }

    #[test]
    fn permission_level_is_read_only() {
        let (_tmp, cfg) = test_config();
        let tool = MemoryFlavourTool::new(cfg);
        assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
    }

    #[test]
    fn permission_level_with_args_is_always_read_only() {
        let (_tmp, cfg) = test_config();
        let tool = MemoryFlavourTool::new(cfg);
        assert_eq!(
            tool.permission_level_with_args(&json!({})),
            PermissionLevel::ReadOnly
        );
        assert_eq!(
            tool.permission_level_with_args(&json!({"flavour": "communication"})),
            PermissionLevel::ReadOnly
        );
    }

    #[tokio::test]
    async fn missing_flavour_is_error() {
        let (_tmp, cfg) = test_config();
        let tool = MemoryFlavourTool::new(cfg);
        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn empty_flavour_is_error() {
        let (_tmp, cfg) = test_config();
        let tool = MemoryFlavourTool::new(cfg);
        let result = tool.execute(json!({"flavour": "   "})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn unknown_flavour_is_error() {
        let (_tmp, cfg) = test_config();
        let tool = MemoryFlavourTool::new(cfg);
        let result = tool.execute(json!({"flavour": "astrology"})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown flavour"));
    }

    #[tokio::test]
    async fn valid_flavour_with_no_tree_yet_returns_no_profile_message() {
        let (_tmp, cfg) = test_config();
        let tool = MemoryFlavourTool::new(cfg);
        let result = tool
            .execute(json!({"flavour": "coding_style"}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.output().contains("No profile built yet"));
    }

    #[tokio::test]
    async fn aliases_are_accepted() {
        for alias in ["comms", "coding", "env", "rules", "dislikes"] {
            let (_tmp, cfg) = test_config();
            let tool = MemoryFlavourTool::new(cfg);
            let result = tool.execute(json!({"flavour": alias})).await;
            assert!(result.is_ok(), "alias `{alias}` should be accepted");
            let result = result.unwrap();
            assert!(!result.is_error, "alias `{alias}` should not error");
            assert!(result.output().contains("No profile built yet"));
        }
    }
}
