//! An embedder's close copy also words the instructions the harness adds to
//! the last two model calls of a turn that is running out of calls: the
//! penultimate call asks for the deliverable, the last asks for the answer.

#[path = "support/capped_turn.rs"]
mod capped_turn;
mod common;

use common::runtime;
use openhuman_embed::{CloseCopy, ENGLISH_CLOSE_COPY};

const RUNTIME_COPY: CloseCopy = CloseCopy {
    max_iteration_instruction: "RUNTIME CLOSE: answer in the user's language.",
    final_write_instruction: "RUNTIME WRITE: save what you found now.",
    ..ENGLISH_CLOSE_COPY
};

const TURN_COPY: CloseCopy = CloseCopy {
    max_iteration_instruction: "TURN CLOSE: answer in the user's language.",
    final_write_instruction: "TURN WRITE: save what you found now.",
    ..ENGLISH_CLOSE_COPY
};

const ENGLISH_CLOSE: &str = "You have reached the maximum number of tool calls";

#[test]
fn an_embedder_close_copy_words_the_wrap_up_calls() {
    runtime().block_on(async { tokio::spawn(scenario()).await.unwrap() });
}

async fn scenario() {
    let runtime = capped_turn::runtime_with(|builder| builder.close_copy(RUNTIME_COPY)).await;

    // The runtime's copy words both wrap-up calls.
    let (agent, provider) = capped_turn::wrapped_up_agent(&runtime, "runtime-copy").await;
    let turn = capped_turn::run(agent.turn("Gather what you can and report"), &provider).await;
    assert_eq!(turn.requests.len(), 4, "{:?}", turn.requests);
    assert!(turn.request_has(2, "RUNTIME WRITE: save what you found now."));
    assert!(turn.request_has(3, "RUNTIME CLOSE: answer in the user's language."));
    assert!(
        !turn.model_saw(ENGLISH_CLOSE),
        "the English close reached the model"
    );
    assert!(
        !turn.model_saw(ENGLISH_CLOSE_COPY.final_write_instruction),
        "the English write instruction reached the model"
    );
    assert_eq!(turn.reply, "wrapped up");

    // A copy set on one turn wins over the runtime's for that turn.
    let (agent, provider) = capped_turn::wrapped_up_agent(&runtime, "turn-copy").await;
    let turn = capped_turn::run(
        agent
            .turn("Gather what you can and report")
            .close_copy(TURN_COPY),
        &provider,
    )
    .await;
    assert!(turn.request_has(2, "TURN WRITE: save what you found now."));
    assert!(turn.request_has(3, "TURN CLOSE: answer in the user's language."));
    assert!(!turn.model_saw("RUNTIME"));
}
