use super::*;

#[test]
fn collect_deep_link_urls_filters_args() {
    let urls = collect_deep_link_urls_from_args([
        "OpenHuman.exe",
        "openhuman://auth?token=secret&key=auth",
        "--flag",
        "https://example.test",
        "openhuman://oauth/success?integrationId=abc",
    ]);

    assert_eq!(
        urls,
        vec![
            "openhuman://auth?token=secret&key=auth",
            "openhuman://oauth/success?integrationId=abc"
        ]
    );
}

#[test]
fn redact_url_removes_query_and_fragment() {
    assert_eq!(
        redact_url_for_log("openhuman://auth?token=secret&key=auth#frag"),
        "openhuman://auth"
    );
}

#[test]
fn pipe_name_is_stable_and_app_scoped() {
    assert_eq!(PIPE_NAME, r"\\.\pipe\com.openhuman.app-deeplink");
}
