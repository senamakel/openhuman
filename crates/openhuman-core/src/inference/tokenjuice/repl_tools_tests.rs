use super::*;
use serde_json::json;
use tinyjuice::cache::store::MemoryCcrStore;
use tinyjuice::types::{CompressOptions, CompressorKind};

/// Reads originals from an in-process store instead of the module.
struct MemorySource(Arc<MemoryCcrStore>);

#[async_trait]
impl OriginalSource for MemorySource {
    async fn original(&self, handle: &str) -> Result<Option<String>, String> {
        Ok(self.0.get(handle))
    }
}

struct FailingSource;

#[async_trait]
impl OriginalSource for FailingSource {
    async fn original(&self, _handle: &str) -> Result<Option<String>, String> {
        Err("module unavailable".to_string())
    }
}

fn tool(tools: &[Box<dyn Tool>], name: &str) -> usize {
    tools
        .iter()
        .position(|t| t.name() == name)
        .unwrap_or_else(|| panic!("no tool named {name}"))
}

fn log_body() -> String {
    let rows: String = (0..900)
        .map(|i| format!("row {i}: value {}\n", i * 7))
        .collect();
    let tail: String = (900..1000)
        .map(|i| format!("row {i}: value {}\n", i * 7))
        .collect();
    format!("# Report\n{rows}ERROR: needle in the middle\n{tail}")
}

/// Compress `content` in handle mode against `store`, returning the model-facing
/// text and the handle.
async fn store_behind_handle(store: &MemoryCcrStore, content: &str) -> (String, String) {
    let options = CompressOptions {
        repl_handle: true,
        ..CompressOptions::default()
    };
    let out = tinyjuice::compress_content_with_store(content, None, &options, store).await;
    assert_eq!(
        out.compressor,
        CompressorKind::Repl,
        "handle mode must apply"
    );
    let handle = out.ccr_token.clone().expect("a handle");
    (out.text, handle)
}

fn result_text(result: &ToolResult) -> String {
    result.output()
}

#[test]
fn declares_the_three_repl_tools_read_only_capped_and_small() {
    let tools = repl_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert_eq!(
        names, REPL_TOOL_NAMES,
        "same tools, same order as TinyJuice"
    );

    let mut schema_bytes = 0;
    for t in &tools {
        assert!(is_repl_tool(t.name()));
        assert_eq!(t.permission_level(), PermissionLevel::ReadOnly);
        assert!(t.is_concurrency_safe(&json!({})));
        assert!(!t.external_effect());
        assert!(
            t.max_result_size_chars().is_some_and(|cap| cap > 0),
            "{} must declare a result cap",
            t.name()
        );
        let schema = t.parameters_schema();
        assert!(
            schema["required"]
                .as_array()
                .is_some_and(|r| r.contains(&json!("handle"))),
            "{} must require a handle",
            t.name()
        );
        schema_bytes += t.name().len() + t.description().len() + schema.to_string().len();
    }
    // The point of three tools is a schema that stays small on every turn.
    assert!(schema_bytes < 3_000, "schemas are {schema_bytes} bytes");
}

