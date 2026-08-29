//! Unit tests for the composio connection-created event handler's gating.

use super::toolkit_is_memory_source_registrable;

/// #4957 regression, in the half that is still this host's.
///
/// The rule the original test locked — which toolkits have a native pipeline —
/// is the driver's now (`MemorySourceSync::is_toolkit_syncable`,
/// tinymemory#106), and it is tested there against the same offenders this one
/// named: `is_composio_toolkit_syncable("googlecalendar")` is false and
/// `" Gmail "` is true, so the normalisation is pinned upstream too. Asserting
/// the list again here would need a loaded module and would only re-check the
/// driver's answer.
///
/// What stays host-side is the **direction the host fails in**, and it is the
/// half that caused #4957: a toolkit that is auto-registered but cannot sync
/// reports ACTIVE and then fails forever, which is a silent lie. So when the
/// driver cannot answer — no binding, no `SourceSync` family, or `Unsupported`
/// — the host must treat the toolkit as *not* registrable rather than assume
/// it is fine.
///
/// The cannot-answer driver is **installed**, not assumed. This note used to
/// say "a unit test has no loaded module, so every answer here is the
/// cannot-answer case" — that is not true: `binding::module_provider`'s
/// `cfg(test)` arm loads `TINYMEMORY_TEST_MODULE`, which CI sets to the pinned
/// module, so the test could reach the module's real `SourceSync` and pass
/// while pinning nothing. It also used `Config::default()`, whose workspace is
/// the developer's own `~/.openhuman`.
///
/// So bind `NullMemoryProvider` — which serves no `SourceSync` family — against
/// a workspace this test owns. The cannot-answer branch is then the branch
/// under test by construction, on any machine, with or without the module.
#[tokio::test]
async fn a_driver_that_cannot_answer_makes_a_toolkit_not_registrable() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let mut config = crate::openhuman::config::Config::default();
    config.workspace_dir = tmp.path().to_path_buf();
    crate::openhuman::memory::binding::install_for_test(
        &config.workspace_dir,
        &config.subsystems.memory,
        std::sync::Arc::new(tinymemory_api::null::NullMemoryProvider::new()),
    );

    for toolkit in ["gmail", "slack", "github", "googlecalendar", ""] {
        assert!(
            !toolkit_is_memory_source_registrable(&config, toolkit).await,
            "with no module bound, '{toolkit}' must not be registrable — \
             registering a source that then fails every sync is #4957"
        );
    }
}
