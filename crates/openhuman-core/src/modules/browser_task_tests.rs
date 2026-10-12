use super::*;
use serde_json::json;
use tinycomputer_bus::agent::AgentError;

fn task() -> BrowserTask {
    BrowserTask {
        goal: "Find the opening hours".into(),
        facts: BTreeMap::from([("name".into(), "Asha".into())]),
        origins: vec!["https://.example.com".into()],
        max_actions: 12,
        flow: None,
        site: None,
    }
}

#[test]
fn start_request_confines_the_task_to_the_browser_under_host_policy() {
    let mut config = Config::default();
    config.browser.headless = false;
    config.browser.task_timeout_secs = 90;
    config.computer.max_rescues = Some(2);
    let request = start_request(&config, &task());
    assert_eq!(request.task.as_deref(), Some("Find the opening hours"));
    assert_eq!(request.facts["name"], "Asha");
    assert_eq!(request.constraints.surfaces, vec![SurfaceKind::Browser]);
    assert_eq!(request.constraints.origins, vec!["https://.example.com"]);
    assert_eq!(request.constraints.payment, PaymentMode::StopAtPayment);
    assert!(!request.constraints.allow_destructive);
    assert!(request.constraints.headed);
    assert!(request.constraints.browser_executable.is_none());
    assert!(request.constraints.browser_profile.is_none());
    assert_eq!(request.budget.max_actions, Some(12));
    assert_eq!(request.budget.max_elapsed_ms, Some(90_000));
    assert_eq!(request.budget.max_rescues, Some(2));
}

#[test]
fn a_task_carries_the_chrome_path_and_the_persistent_profile_from_settings() {
    let mut config = Config::default();
    config.browser.chrome_path =
        Some("/Users/asha/Desktop/Google Chrome.app/Contents/MacOS/Google Chrome".into());
    config.browser.profile_mode = "persistent".into();
    config.browser.profile_path = Some("/Users/asha/.openhuman/chrome".into());
    let request = start_request(&config, &task());
    assert_eq!(
        request.constraints.browser_executable.as_deref(),
        Some("/Users/asha/Desktop/Google Chrome.app/Contents/MacOS/Google Chrome")
    );
    assert_eq!(
        request.constraints.browser_profile.as_deref(),
        Some("/Users/asha/.openhuman/chrome")
    );
    // A fresh profile is the module's own temporary one.
    config.browser.profile_mode = "fresh".into();
    assert!(start_request(&config, &task())
        .constraints
        .browser_profile
        .is_none());
    assert!(start_request(&Config::default(), &task())
        .constraints
        .browser_executable
        .is_none());
}

#[test]
fn a_traced_host_asks_the_module_to_record_every_decision() {
    let mut config = Config::default();
    config.computer.trace = true;
    assert!(start_request(&config, &task()).trace);
}

#[tokio::test]
async fn start_refuses_a_task_without_origins() {
    let mut empty = task();
    empty.origins.clear();
    let error = start(&Config::default(), &empty).await.unwrap_err();
    assert!(error.contains("allowed origin"), "{error}");
}

#[test]
fn only_the_module_refusing_a_flow_reads_as_a_refused_flow() {
    let refused: AgentResponse<u32> = AgentResponse {
        ok: false,
        data: None,
        error: Some(AgentError::new(
            "INVALID_FLOW",
            "step 2 is not a step",
            "fix the flow",
            true,
        )),
    };
    assert!(refused_flow(
        &unwrap_response("StartTask", refused).unwrap_err()
    ));
    let other: AgentResponse<u32> = AgentResponse {
        ok: false,
        data: None,
        error: Some(AgentError::new(
            "PLANNER_UNAVAILABLE",
            "no planner",
            "configure one",
            false,
        )),
    };
    assert!(!refused_flow(
        &unwrap_response("StartTask", other).unwrap_err()
    ));
    // Only the code counts, not a message that quotes it.
    let quoted: AgentResponse<u32> = AgentResponse {
        ok: false,
        data: None,
        error: Some(AgentError::new(
            "PLANNER_UNAVAILABLE",
            "[INVALID_FLOW] was the last answer",
            "configure one",
            false,
        )),
    };
    assert!(!refused_flow(
        &unwrap_response("StartTask", quoted).unwrap_err()
    ));
}

#[test]
fn unwrap_response_returns_data_or_the_structured_error() {
    let ok = AgentResponse {
        ok: true,
        data: Some(7),
        error: None,
    };
    assert_eq!(unwrap_response("StartTask", ok), Ok(7));
    let failed: AgentResponse<u32> = AgentResponse {
        ok: false,
        data: None,
        error: Some(AgentError::new(
            "PLANNER_UNAVAILABLE",
            "no planner",
            "configure one",
            false,
        )),
    };
    let error = unwrap_response("StartTask", failed).unwrap_err();
    assert!(error.contains("PLANNER_UNAVAILABLE"), "{error}");
    let empty: AgentResponse<u32> = AgentResponse {
        ok: true,
        data: None,
        error: None,
    };
    assert!(unwrap_response("AwaitTask", empty)
        .unwrap_err()
        .contains("no result"));
}

#[test]
fn a_chrome_path_the_module_refuses_says_where_the_user_fixes_it() {
    // TinyComputer refuses a task whose Chrome path names no program in its
    // own words; the agent is told where the user sets that path.
    let refused: AgentResponse<u32> = AgentResponse {
        ok: false,
        data: None,
        error: Some(AgentError::new(
            "INVALID_REQUEST",
            "browser_executable must be the absolute path of a browser binary on this machine",
            "",
            false,
        )),
    };
    let error = unwrap_response("StartTask", refused).unwrap_err();
    assert!(
        error.starts_with(
            "browser task StartTask failed [INVALID_REQUEST]: browser_executable must be"
        ),
        "{error}"
    );
    assert!(
        error.ends_with(crate::modules::browser::CHROME_PATH_HINT),
        "{error}"
    );
}

