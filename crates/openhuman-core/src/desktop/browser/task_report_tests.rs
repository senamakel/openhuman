//! A stopped task is summarised without page content, recorded once per
//! state (and again when its report could not be fetched), and its full
//! report is written inside the workspace, readable by its owner alone.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::*;
use serde_json::{json, Value};

/// Log lines written on this thread while the guard from [`capture_logs`]
/// lives.
#[derive(Clone, Default)]
struct Capture(Arc<std::sync::Mutex<Vec<u8>>>);

impl Capture {
    /// Everything logged so far.
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl std::io::Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Captures this thread's log lines, at every level, until the guard drops.
fn capture_logs() -> (Capture, tracing::subscriber::DefaultGuard) {
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(capture.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .finish();
    (capture, tracing::subscriber::set_default(subscriber))
}

/// A config whose workspace is inside `dir`.
fn workspace_config(dir: &tempfile::TempDir) -> Config {
    let mut config = Config::default();
    config.workspace_dir = dir.path().join("workspace");
    config
}

/// A `fetch` for [`record_with`] that counts its calls and answers `reply`.
fn fetch_counted<'a>(
    calls: &'a AtomicUsize,
    reply: Result<TaskReport, String>,
) -> impl FnOnce(TaskId, bool) -> ReportFetch<'a> {
    move |_, _| {
        calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { reply })
    }
}

/// A view of task `id` in `status`.
fn view(id: &str, status: Value, progress: f32) -> TaskView {
    serde_json::from_value(json!({
        "id": id,
        "status": status,
        "summary": "searching",
        "progress": progress,
        "next": []
    }))
    .unwrap()
}

/// A failed status at step 2, with the module's one-line reason.
fn failed() -> Value {
    json!({
        "state": "failed",
        "step": 2,
        "reason": "the last three actions changed nothing on screen",
        "hint": "",
        "recoverable": true
    })
}

/// A report of three steps, the last one failed, with one rescue.
fn report(view: &TaskView) -> TaskReport {
    serde_json::from_value(json!({
        "view": view,
        "steps": [
            {"path": "0", "kind": "browse", "text": "open the site", "outcome": "done",
             "turns": 0, "jev_calls": 0, "actions": [], "loops": [], "note": ""},
            {"path": "1", "kind": "enter", "text": "type Asha", "outcome": "done",
             "turns": 2, "jev_calls": 14, "actions": [{"action": "fill", "ok": true}],
             "loops": [], "note": ""},
            {"path": "2", "kind": "do", "text": "search for trains", "outcome": "failed",
             "turns": 3, "jev_calls": 21,
             "actions": [{"action": "click", "ok": true}, {"action": "click", "ok": false}],
             "loops": [], "note": "nothing changed"}
        ],
        "records": {},
        "artifacts": [],
        "learned": [],
        "trace": [],
        "rescues": [
            {"step": 2, "failure": "nothing changed", "reason": "timed out", "outcome": "gave_up"}
        ]
    }))
    .unwrap()
}

#[test]
fn a_task_that_stopped_short_is_summarised_with_counts_not_page_text() {
    let view = view("t-summary", failed(), 0.33);
    let summary = Summary::of(&view.status, &report(&view));
    assert_eq!(
        summary,
        Summary {
            steps: 3,
            jev_calls: 35,
            actions: 3,
            rescues: 1,
            recovered: 0,
            failed_step: Some(2),
            failed_kind: Some("do"),
        }
    );
}

#[test]
fn only_a_task_that_stopped_short_or_a_traced_settled_one_is_recorded() {
    let done = view(
        "t-done",
        json!({"state": "done", "answer": "found", "records": {}}),
        1.0,
    );
    let running = view("t-running", json!({"state": "running"}), 0.5);
    let human = view(
        "t-human",
        json!({"state": "needs_human", "reason": "log in"}),
        0.5,
    );
    let plan = view("t-plan", json!({"state": "needs_plan", "guide": "…"}), 0.0);
    let failed = view("t-failed", failed(), 0.3);

    assert!(worth_recording(&failed.status, false));
    assert!(worth_recording(&human.status, false));
    assert!(worth_recording(&plan.status, false));
    assert!(!worth_recording(&done.status, false));
    assert!(worth_recording(&done.status, true));
    assert!(!worth_recording(&running.status, true));
}

