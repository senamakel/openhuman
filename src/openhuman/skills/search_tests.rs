//! Tests for `skill_search`.

use super::*;

fn skill(dir: &str, name: &str, description: &str, tags: &[&str]) -> Workflow {
    Workflow {
        dir_name: dir.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        ..Default::default()
    }
}

fn corpus() -> Vec<Workflow> {
    vec![
        skill(
            "flow-authoring",
            "flow-authoring",
            "The tinyflows authoring reference: expression syntax, node configuration, dry runs.",
            &["flows", "workflows", "reference"],
        ),
        skill(
            "changelog",
            "Release changelog",
            "Turn a git commit range into a written changelog for a release.",
            &["git", "release"],
        ),
        skill(
            "ascii-art",
            "ASCII art",
            "Render text as ASCII art via pyfiglet.",
            &["fun"],
        ),
    ]
}

#[test]
fn a_capability_query_finds_the_skill_that_serves_it() {
    let all = corpus();
    let hits = rank(&all, "write a changelog from commits", 3);
    assert_eq!(id_of(hits[0]), "changelog");
}

#[test]
fn a_skill_is_findable_by_its_directory_id_even_when_the_display_name_differs() {
    // The reason `dir_name` is in the searchable text. `changelog`'s display
    // name is "Release changelog"; the id the model must eventually type is
    // `changelog`, and a query using that id must not miss.
    let all = corpus();
    let hits = rank(&all, "ascii-art", 3);
    assert_eq!(id_of(hits[0]), "ascii-art");
}

#[test]
fn tags_are_searchable() {
    let all = corpus();
    let hits = rank(&all, "pyfiglet fun", 3);
    assert_eq!(id_of(hits[0]), "ascii-art");
}

#[test]
fn nothing_relevant_returns_nothing() {
    // The property that makes a miss legible. A ranker that always returns
    // `limit` entries would hand the model `ascii-art` for a database query
    // and invite it to run the thing.
    let all = corpus();
    assert!(rank(&all, "provision a kubernetes cluster", 3).is_empty());
}

#[test]
fn the_limit_caps_the_result_set() {
    let all = corpus();
    assert_eq!(rank(&all, "a", 10).len(), rank(&all, "a", 10).len().min(3));
    assert!(rank(&all, "the", 1).len() <= 1);
}

#[test]
fn an_empty_corpus_is_safe() {
    assert!(rank(&[], "anything", 5).is_empty());
}

#[test]
fn the_projection_omits_the_body_and_caps_the_description() {
    // The whole point of projecting: `list_workflows` serialises frontmatter,
    // resources and location for every skill. If this ever returns the struct
    // the tool has stopped saving anything.
    let long = "x".repeat(1000);
    let workflow = skill("s", "S", &long, &["t"]);
    let value = project(&workflow);
    let obj = value.as_object().expect("object");
    let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, ["description", "id", "name", "scope", "tags"]);
    assert!(
        obj["description"].as_str().expect("str").len() <= MAX_DESCRIPTION + 8,
        "description must be capped"
    );
}

#[tokio::test]
async fn an_empty_query_is_an_error_rather_than_every_skill() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = Config {
        workspace_dir: tmp.path().to_path_buf(),
        ..Default::default()
    };
    let tool = SkillSearchTool::new(Arc::new(config));
    let result = tool
        .execute(json!({ "query": "   " }))
        .await
        .expect("dispatch");
    assert!(result.is_error);
}

#[tokio::test]
async fn a_miss_says_so_instead_of_returning_a_bare_empty_list() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = Config {
        workspace_dir: tmp.path().to_path_buf(),
        ..Default::default()
    };
    let tool = SkillSearchTool::new(Arc::new(config));
    let result = tool
        .execute(json!({ "query": "provision a kubernetes cluster" }))
        .await
        .expect("dispatch");
    assert!(!result.is_error);
    let text = format!("{result:?}");
    assert!(text.contains("\\\"matched\\\":0") || text.contains("matched"));
    assert!(
        text.contains("skill_registry_search"),
        "a miss must say what to do next: {text}"
    );
}

#[tokio::test]
async fn the_bundled_skill_is_findable_once_installed() {
    // End to end over the real pipeline: materialise the compiled-in bundles
    // into a temp workspace, then search. This is the test that would fail if
    // discovery stopped scanning the builtin root, if the scope were rejected,
    // or if the bundle's frontmatter stopped parsing.
    let tmp = tempfile::tempdir().expect("tempdir");
    let report = crate::openhuman::skills::bundled::install(tmp.path());
    assert!(report.failed.is_empty(), "install failed: {report:?}");
    if crate::openhuman::skills::bundled::BUNDLED.is_empty() {
        return; // nothing ships in this feature configuration
    }

    let config = Config {
        workspace_dir: tmp.path().to_path_buf(),
        ..Default::default()
    };
    let tool = SkillSearchTool::new(Arc::new(config));
    let result = tool
        .execute(json!({ "query": "tinyflows expression syntax and node configuration" }))
        .await
        .expect("dispatch");
    let text = format!("{result:?}");
    assert!(
        text.contains("flow-authoring"),
        "the bundled skill must be discoverable through search: {text}"
    );
}
