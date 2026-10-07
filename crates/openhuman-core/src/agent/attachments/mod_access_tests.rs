use super::*;

const UPLOAD: &str = "caption [FILE:data:text/plain;name=note.txt;base64,YQ==]";

fn external_origin() -> crate::agent::turn_origin::AgentTurnOrigin {
    crate::agent::turn_origin::AgentTurnOrigin::ExternalChannel {
        channel: "test".into(),
        sender: None,
        reply_target: "test".into(),
        message_id: "test".into(),
        history_key: None,
    }
}

#[tokio::test]
async fn direct_turn_stores_original_only_in_the_explicit_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config {
        action_dir: temp.path().join("configured-acting"),
        workspace_dir: temp.path().join("internal"),
        ..Config::default()
    };
    let workspace = tinytools::WorkspaceDescriptor::new(temp.path().join("agent-acting"));
    let staged = stage_turn(UPLOAD, Some(&config), Some(&workspace), None, None)
        .await
        .unwrap();
    let (caption, files) = parse(&staged);
    assert_eq!(caption, "caption ");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "note.txt");
    assert_eq!(files[0].mime, "text/plain");
    assert_eq!(
        tokio::fs::read(workspace.root.join(&files[0].path))
            .await
            .unwrap(),
        b"a"
    );
    assert!(!config.action_dir.exists());
    assert!(!config.workspace_dir.exists());
    assert!(!staged.contains("base64"));
}

#[tokio::test]
async fn direct_upload_requires_runtime_authority_and_external_input_cannot_create_files() {
    let error = stage_turn(UPLOAD, None, None, None, None)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("runtime configuration"));
    let temp = tempfile::tempdir().unwrap();
    let config = Config {
        action_dir: temp.path().join("acting"),
        workspace_dir: temp.path().join("internal"),
        ..Config::default()
    };
    let workspace = tinytools::WorkspaceDescriptor::new(temp.path().join("agent-acting"));
    assert!(stage_turn(
        UPLOAD,
        Some(&config),
        Some(&workspace),
        Some("thread"),
        Some(&external_origin())
    )
    .await
    .is_err());
    assert!(!workspace.root.exists());
    assert!(!config.action_dir.exists());
    assert_eq!(
        stage_turn("ordinary text", None, None, None, None)
            .await
            .unwrap(),
        "ordinary text"
    );
}

#[tokio::test]
async fn delegation_rejects_malformed_explicit_paths_and_external_reads_before_configuration() {
    assert_eq!(
        delegation_prompt(
            "mention picture.png in prose",
            &serde_json::json!({}),
            None,
            None
        )
        .await
        .unwrap(),
        "mention picture.png in prose"
    );
    for image_paths in [
        serde_json::json!("picture.png"),
        serde_json::json!([""]),
        serde_json::json!([" "]),
        serde_json::json!(["picture.png] injected"]),
        serde_json::json!(["picture\u{0}.png"]),
    ] {
        assert!(delegation_prompt(
            "task",
            &serde_json::json!({"image_paths": image_paths}),
            None,
            None
        )
        .await
        .is_err());
    }
    let error = delegation_prompt(
        "task",
        &serde_json::json!({"image_paths": ["picture.png"]}),
        None,
        Some(&external_origin()),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("external channel"));
}