#[test]
fn the_same_stop_is_claimed_once_and_can_be_given_back() {
    let mut recorded = Recorded::new(8);
    let first = record_key(&view("t-dedupe", failed(), 0.33));
    assert!(recorded.claim(&first));
    assert!(
        !recorded.claim(&first),
        "followed again, it is not recorded twice"
    );
    let later = record_key(&view("t-dedupe", failed(), 0.66));
    assert!(
        recorded.claim(&later),
        "a later stop of the same task is new"
    );
    recorded.release(&first);
    assert!(
        recorded.claim(&first),
        "a released stop can be claimed again"
    );
}

#[test]
fn the_oldest_remembered_stop_is_forgotten_first() {
    let mut recorded = Recorded::new(2);
    assert!(recorded.claim("a"));
    assert!(recorded.claim("b"));
    assert!(recorded.claim("c"));
    assert!(recorded.claim("a"), "the oldest was forgotten to make room");
    assert!(!recorded.claim("c"));
}

#[test]
fn every_state_has_its_own_name() {
    let names = [
        (json!({"state": "running"}), "running"),
        (json!({"state": "needs_input", "fields": []}), "needs_input"),
        (
            json!({"state": "needs_approval", "action": "send it", "target": "Send"}),
            "needs_approval",
        ),
        (
            json!({"state": "checkpoint", "reason": "payment", "location": "pay page",
                   "summary": "", "continuable": false}),
            "checkpoint",
        ),
        (
            json!({"state": "needs_human", "reason": "log in"}),
            "needs_human",
        ),
        (json!({"state": "needs_plan", "guide": "…"}), "needs_plan"),
        (
            json!({"state": "done", "answer": "found", "records": {}}),
            "done",
        ),
        (failed(), "failed"),
        (json!({"state": "cancelled"}), "cancelled"),
    ];
    for (status, name) in names {
        let status: TaskStatus = serde_json::from_value(status).unwrap();
        assert_eq!(status_name(&status), name);
    }
}

#[test]
fn a_task_that_needs_a_person_names_no_failed_step() {
    let view = view(
        "t-human-summary",
        json!({"state": "needs_human", "reason": "log in"}),
        0.5,
    );
    let summary = Summary::of(&view.status, &report(&view));
    assert_eq!(summary.failed_step, None);
    assert_eq!(summary.failed_kind, Some("do"));
}

#[tokio::test]
async fn a_stopped_task_is_logged_once_with_counts_and_no_step_text() {
    let dir = tempfile::tempdir().unwrap();
    let config = workspace_config(&dir);
    let view = view("t-rec-once", failed(), 0.33);
    let (logs, _guard) = capture_logs();
    let calls = AtomicUsize::new(0);

    record_with(
        &config,
        &view,
        false,
        fetch_counted(&calls, Ok(report(&view))),
    )
    .await;
    record_with(
        &config,
        &view,
        false,
        fetch_counted(&calls, Ok(report(&view))),
    )
    .await;

    assert_eq!(calls.load(Ordering::SeqCst), 1, "fetched once per stop");
    let text = logs.text();
    assert_eq!(text.matches("task stopped short").count(), 1, "{text}");
    assert!(text.contains("jev_calls=35"), "{text}");
    assert!(text.contains("changed nothing on screen"), "{text}");
    for page_text in ["type Asha", "search for trains", "open the site"] {
        assert!(!text.contains(page_text), "step text leaked: {text}");
    }
    assert!(
        !super::super::computer_config::trace_dir(&config).exists(),
        "an untraced stop writes no report"
    );
}

#[tokio::test]
async fn a_report_that_could_not_be_fetched_is_fetched_on_the_next_settle() {
    let dir = tempfile::tempdir().unwrap();
    let config = workspace_config(&dir);
    let view = view("t-rec-retry", failed(), 0.33);
    let (logs, _guard) = capture_logs();
    let calls = AtomicUsize::new(0);

    record_with(
        &config,
        &view,
        false,
        fetch_counted(&calls, Err("module unavailable".to_owned())),
    )
    .await;
    assert!(
        logs.text().contains("report unavailable"),
        "{}",
        logs.text()
    );
    assert!(!logs.text().contains("task stopped short"));

    record_with(
        &config,
        &view,
        false,
        fetch_counted(&calls, Ok(report(&view))),
    )
    .await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "the failed fetch was retried"
    );
    assert!(
        logs.text().contains("task stopped short"),
        "{}",
        logs.text()
    );
}