#[tokio::test]
async fn handle_round_trip_find_summarize_and_extract() {
    let store = Arc::new(MemoryCcrStore::default());
    let content = log_body();
    let (preview, handle) = store_behind_handle(&store, &content).await;

    // The model sees a small preview naming the handle and the tools.
    assert!(preview.len() < content.len() / 4);
    assert!(preview.contains(&handle));
    // It names the slice read and the outline, then the whole-original
    // retrieve last; `juice_extract` (HTML/Markdown only) is left to its
    // description.
    for name in ["juice_find", "juice_summarize"] {
        assert!(preview.contains(name), "footer must name {name}");
    }
    let find = preview.find("juice_find").unwrap();
    let retrieve = preview
        .find(crate::inference::tokenjuice::RETRIEVE_TOOL_NAME)
        .expect("footer names the whole-original retrieve");
    assert!(
        find < retrieve,
        "slice read before whole original: {preview}"
    );
    assert!(
        !preview.contains("needle in the middle"),
        "the needle must be behind the handle, not in the preview"
    );

    let tools = repl_tools_with(Arc::new(MemorySource(store)), ReplLimits::default());

    let found = tools[tool(&tools, "juice_find")]
        .execute(json!({ "handle": handle, "query": "needle" }))
        .await
        .unwrap();
    assert!(!found.is_error, "{}", result_text(&found));
    assert!(result_text(&found).contains("needle in the middle"));

    let by_grep = tools[tool(&tools, "juice_find")]
        .execute(json!({ "handle": handle, "query": "^row 899:", "mode": "grep" }))
        .await
        .unwrap();
    assert!(result_text(&by_grep).contains("row 899"));

    let summary = tools[tool(&tools, "juice_summarize")]
        .execute(json!({ "handle": handle, "hint": "needle" }))
        .await
        .unwrap();
    assert!(!summary.is_error, "{}", result_text(&summary));

    let md = "# Title\ntext\n## Section\nmore\n".repeat(200);
    let md_store = Arc::new(MemoryCcrStore::default());
    let (_, md_handle) = store_behind_handle(&md_store, &md).await;
    let md_tools = repl_tools_with(Arc::new(MemorySource(md_store)), ReplLimits::default());
    let headings = md_tools[tool(&md_tools, "juice_extract")]
        .execute(json!({ "handle": md_handle, "what": "headings" }))
        .await
        .unwrap();
    assert!(!headings.is_error, "{}", result_text(&headings));
    assert!(result_text(&headings).contains("Section"));
}

#[tokio::test]
async fn answers_stay_within_the_declared_cap() {
    let store = Arc::new(MemoryCcrStore::default());
    let content = log_body();
    let (_, handle) = store_behind_handle(&store, &content).await;
    let tools = repl_tools_with(Arc::new(MemorySource(store)), ReplLimits::default());
    let find = &tools[tool(&tools, "juice_find")];
    // Every line matches.
    let all = find
        .execute(json!({ "handle": handle, "query": "row" }))
        .await
        .unwrap();
    let cap = find.max_result_size_chars().unwrap();
    assert!(
        result_text(&all).chars().count() <= cap,
        "{} chars exceeds the {cap} cap",
        result_text(&all).chars().count()
    );
    assert!(result_text(&all).len() < content.len() / 4);
}

#[tokio::test]
async fn the_marker_form_of_a_handle_is_accepted() {
    let store = Arc::new(MemoryCcrStore::default());
    let (_, handle) = store_behind_handle(&store, &log_body()).await;
    let tools = repl_tools_with(Arc::new(MemorySource(store)), ReplLimits::default());
    let res = tools[0]
        .execute(json!({ "handle": format!("⟦tj:{handle}⟧"), "query": "needle" }))
        .await
        .unwrap();
    assert!(!res.is_error, "{}", result_text(&res));
    assert!(result_text(&res).contains("needle"));
}

#[tokio::test]
async fn bad_or_unknown_handles_are_errors_that_do_not_invite_a_re_run() {
    let store = Arc::new(MemoryCcrStore::default());
    let tools = repl_tools_with(Arc::new(MemorySource(store)), ReplLimits::default());
    let find = &tools[tool(&tools, "juice_find")];

    let missing = find.execute(json!({ "query": "x" })).await.unwrap();
    assert!(missing.is_error);

    for bad in ["", "../etc/passwd", "a b", &"a".repeat(200)] {
        let res = find
            .execute(json!({ "handle": bad, "query": "x" }))
            .await
            .unwrap();
        assert!(res.is_error, "handle {bad:?} must be rejected");
    }

    let unknown = find
        .execute(json!({ "handle": "deadbeefdeadbeef", "query": "x" }))
        .await
        .unwrap();
    assert!(unknown.is_error);
    let msg = result_text(&unknown).to_lowercase();
    assert!(msg.contains("do not re-run"), "{msg}");
}

#[tokio::test]
async fn a_source_failure_is_reported_not_raised() {
    let tools = repl_tools_with(Arc::new(FailingSource), ReplLimits::default());
    let res = tools[0]
        .execute(json!({ "handle": "abc123", "query": "x" }))
        .await
        .unwrap();
    assert!(res.is_error);
    assert!(result_text(&res).contains("module unavailable"));
}

