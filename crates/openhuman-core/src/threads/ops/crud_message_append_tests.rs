use super::*;
use serde_json::json;

async fn setup() -> (tempfile::TempDir, crate::config::Config) {
    let temp = tempfile::tempdir().unwrap();
    let mut config = crate::config::Config::default();
    config.workspace_dir = temp.path().join("internal");
    config.action_dir = temp.path().join("acting");
    conversations::blocking::ensure_thread(
        crate::core::runtime::DispatchCtx::ambient(),
        config.workspace_dir.clone(),
        conversations::CreateConversationThread {
            id: "uploads".into(),
            title: "Uploads".into(),
            created_at: "2026-10-04T00:00:00Z".into(),
            parent_thread_id: None,
            labels: None,
            personality_id: None,
            working_dir: None,
        },
    )
    .await
    .unwrap();
    (temp, config)
}

async fn append_with_test_config(
    request: AppendConversationMessageRequest,
    config: &crate::config::Config,
    origin: Option<&crate::security::turn_origin::AgentTurnOrigin>,
) -> Result<Outcome<ApiEnvelope<ConversationMessageRecord>>, ThreadsError> {
    message_append_with_config(
        &crate::core::runtime::DispatchCtx::ambient(),
        request,
        config,
        origin,
        &config.workspace_dir,
    )
    .await
}

fn request(id: &str, content: &str, sender: &str) -> AppendConversationMessageRequest {
    AppendConversationMessageRequest {
        thread_id: "uploads".into(),
        message: ConversationMessageRecord {
            id: id.into(),
            content: content.into(),
            message_type: "text".into(),
            extra_metadata: json!({"attachmentNames":["image.png"], "attachmentKinds":["image"],
                "attachmentDataUris":["data:image/png;base64,iVBORw0KGgo="], "attachmentData":"bytes",
                "attachmentPosters":["data:image/png;base64,iVBORw0KGgo="],
                "attachmentPreview":"data:image/png;base64,iVBORw0KGgo=", "other":"kept"}),
            sender: sender.into(),
            created_at: "2026-10-04T00:00:00Z".into(),
        },
    }
}

#[tokio::test]
async fn user_append_stages_original_strips_upload_bytes_and_reuses_returned_content() {
    let (_temp, config) = setup().await;
    let first = append_with_test_config(
        request(
            "one",
            "look [IMAGE:data:image/png;base64,iVBORw0KGgo=]",
            "user",
        ),
        &config,
        None,
    )
    .await
    .unwrap()
    .value
    .data
    .unwrap();
    let (_, files) = crate::attachments::parse(&first.content);
    assert_eq!(files.len(), 1);
    assert_eq!(
        tokio::fs::read(config.action_dir.join(&files[0].path))
            .await
            .unwrap(),
        b"\x89PNG\r\n\x1a\n"
    );
    assert!(first.extra_metadata.get("attachmentDataUris").is_none());
    assert!(first.extra_metadata.get("attachmentData").is_none());
    assert!(first.extra_metadata.get("attachmentPosters").is_none());
    assert!(first.extra_metadata.get("attachmentPreview").is_none());
    assert_eq!(
        first.extra_metadata["attachmentNames"],
        json!(["image.png"])
    );
    assert_eq!(first.extra_metadata["attachmentKinds"], json!(["image"]));
    assert_eq!(first.extra_metadata["other"], "kept");
    let second = append_with_test_config(request("two", &first.content, "user"), &config, None)
        .await
        .unwrap()
        .value
        .data
        .unwrap();
    assert_eq!(second.content, first.content);
    let staged =
        crate::attachments::stage_turn(&second.content, Some(&config), None, Some("uploads"), None)
            .await
            .unwrap();
    assert_eq!(staged, first.content);
    let persisted = conversations::blocking::get_messages(
        crate::core::runtime::DispatchCtx::ambient(),
        config.workspace_dir.clone(),
        "uploads".into(),
    )
    .await
    .unwrap();
    assert_eq!(persisted.len(), 2);
    assert!(!serde_json::to_string(&persisted)
        .unwrap()
        .contains("base64,"));
}

#[tokio::test]
async fn rejected_upload_never_appends_a_message() {
    let (_temp, mut config) = setup().await;
    config.multimodal_files.max_files = 0;
    assert!(append_with_test_config(
        request(
            "disabled",
            "[IMAGE:data:image/png;base64,iVBORw0KGgo=]",
            "user"
        ),
        &config,
        None,
    )
    .await
    .is_err());
    config.multimodal_files.max_files = 3;
    assert!(append_with_test_config(
        request("bad", "[FILE:data:application/zip;base64,!]", "user"),
        &config,
        None,
    )
    .await
    .is_err());
    assert!(
        append_with_test_config(request("missing", "image.png", "user"), &config, None)
            .await
            .is_err()
    );
    assert!(conversations::blocking::get_messages(
        crate::core::runtime::DispatchCtx::ambient(),
        config.workspace_dir.clone(),
        "uploads".into()
    )
    .await
    .unwrap()
    .is_empty());
}

