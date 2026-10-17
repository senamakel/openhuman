use super::*;

use crate::core::runtime::context::CoreContext;
use crate::core::runtime::{DispatchCtx, DomainSet};
use serde_json::{Map, Value};

#[tokio::test]
async fn computer_status_handler_returns_serialized_status() {
    let ctx =
        CoreContext::for_test_with_config(DomainSet::full(), crate::config::Config::default());

    let status = CoreContext::scope(ctx.clone(), async {
        handle_computer_status(DispatchCtx::new(ctx.clone()), Map::new()).await
    })
    .await
    .expect("computer status should serialize");

    assert!(status.get("decision_model").is_some());
}

#[tokio::test]
async fn forgetting_sites_reports_how_many_went() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = crate::config::Config::default();
    config.workspace_dir = dir.path().join("workspace");
    let sites = crate::desktop::computer_config::trace_dir(&config).join("sites");
    std::fs::create_dir_all(&sites).unwrap();
    for site in ["shop.test", "books.test"] {
        std::fs::write(sites.join(format!("{site}.json")), b"{}").unwrap();
    }
    let ctx = CoreContext::for_test_with_config(DomainSet::full(), config);

    let (one, every) = CoreContext::scope(ctx.clone(), async {
        let mut one = Map::new();
        one.insert(
            "site".to_string(),
            Value::String("https://www.shop.test/cart".to_string()),
        );
        let one = handle_browser_forget_sites(DispatchCtx::new(ctx.clone()), one).await;
        let every = handle_browser_forget_sites(DispatchCtx::new(ctx.clone()), Map::new()).await;
        (one, every)
    })
    .await;

    assert_eq!(one.unwrap()["forgotten"], 1);
    assert_eq!(every.unwrap()["forgotten"], 1);
    assert!(!sites.join("books.test.json").exists());
}

#[test]
fn the_chrome_check_says_where_to_fix_a_missing_chrome() {
    let missing =
        readiness_error("BrowserUnavailable: browser unavailable: Chrome not found. Checked: …");
    assert!(missing.starts_with("Chrome was not found."), "{missing}");
    assert!(missing.contains("Chrome path"), "{missing}");
    assert_eq!(
        readiness_error("Bus: launch timed out"),
        "Chrome could not start: Bus: launch timed out"
    );
    let missing = readiness_error(
        "BrowserUnavailable: browser unavailable: no Chrome or Chromium was found on this machine; give the path of the browser to use",
    );
    assert!(missing.starts_with("Chrome was not found."), "{missing}");
    let unusable = readiness_error(
        "BrowserUnavailable: browser unavailable: the browser binary given could not be started (Chrome exited before providing DevTools URL); check that its path names Chrome or Chromium",
    );
    assert!(
        unusable.starts_with("Chrome could not be started from Chrome path."),
        "{unusable}"
    );
}

#[tokio::test]
async fn a_blank_or_unreadable_site_forgets_nothing() {
    for site in [Value::String("  ".to_string()), Value::from(7)] {
        let mut params = Map::new();
        params.insert("site".to_string(), site);
        let error = handle_browser_forget_sites(DispatchCtx::process_default(), params)
            .await
            .unwrap_err();
        assert!(error.contains("must name a site"), "{error}");
    }
}
