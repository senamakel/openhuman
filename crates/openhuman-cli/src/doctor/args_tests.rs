use serde_json::{json, Value};

use super::{exit_code, parse_args, DoctorArgs};

fn args(values: &[&str]) -> DoctorArgs {
    let values = values
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    parse_args(&values).expect("valid doctor arguments")
}

fn report(findings: Value) -> Value {
    json!({
        "schemaVersion": 2,
        "profile": null,
        "configSource": "profile",
        "tier": "connect",
        "checksRun": [],
        "checksSkipped": [],
        "findings": findings,
        "summary": { "pass": 0, "info": 0, "warn": 0, "error": 0, "skipped": 0 },
        "ok": true
    })
}

#[test]
fn lint_defaults_to_error_threshold() {
    let options = args(&["--lint"]);
    let warnings = report(json!([{
        "checkId": "config/provenance",
        "severity": "warn",
        "message": "The setting came from an implicit source.",
        "fixHint": "Set the intended source explicitly.",
        "durationMs": 1,
        "affects": []
    }]));

    assert_eq!(exit_code(&options, Ok(&warnings)), 0);
}

#[test]
fn lint_error_exits_one() {
    let options = args(&["--lint"]);
    let errors = report(json!([{
        "checkId": "config/file-integrity",
        "severity": "error",
        "message": "The config file is invalid.",
        "fixHint": "Repair the config file.",
        "durationMs": 1,
        "affects": []
    }]));

    assert_eq!(exit_code(&options, Ok(&errors)), 1);
}

#[test]
fn lint_warning_exits_one_when_warn_is_the_minimum() {
    let options = args(&["--lint", "--severity-min", "warn"]);
    let warnings = report(json!([{
        "checkId": "config/provenance",
        "severity": "warn",
        "message": "The setting came from an implicit source.",
        "fixHint": "Set the intended source explicitly.",
        "durationMs": 1,
        "affects": []
    }]));

    assert_eq!(exit_code(&options, Ok(&warnings)), 1);
}

#[test]
fn runtime_failure_exits_two_before_a_report_exists() {
    let options = args(&["--lint"]);

    assert_eq!(exit_code(&options, Err(())), 2);
}

#[test]
fn unknown_only_selector_error_finding_fails_lint() {
    let options = args(&["--lint", "--only", "unknown/check"]);
    let selection_error = report(json!([{
        "checkId": "doctor/selection",
        "severity": "error",
        "message": "A requested check id is not registered.",
        "fixHint": "Use a registered exact check id.",
        "durationMs": 0,
        "affects": []
    }]));

    assert_eq!(exit_code(&options, Ok(&selection_error)), 1);
}

#[test]
fn json_without_lint_is_advisory_even_for_errors() {
    let options = args(&["--json"]);
    let errors = report(json!([{
        "checkId": "config/file-integrity",
        "severity": "error",
        "message": "The config file is invalid.",
        "fixHint": "Repair the config file.",
        "durationMs": 1,
        "affects": []
    }]));

    assert_eq!(exit_code(&options, Ok(&errors)), 0);
}

#[test]
fn live_profile_and_repeatable_selectors_map_to_rpc_options() {
    let options = args(&[
        "--live",
        "--profile",
        "desktop-default",
        "--only",
        "config/provenance",
        "--only",
        "workspace/layout",
        "--skip",
        "config/provenance",
    ]);

    assert_eq!(options.tier, "live");
    assert_eq!(options.profile.as_deref(), Some("desktop-default"));
    assert_eq!(options.only, ["config/provenance", "workspace/layout"]);
    assert_eq!(options.skip, ["config/provenance"]);
}
