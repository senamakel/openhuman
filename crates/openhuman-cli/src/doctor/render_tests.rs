use serde_json::json;

use super::render_table;

#[test]
fn table_groups_by_check_category_and_renders_fix_hints() {
    let report = json!({
        "findings": [
            {
                "checkId": "config/provenance",
                "severity": "warn",
                "message": "The setting came from an implicit source.",
                "fixHint": "Set the intended source explicitly.",
                "durationMs": 1,
                "affects": []
            },
            {
                "checkId": "config/file-integrity",
                "severity": "error",
                "message": "The config file is invalid.",
                "fixHint": "Repair the config file.",
                "durationMs": 1,
                "affects": []
            },
            {
                "checkId": "workspace/layout",
                "severity": "pass",
                "message": "The workspace layout is valid.",
                "fixHint": "No action required.",
                "durationMs": 1,
                "affects": []
            }
        ],
        "summary": { "pass": 1, "info": 0, "warn": 1, "error": 1, "skipped": 0 },
        "ok": false
    });

    let rendered = render_table(&report);

    assert!(rendered.contains("config"));
    assert!(rendered.contains("workspace"));
    assert!(rendered.contains("[warn] config/provenance The setting came from an implicit source."));
    assert!(rendered.contains("[error] config/file-integrity The config file is invalid."));
    assert!(rendered.contains("  fix: Set the intended source explicitly."));
    assert!(rendered.contains("  fix: Repair the config file."));
    assert!(rendered.contains("Summary:"));
}
