//! What the host keeps from a browser task once it stops: a content-free
//! summary in the log when the task stopped short (failed, or needs a person
//! or a plan), and — with tracing on — the module's full report under
//! `<workspace>/state/computer/tasks/`.
//!
//! A report holds page text (step notes, records and, when traced, every Jev
//! exchange), so it is written only to the workspace, readable by its owner
//! alone, and never logged: the log gets counts, the failed step's position
//! and kind, and the module's own one-line reason.

use std::collections::VecDeque;
use std::future::Future;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use tinycomputer_bus::agent::{RescueOutcome, TaskId, TaskReport, TaskStatus, TaskView};
use tinycomputer_bus::flow::{StepOutcome, STEP_KINDS};
use tokio::io::AsyncWriteExt;

use crate::config::Config;

#[cfg(test)]
#[path = "task_report_tests.rs"]
mod tests;

/// How many stopped tasks are remembered, so a view followed again does not
/// record the same stop twice.
const REMEMBERED: usize = 64;
/// The most characters of a failure reason written to the log.
const REASON_CHARS: usize = 240;

/// A pending `TaskReport` call.
type ReportFetch<'a> = Pin<Box<dyn Future<Output = Result<TaskReport, String>> + Send + 'a>>;

/// Records `view` once per task and state: a log summary when it stopped
/// short, and the full report on disk when tracing is on and it settled.
/// Never fails the caller; a report that cannot be fetched is only logged,
/// and the next settle of the same state tries again.
pub(crate) async fn record(config: &Config, view: &TaskView) {
    let traced = crate::modules::computer_config::tracing_enabled(config);
    record_with(config, view, traced, |id, trace| {
        Box::pin(super::task::report_with(config, id, trace))
    })
    .await;
}

/// [`record`] with tracing on or off as `traced` says, and the module's
/// `TaskReport` call passed in as `fetch`.
async fn record_with<'a>(
    config: &'a Config,
    view: &TaskView,
    traced: bool,
    fetch: impl FnOnce(TaskId, bool) -> ReportFetch<'a>,
) {
    if !worth_recording(&view.status, traced) {
        return;
    }
    let key = record_key(view);
    let claimed = recorded().claim(&key);
    if !claimed {
        return;
    }
    let report = match fetch(view.id.clone(), traced).await {
        Ok(report) => report,
        Err(error) => {
            // Give the claim back, so the next settle of this state tries again.
            recorded().release(&key);
            tracing::warn!(task = %view.id, %error, "[browser-task] report unavailable");
            return;
        }
    };
    if stopped_short(&view.status) {
        let summary = Summary::of(&view.status, &report);
        tracing::warn!(
            task = %view.id,
            state = status_name(&view.status),
            step = ?summary.failed_step,
            kind = summary.failed_kind.unwrap_or("-"),
            steps = summary.steps,
            jev_calls = summary.jev_calls,
            actions = summary.actions,
            rescues = summary.rescues,
            recovered = summary.recovered,
            reason = %reason(&view.status),
            "[browser-task] task stopped short"
        );
    }
    if traced {
        write(config, &report).await;
    }
}

/// A task that ended without doing what it was asked.
fn stopped_short(status: &TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Failed { .. } | TaskStatus::NeedsHuman { .. } | TaskStatus::NeedsPlan { .. }
    )
}

/// Whether a stop in `status` is recorded: always when it stopped short, and
/// any settled stop when `traced`.
fn worth_recording(status: &TaskStatus, traced: bool) -> bool {
    stopped_short(status) || (traced && !matches!(status, TaskStatus::Running))
}

/// What a task's stop is remembered by: the task, its state and its progress.
fn record_key(view: &TaskView) -> String {
    format!(
        "{}|{}|{:.3}",
        view.id,
        status_name(&view.status),
        view.progress
    )
}

/// The stops this process has recorded, or is recording.
fn recorded() -> MutexGuard<'static, Recorded> {
    static RECORDED: OnceLock<Mutex<Recorded>> = OnceLock::new();
    RECORDED
        .get_or_init(|| Mutex::new(Recorded::new(REMEMBERED)))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Stops claimed for recording, oldest first, at most `cap` of them.
#[derive(Debug)]
struct Recorded {
    keys: VecDeque<String>,
    cap: usize,
}

impl Recorded {
    /// Remembers at most `cap` stops.
    fn new(cap: usize) -> Self {
        Self {
            keys: VecDeque::new(),
            cap,
        }
    }

