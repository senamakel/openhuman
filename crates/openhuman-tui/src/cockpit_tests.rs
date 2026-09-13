use super::*;

#[test]
fn overlay_filter_matches_label_and_detail() {
    let mut overlay = Overlay::new(OverlayKind::Threads, "Threads");
    overlay.rows.push(OverlayRow {
        id: "1".into(),
        label: "Release prep".into(),
        detail: "yesterday".into(),
        payload: Value::Null,
    });
    overlay.filter = "yester".into();
    assert_eq!(overlay.visible_rows().len(), 1);
    overlay.filter = "missing".into();
    assert!(overlay.visible_rows().is_empty());
}

#[test]
fn unwrap_rpc_handles_nested_envelopes() {
    let value = serde_json::json!({"result":{"data":{"threads":[]}}});
    assert!(unwrap_rpc(&value).get("threads").is_some());
}
