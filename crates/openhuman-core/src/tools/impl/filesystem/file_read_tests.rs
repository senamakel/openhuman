use super::*;
use crate::security::{AutonomyLevel, SecurityPolicy};

fn test_security(workspace: std::path::PathBuf) -> Arc<SecurityPolicy> {
    Arc::new(SecurityPolicy {
        autonomy: AutonomyLevel::Supervised,
        action_dir: workspace.clone(),
        workspace_dir: workspace,
        ..SecurityPolicy::default()
    })
}

fn test_security_with(
    workspace: std::path::PathBuf,
    autonomy: AutonomyLevel,
    max_actions_per_hour: u32,
) -> Arc<SecurityPolicy> {
    Arc::new(SecurityPolicy {
        autonomy,
        action_dir: workspace.clone(),
        workspace_dir: workspace,
        max_actions_per_hour,
        ..SecurityPolicy::default()
    })
}

#[test]
fn file_read_name() {
    let tool = FileReadTool::new(test_security(std::env::temp_dir()));
    assert_eq!(tool.name(), "file_read");
}

#[test]
fn file_read_schema_has_path() {
    let tool = FileReadTool::new(test_security(std::env::temp_dir()));
    let schema = tool.parameters_schema();
    assert!(schema["properties"]["path"].is_object());
    assert!(schema["required"]
        .as_array()
        .unwrap()
        .contains(&json!("path")));
}