    /// Claims `key`; false when it is already recorded or being recorded.
    fn claim(&mut self, key: &str) -> bool {
        if self.keys.iter().any(|seen| seen == key) {
            return false;
        }
        if self.keys.len() == self.cap {
            self.keys.pop_front();
        }
        self.keys.push_back(key.to_owned());
        true
    }

    /// Gives a claim back, so the stop can be recorded later.
    fn release(&mut self, key: &str) {
        self.keys.retain(|seen| seen != key);
    }
}

/// The state's wire name, for the log and the record key.
fn status_name(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Running => "running",
        TaskStatus::NeedsInput { .. } => "needs_input",
        TaskStatus::NeedsApproval { .. } => "needs_approval",
        TaskStatus::Checkpoint { .. } => "checkpoint",
        TaskStatus::NeedsHuman { .. } => "needs_human",
        TaskStatus::NeedsPlan { .. } => "needs_plan",
        TaskStatus::Done { .. } => "done",
        TaskStatus::Failed { .. } => "failed",
        TaskStatus::Cancelled => "cancelled",
    }
}

/// The module's own reason for stopping, cut to [`REASON_CHARS`].
fn reason(status: &TaskStatus) -> String {
    let reason = match status {
        TaskStatus::Failed { reason, .. } | TaskStatus::NeedsHuman { reason, .. } => {
            reason.as_str()
        }
        TaskStatus::NeedsPlan { .. } => "no planner is configured",
        _ => "",
    };
    reason.chars().take(REASON_CHARS).collect()
}

/// The counts a stopped task is logged with; nothing a page showed.
#[derive(Debug, PartialEq, Eq)]
struct Summary {
    steps: usize,
    jev_calls: u64,
    actions: usize,
    rescues: usize,
    recovered: usize,
    failed_step: Option<usize>,
    failed_kind: Option<&'static str>,
}

impl Summary {
    /// The counts in `report`, and the failed step when `status` names one.
    fn of(status: &TaskStatus, report: &TaskReport) -> Self {
        let failed_step = match status {
            TaskStatus::Failed { step, .. } => *step,
            _ => None,
        };
        Self {
            steps: report.steps.len(),
            jev_calls: report
                .steps
                .iter()
                .map(|step| u64::from(step.jev_calls))
                .sum(),
            actions: report.steps.iter().map(|step| step.actions.len()).sum(),
            rescues: report.rescues.len(),
            recovered: report
                .rescues
                .iter()
                .filter(|rescue| rescue.outcome == RescueOutcome::Recovered)
                .count(),
            failed_step,
            failed_kind: report
                .steps
                .iter()
                .rev()
                .find(|step| step.outcome == StepOutcome::Failed)
                .map(|step| kind_name(&step.kind)),
        }
    }
}

/// A step kind as the log may show it: one of the flow grammar's own words,
/// never free text.
fn kind_name(kind: &str) -> &'static str {
    STEP_KINDS
        .iter()
        .find(|known| kind.eq_ignore_ascii_case(known))
        .copied()
        .unwrap_or("other")
}

/// Where a report of task `id`, recorded at `millis`, is written.
fn report_path(config: &Config, id: &str, millis: u128) -> PathBuf {
    let id: String = id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .collect();
    crate::modules::computer_config::trace_dir(config)
        .join("tasks")
        .join(format!("{millis}-{id}.json"))
}

/// Writes `report` under the workspace's trace directory, and logs where;
/// a failure is logged, never raised.
async fn write(config: &Config, report: &TaskReport) {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let path = report_path(config, &report.view.id.0, millis);
    match write_private(&path, report).await {
        Ok(bytes) => tracing::info!(
            task = %report.view.id,
            path = %path.display(),
            bytes,
            exchanges = report.trace.len(),
            "[browser-task] report written"
        ),
        Err(error) => tracing::warn!(
            task = %report.view.id,
            %error,
            "[browser-task] report not written"
        ),
    }
}

/// Writes `report` to `path` as JSON, in a directory and a file only their
/// owner can read, also when either already existed: a report holds page
/// text. Returns how many bytes were written.
async fn write_private(path: &Path, report: &TaskReport) -> std::io::Result<usize> {
    let bytes = serde_json::to_vec_pretty(report).map_err(std::io::Error::other)?;
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
        #[cfg(unix)]
        tokio::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).await?;
    }
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path).await?;
    #[cfg(unix)]
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .await?;
    file.write_all(&bytes).await?;
    file.flush().await?;
    Ok(bytes.len())
}
