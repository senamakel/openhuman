//! Host capability: turns the crate's coarse [`ProgressEvent`] stream into
//! OpenHuman's richer [`AgentProgress`] events.
//!
//! Adapts `crate::openhuman::agent::progress` (the `AgentProgress` UI contract)
//! for `tinyagents::harness::host::ProgressSink`. This is Phase 4 of
//! `docs/specs/plan-agents.md`.
//!
//! # Why this file is the boundary
//!
//! `AgentProgress` is the **host's** UI contract — the chat processing
//! timeline, the subagent drawer, the cost footer and the trace exporter all
//! read it. The crate deliberately keeps `ProgressEvent` at five variants and
//! says so in its own module docs: a presentation-shaped field added there
//! becomes a compatibility surface for every consumer of a redistributed
//! crate. So the projection runs here, host-side, and `AgentProgress` never
//! crosses into `vendor/tinyagents`.
//!
//! # Why it forwards to an mpsc channel and not to `publish_web_channel_event`
//!
//! The obvious-looking shortcut — build a `WebChannelEvent` and publish it
//! straight onto the web-channel bus — is wrong here. Everything that makes a
//! web-channel progress event correct is owned by
//! [`crate::openhuman::web_chat::progress_bridge::spawn_progress_bridge`]: the
//! per-request monotonic `seq` stamp the frontend dedups on, the
//! `TurnStateMirror` snapshot, the run-ledger upserts, the tracing span
//! collector, and the client/thread/request routing ids that a `ProgressEvent`
//! does not carry at all. A second independent producer of "what is happening
//! now" would drift from the bridge's ordering and from the persisted turn
//! state, and the drift would only show up as a mis-rendered timeline.
//!
//! So this sink writes `AgentProgress` into the same
//! `mpsc::Sender<AgentProgress>` the existing agent turn loop uses
//! (`Agent::set_on_progress`), and the established bridge does the publishing.
//! `publish_web_channel_event` is still the terminal step — one hop further
//! down, where it already lives.
//!
//! # Contract mismatches, and how they are resolved
//!
//! * **`emit` cannot fail, but not every event is equally droppable.** Token
//!   deltas use `try_send` and are *dropped* when the bridge is behind, per the
//!   crate's "dropping progress events is always preferable to slowing the
//!   turn" rule. Lifecycle events (`TurnStarted`, `ToolCallStarted`,
//!   `TurnCompleted`) are not interchangeable with them: the bridge is a state
//!   machine, so a lost one leaves a tool row stuck in `running` forever rather
//!   than costing a UI tick. They therefore wait up to
//!   [`LIFECYCLE_SEND_GRACE`] for room — bounded, never indefinite, because an
//!   unbounded await on this shared channel is the documented subagent-stall
//!   flake (`tool_progress.rs::emit`). Nothing here blocks a turn or panics.
//! * **The coarse stream has no iteration boundary.** `AgentProgress` carries a
//!   1-based `iteration` on almost every variant; `ProgressEvent` has no
//!   equivalent. The sink keeps a local round counter that advances on each
//!   `ToolCall` — a *lower bound* on the real iteration count, since a turn
//!   with no tool calls is one round. It is used for `iteration` fields and for
//!   `TurnCompleted { iterations }`. See the `TODO(phase4)` below.
//! * **`ProgressEvent::Error` has no `AgentProgress` counterpart.** The host
//!   enum models a turn's failure through the turn's own `Err` return (which
//!   `web_chat::ops` renders as `chat_error`), not through a progress event.
//!   Synthesising a `TurnCompleted` here would report a failed turn as a
//!   successful one to the ledger and the timeline, so the sink logs the
//!   failure and forwards nothing.
//! * **`ProgressEvent::Finished { usage }` is not a cost event.**
//!   `AgentProgress::TurnCostUpdated` requires a model id and a USD total that
//!   the crate event does not carry, and OpenHuman's authoritative cost figures
//!   come from the inference layer's charged amounts. Fabricating a
//!   `model: ""` / `total_usd: 0.0` update would silently under-report in the
//!   chat cost footer, so the usage is logged and only `TurnCompleted` is
//!   forwarded.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use async_trait::async_trait;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::Sender;

use tinyagents::harness::host::{ProgressEvent, ProgressSink};

use crate::openhuman::agent::progress::AgentProgress;

