use super::*;
use crate::agent::progress::AgentProgress;

fn user(text: &str) -> Message {
    Message::user(text)
}

fn assistant(text: &str) -> Message {
    Message::assistant(text)
}

/// A long conversation with one model call this turn reports one iteration,
/// not one per assistant row in the whole history.
#[test]
fn iterations_count_this_turn_not_the_conversation() {
    let mut history = Vec::new();
    for turn in 0..60 {
        history.push(user(&format!("question {turn}")));
        history.push(assistant(&format!("answer {turn}")));
    }
    assert_eq!(turn_iterations(1, &history), 1);
    assert_eq!(turn_iterations(3, &history), 3);
}

#[test]
fn iterations_fall_back_to_this_turns_exchange() {
    let history = vec![
        user("old"),
        assistant("old answer"),
        assistant("old follow-up"),
        user("new"),
        assistant("calling a tool"),
        assistant("final"),
    ];
    assert_eq!(turn_iterations(0, &history), 2);
    assert_eq!(turn_iterations(0, &[user("only")]), 1, "never zero");
}

/// The completion reaches the progress channel while a slow post-commit step
/// is still running. Before, it waited for the step to finish.
#[tokio::test]
async fn completion_is_published_before_slow_post_commit_work_finishes() {
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel::<AgentProgress>(8);
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let tail_done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let tail_done_in_task = tail_done.clone();

    let slow_tail = async move {
        // Stands in for slow goal accounting: blocks until released.
        let _ = release_rx.await;
        tail_done_in_task.store(true, std::sync::atomic::Ordering::SeqCst);
    };
    let publish = async {
        progress_tx
            .send(AgentProgress::TurnCompleted {
                iterations: 1,
                stop: None,
            })
            .await
            .is_ok()
    };

    let (delivered, pending) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        complete_then_defer(publish, slow_tail),
    )
    .await
    .expect("completion does not wait for the slow tail");

    assert!(delivered);
    assert!(matches!(
        progress_rx.try_recv(),
        Ok(AgentProgress::TurnCompleted {
            iterations: 1,
            stop: None
        })
    ));
    assert!(
        !tail_done.load(std::sync::atomic::Ordering::SeqCst),
        "the slow step is still running after completion was published"
    );

    release_tx.send(()).expect("tail still waiting");
    await_pending(Some(pending)).await;
    assert!(tail_done.load(std::sync::atomic::Ordering::SeqCst));
}

#[tokio::test]
async fn await_pending_is_a_no_op_without_deferred_work() {
    await_pending(None).await;
}