#[tokio::test]
async fn external_origin_cannot_reuse_an_existing_attachment_marker() {
    let (_temp, config) = setup().await;
    let staged = crate::attachments::stage_turn(
        "[IMAGE:data:image/png;base64,iVBORw0KGgo=]",
        Some(&config),
        None,
        Some("uploads"),
        None,
    )
    .await
    .unwrap();
    let origin = crate::security::turn_origin::AgentTurnOrigin::ExternalChannel {
        sender_name: None,
        channel: "test".into(),
        sender: None,
        reply_target: "room".into(),
        message_id: "message".into(),
        history_key: None,
    };
    assert!(
        append_with_test_config(request("external", &staged, "user"), &config, Some(&origin))
            .await
            .is_err()
    );
    assert!(conversations::blocking::get_messages(
        crate::core::runtime::DispatchCtx::ambient(),
        config.workspace_dir,
        "uploads".into()
    )
    .await
    .unwrap()
    .is_empty());
}

#[tokio::test]
async fn ordinary_user_text_and_agent_images_are_preserved() {
    let (_temp, config) = setup().await;
    let mut user = request("text", "ordinary text", "user");
    user.message.extra_metadata = json!({"other":"kept"});
    let stored = append_with_test_config(user, &config, None)
        .await
        .unwrap()
        .value
        .data
        .unwrap();
    assert_eq!(stored.content, "ordinary text");
    assert_eq!(stored.extra_metadata, json!({"other":"kept"}));
    let assistant = request(
        "output",
        "[IMAGE:data:image/png;base64,iVBORw0KGgo=]",
        "assistant",
    );
    let expected = serde_json::to_value(&assistant.message).unwrap();
    let stored = append_with_test_config(assistant, &config, None)
        .await
        .unwrap()
        .value
        .data
        .unwrap();
    assert_eq!(serde_json::to_value(stored).unwrap(), expected);
}

#[tokio::test]
async fn append_uses_host_persistence_workspace_separate_from_rpc_config() {
    let (temp, config) = setup().await;
    let persistence_dir = temp.path().join("host-thread-store");
    conversations::blocking::ensure_thread(
        crate::core::runtime::DispatchCtx::ambient(),
        persistence_dir.clone(),
        conversations::CreateConversationThread {
            id: "uploads".into(),
            title: "Host thread store".into(),
            created_at: "2026-10-04T00:00:00Z".into(),
            parent_thread_id: None,
            labels: None,
            personality_id: None,
            working_dir: None,
        },
    )
    .await
    .unwrap();

    message_append_with_config(
        &crate::core::runtime::DispatchCtx::ambient(),
        request("host-store", "saved in host workspace", "assistant"),
        &config,
        None,
        &persistence_dir,
    )
    .await
    .unwrap();

    let persisted = conversations::blocking::get_messages(
        crate::core::runtime::DispatchCtx::ambient(),
        persistence_dir,
        "uploads".into(),
    )
    .await
    .unwrap();
    assert_eq!(persisted.len(), 1);
    assert!(conversations::blocking::get_messages(
        crate::core::runtime::DispatchCtx::ambient(),
        config.workspace_dir,
        "uploads".into()
    )
    .await
    .unwrap()
    .is_empty());
}

#[tokio::test]
async fn poster_only_user_metadata_is_rejected_before_persistence() {
    let (_temp, config) = setup().await;
    let mut upload = request("poster", "video.mp4", "user");
    upload.message.extra_metadata = json!({
        "attachmentNames": ["video.mp4"],
        "attachmentPosters": ["data:image/png;base64,iVBORw0KGgo="]
    });
    assert!(append_with_test_config(upload, &config, None)
        .await
        .is_err());
    assert!(conversations::blocking::get_messages(
        crate::core::runtime::DispatchCtx::ambient(),
        config.workspace_dir.clone(),
        "uploads".into()
    )
    .await
    .unwrap()
    .is_empty());
}