/// How long a **lifecycle** event may wait for room on a full channel.
///
/// Sized to ride out the transient window a burst of token deltas opens, while
/// staying far below anything a user or a parent turn would perceive as a
/// stall. Deltas never wait at all.
const LIFECYCLE_SEND_GRACE: std::time::Duration = std::time::Duration::from_millis(50);

/// Forwards crate progress into an OpenHuman [`AgentProgress`] channel.
///
/// Construct one per turn with the same sender that would otherwise be handed
/// to `Agent::set_on_progress`, so the existing
/// `web_chat::progress_bridge` consumer sees an identical event stream
/// regardless of which runtime produced it.
///
/// Cheap to clone-by-`Arc`: the crate's blanket
/// `impl ProgressSink for Arc<T>` makes `Arc<OpenHumanProgressSink>` usable
/// wherever a sink is wanted, which is how a sink shared by concurrent
/// sub-runs is passed around.
pub struct OpenHumanProgressSink {
    /// The per-request progress channel the turn loop's consumer owns.
    ///
    /// Bounded by whoever created it. Backpressure is handled by dropping, not
    /// by awaiting — see the module docs.
    tx: Sender<AgentProgress>,

    /// 0-based count of tool calls observed, used to derive the 1-based
    /// `iteration` the host events require.
    ///
    /// A lower bound rather than a guess dressed up as a fact: the coarse
    /// stream genuinely does not report iteration boundaries.
    rounds: AtomicU32,

    /// How many events were dropped because the channel was full or closed.
    ///
    /// Exposed via [`Self::dropped`] so a diagnostic can distinguish "the UI
    /// showed nothing because nothing happened" from "the UI showed nothing
    /// because the bridge fell behind". Never surfaced to the turn.
    dropped: AtomicU64,
}

impl OpenHumanProgressSink {
    /// Wraps a per-request `AgentProgress` sender.
    pub fn new(tx: Sender<AgentProgress>) -> Self {
        Self {
            tx,
            rounds: AtomicU32::new(0),
            dropped: AtomicU64::new(0),
        }
    }

    /// Number of events dropped so far (full or closed channel).
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// The current 1-based iteration index implied by the events seen so far.
    ///
    /// Starts at `1` — a turn is in its first round before it has called any
    /// tool — and advances once per observed `ToolCall`.
    fn iteration(&self) -> u32 {
        self.rounds.load(Ordering::Relaxed).saturating_add(1)
    }

    /// Hands one **lifecycle** event to the channel, waiting briefly if the
    /// channel is momentarily full.
    ///
    /// Lifecycle events are not interchangeable with token deltas: the web
    /// progress bridge is a state machine, so a lost `ToolCallStarted` or
    /// `TurnCompleted` leaves a tool row stuck in `running` forever, whereas a
    /// lost `TextDelta` is one missed UI tick. `turn/tools.rs::emit_progress`
    /// awaits its lifecycle sends for exactly that reason.
    ///
    /// It waits with a **bound** rather than awaiting outright, because the
    /// opposite failure is also real and also documented: the sink is a bounded
    /// channel shared by the orchestrator, every inline sub-agent, and their
    /// delta forwarders, and an unbounded `send().await` here can park a
    /// sub-agent's loop and hang the parent turn that is awaiting it — the
    /// subagent-stall flake `tool_progress.rs::emit` was written to avoid.
    /// [`LIFECYCLE_SEND_GRACE`] is the compromise: long enough to ride out the
    /// transient full-channel window a burst of deltas creates, far too short
    /// to stall a turn.
    async fn forward_lifecycle(&self, event: AgentProgress) {
        match self.tx.try_send(event) {
            Ok(()) => return,
            Err(TrySendError::Closed(dropped)) => {
                // No listener; waiting cannot help.
                let total = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                log::trace!(
                    "[tinyagents][progress] dropped lifecycle event on closed channel kind={:?} dropped_total={}",
                    std::mem::discriminant(&dropped),
                    total,
                );
                return;
            }
            Err(TrySendError::Full(event)) => {
                if self
                    .tx
                    .send_timeout(event, LIFECYCLE_SEND_GRACE)
                    .await
                    .is_ok()
                {
                    return;
                }
            }
        }

        let total = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
        log::warn!(
            "[tinyagents][progress] dropped a lifecycle event after waiting {:?} — the progress \
             bridge may now be out of sync (dropped_total={})",
            LIFECYCLE_SEND_GRACE,
            total,
        );
    }

