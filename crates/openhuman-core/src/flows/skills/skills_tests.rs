use super::*;

fn skill_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/flows/skills/flow-authoring")
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
fn the_frontmatter_description_does_not_advertise_a_dropped_page() {
    // The description is what the model reads in the `## Installed Skills`
    // catalogue to decide whether to open the skill at all, and it is prose
    // rather than a path — so the `references/` token check below cannot
    // see it. It went stale the first time a page moved back into the
    // standing prompt: the description still promised graph sizing after
    // `graph-shape.md` was deleted.
    let manifest = FLOW_AUTHORING
        .files
        .iter()
        .find(|f| f.path == "WORKFLOW.md")
        .expect("manifest")
        .contents;
    let description = manifest
        .lines()
        .find(|l| l.starts_with("description:"))
        .expect("frontmatter description");
    for dropped in ["how large a graph", "graph should be", "graph-shape"] {
        assert!(
            !description.contains(dropped),
            "the description still advertises `{dropped}`, which no longer ships"
        );
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
    assert!(pointed >= 3, "the prompt's pointer table lost its rows");
}