#[tokio::test]
async fn raw_append_stages_in_the_bound_thread_folder() {
    let (temp, config) = setup().await;
    let folder = temp.path().join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let folder = folder.canonicalize().unwrap();
    conversations::blocking::ensure_thread(
        crate::core::runtime::DispatchCtx::ambient(),
        config.workspace_dir.clone(),
        conversations::CreateConversationThread {
            id: "bound".into(),
            title: "Bound".into(),
            created_at: "2026-10-04T00:00:00Z".into(),
            parent_thread_id: None,
            labels: None,
            personality_id: None,
            working_dir: Some(folder.to_string_lossy().into_owned()),
        },
    )
    .await
    .unwrap();
    let mut raw = request("raw", "[IMAGE:data:image/png;base64,iVBORw0KGgo=]", "user");
    raw.thread_id = "bound".into();
    let staged = append_with_test_config(raw, &config, None)
        .await
        .unwrap()
        .value
        .data
        .unwrap();
    let (_, files) = crate::attachments::parse(&staged.content);
    assert_eq!(files.len(), 1);
    assert!(folder.join(&files[0].path).is_file());
    assert!(!config.action_dir.join("uploads").exists());
}

async fn append_one_attachment(
    config: &crate::config::Config,
    path: &str,
    mime: &str,
    bytes: usize,
) -> Result<(), ThreadsError> {
    let file = config.action_dir.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, vec![b'x'; bytes]).unwrap();
    let marker = crate::attachments::Attachment {
        path: path.into(),
        name: path.rsplit('/').next().unwrap().into(),
        mime: mime.into(),
        size_bytes: bytes,
    }
    .marker();
    let mut upload = request("limit", &marker, "user");
    upload.message.extra_metadata = json!({});
    append_with_test_config(upload, config, None)
        .await
        .map(|_| ())
}

async fn no_messages(config: &crate::config::Config) -> bool {
    conversations::blocking::get_messages(
        crate::core::runtime::DispatchCtx::ambient(),
        config.workspace_dir.clone(),
        "uploads".into(),
    )
    .await
    .unwrap()
    .is_empty()
}

#[tokio::test]
async fn an_attachment_over_the_file_limit_is_never_appended() {
    let (_temp, mut config) = setup().await;
    config.multimodal_files.max_file_size_mb = 1;
    let error = append_one_attachment(&config, "uploads/big.txt", "text/plain", 1024 * 1024 + 1)
        .await
        .expect_err("over the file limit");
    assert!(
        error.to_string().contains("exceeds configured file limit"),
        "{error}"
    );
    assert!(no_messages(&config).await);

    // At the limit it goes through, so the refusal above is the size cap.
    append_one_attachment(&config, "uploads/ok.txt", "text/plain", 1024 * 1024)
        .await
        .expect("at the file limit");
}

#[tokio::test]
async fn an_image_is_refused_when_images_are_disabled() {
    let (_temp, mut config) = setup().await;
    config.multimodal.max_images = 0;
    let error = append_one_attachment(&config, "uploads/a.png", "image/png", 16)
        .await
        .expect_err("images disabled");
    assert!(error.to_string().contains("count exceeds"), "{error}");
    assert!(no_messages(&config).await);
}

/// A message appends on the backend production opens by default (SQLite),
/// not only on the in-memory test default. SQLite advertises conditional
/// stream appends on its stream port alone, and TinyAgents once checked the
/// document port for it, refusing every append with "unsupported capability
/// conditional_stream_append".
#[cfg(feature = "storage-sqlite")]
#[tokio::test]
#[allow(clippy::await_holding_lock)] // Serializes the process-wide storage slot.
async fn a_message_appends_on_a_sqlite_workspace() {
    // `backend_for` prefers a process-installed backend over the workspace
    // default; hold the slot so no other test installs one (an in-memory
    // backend lists every capability and would hide the bug), and prove it.
    let _storage_slot = crate::storage::test_support::lock_storage_slot();
    assert!(
        crate::storage::installed().is_none(),
        "the append must run on the workspace's SQLite default, not an installed backend"
    );
    let temp = tempfile::tempdir().unwrap();
    let mut config = crate::config::Config::default();
    config.workspace_dir = temp.path().join("internal");
    config.action_dir = temp.path().join("acting");
    std::fs::create_dir_all(&config.workspace_dir).unwrap();
    crate::storage::local::use_persistent_default_for_test(&config.workspace_dir);
    conversations::blocking::ensure_thread(
        crate::core::runtime::DispatchCtx::ambient(),
        config.workspace_dir.clone(),
        conversations::CreateConversationThread {
            id: "uploads".into(),
            title: "Uploads".into(),
            created_at: "2026-10-04T00:00:00Z".into(),
            parent_thread_id: None,
            labels: None,
            personality_id: None,
            working_dir: None,
        },
    )
    .await
    .unwrap();
    let plain = AppendConversationMessageRequest {
        thread_id: "uploads".into(),
        message: ConversationMessageRecord {
            id: "m1".into(),
            content: "hello".into(),
            message_type: "text".into(),
            extra_metadata: json!({}),
            sender: "agent".into(),
            created_at: "2026-10-04T00:00:00Z".into(),
        },
    };
    append_with_test_config(plain, &config, None)
        .await
        .expect("a message appends on a SQLite-backed workspace");
}
