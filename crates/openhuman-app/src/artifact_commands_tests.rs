use super::*;

#[test]
fn sanitize_rejects_path_separators() {
    assert!(sanitize_filename("../etc/passwd").is_err());
    assert!(sanitize_filename("a\\b.pptx").is_err());
    assert!(sanitize_filename("a/b.pptx").is_err());
    assert!(sanitize_filename("").is_err());
    assert!(sanitize_filename(".").is_err());
    assert!(sanitize_filename("..").is_err());
    assert!(sanitize_filename("ok.pptx\0").is_err());
}

#[test]
fn sanitize_accepts_plain_names() {
    assert_eq!(
        sanitize_filename("Quarterly Update.pptx").unwrap(),
        "Quarterly Update.pptx"
    );
    assert_eq!(sanitize_filename("  trim me  ").unwrap(), "trim me");
}

#[test]
fn validate_source_rejects_relative_and_empty() {
    assert!(validate_source("").is_err());
    assert!(validate_source("relative/path.pptx").is_err());
    assert!(validate_source("/definitely/not/here.pptx").is_err());
}

#[test]
fn assert_artifact_source_accepts_file_under_artifacts_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let art = root.join("users/u1/workspace/artifacts/a-1");
    std::fs::create_dir_all(&art).unwrap();
    let file = art.join("deck.pptx");
    std::fs::write(&file, b"x").unwrap();
    assert!(assert_artifact_source(&file, root).is_ok());
}

#[test]
fn assert_artifact_source_rejects_file_without_artifacts_component() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let other = root.join("users/u1/secrets");
    std::fs::create_dir_all(&other).unwrap();
    let file = other.join("token.txt");
    std::fs::write(&file, b"x").unwrap();
    assert!(assert_artifact_source(&file, root).is_err());
}

#[test]
fn assert_artifact_source_rejects_file_outside_root() {
    let root_dir = tempfile::tempdir().unwrap();
    let outside_dir = tempfile::tempdir().unwrap();
    // Even with an `artifacts` segment, a path outside the root is denied.
    let art = outside_dir.path().join("artifacts");
    std::fs::create_dir_all(&art).unwrap();
    let file = art.join("evil.pptx");
    std::fs::write(&file, b"x").unwrap();
    assert!(assert_artifact_source(&file, root_dir.path()).is_err());
}

#[tokio::test]
async fn copy_to_path_copies_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("src.pptx");
    let dst = temp.path().join("dst.pptx");
    std::fs::write(&src, b"deck-bytes").unwrap();
    let n = copy_to_path(&src, &dst).await.unwrap();
    assert_eq!(n, b"deck-bytes".len() as u64);
    assert_eq!(std::fs::read(&dst).unwrap(), b"deck-bytes");
}

#[tokio::test]
async fn download_rejects_bad_source() {
    // Source validation runs before the Downloads directory is touched,
    // so these resolve without writing anything.
    assert!(
        download_artifact_to_downloads(String::new(), "x.pptx".to_string())
            .await
            .is_err()
    );
    assert!(
        download_artifact_to_downloads("relative".to_string(), "x.pptx".to_string())
            .await
            .is_err()
    );
}

#[test]
fn split_stem_ext_pairs() {
    assert_eq!(
        split_stem_ext("file.pptx"),
        ("file".to_string(), "pptx".to_string())
    );
    assert_eq!(
        split_stem_ext("noext"),
        ("noext".to_string(), String::new())
    );
    assert_eq!(
        split_stem_ext(".hidden"),
        (".hidden".to_string(), String::new())
    );
    assert_eq!(
        split_stem_ext("trailing."),
        ("trailing.".to_string(), String::new())
    );
    assert_eq!(
        split_stem_ext("a.b.c"),
        ("a.b".to_string(), "c".to_string())
    );
}

#[test]
fn pick_unique_inserts_collision_suffix() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let first = pick_unique_path(dir, "deck.pptx");
    assert_eq!(first, dir.join("deck.pptx"));

    std::fs::write(&first, b"").unwrap();
    let second = pick_unique_path(dir, "deck.pptx");
    assert_eq!(second, dir.join("deck (1).pptx"));

    std::fs::write(&second, b"").unwrap();
    let third = pick_unique_path(dir, "deck.pptx");
    assert_eq!(third, dir.join("deck (2).pptx"));
}

#[test]
fn pick_unique_handles_no_extension() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let first = pick_unique_path(dir, "noext");
    assert_eq!(first, dir.join("noext"));
    std::fs::write(&first, b"").unwrap();
    let second = pick_unique_path(dir, "noext");
    assert_eq!(second, dir.join("noext (1)"));
}

#[tokio::test]
async fn download_rejects_invalid_inputs() {
    assert!(
        download_artifact_to_downloads(String::new(), "x.pptx".to_string())
            .await
            .is_err()
    );
    assert!(
        download_artifact_to_downloads("/tmp/x".to_string(), String::new())
            .await
            .is_err()
    );
    assert!(
        download_artifact_to_downloads("relative".to_string(), "x.pptx".to_string())
            .await
            .is_err()
    );
    assert!(
        download_artifact_to_downloads("/nope".to_string(), "../escape.pptx".to_string())
            .await
            .is_err()
    );
}
