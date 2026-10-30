//! An embedder sets the wording a capped turn is closed with, once on its
//! runtime and, when one turn needs other wording, on that turn.

#[path = "support/capped_turn.rs"]
mod capped_turn;
mod common;

use common::runtime;
use openhuman_embed::{CloseCopy, ENGLISH_CLOSE_COPY};

const RUNTIME_COPY: CloseCopy = CloseCopy {
    max_iteration_instruction: "RUNTIME CLOSE: answer the user in their own language.",
    checkpoint_template: "RUNTIME CHECKPOINT after {steps} calls:{results}",
    ..ENGLISH_CLOSE_COPY
};

const TURN_COPY: CloseCopy = CloseCopy {
    max_iteration_instruction: "TURN CLOSE: answer the user in their own language.",
    checkpoint_template: "TURN CHECKPOINT after {steps} calls:{results}",
    ..ENGLISH_CLOSE_COPY
};

#[test]
fn an_embedder_close_copy_reaches_the_capped_turn() {
    runtime().block_on(async { tokio::spawn(scenario()).await.unwrap() });
}

async fn scenario() {
    let runtime = capped_turn::runtime_with(|builder| builder.close_copy(RUNTIME_COPY)).await;

    // The runtime's copy is what an agent's capped turn is closed with.
    let (agent, provider) = capped_turn::capped_agent(&runtime, "runtime-copy").await;
    let turn = capped_turn::run(agent.turn("go"), &provider).await;
    assert!(turn.model_saw("RUNTIME CLOSE: answer the user in their own language."));
    assert!(
        !turn.model_saw("maximum number of tool calls"),
        "the English default instruction still reached the model"
    );
    assert!(
        turn.reply.starts_with("RUNTIME CHECKPOINT after 1 calls:"),
        "{}",
        turn.reply
    );

    // A copy set on one turn wins over the runtime's for that turn.
    let (agent, provider) = capped_turn::capped_agent(&runtime, "turn-copy").await;
    let turn = capped_turn::run(agent.turn("go").close_copy(TURN_COPY), &provider).await;
    assert!(turn.model_saw("TURN CLOSE: answer the user in their own language."));
    assert!(!turn.model_saw("RUNTIME CLOSE"));
    assert!(
        turn.reply.starts_with("TURN CHECKPOINT after 1 calls:"),
        "{}",
        turn.reply
    );
}
