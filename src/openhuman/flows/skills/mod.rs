//! Skills the flows domain ships inside the binary.
//!
//! `flow-authoring` is the Workflow Builder's reference manual. It used to be
//! ~25 KB of the agent's system prompt — paid on every turn of every authoring
//! session, including the turns that only wire two nodes together. As a bundled
//! skill the same text is one `read_workflow_resource` call away and costs
//! nothing until a page is actually needed.
//!
//! The split follows one rule, and it is the reason the whole prompt did not
//! move: **a rule that binds goes in the prompt; a rule you look up goes
//! here.** "Propose, never persist" cannot live in a manual, because a manual
//! only binds a model that chose to open it. An expression's jq syntax is the
//! opposite — nothing goes wrong by not knowing it until you need it, and a
//! lot goes wrong by half-remembering it.
//!
//! # Where this belongs eventually
//!
//! Upstream, in tinyflows. The pages name no OpenHuman type and no host
//! concept beyond the tool slugs, so moving them is a directory move plus a
//! changed `include_str!` path. The pinned `vendor/tinyflows` submodule has no
//! crate to hold them yet — there is no `tinyflows-copilot` in it — so they sit
//! with the flows domain here in the meantime. Keeping them free of host
//! coupling is what keeps that move cheap; do not reach into `crate::` from a
//! page.

use crate::openhuman::skills::bundled::{BundledFile, BundledSkill};

/// The `flow-authoring` bundle, embedded from the sibling directory.
///
/// Listed file by file rather than swept from the directory: a build-time
/// directory walk would silently ship whatever happened to be sitting there,
/// and `include_str!` needs literal paths anyway. The `bundled_skill_matches_
/// the_directory_on_disk` test below fails when a page is added to the
/// directory and not to this list, which is the mistake this shape actually
/// invites.
pub const FLOW_AUTHORING: BundledSkill = BundledSkill {
    dir_name: "flow-authoring",
    files: &[
        BundledFile {
            path: "WORKFLOW.md",
            contents: include_str!("flow-authoring/WORKFLOW.md"),
        },
        BundledFile {
            path: "references/expressions.md",
            contents: include_str!("flow-authoring/references/expressions.md"),
        },
        BundledFile {
            path: "references/node-config.md",
            contents: include_str!("flow-authoring/references/node-config.md"),
        },
        BundledFile {
            path: "references/graph-shape.md",
            contents: include_str!("flow-authoring/references/graph-shape.md"),
        },
        BundledFile {
            path: "references/dry-run.md",
            contents: include_str!("flow-authoring/references/dry-run.md"),
        },
    ],
};

#[cfg(test)]
mod tests {
    use super::*;

    fn skill_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/openhuman/flows/skills/flow-authoring")
    }

    #[test]
    fn bundled_skill_matches_the_directory_on_disk() {
        // A page added to the directory but not to `FLOW_AUTHORING` is not a
        // compile error and not a test failure anywhere else — it simply never
        // ships, and the prompt's pointer table sends the model to a file that
        // does not exist. This is that check.
        let root = skill_dir();
        let mut on_disk = Vec::new();
        for entry in walkdir(&root) {
            let rel = entry
                .strip_prefix(&root)
                .expect("under root")
                .to_string_lossy()
                .replace('\\', "/");
            on_disk.push(rel);
        }
        on_disk.sort();

        let mut listed: Vec<String> = FLOW_AUTHORING
            .files
            .iter()
            .map(|f| f.path.to_string())
            .collect();
        listed.sort();

        assert_eq!(
            listed, on_disk,
            "FLOW_AUTHORING's file list and the on-disk bundle have diverged"
        );
    }

    fn walkdir(root: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("read_dir").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    out.push(path);
                }
            }
        }
        out
    }

    #[test]
    fn every_page_the_manifest_advertises_exists() {
        // The WORKFLOW.md table is what the model reads to choose a page. A
        // row naming a file that does not ship is a dead end the model cannot
        // diagnose — it just gets an error and gives up on the manual.
        let manifest = FLOW_AUTHORING
            .files
            .iter()
            .find(|f| f.path == "WORKFLOW.md")
            .expect("manifest")
            .contents;
        for file in FLOW_AUTHORING.files {
            if file.path == "WORKFLOW.md" {
                continue;
            }
            assert!(
                manifest.contains(file.path),
                "`{}` ships but the manifest's table never names it",
                file.path
            );
        }
        for line in manifest.lines() {
            for token in line.split('`') {
                if token.starts_with("references/") {
                    assert!(
                        FLOW_AUTHORING.files.iter().any(|f| f.path == token),
                        "the manifest points at `{token}`, which does not ship"
                    );
                }
            }
        }
    }

    #[test]
    fn the_builder_prompt_points_at_pages_that_ship() {
        // Same check from the other side. The prompt carries its own copy of
        // the table (the model needs to know the manual exists before it has
        // read the manual), so the two can drift independently.
        const PROMPT: &str = include_str!("../agents/workflow_builder/prompt.md");
        assert!(
            PROMPT.contains("flow-authoring"),
            "the builder prompt must name the skill that holds its reference manual"
        );
        let mut pointed = 0;
        for token in PROMPT.split('`') {
            if token.starts_with("references/") {
                pointed += 1;
                assert!(
                    FLOW_AUTHORING.files.iter().any(|f| f.path == token),
                    "the builder prompt points at `{token}`, which does not ship"
                );
            }
        }
        assert!(pointed >= 4, "the prompt's pointer table lost its rows");
    }
}