#[test]
fn handle_normalization_only_accepts_alphanumeric_tokens() {
    assert_eq!(normalize_handle(" abc123 "), Some("abc123"));
    assert_eq!(normalize_handle("⟦tj:abc123⟧"), Some("abc123"));
    assert_eq!(normalize_handle(""), None);
    assert_eq!(normalize_handle("../x"), None);
    assert_eq!(normalize_handle(&"f".repeat(MAX_HANDLE_LEN + 1)), None);
}

#[test]
fn repl_tools_share_the_harness_tool_trait() {
    // `tinyjuice::repl::tools` and this crate must name one `tinytools::Tool`;
    // this only compiles when `repl_tools()` yields the trait objects the tool
    // registry (`Vec<Box<dyn tinytools::Tool>>`) takes.
    let registry: Vec<Box<dyn tinytools::Tool>> = repl_tools();
    assert_eq!(registry.len(), REPL_TOOL_NAMES.len());
    let raw: Vec<Box<dyn tinytools::Tool>> = tinyjuice::repl::tools::repl_tools(
        Arc::new(MemoryCcrStore::default()),
        ReplLimits::default(),
    );
    assert_eq!(raw.len(), registry.len());
}

/// A workspace whose tool-results dir holds one persisted shell output, the
/// way `ToolResultArtifactStore::detached` writes it.
fn artifacts_fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = crate::security::policy::tool_result_artifacts_dir(tmp.path());
    let file = dir.join("sess1").join("shell").join("call_abc123.txt");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, log_body()).unwrap();
    (tmp, dir, file)
}

fn tools_with_artifacts(dir: std::path::PathBuf) -> Vec<Box<dyn Tool>> {
    repl_tools_with_artifacts(
        Arc::new(MemorySource(Arc::new(MemoryCcrStore::default()))),
        ReplLimits::default(),
        Some(dir),
    )
}

#[tokio::test]
async fn an_artifact_path_in_the_handle_slot_is_read_from_the_tool_results_dir() {
    // Production: the model copies `artifact_path` from the oversized-output
    // envelope into `handle` and got "invalid handle".
    let (_tmp, dir, file) = artifacts_fixture();
    let tools = tools_with_artifacts(dir);

    let found = tools[tool(&tools, "juice_find")]
        .execute(json!({ "handle": file.to_string_lossy(), "query": "needle" }))
        .await
        .unwrap();
    assert!(!found.is_error, "{}", result_text(&found));
    assert!(result_text(&found).contains("needle in the middle"));

    let summary = tools[tool(&tools, "juice_summarize")]
        .execute(json!({ "handle": file.to_string_lossy() }))
        .await
        .unwrap();
    assert!(!summary.is_error, "{}", result_text(&summary));

    // The legacy relative pointer form resolves against the same dir.
    let relative = format!(
        "artifacts/tool-results/{}",
        file.strip_prefix(dir_of(&file, 3))
            .unwrap()
            .to_string_lossy()
    );
    let by_relative = tools[tool(&tools, "juice_find")]
        .execute(json!({ "handle": relative, "query": "needle" }))
        .await
        .unwrap();
    assert!(!by_relative.is_error, "{}", result_text(&by_relative));
}

/// `path` with its last `levels` components removed.
fn dir_of(path: &std::path::Path, levels: usize) -> std::path::PathBuf {
    let mut p = path.to_path_buf();
    for _ in 0..levels {
        p.pop();
    }
    p
}

