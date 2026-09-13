use super::*;

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn plain_digits_remain_chat_input_and_alt_digits_switch_tabs() {
    for digit in ['1', '2', '3', '4'] {
        assert_eq!(
            tab_shortcut(key(KeyCode::Char(digit), KeyModifiers::NONE), AppTab::Chat),
            None
        );
    }
    assert_eq!(
        tab_shortcut(key(KeyCode::Char('3'), KeyModifiers::ALT), AppTab::Chat),
        Some(AppTab::Config)
    );
}

#[test]
fn paste_routes_only_to_the_active_editable_surface() {
    let mut ui = UiState::new("thread".into(), "client".into());
    ui.active_tab = AppTab::Chat;
    handle_paste("model-4", &mut ui);
    assert_eq!(ui.composer.text(), "model-4");

    ui.active_tab = AppTab::Settings;
    ui.login_token = Some(String::new());
    handle_paste("one-time-token", &mut ui);
    assert_eq!(ui.login_token.as_deref(), Some("one-time-token"));
}

#[test]
fn workspace_file_picker_is_bounded_filtered_and_skips_heavy_trees() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::create_dir_all(root.path().join("target")).unwrap();
    std::fs::write(root.path().join("src/main.rs"), "fn main() {}").unwrap();
    std::fs::write(root.path().join("target/hidden.rs"), "").unwrap();
    let files = collect_files(root.path().to_str().unwrap(), "main", 10).unwrap();
    assert_eq!(files, vec!["src/main.rs"]);
}

#[test]
fn workspace_file_picker_stops_at_the_depth_limit() {
    let root = tempfile::tempdir().unwrap();
    let mut deep = root.path().to_path_buf();
    for level in 0..10 {
        deep.push(format!("level-{level}"));
    }
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("too-deep.txt"), "hidden").unwrap();
    let files = collect_files(root.path().to_str().unwrap(), "too-deep", 10).unwrap();
    assert!(files.is_empty());
}

#[test]
fn decision_shortcuts_do_not_treat_filter_text_as_deny() {
    assert_eq!(
        decision_shortcut(OverlayKind::Approvals, false, KeyCode::Char('d')),
        None
    );
    assert_eq!(
        decision_shortcut(OverlayKind::Approvals, false, KeyCode::Delete),
        Some("deny")
    );
    assert_eq!(
        decision_shortcut(OverlayKind::PlanReview, true, KeyCode::Char('a')),
        None
    );
}

#[test]
fn inbound_approval_preserves_typed_overlay_input() {
    let mut state = TranscriptState::new("client");
    state.set_thread("thread");
    let mut ui = UiState::new("thread".into(), "client".into());
    let mut overlay = Overlay::new(OverlayKind::Rename, "Rename");
    overlay.input = Some("draft title".into());
    ui.overlay = Some(overlay);
    handle_web_event(
        &WebChannelEvent {
            event: "approval_request".into(),
            client_id: "client".into(),
            thread_id: "thread".into(),
            request_id: "approval-1".into(),
            tool_name: Some("shell".into()),
            ..Default::default()
        },
        &mut state,
        &mut ui,
    );
    assert_eq!(
        ui.overlay
            .as_ref()
            .and_then(|overlay| overlay.input.as_deref()),
        Some("draft title")
    );
    assert_eq!(ui.pending_approvals.len(), 1);
}

#[test]
fn inbound_plan_review_waits_for_typed_overlay_to_close() {
    let mut state = TranscriptState::new("client");
    state.set_thread("thread");
    let mut ui = UiState::new("thread".into(), "client".into());
    let mut overlay = Overlay::new(OverlayKind::Rename, "Rename");
    overlay.input = Some("draft title".into());
    ui.overlay = Some(overlay);
    handle_web_event(
        &WebChannelEvent {
            event: "plan_review_request".into(),
            client_id: "client".into(),
            thread_id: "thread".into(),
            request_id: "review-1".into(),
            message: Some("Review this plan".into()),
            args: Some(json!({"steps": ["Inspect", "Implement"]})),
            ..Default::default()
        },
        &mut state,
        &mut ui,
    );
    assert_eq!(
        ui.overlay
            .as_ref()
            .and_then(|overlay| overlay.input.as_deref()),
        Some("draft title")
    );

    ui.overlay = None;
    present_pending_plan_review(&mut ui);
    assert_eq!(
        ui.overlay.as_ref().map(|overlay| overlay.kind),
        Some(OverlayKind::PlanReview)
    );
    assert_eq!(
        ui.pending_plan_review
            .as_ref()
            .map(|review| review.request_id.as_str()),
        Some("review-1")
    );
}