#[tokio::test]
async fn file_read_existing_file() {
    let dir = std::env::temp_dir().join("openhuman_test_file_read");
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(dir.join("test.txt"), "hello world")
        .await
        .unwrap();

    let tool = FileReadTool::new(test_security(dir.clone()));
    let result = tool.execute(json!({"path": "test.txt"})).await.unwrap();
    assert!(!result.is_error);
    assert_eq!(result.output(), "hello world");
    assert!(!result.is_error);

    let _ = tokio::fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn file_read_nonexistent_file() {
    let dir = std::env::temp_dir().join("openhuman_test_file_read_missing");
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(&dir).await.unwrap();

    let tool = FileReadTool::new(test_security(dir.clone()));
    let result = tool.execute(json!({"path": "nope.txt"})).await.unwrap();
    assert!(result.is_error);
    assert!(&result.output().contains("Failed to resolve"));

    let _ = tokio::fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn file_read_blocks_path_traversal() {
    let dir = std::env::temp_dir().join("openhuman_test_file_read_traversal");
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(&dir).await.unwrap();

    let tool = FileReadTool::new(test_security(dir.clone()));
    let result = tool
        .execute(json!({"path": "../../../etc/passwd"}))
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(&result.output().contains("not allowed"));

    let _ = tokio::fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn file_read_blocks_absolute_path() {
    let tool = FileReadTool::new(test_security(std::env::temp_dir()));
    let result = tool.execute(json!({"path": "/etc/passwd"})).await.unwrap();
    assert!(result.is_error);
    assert!(&result.output().contains("not allowed"));
}

#[tokio::test]
async fn file_read_blocks_when_rate_limited() {
    let dir = std::env::temp_dir().join("openhuman_test_file_read_rate_limited");
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(dir.join("test.txt"), "hello world")
        .await
        .unwrap();

    let tool = FileReadTool::new(test_security_with(
        dir.clone(),
        AutonomyLevel::Supervised,
        0,
    ));
    let result = tool.execute(json!({"path": "test.txt"})).await.unwrap();

    assert!(result.is_error);
    assert!(result.output().contains("Rate limit exceeded"));

    let _ = tokio::fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn file_read_allows_readonly_mode() {
    let dir = std::env::temp_dir().join("openhuman_test_file_read_readonly");
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(dir.join("test.txt"), "readonly ok")
        .await
        .unwrap();

    let tool = FileReadTool::new(test_security_with(dir.clone(), AutonomyLevel::ReadOnly, 20));
    let result = tool.execute(json!({"path": "test.txt"})).await.unwrap();

    assert!(!result.is_error);
    assert_eq!(result.output(), "readonly ok");

    let _ = tokio::fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn file_read_missing_path_param() {
    let tool = FileReadTool::new(test_security(std::env::temp_dir()));
    let result = tool.execute(json!({})).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn file_read_empty_file() {
    let dir = std::env::temp_dir().join("openhuman_test_file_read_empty");
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(dir.join("empty.txt"), "").await.unwrap();

    let tool = FileReadTool::new(test_security(dir.clone()));
    let result = tool.execute(json!({"path": "empty.txt"})).await.unwrap();
    assert!(!result.is_error);
    assert_eq!(result.output(), "");

    let _ = tokio::fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn file_read_nested_path() {
    let dir = std::env::temp_dir().join("openhuman_test_file_read_nested");
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(dir.join("sub/dir"))
        .await
        .unwrap();
    tokio::fs::write(dir.join("sub/dir/deep.txt"), "deep content")
        .await
        .unwrap();

    let tool = FileReadTool::new(test_security(dir.clone()));
    let result = tool
        .execute(json!({"path": "sub/dir/deep.txt"}))
        .await
        .unwrap();
    assert!(!result.is_error);
    assert_eq!(result.output(), "deep content");

    let _ = tokio::fs::remove_dir_all(&dir).await;
}

#[cfg(unix)]
#[tokio::test]
async fn file_read_blocks_symlink_escape() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join("openhuman_test_file_read_symlink_escape");
    let workspace = root.join("workspace");
    let outside = root.join("outside");

    let _ = tokio::fs::remove_dir_all(&root).await;
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    tokio::fs::create_dir_all(&outside).await.unwrap();

    tokio::fs::write(outside.join("secret.txt"), "outside workspace")
        .await
        .unwrap();

    symlink(outside.join("secret.txt"), workspace.join("escape.txt")).unwrap();

    let tool = FileReadTool::new(test_security(workspace.clone()));
    let result = tool.execute(json!({"path": "escape.txt"})).await.unwrap();

    assert!(result.is_error);
    // After the symlink-safe canonical check landed in
    // SecurityPolicy::is_path_allowed (#1927), the policy layer blocks
    // the escape before file_read's own resolved-path check runs — the
    // error becomes "Path not allowed by security policy". Either
    // message signals defense-in-depth worked.
    let out = result.output();
    assert!(
        out.contains("escapes workspace") || out.contains("not allowed"),
        "expected escape/not-allowed error, got: {out}"
    );

    let _ = tokio::fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn file_read_nonexistent_consumes_rate_limit_budget() {
    let dir = std::env::temp_dir().join("openhuman_test_file_read_probe");
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(&dir).await.unwrap();

    // Allow only 2 actions total
    let tool = FileReadTool::new(test_security_with(
        dir.clone(),
        AutonomyLevel::Supervised,
        2,
    ));

    // Both reads fail (file doesn't exist) but should consume budget
    let r1 = tool.execute(json!({"path": "nope1.txt"})).await.unwrap();
    assert!(r1.is_error);
    assert!(r1.output().contains("Failed to resolve"));

    let r2 = tool.execute(json!({"path": "nope2.txt"})).await.unwrap();
    assert!(r2.is_error);
    assert!(r2.output().contains("Failed to resolve"));

    // Third attempt should be rate limited even though file doesn't exist
    let r3 = tool.execute(json!({"path": "nope3.txt"})).await.unwrap();
    assert!(r3.is_error);
    let r3_output = r3.output();
    assert!(
        r3_output.contains("Rate limit"),
        "Expected rate limit error, got: {:?}",
        r3_output
    );

    let _ = tokio::fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn file_read_rejects_oversized_file() {
    let dir = std::env::temp_dir().join("openhuman_test_file_read_large");
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(&dir).await.unwrap();

    // Create a file just over 10 MB
    let big = vec![b'x'; 10 * 1024 * 1024 + 1];
    tokio::fs::write(dir.join("huge.bin"), &big).await.unwrap();

    let tool = FileReadTool::new(test_security(dir.clone()));
    let result = tool.execute(json!({"path": "huge.bin"})).await.unwrap();
    assert!(result.is_error);
    assert!(&result.output().contains("File too large"));

    let _ = tokio::fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn file_read_reports_invalid_utf8_as_an_error_not_a_panic() {
    let dir = std::env::temp_dir().join("openhuman_test_file_read_non_utf8");
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(&dir).await.unwrap();

    // Passes the size cap, fails `read_to_string`: 0x80 is a lone UTF-8
    // continuation byte with no lead byte, invalid at any position.
    tokio::fs::write(dir.join("binary.dat"), [0xFFu8, 0xFE, 0x00, 0x80])
        .await
        .unwrap();

    let tool = FileReadTool::new(test_security(dir.clone()));
    let result = tool.execute(json!({"path": "binary.dat"})).await.unwrap();
    assert!(
        result.is_error,
        "invalid UTF-8 must surface as a tool error, not succeed with replaced/garbled text"
    );
    assert!(&result.output().contains("Failed to read file"));

    let _ = tokio::fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn file_read_offset_continues_from_the_given_byte() {
    let dir = tempfile::tempdir().unwrap();
    tokio::fs::write(dir.path().join("paged.txt"), "hello world")
        .await
        .unwrap();
    let tool = FileReadTool::new(test_security(dir.path().to_path_buf()));

    let page = tool
        .execute(json!({"path": "paged.txt", "offset": 6}))
        .await
        .unwrap();
    assert!(!page.is_error, "{}", page.output());
    assert_eq!(
        page.output(),
        "world",
        "a read with an offset must start at that byte"
    );

    let past_end = tool
        .execute(json!({"path": "paged.txt", "offset": 99}))
        .await
        .unwrap();
    assert!(
        past_end.is_error,
        "an offset past the end must be reported, not returned as an empty read"
    );
}

#[tokio::test]
async fn file_read_pages_long_files_and_reports_a_working_continuation() {
    let dir = tempfile::tempdir().unwrap();
    let contents = format!("{}éTAIL", "x".repeat(MAX_PAGE_BYTES - 1));
    tokio::fs::write(dir.path().join("long.txt"), &contents)
        .await
        .unwrap();
    let tool = FileReadTool::new(test_security(dir.path().to_path_buf()));

    let first = tool.execute(json!({"path": "long.txt"})).await.unwrap();
    assert!(!first.is_error, "{}", first.output());
    assert!(
        first.output().contains("continue with file_read"),
        "a truncated page must tell the caller exactly how to continue: {}",
        first.output()
    );
    assert!(
        first.output().len() < 16 * 1024,
        "the page and marker must survive the harness's per-result budget"
    );
    let marker = first
        .output()
        .split("\"offset\":")
        .nth(1)
        .and_then(|tail| tail.split('}').next())
        .and_then(|value| value.parse::<usize>().ok())
        .expect("continuation offset in marker");
    assert!(contents.is_char_boundary(marker));

    let second = tool
        .execute(json!({"path": "long.txt", "offset": marker}))
        .await
        .unwrap();
    assert!(!second.is_error, "{}", second.output());
    assert_eq!(
        format!("{}{}", &first.output()[..marker], second.output()),
        contents
    );
}

#[test]
fn file_read_reserves_the_continuation_marker_for_a_long_escaped_path() {
    let contents = "x".repeat(MAX_PAGE_BYTES * 2);
    let path = format!("{}quoted\\\"name.txt", "nested\\\\".repeat(900));
    let page = page_contents(&contents, &path, 0);

    assert!(page.len() <= MAX_TOOL_OUTPUT_BYTES, "{}", page.len());
    assert!(page.contains("continue with file_read"));
    assert!(page.contains("\"offset\":"));
    assert!(
        !page.contains(&path),
        "an oversized escaped path should be omitted so the offset survives"
    );
}

#[tokio::test]
async fn file_read_rejects_an_offset_inside_a_multibyte_character() {
    let dir = tempfile::tempdir().unwrap();
    // "é" is two bytes, so byte 1 is inside it.
    tokio::fs::write(dir.path().join("utf8.txt"), "é world")
        .await
        .unwrap();
    let tool = FileReadTool::new(test_security(dir.path().to_path_buf()));

    let inside = tool
        .execute(json!({"path": "utf8.txt", "offset": 1}))
        .await
        .unwrap();
    assert!(
        inside.is_error,
        "an offset inside a character must be rejected, not moved, or paging skips bytes: {}",
        inside.output()
    );

    let boundary = tool
        .execute(json!({"path": "utf8.txt", "offset": 2}))
        .await
        .unwrap();
    assert_eq!(boundary.output(), " world");
}

#[tokio::test]
async fn file_read_rejects_an_offset_that_is_not_a_non_negative_integer() {
    let dir = tempfile::tempdir().unwrap();
    tokio::fs::write(dir.path().join("offsets.txt"), "hello world")
        .await
        .unwrap();
    let tool = FileReadTool::new(test_security(dir.path().to_path_buf()));

    for bad in [json!(-5), json!("6"), json!(1.5)] {
        let result = tool
            .execute(json!({"path": "offsets.txt", "offset": bad.clone()}))
            .await
            .unwrap();
        assert!(
            result.is_error,
            "offset {bad} must be rejected, not treated as 0: got {:?}",
            result.output()
        );
    }

    let null_offset = tool
        .execute(json!({"path": "offsets.txt", "offset": null}))
        .await
        .unwrap();
    assert_eq!(
        null_offset.output(),
        "hello world",
        "a null offset reads from the start"
    );
}