#[tokio::test]
async fn an_artifact_path_outside_the_tool_results_dir_is_refused() {
    let (tmp, dir, _file) = artifacts_fixture();
    let secret = tmp.path().join("secret.txt");
    std::fs::write(&secret, "needle secret").unwrap();
    let tools = tools_with_artifacts(dir.clone());
    let find = &tools[tool(&tools, "juice_find")];

    let traversal = dir
        .join("sess1")
        .join("..")
        .join("..")
        .join("..")
        .join("secret.txt");
    for bad in [
        secret.to_string_lossy().into_owned(),
        traversal.to_string_lossy().into_owned(),
        "artifacts/tool-results/../../secret.txt".to_string(),
        "/etc/passwd".to_string(),
    ] {
        let res = find
            .execute(json!({ "handle": bad, "query": "needle" }))
            .await
            .unwrap();
        assert!(res.is_error, "{bad} must be refused");
        assert!(
            !result_text(&res).contains("needle secret"),
            "{bad} leaked content"
        );
        assert!(
            result_text(&res).contains("file_read"),
            "{}",
            result_text(&res)
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_symlink_inside_the_tool_results_dir_cannot_escape_it() {
    let (tmp, dir, _file) = artifacts_fixture();
    let secret = tmp.path().join("secret.txt");
    std::fs::write(&secret, "needle secret").unwrap();
    let link = dir.join("sess1").join("shell").join("call_link.txt");
    std::os::unix::fs::symlink(&secret, &link).unwrap();
    let tools = tools_with_artifacts(dir);
    let res = tools[tool(&tools, "juice_find")]
        .execute(json!({ "handle": link.to_string_lossy(), "query": "needle" }))
        .await
        .unwrap();
    assert!(res.is_error);
    assert!(!result_text(&res).contains("needle secret"));
}

#[test]
fn an_oversized_artifact_is_refused_before_it_is_read() {
    let (_tmp, dir, file) = artifacts_fixture();
    let err = read_tool_result_artifact(&dir, &file.to_string_lossy(), 16).unwrap_err();
    assert!(err.contains("file_read"), "{err}");
    let ok = read_tool_result_artifact(&dir, &file.to_string_lossy(), 10 * 1024 * 1024).unwrap();
    assert!(ok.contains("needle in the middle"));
}

#[tokio::test]
async fn a_call_id_or_other_non_handle_gets_an_actionable_message() {
    let (_tmp, dir, _file) = artifacts_fixture();
    for tools in [
        tools_with_artifacts(dir),
        repl_tools_with(
            Arc::new(MemorySource(Arc::new(MemoryCcrStore::default()))),
            ReplLimits::default(),
        ),
    ] {
        let find = &tools[tool(&tools, "juice_find")];
        for bad in ["call_abc123", "toolu_01XyZ", "a b"] {
            let res = find
                .execute(json!({ "handle": bad, "query": "x" }))
                .await
                .unwrap();
            assert!(res.is_error, "{bad} must be rejected");
            let msg = result_text(&res);
            assert!(msg.contains("32"), "names the handle shape: {msg}");
            assert!(msg.contains("file_read"), "points at file_read: {msg}");
        }
    }
}

#[tokio::test]
async fn an_artifact_path_without_a_configured_dir_points_at_file_read() {
    let (_tmp, _dir, file) = artifacts_fixture();
    let tools = repl_tools_with(
        Arc::new(MemorySource(Arc::new(MemoryCcrStore::default()))),
        ReplLimits::default(),
    );
    let res = tools[tool(&tools, "juice_find")]
        .execute(json!({ "handle": file.to_string_lossy(), "query": "needle" }))
        .await
        .unwrap();
    assert!(res.is_error);
    assert!(result_text(&res).contains("file_read"));
}

/// Every `juice_*` / `*_retrieve` identifier a footer mentions.
fn tool_names_in(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| w.starts_with("juice_") || w.ends_with("_retrieve") || w.contains("juice_"))
        .map(str::to_string)
        .collect()
}

#[tokio::test]
async fn compressed_output_footers_only_name_tools_that_exist() {
    // Regression for the "unknown tool juice_*" cluster: a footer naming a tool
    // the model is not offered (or a pre-rename alias like `tinyjuice_retrieve`)
    // burns the model's failure budget on a call that cannot dispatch.
    use crate::inference::tokenjuice::RETRIEVE_TOOL_NAME;
    let offered: Vec<&str> = REPL_TOOL_NAMES
        .iter()
        .copied()
        .chain([RETRIEVE_TOOL_NAME])
        .collect();

    let store = MemoryCcrStore::default();
    let (handle_footer, _) = store_behind_handle(&store, &log_body()).await;

    let ccr = tinyjuice::compress_content_with_store(
        &log_body(),
        None,
        &CompressOptions::default(),
        &MemoryCcrStore::default(),
    )
    .await;

    let mut seen = 0;
    for footer in [handle_footer, ccr.text] {
        assert!(!footer.contains("tinyjuice_retrieve"), "{footer}");
        assert!(!footer.contains("tokenjuice_retrieve"), "{footer}");
        for name in tool_names_in(&footer) {
            seen += 1;
            assert!(
                offered.contains(&name.as_str()),
                "footer names `{name}`, which is not an offered tool: {footer}"
            );
        }
    }
    assert!(seen > 0, "the footers must name at least one tool");
}
