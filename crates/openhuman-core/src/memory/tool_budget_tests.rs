use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[tokio::test(start_paused = true)]
async fn a_slow_memory_call_returns_and_disables_memory_only_for_its_turn() {
    let budget = ToolBudget::default();
    let result = budget
        .run(Some("turn-one".into()), async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            ToolResult::success("late")
        })
        .await;
    assert!(result.is_error);
    assert!(result.text().contains("Continue answering"));
    let calls = AtomicUsize::new(0);
    let result = budget
        .run(Some("turn-one".into()), async {
            calls.fetch_add(1, Ordering::SeqCst);
            ToolResult::success("should not run")
        })
        .await;
    assert!(result.is_error);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let next = budget
        .run(Some("turn-two".into()), async {
            ToolResult::success("healthy")
        })
        .await;
    assert!(!next.is_error);
}

#[tokio::test]
async fn successful_memory_churn_is_bounded_without_blocking_the_next_turn() {
    let budget = ToolBudget::default();
    let calls = AtomicUsize::new(0);
    for _ in 0..20 {
        budget
            .run(Some("turn-one".into()), async {
                calls.fetch_add(1, Ordering::SeqCst);
                ToolResult::success("saved with a different id")
            })
            .await;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 8);
    let next = budget
        .run(Some("turn-two".into()), async {
            ToolResult::success("healthy")
        })
        .await;
    assert!(!next.is_error);
}

#[tokio::test(start_paused = true)]
async fn successful_memory_calls_share_one_latency_budget() {
    let budget = ToolBudget::default();
    let started = tokio::time::Instant::now();
    for i in 0..3 {
        let result = budget
            .run(Some("turn".into()), async {
                tokio::time::sleep(Duration::from_secs(12)).await;
                ToolResult::success("saved")
            })
            .await;
        assert_eq!(result.is_error, i == 2);
    }
    assert_eq!(started.elapsed(), RUN_TIME_BUDGET);
}

#[tokio::test(start_paused = true)]
async fn an_ordinary_tool_error_refunds_unused_time_without_opening_the_circuit() {
    let budget = ToolBudget::default();
    let error = budget
        .run(Some("turn".into()), async {
            tokio::time::sleep(Duration::from_secs(3)).await;
            ToolResult::error("invalid memory arguments")
        })
        .await;
    assert!(error.is_error);
    let next = budget
        .run(Some("turn".into()), async {
            ToolResult::success("healthy")
        })
        .await;
    assert!(!next.is_error);
    let runs = budget.runs.lock().unwrap();
    assert!(!runs.states["turn"].disabled);
    assert_eq!(runs.states["turn"].remaining, Duration::from_secs(27));
}

#[tokio::test(start_paused = true)]
async fn callers_without_a_run_id_still_have_a_deadline() {
    let budget = ToolBudget::default();
    let result = budget
        .run(None, async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            ToolResult::success("late")
        })
        .await;
    assert!(result.is_error);
}

#[tokio::test(start_paused = true)]
async fn parallel_memory_reads_reserve_from_the_same_time_budget() {
    let budget = ToolBudget::default();
    let calls = AtomicUsize::new(0);
    let read = || async {
        calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_secs(12)).await;
        ToolResult::success("read")
    };
    let (first, second, third) = tokio::join!(
        budget.run(Some("turn".into()), read()),
        budget.run(Some("turn".into()), read()),
        budget.run(Some("turn".into()), read()),
    );
    assert!(!first.is_error);
    assert!(!second.is_error);
    assert!(third.is_error);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        budget.runs.lock().unwrap().states["turn"].remaining,
        Duration::from_secs(6)
    );
}

#[tokio::test]
async fn completed_run_tracking_is_bounded() {
    let budget = ToolBudget::default();
    for i in 0..200 {
        budget
            .run(Some(format!("turn-{i}")), async {
                ToolResult::success("saved")
            })
            .await;
    }
    let runs = budget.runs.lock().unwrap();
    assert_eq!(runs.states.len(), MAX_TRACKED_RUNS);
    assert_eq!(runs.order.len(), MAX_TRACKED_RUNS);
}

#[tokio::test(start_paused = true)]
async fn an_outstanding_memory_call_survives_other_runs_and_keeps_its_timeout_circuit() {
    let budget = ToolBudget::default();
    let call = budget.run(Some("active".into()), std::future::pending());
    tokio::pin!(call);
    assert!(futures::poll!(call.as_mut()).is_pending());
    for i in 0..MAX_TRACKED_RUNS {
        budget
            .run(Some(format!("other-{i}")), async {
                ToolResult::success("saved")
            })
            .await;
    }
    assert!(budget.runs.lock().unwrap().states.contains_key("active"));
    assert!(call.await.is_error);
    let calls = AtomicUsize::new(0);
    let result = budget
        .run(Some("active".into()), async {
            calls.fetch_add(1, Ordering::SeqCst);
            ToolResult::success("must not execute")
        })
        .await;
    assert!(result.is_error);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn full_tracking_refuses_new_work_until_an_active_reservation_is_released() {
    let budget = ToolBudget::default();
    let mut pending = Vec::new();
    for i in 0..MAX_TRACKED_RUNS {
        let mut call = Box::pin(budget.run(Some(format!("active-{i}")), std::future::pending()));
        assert!(futures::poll!(call.as_mut()).is_pending());
        pending.push(call);
    }
    let calls = AtomicUsize::new(0);
    let result = budget
        .run(Some("new".into()), async {
            calls.fetch_add(1, Ordering::SeqCst);
            ToolResult::success("must not execute")
        })
        .await;
    assert!(result.is_error);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    drop(pending);
    assert!(
        !budget
            .run(Some("new".into()), async { ToolResult::success("healthy") })
            .await
            .is_error
    );
}

#[tokio::test(start_paused = true)]
async fn cancelling_a_call_returns_its_unused_time_reservation() {
    let budget = ToolBudget::default();
    let mut call = Box::pin(budget.run(Some("cancelled".into()), std::future::pending()));
    assert!(futures::poll!(call.as_mut()).is_pending());
    drop(call);
    let runs = budget.runs.lock().unwrap();
    assert_eq!(runs.states["cancelled"].remaining, RUN_TIME_BUDGET);
}
