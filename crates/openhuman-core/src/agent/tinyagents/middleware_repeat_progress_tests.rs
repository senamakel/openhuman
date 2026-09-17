//! Repeat-progress guard tests for repeats that are not back to back (#6275).

use super::*;

// ── #6275: repeats that are not back to back ────────────────────────────

/// One A, B round of two different successful calls, each returning the given
/// output. The adjacent-batch streak restarts on every step of such a cycle.
async fn run_alternating_round(mw: &RepeatProgressMiddleware, a_output: &str, b_output: &str) {
    run_successful_repeat_cycle(mw, "use_skill", json!({"skill": "skills"}), a_output, None).await;
    run_successful_repeat_cycle(
        mw,
        "use_skill",
        json!({"skill": "skills", "tool": "skill_search"}),
        b_output,
        None,
    )
    .await;
}

#[tokio::test]
async fn alternating_identical_successful_calls_halt_on_recurrence() {
    let handle = SteeringHandle::allow_all();
    let summary = std::sync::Arc::new(std::sync::Mutex::new(None));
    let mw = RepeatProgressMiddleware::new(handle.clone(), summary.clone());

    for _ in 0..DEFAULT_REPEAT_CALL_THRESHOLD - 1 {
        run_alternating_round(&mw, "doc", "hits").await;
    }
    assert_eq!(drain_pause_count(&handle), 0);

    run_successful_repeat_cycle(&mw, "use_skill", json!({"skill": "skills"}), "doc", None).await;
    assert_eq!(
        drain_pause_count(&handle),
        1,
        "an alternating call returning the identical result for the third time must halt the run"
    );
    assert!(
        summary
            .lock()
            .unwrap()
            .as_deref()
            .is_some_and(|text| text.contains("identical result")),
        "the recurrence halt summary should reach the host turn result"
    );
}

#[tokio::test]
async fn alternating_calls_whose_output_changed_do_not_halt() {
    let handle = SteeringHandle::allow_all();
    let mw = RepeatProgressMiddleware::new(
        handle.clone(),
        std::sync::Arc::new(std::sync::Mutex::new(None)),
    );
    for i in 0..DEFAULT_REPEAT_CALL_THRESHOLD * 3 {
        run_alternating_round(&mw, &format!("doc-{i}"), &format!("hits-{i}")).await;
    }
    assert_eq!(
        drain_pause_count(&handle),
        0,
        "a re-read with identical arguments whose output changed is progress, not a repeat"
    );
}

#[tokio::test]
async fn alternating_exempt_polling_calls_do_not_halt() {
    let handle = SteeringHandle::allow_all();
    let mw = RepeatProgressMiddleware::new(
        handle.clone(),
        std::sync::Arc::new(std::sync::Mutex::new(None)),
    );
    for i in 0..DEFAULT_REPEAT_CALL_THRESHOLD * 3 {
        run_successful_repeat_cycle(
            &mw,
            "wait_subagent",
            json!({"task_id": "t"}),
            "still running",
            None,
        )
        .await;
        let output = format!("status-{i}");
        run_successful_repeat_cycle(&mw, "lookup", json!({"id": 1}), &output, None).await;
    }
    assert_eq!(
        drain_pause_count(&handle),
        0,
        "identical polling results must not feed the recurrence ledger"
    );
}

#[tokio::test]
async fn evicting_a_recorded_result_resets_the_recurrence_ledger() {
    // What the last `before_model` sees for the recorded result `repeat-1`:
    // untouched, blanked by microcompact, or dropped by compression / trim.
    let cases: [(Vec<TaMessage>, usize, &str); 3] = [
        (
            vec![TaMessage::tool("repeat-1", "doc")],
            1,
            "a result still in context keeps counting",
        ),
        (
            vec![TaMessage::tool("repeat-1", CLEARED_PLACEHOLDER)],
            0,
            "a result blanked by compaction must not count toward a repeat",
        ),
        (
            vec![],
            0,
            "a result dropped by compaction must not count toward a repeat",
        ),
    ];
    for (compacted, expected_pauses, why) in cases {
        let handle = SteeringHandle::allow_all();
        let mw = RepeatProgressMiddleware::new(
            handle.clone(),
            std::sync::Arc::new(std::sync::Mutex::new(None)),
        );
        let observer = mw.eviction_observer();
        for _ in 0..DEFAULT_REPEAT_CALL_THRESHOLD - 1 {
            run_alternating_round(&mw, "doc", "hits").await;
        }

        // The next model call: the guard sees the request before the reduction
        // steps run, the observer after them.
        let mut request = ModelRequest::new(vec![TaMessage::tool("repeat-1", "doc")]);
        mw.before_model(&mut ctx(), &(), &mut request)
            .await
            .unwrap();
        let mut request = ModelRequest::new(compacted);
        observer
            .before_model(&mut ctx(), &(), &mut request)
            .await
            .unwrap();

        run_successful_repeat_cycle(&mw, "use_skill", json!({"skill": "skills"}), "doc", None)
            .await;
        assert_eq!(drain_pause_count(&handle), expected_pauses, "{why}");
    }
}