#[tokio::test]
async fn a_traced_task_that_settled_writes_its_traced_report() {
    let dir = tempfile::tempdir().unwrap();
    let config = workspace_config(&dir);
    let view = view(
        "t-rec-traced",
        json!({"state": "done", "answer": "found", "records": {}}),
        1.0,
    );
    let asked_for_trace = Arc::new(std::sync::Mutex::new(None));
    let asked = Arc::clone(&asked_for_trace);
    let reply = report(&view);

    record_with(&config, &view, true, move |id, trace| {
        *asked.lock().unwrap() = Some((id, trace));
        Box::pin(async move { Ok(reply) })
    })
    .await;

    assert_eq!(
        *asked_for_trace.lock().unwrap(),
        Some((TaskId("t-rec-traced".to_owned()), true))
    );
    let tasks = super::super::computer_config::trace_dir(&config).join("tasks");
    assert_eq!(std::fs::read_dir(&tasks).unwrap().count(), 1);
}

#[tokio::test]
async fn an_untraced_task_that_finished_is_not_fetched() {
    let dir = tempfile::tempdir().unwrap();
    let config = workspace_config(&dir);
    let view = view(
        "t-rec-done",
        json!({"state": "done", "answer": "found", "records": {}}),
        1.0,
    );
    let calls = AtomicUsize::new(0);
    record_with(
        &config,
        &view,
        false,
        fetch_counted(&calls, Ok(report(&view))),
    )
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn the_logged_reason_is_the_modules_own_line_cut_short() {
    let long = "x".repeat(500);
    let status: TaskStatus = serde_json::from_value(json!({
        "state": "failed", "step": null, "reason": long, "hint": "", "recoverable": false
    }))
    .unwrap();
    assert_eq!(reason(&status).chars().count(), REASON_CHARS);
    let plan: TaskStatus =
        serde_json::from_value(json!({"state": "needs_plan", "guide": "write a flow"})).unwrap();
    assert_eq!(reason(&plan), "no planner is configured");
    assert_eq!(reason(&TaskStatus::Cancelled), "");
}

#[test]
fn a_step_kind_outside_the_flow_grammar_is_logged_as_other() {
    assert_eq!(kind_name("DO"), "do");
    assert_eq!(kind_name("stop_before"), "stop_before");
    assert_eq!(kind_name("<b>Pay</b>"), "other");
}

#[test]
fn a_report_path_stays_in_the_workspace_and_names_the_task_safely() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.workspace_dir = dir.path().join("workspace");
    let path = report_path(&config, "t-1/../etc", 42);
    assert_eq!(
        path,
        config
            .workspace_dir
            .join("state")
            .join("computer")
            .join("tasks")
            .join("42-t-1etc.json")
    );
}

#[tokio::test]
async fn a_traced_report_is_written_whole_into_the_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.workspace_dir = dir.path().join("workspace");
    let view = view("t-write", failed(), 0.33);
    let report = report(&view);
    write(&config, &report).await;
    let tasks = super::super::computer_config::trace_dir(&config).join("tasks");
    let written = std::fs::read_dir(&tasks)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(written.len(), 1, "{written:?}");
    let name = written[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(name.ends_with("-t-write.json"), "{name}");
    let back: TaskReport = serde_json::from_slice(&std::fs::read(&written[0]).unwrap()).unwrap();
    assert_eq!(back, report);
}

#[cfg(unix)]
#[tokio::test]
async fn a_report_and_its_directory_are_readable_by_their_owner_alone() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let config = workspace_config(&dir);
    let tasks = super::super::computer_config::trace_dir(&config).join("tasks");
    // A directory left open by an earlier run is narrowed too.
    std::fs::create_dir_all(&tasks).unwrap();
    std::fs::set_permissions(&tasks, std::fs::Permissions::from_mode(0o755)).unwrap();

    let view = view("t-private", failed(), 0.33);
    write(&config, &report(&view)).await;

    let mode =
        |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&tasks), 0o700);
    let file = std::fs::read_dir(&tasks)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(mode(&file), 0o600);

    // Overwriting a file that was readable by others narrows it as well.
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    write_private(&file, &report(&view)).await.unwrap();
    assert_eq!(mode(&file), 0o600);
}

#[tokio::test]
async fn a_report_that_cannot_be_written_is_only_logged() {
    let dir = tempfile::tempdir().unwrap();
    let config = workspace_config(&dir);
    // `state` is a file, so the report's directory cannot be made.
    std::fs::create_dir_all(&config.workspace_dir).unwrap();
    std::fs::write(config.workspace_dir.join("state"), b"not a directory").unwrap();
    let (logs, _guard) = capture_logs();

    let view = view("t-unwritable", failed(), 0.33);
    write(&config, &report(&view)).await;

    assert!(
        logs.text().contains("report not written"),
        "{}",
        logs.text()
    );
}