    /// Hands one high-frequency event to the channel, dropping it if the
    /// channel cannot take it right now.
    ///
    /// This is the whole of the sink's failure handling for token deltas, and it
    /// deliberately has no error path out: `ProgressSink::emit` returns unit
    /// precisely so a dead UI socket can never fail a turn.
    fn forward(&self, event: AgentProgress) {
        match self.tx.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(dropped)) => {
                let total = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                log::debug!(
                    "[tinyagents][progress] dropped event on full channel kind={:?} dropped_total={}",
                    std::mem::discriminant(&dropped),
                    total,
                );
            }
            Err(TrySendError::Closed(dropped)) => {
                let total = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                log::trace!(
                    "[tinyagents][progress] dropped event on closed channel kind={:?} dropped_total={}",
                    std::mem::discriminant(&dropped),
                    total,
                );
            }
        }
    }
}

#[async_trait]
impl ProgressSink for OpenHumanProgressSink {
    /// Projects one [`ProgressEvent`] onto zero or one [`AgentProgress`]
    /// events and forwards it.
    ///
    /// Zero, for the two variants OpenHuman models elsewhere — see the module
    /// docs for `Error` and for the cost half of `Finished`. Never awaits
    /// anything that can block; the body is a counter bump and a `try_send`.
    async fn emit(&self, ev: ProgressEvent) {
        match ev {
            ProgressEvent::Started { run, thread, agent } => {
                // `TurnStarted` is a unit variant: the consumer already knows
                // its own client/thread/request ids (that is exactly why
                // `AgentProgress` carries no routing info), so run/thread/agent
                // are logged for correlation rather than forwarded.
                log::debug!(
                    "[tinyagents][progress] turn started run={} thread={:?} agent={}",
                    run,
                    thread.as_ref().map(|t| t.as_str()),
                    agent,
                );
                self.rounds.store(0, Ordering::Relaxed);
                self.forward_lifecycle(AgentProgress::TurnStarted).await;
            }

            ProgressEvent::ToolCall { run, call, tool } => {
                // A tool call closes the current round, so the counter advances
                // first and the event is attributed to the round it belongs to.
                let iteration = self.rounds.fetch_add(1, Ordering::Relaxed) + 1;
                log::debug!(
                    "[tinyagents][progress] tool_call run={} call={} tool={} iteration={}",
                    run,
                    call,
                    tool,
                    iteration,
                );
                self.forward_lifecycle(AgentProgress::ToolCallStarted {
                    call_id: call.as_str().to_string(),
                    tool_name: tool,
                    // The crate deliberately omits tool arguments from the
                    // progress side channel (they can be large and can carry
                    // untrusted or sensitive text). `Null` is already the
                    // documented shape on the tinyagents path — see
                    // `AgentProgress::ToolCallCompleted::arguments`, which
                    // backfills the span input for exactly this reason.
                    arguments: serde_json::Value::Null,
                    iteration,
                    // Server-computed timeline copy comes from
                    // `Tool::display_label` / `display_detail` at the tool
                    // registry, which this sink has no handle to; `None` tells
                    // the client to use its own formatter.
                    // TODO(phase4): resolve labels from the tool registry
                    // (`crate::openhuman::tools::traits::Tool::display_label`)
                    // once the sink is constructed with a registry handle.
                    display_label: None,
                    display_detail: None,
                })
                .await;
            }

            ProgressEvent::Token { run, text } => {
                let iteration = self.iteration();
                log::trace!(
                    "[tinyagents][progress] token run={} chars={} iteration={}",
                    run,
                    text.len(),
                    iteration,
                );
                self.forward(AgentProgress::TextDelta {
                    delta: text,
                    iteration,
                });
            }

            ProgressEvent::Finished { run, usage } => {
                let iterations = self.iteration();
                log::debug!(
                    "[tinyagents][progress] turn finished run={} iterations={} usage={:?}",
                    run,
                    iterations,
                    usage,
                );
                // TODO(phase4): the usage block is dropped rather than mapped
                // to `AgentProgress::TurnCostUpdated`, which needs a model id
                // and a USD total the crate event does not carry. The
                // authoritative cost path is
                // `crate::openhuman::agent::cost::TurnCost`, fed from the
                // inference layer's charged amounts; wiring usage through would
                // mean threading the resolved model + a cost estimate into this
                // sink rather than inventing `model: ""` / `total_usd: 0.0`.
                self.forward_lifecycle(AgentProgress::TurnCompleted { iterations })
                    .await;
            }

            ProgressEvent::Error { run, message } => {
                // Not forwarded on purpose. `AgentProgress` has no turn-level
                // failure variant — a failed turn surfaces through the turn's
                // own `Err`, which `web_chat::ops` renders as `chat_error` —
                // and emitting `TurnCompleted` here would record a failure as a
                // success in both the run ledger and the chat timeline.
                log::warn!("[tinyagents][progress] turn failed run={run} err={message}");
                // TODO(phase4): if the runtime ever becomes the only producer
                // of turn outcomes, this needs a real host-side failure event
                // (a new `AgentProgress` variant plus a `chat_error` mapping in
                // `web_chat::progress_bridge`), not a repurposed one.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tinyagents::harness::ids::{CallId, RunId, ThreadId};
    use tinyagents::harness::usage::Usage;
    use tokio::sync::mpsc;

    fn run() -> RunId {
        RunId::new("run-1")
    }

    fn started() -> ProgressEvent {
        ProgressEvent::Started {
            run: run(),
            thread: Some(ThreadId::new("thread-1")),
            agent: "orchestrator".to_string(),
        }
    }

    fn tool_call(name: &str) -> ProgressEvent {
        ProgressEvent::ToolCall {
            run: run(),
            call: CallId::new("call-1"),
            tool: name.to_string(),
        }
    }

    fn sink(capacity: usize) -> (OpenHumanProgressSink, mpsc::Receiver<AgentProgress>) {
        let (tx, rx) = mpsc::channel(capacity);
        (OpenHumanProgressSink::new(tx), rx)
    }

    #[tokio::test]
    async fn started_maps_to_turn_started() {
        let (sink, mut rx) = sink(8);
        sink.emit(started()).await;
        assert!(matches!(
            rx.try_recv().expect("event forwarded"),
            AgentProgress::TurnStarted
        ));
    }

    #[tokio::test]
    async fn tool_call_maps_to_tool_call_started_with_null_arguments() {
        let (sink, mut rx) = sink(8);
        sink.emit(tool_call("search")).await;

        match rx.try_recv().expect("event forwarded") {
            AgentProgress::ToolCallStarted {
                call_id,
                tool_name,
                arguments,
                iteration,
                display_label,
                display_detail,
            } => {
                assert_eq!(call_id, "call-1");
                assert_eq!(tool_name, "search");
                // The crate keeps tool arguments off the progress side channel;
                // a non-null value here would mean the adapter invented one.
                assert!(arguments.is_null());
                assert_eq!(iteration, 1, "iterations are 1-based");
                assert!(display_label.is_none());
                assert!(display_detail.is_none());
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn token_maps_to_text_delta_on_the_current_round() {
        let (sink, mut rx) = sink(8);
        sink.emit(ProgressEvent::Token {
            run: run(),
            text: "hello".to_string(),
        })
        .await;

        match rx.try_recv().expect("event forwarded") {
            AgentProgress::TextDelta { delta, iteration } => {
                assert_eq!(delta, "hello");
                assert_eq!(iteration, 1);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn each_tool_call_advances_the_round_counter() {
        let (sink, mut rx) = sink(16);
        sink.emit(tool_call("a")).await;
        sink.emit(tool_call("b")).await;
        sink.emit(ProgressEvent::Token {
            run: run(),
            text: "x".to_string(),
        })
        .await;

        let iterations: Vec<u32> = std::iter::from_fn(|| rx.try_recv().ok())
            .map(|ev| match ev {
                AgentProgress::ToolCallStarted { iteration, .. } => iteration,
                AgentProgress::TextDelta { iteration, .. } => iteration,
                other => panic!("unexpected event: {other:?}"),
            })
            .collect();
        // Two tool calls close two rounds, so the trailing text belongs to the
        // third.
        assert_eq!(iterations, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn started_resets_the_round_counter_for_a_reused_sink() {
        let (sink, mut rx) = sink(16);
        sink.emit(tool_call("a")).await;
        sink.emit(started()).await;
        sink.emit(tool_call("b")).await;

        let mut iterations = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            if let AgentProgress::ToolCallStarted { iteration, .. } = ev {
                iterations.push(iteration);
            }
        }
        assert_eq!(iterations, vec![1, 1], "a new turn restarts at round 1");
    }

    #[tokio::test]
    async fn finished_maps_to_turn_completed_carrying_the_round_count() {
        let (sink, mut rx) = sink(16);
        sink.emit(tool_call("a")).await;
        let _ = rx.try_recv();

        sink.emit(ProgressEvent::Finished {
            run: run(),
            usage: Some(Usage {
                input_tokens: 12,
                output_tokens: 3,
                total_tokens: 15,
                ..Usage::default()
            }),
        })
        .await;

        match rx.try_recv().expect("event forwarded") {
            AgentProgress::TurnCompleted { iterations } => assert_eq!(iterations, 2),
            other => panic!("unexpected event: {other:?}"),
        }
        // Usage is deliberately NOT projected onto TurnCostUpdated — see the
        // module docs. A fabricated cost update would under-report in the UI.
        assert!(rx.try_recv().is_err(), "no cost event is synthesised");
    }

    #[tokio::test]
    async fn finished_without_usage_still_completes_the_turn() {
        let (sink, mut rx) = sink(8);
        sink.emit(ProgressEvent::Finished {
            run: run(),
            usage: None,
        })
        .await;
        assert!(matches!(
            rx.try_recv().expect("event forwarded"),
            AgentProgress::TurnCompleted { iterations: 1 }
        ));
    }

    #[tokio::test]
    async fn error_forwards_nothing() {
        let (sink, mut rx) = sink(8);
        sink.emit(ProgressEvent::Error {
            run: run(),
            message: "provider unavailable".to_string(),
        })
        .await;
        // Reporting a failure as a completion would corrupt both the run ledger
        // and the chat timeline; the turn's own Err is the authoritative path.
        assert!(rx.try_recv().is_err());
        assert_eq!(sink.dropped(), 0, "not forwarding is not dropping");
    }

    #[tokio::test]
    async fn a_permanently_full_channel_drops_instead_of_stalling_the_turn() {
        let (sink, mut rx) = sink(1);
        sink.emit(started()).await;
        // Nothing ever drains, so the grace window expires and the events are
        // dropped. The turn must still finish rather than park forever.
        sink.emit(tool_call("a")).await;
        sink.emit(tool_call("b")).await;

        assert!(matches!(
            rx.try_recv().expect("first event fits"),
            AgentProgress::TurnStarted
        ));
        assert_eq!(sink.dropped(), 2);
    }

    #[tokio::test]
    async fn a_lifecycle_event_survives_transient_backpressure_that_drops_a_delta() {
        // The regression: a burst of deltas fills the channel, and a
        // `ToolCallStarted` lost in that window leaves the tool row stuck in
        // `running` forever. A delta lost in the same window costs one UI tick.
        let (sink, mut rx) = sink(1);
        sink.emit(started()).await;

        // A delta finds the channel full and is dropped immediately.
        sink.emit(ProgressEvent::Token {
            run: run(),
            text: "hi".to_string(),
        })
        .await;
        assert_eq!(sink.dropped(), 1, "deltas never wait");

        // Drive the blocked send and the consumer concurrently on one task, so
        // the ordering comes from poll order rather than from wall-clock sleeps
        // racing the grace window — the send registers as a waiter, then the
        // first `recv` frees a slot and wakes it. No margin to lose under load.
        let ((), first, second) = tokio::join!(
            sink.emit(tool_call("a")),
            rx.recv(),
            async { rx2.recv().await }
        );
        assert!(matches!(first, Some(AgentProgress::TurnStarted)));
        assert!(
            matches!(second, Some(AgentProgress::ToolCallStarted { .. })),
            "the lifecycle event must ride out the transient window, got {second:?}"
        );
        assert_eq!(sink.dropped(), 1, "only the delta was dropped");
    }

    #[tokio::test]
    async fn a_closed_channel_is_survivable() {
        let (sink, rx) = sink(8);
        drop(rx);
        // A UI that hung up must not be able to fail the turn.
        sink.emit(started()).await;
        sink.emit(ProgressEvent::Finished {
            run: run(),
            usage: None,
        })
        .await;
        assert_eq!(sink.dropped(), 2);
    }

    #[tokio::test]
    async fn works_through_an_arc_trait_object() {
        let (tx, mut rx) = mpsc::channel(8);
        let dynamic: Arc<dyn ProgressSink> = Arc::new(OpenHumanProgressSink::new(tx));
        dynamic.emit(started()).await;
        assert!(matches!(
            rx.try_recv().expect("event forwarded"),
            AgentProgress::TurnStarted
        ));
    }
}