#[tokio::test]
async fn a_task_keeps_the_persistent_profile_only_while_no_other_chrome_holds_it() {
    let mut config = Config::default();
    config.browser.profile_mode = "persistent".into();
    let profile =
        std::env::temp_dir().join(format!("openhuman-task-profile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&profile);
    std::fs::create_dir_all(&profile).unwrap();
    config.browser.profile_path = Some(profile.to_string_lossy().into_owned());

    // Free: the task keeps it.
    let mut request = start_request(&config, &task());
    assert_eq!(
        claim_profile(&mut request, Duration::from_millis(50)).await,
        Some(profile.clone())
    );
    assert_eq!(
        request.constraints.browser_profile.as_deref(),
        config.browser.profile_path.as_deref()
    );

    // Held by a running Chrome (here, this process): a fresh profile instead.
    #[cfg(not(windows))]
    {
        let here = sysinfo::System::host_name().unwrap_or_default();
        std::os::unix::fs::symlink(
            format!("{here}-{}", std::process::id()),
            profile.join("SingletonLock"),
        )
        .unwrap();
        let mut request = start_request(&config, &task());
        assert_eq!(
            claim_profile(&mut request, Duration::from_millis(50)).await,
            None
        );
        assert!(request.constraints.browser_profile.is_none());
    }
    super::super::browser_profile::release(&profile);
    let _ = std::fs::remove_dir_all(&profile);

    // No persistent profile: nothing to wait for.
    let mut request = start_request(&Config::default(), &task());
    assert_eq!(
        claim_profile(&mut request, Duration::from_millis(50)).await,
        None
    );
    assert!(request.constraints.browser_profile.is_none());
}

/// A config whose persistent profile is a fresh folder named `name`, with
/// its workspace in `dir`.
fn persistent_config(dir: &tempfile::TempDir, name: &str) -> (Config, PathBuf) {
    let profile = dir.path().join(name);
    std::fs::create_dir_all(&profile).unwrap();
    let mut config = Config::default();
    config.workspace_dir = dir.path().join("workspace");
    config.browser.profile_mode = "persistent".into();
    config.browser.profile_path = Some(profile.to_string_lossy().into_owned());
    (config, profile)
}

#[tokio::test]
async fn of_two_tasks_starting_together_one_runs_in_a_fresh_profile() {
    // Neither one's Chrome has locked the folder when the other looks.
    let dir = tempfile::tempdir().unwrap();
    let (config, profile) = persistent_config(&dir, "together");
    let (mut first, mut second) = (
        start_request(&config, &task()),
        start_request(&config, &task()),
    );
    let (kept_first, kept_second) = tokio::join!(
        claim_profile(&mut first, Duration::from_millis(50)),
        claim_profile(&mut second, Duration::from_millis(50)),
    );
    let profiles = [
        first.constraints.browser_profile,
        second.constraints.browser_profile,
    ];
    assert_eq!(
        profiles.iter().filter(|kept| kept.is_some()).count(),
        1,
        "{profiles:?}"
    );
    assert_eq!(kept_first.or(kept_second), Some(profile.clone()));
    super::super::browser_profile::release(&profile);
}

#[tokio::test]
async fn a_task_the_module_did_not_start_gives_its_profile_back() {
    let dir = tempfile::tempdir().unwrap();
    let (config, profile) = persistent_config(&dir, "refused");
    let error = begin(&config, &task(), |_| {
        Box::pin(async { Err::<TaskView, _>("browser task StartTask failed: bus closed".into()) })
    })
    .await
    .unwrap_err();
    assert!(error.contains("bus closed"));
    assert!(
        super::super::browser_profile::claim_within(&profile, Duration::ZERO).await,
        "free for the next task at once"
    );
    super::super::browser_profile::release(&profile);

    // A task that started keeps its claim until its Chrome holds the folder.
    begin(&config, &task(), |request| {
        assert!(request.constraints.browser_profile.is_some());
        Box::pin(async {
            Ok(serde_json::from_value::<TaskView>(json!({
                "id": "t-claimed", "status": {"state": "running"},
                "summary": "", "progress": 0.0, "next": []
            }))
            .unwrap())
        })
    })
    .await
    .unwrap();
    assert!(!super::super::browser_profile::claim_within(&profile, Duration::ZERO).await);
    super::super::browser_profile::release(&profile);
}

#[test]
fn a_saved_flow_is_run_instead_of_planned() {
    let flow: tinycomputer_bus::Flow = serde_json::from_value(serde_json::json!({
        "app": "browser",
        "steps": [{"browse": "https://www.example.com/"}, {"stop_before": "paying"}]
    }))
    .unwrap();
    let mut saved = task();
    saved.flow = Some(flow.clone());
    let request = start_request(&Config::default(), &saved);
    assert_eq!(request.flow, Some(flow));
    assert_eq!(request.task.as_deref(), Some("Find the opening hours"));
}

#[tokio::test]
async fn a_task_that_already_stopped_settles_without_calling_the_module() {
    // Not running, so no `AwaitTask`; finished and untraced, so no report is
    // fetched either.
    let mut config = Config::default();
    config.computer.trace = false;
    let view: TaskView = serde_json::from_value(json!({
        "id": "t-settled",
        "status": {"state": "done", "answer": "found", "records": {}},
        "summary": "found it",
        "progress": 1.0,
        "next": []
    }))
    .unwrap();
    let settled = settle(&config, view.clone()).await.unwrap();
    assert_eq!(settled, view);
}
