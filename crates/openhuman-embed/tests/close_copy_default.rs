//! Without a close copy of its own, a capped turn is closed with the harness's
//! English wording, exactly as before the copy became settable.

#[path = "support/capped_turn.rs"]
mod capped_turn;
mod common;

use common::runtime;

#[test]
fn the_default_close_copy_is_unchanged() {
    runtime().block_on(async { tokio::spawn(scenario()).await.unwrap() });
}

async fn scenario() {
    let runtime = capped_turn::runtime_with(|builder| builder).await;
    let (agent, provider) = capped_turn::capped_agent(&runtime, "capped").await;
    let turn = capped_turn::run(
        agent.turn("Run the probe operation and tell me the result"),
        &provider,
    )
    .await;

    assert!(turn.model_saw(
        "You have reached the maximum number of tool calls allowed for this single turn"
    ));
    assert!(turn.model_saw("close with a brief **Still to do** line"));
    assert!(
        turn.reply
            .starts_with("I reached the tool-call limit for this turn (1 steps)"),
        "{}",
        turn.reply
    );
}
