use super::super::context::{ChannelRuntimeContext, CHANNEL_MESSAGE_TIMEOUT_SECS};
use super::super::runtime::{run_message_dispatch_loop, RuntimeChannelMessage};
use super::super::Channel;
use super::common::{channel_envelope, RecordingChannel, SlowModel, TelegramRecordingChannel};
use crate::agent::bus::{mock_agent_run_turn, AgentTurnRequest, AgentTurnResponse};
use crate::config::InboundDebounceConfig;
use crate::core::bus_testing::MockBusGuard;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, Notify};
use tokio::task::JoinHandle;

#[derive(Default)]
struct Turns {
    histories: Mutex<Vec<Vec<(String, String)>>>,
    active: AtomicUsize,
    peak: AtomicUsize,
}

impl Turns {
    fn count(&self) -> usize {
        self.histories.lock().unwrap().len()
    }

    fn last_user(&self, turn: usize) -> String {
        let histories = self.histories.lock().unwrap();
        histories[turn]
            .iter()
            .rev()
            .find(|(role, _)| role == "user")
            .map(|(_, content)| content.clone())
            .unwrap_or_default()
    }

    fn history(&self, turn: usize) -> Vec<(String, String)> {
        self.histories.lock().unwrap()[turn].clone()
    }
}

async fn install_handler(turns: Arc<Turns>, gate: Option<Arc<Notify>>) -> MockBusGuard {
    mock_agent_run_turn(move |req: AgentTurnRequest| {
        let turns = Arc::clone(&turns);
        let gate = gate.clone();
        async move {
            let index = {
                let mut histories = turns.histories.lock().unwrap();
                histories.push(
                    req.history
                        .iter()
                        .map(|m| (m.role.clone(), m.content.clone()))
                        .collect(),
                );
                histories.len()
            };
            let active = turns.active.fetch_add(1, Ordering::SeqCst) + 1;
            turns.peak.fetch_max(active, Ordering::SeqCst);
            if index == 1 {
                if let Some(gate) = gate {
                    gate.notified().await;
                }
            }
            turns.active.fetch_sub(1, Ordering::SeqCst);
            Ok(AgentTurnResponse::new(format!("reply {index}")))
        }
    })
    .await
}

fn runtime() -> (Arc<RecordingChannel>, Arc<ChannelRuntimeContext>) {
    let channel_impl = Arc::new(RecordingChannel::default());
    let ctx = runtime_with(channel_impl.clone());
    (channel_impl, ctx)
}

fn runtime_with(channel: Arc<dyn Channel>) -> Arc<ChannelRuntimeContext> {
    let mut channels_by_name = HashMap::new();
    channels_by_name.insert(channel.name().to_string(), channel);
    Arc::new(ChannelRuntimeContext {
        relay_thread: None,
        channels_by_name: Arc::new(channels_by_name),
        turn_model_source: Some(crate::agent::tinyagents::TurnModelSource::from_model(
            Arc::new(SlowModel {
                delay: Duration::from_millis(1),
            }),
        )),
        default_provider: Arc::new("test-provider".to_string()),
        tools_registry: Arc::new(vec![]),
        system_prompt: crate::channels::ChannelSystemPrompt::fixed("test-system-prompt"),
        model: Arc::new("test-model".to_string()),
        temperature: 0.0,
        max_tool_iterations: 10,
        conversation_histories: Arc::new(Mutex::new(HashMap::new())),
        turn_model_source_cache: Arc::new(Mutex::new(HashMap::new())),
        route_overrides: Arc::new(Mutex::new(HashMap::new())),
        api_url: None,
        inference_url: None,
        reliability: Arc::new(crate::config::ReliabilityConfig::default()),
        provider_runtime_options: crate::tools::delegate::ProviderRuntimeOptions::default(),
        workspace_dir: Arc::new(std::env::temp_dir()),
        message_timeout_secs: CHANNEL_MESSAGE_TIMEOUT_SECS,
        multimodal: crate::config::MultimodalConfig::default(),
        multimodal_files: crate::config::MultimodalFileConfig::default(),
        config: None,
        core: None,
    })
}

struct Running {
    tx: mpsc::Sender<RuntimeChannelMessage>,
    task: JoinHandle<()>,
    next_ts: u64,
    channel: &'static str,
}

impl Running {
    fn start(ctx: Arc<ChannelRuntimeContext>, config: InboundDebounceConfig) -> Self {
        let (tx, rx) = mpsc::channel(32);
        let task = crate::core::runtime::DispatchCtx::ambient()
            .spawn(run_message_dispatch_loop(rx, ctx, 4, config));
        Self {
            tx,
            task,
            next_ts: 1,
            channel: "test-channel",
        }
    }

    fn message(&mut self, sender: &str, content: &str) -> tinychannels_bus::ChannelInboundEnvelope {
        let ts = self.next_ts;
        self.next_ts += 1;
        channel_envelope(
            self.channel,
            sender,
            sender,
            content,
            &format!("m{ts}"),
            ts,
            None,
            None,
        )
    }

    async fn send(&mut self, sender: &str, content: &str) {
        let message = self.prepared(sender, content);
        self.tx.send(message).await.unwrap();
    }

    fn prepared(&mut self, sender: &str, content: &str) -> RuntimeChannelMessage {
        let message = self.message(sender, content);
        let history_key = format!("{}_{sender}_{sender}", self.channel);
        let prepared = tinychannels_bus::module_inbound::PreparedChannelInbound {
            message: crate::channels::routes::route_compat_message(&message),
            envelope: message.clone(),
            history_key: history_key.clone(),
            history_key_ignores_thread: false,
            thread_id: history_key.clone(),
            client_id: self.channel.to_string(),
            session_key: history_key,
        };
        RuntimeChannelMessage {
            message,
            authenticated_relay: false,
            prepared_inbound: Some(prepared),
        }
    }

    async fn send_unprepared(&mut self, sender: &str, content: &str) {
        let message = self.message(sender, content);
        self.tx
            .send(RuntimeChannelMessage::with_inbound_envelope(message))
            .await
            .unwrap();
    }

    async fn finish(self) {
        drop(self.tx);
        tokio::time::timeout(Duration::from_secs(600), self.task)
            .await
            .expect("dispatch loop drains")
            .expect("dispatch loop does not panic");
    }
}

async fn sleep_ms(ms: u64) {
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

#[tokio::test(start_paused = true)]
async fn two_quick_messages_from_one_sender_become_one_turn() {
    let turns = Arc::new(Turns::default());
    let _guard = install_handler(Arc::clone(&turns), None).await;
    let (channel, ctx) = runtime();
    let mut run = Running::start(ctx, InboundDebounceConfig::default());

    run.send("alice", "first").await;
    sleep_ms(500).await;
    run.send("alice", "second").await;
    sleep_ms(1000).await;
    assert_eq!(turns.count(), 0, "still inside the quiet window");
    sleep_ms(2000).await;

    assert_eq!(turns.count(), 1);
    assert!(turns.last_user(0).ends_with("first\nsecond"));
    assert_eq!(channel.sent_messages.lock().await.len(), 1);
    assert!(channel.start_typing_calls.load(Ordering::SeqCst) >= 1);
    run.finish().await;
    assert_eq!(turns.count(), 1);
}

#[tokio::test(start_paused = true)]
async fn text_after_an_image_joins_it_through_the_media_window() {
    let turns = Arc::new(Turns::default());
    let _guard = install_handler(Arc::clone(&turns), None).await;
    let (_channel, ctx) = runtime();
    let mut run = Running::start(ctx, InboundDebounceConfig::default());

    run.send("alice", "[IMAGE:https://example.com/a.png]\nlook")
        .await;
    sleep_ms(3000).await;
    assert_eq!(turns.count(), 0);
    run.send("alice", "what is this?").await;
    sleep_ms(5000).await;

    assert_eq!(turns.count(), 1);
    let user = turns.last_user(0);
    assert!(user.contains("look\nwhat is this?"), "{user}");
    run.finish().await;
}

#[tokio::test(start_paused = true)]
async fn messages_during_a_running_turn_become_one_follow_up_that_sees_its_history() {
    let turns = Arc::new(Turns::default());
    let gate = Arc::new(Notify::new());
    let _guard = install_handler(Arc::clone(&turns), Some(Arc::clone(&gate))).await;
    let (channel, ctx) = runtime();
    let mut run = Running::start(ctx, InboundDebounceConfig::default());

    run.send("alice", "first").await;
    sleep_ms(2000).await;
    assert_eq!(turns.count(), 1, "first turn is running");
    run.send("alice", "second").await;
    sleep_ms(200).await;
    run.send("alice", "third").await;
    sleep_ms(5000).await;
    assert_eq!(turns.count(), 1, "follow-up waits for the running turn");

    gate.notify_one();
    sleep_ms(100).await;
    assert_eq!(turns.count(), 2, "quiet window already passed");
    assert_eq!(turns.peak.load(Ordering::SeqCst), 1);
    assert!(turns.last_user(1).ends_with("second\nthird"));
    let history = turns.history(1);
    assert!(history
        .iter()
        .any(|(role, content)| role == "assistant" && content == "reply 1"));
    assert!(history
        .iter()
        .any(|(role, content)| role == "user" && content.ends_with("first")));
    run.finish().await;
    assert_eq!(channel.sent_messages.lock().await.len(), 2);
}

#[tokio::test(start_paused = true)]
async fn follow_up_keeps_waiting_while_the_user_is_still_typing() {
    let turns = Arc::new(Turns::default());
    let gate = Arc::new(Notify::new());
    let _guard = install_handler(Arc::clone(&turns), Some(Arc::clone(&gate))).await;
    let (_channel, ctx) = runtime();
    let mut run = Running::start(ctx, InboundDebounceConfig::default());

    run.send("alice", "first").await;
    sleep_ms(2000).await;
    run.send("alice", "second").await;
    sleep_ms(100).await;
    gate.notify_one();
    sleep_ms(100).await;
    assert_eq!(turns.count(), 1, "second is still inside its quiet window");
    run.send("alice", "third").await;
    sleep_ms(2000).await;
    assert_eq!(turns.count(), 2);
    assert!(turns.last_user(1).ends_with("second\nthird"));
    run.finish().await;
}

#[tokio::test]
async fn runtime_command_is_answered_without_waiting_for_the_buffer() {
    let turns = Arc::new(Turns::default());
    let _guard = install_handler(Arc::clone(&turns), None).await;
    let channel = Arc::new(TelegramRecordingChannel::default());
    let ctx = runtime_with(channel.clone());
    let long_window = InboundDebounceConfig {
        debounce_ms: 600_000,
        media_debounce_ms: 600_000,
        max_wait_ms: 600_000,
        ..Default::default()
    };
    let mut run = Running::start(ctx, long_window);
    run.channel = "telegram";

    run.send("alice", "hello").await;
    run.send_unprepared("alice", "/model").await;
    tokio::time::timeout(Duration::from_secs(30), async {
        while channel.sent_messages.lock().await.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the command is answered while the buffer still holds hello");
    assert_eq!(turns.count(), 0);

    run.finish().await;
    assert_eq!(turns.count(), 1);
    assert!(turns.last_user(0).ends_with("hello"));
}

#[tokio::test(start_paused = true)]
async fn a_steady_stream_flushes_at_max_wait() {
    let turns = Arc::new(Turns::default());
    let _guard = install_handler(Arc::clone(&turns), None).await;
    let (_channel, ctx) = runtime();
    let mut run = Running::start(ctx, InboundDebounceConfig::default());

    for index in 0..12 {
        run.send("alice", &format!("part {index}")).await;
        sleep_ms(900).await;
    }

    assert_eq!(
        turns.count(),
        1,
        "deadline at 10s, quiet window would be 11.4s"
    );
    let user = turns.last_user(0);
    assert!(
        user.contains("part 0\n") && user.ends_with("part 11"),
        "{user}"
    );
    run.finish().await;
}

#[tokio::test(start_paused = true)]
async fn disabled_config_runs_one_turn_per_message_in_order() {
    let turns = Arc::new(Turns::default());
    let _guard = install_handler(Arc::clone(&turns), None).await;
    let (_channel, ctx) = runtime();
    let mut run = Running::start(ctx, InboundDebounceConfig::disabled());

    run.send("alice", "one").await;
    run.send("alice", "two").await;
    sleep_ms(100).await;

    assert_eq!(turns.count(), 2);
    assert_eq!(turns.peak.load(Ordering::SeqCst), 1);
    assert!(turns.last_user(0).ends_with("one"));
    assert!(turns.last_user(1).ends_with("two"));
    run.finish().await;
}

#[tokio::test(start_paused = true)]
async fn different_senders_are_never_merged() {
    let turns = Arc::new(Turns::default());
    let _guard = install_handler(Arc::clone(&turns), None).await;
    let (channel, ctx) = runtime();
    let mut run = Running::start(ctx, InboundDebounceConfig::default());

    run.send("alice", "from alice").await;
    run.send("bob", "from bob").await;
    sleep_ms(2000).await;

    assert_eq!(turns.count(), 2);
    let mut users = [turns.last_user(0), turns.last_user(1)];
    users.sort();
    assert!(users[0].ends_with("from alice"));
    assert!(users[1].ends_with("from bob"));
    run.finish().await;
    assert_eq!(channel.sent_messages.lock().await.len(), 2);
}

#[tokio::test(start_paused = true)]
async fn closing_the_queue_flushes_buffered_messages() {
    let turns = Arc::new(Turns::default());
    let _guard = install_handler(Arc::clone(&turns), None).await;
    let (_channel, ctx) = runtime();
    let mut run = Running::start(ctx, InboundDebounceConfig::default());

    run.send("alice", "a").await;
    run.send("alice", "b").await;
    run.finish().await;

    assert_eq!(turns.count(), 1);
    assert!(turns.last_user(0).ends_with("a\nb"));
}

#[tokio::test(start_paused = true)]
async fn a_full_intake_budget_stops_receiving_until_turns_drain() {
    let turns = Arc::new(Turns::default());
    let gate = Arc::new(Notify::new());
    let _guard = install_handler(Arc::clone(&turns), Some(Arc::clone(&gate))).await;
    let (_channel, ctx) = runtime();
    let config = InboundDebounceConfig {
        max_batch_messages: 1,
        ..InboundDebounceConfig::default()
    };
    let mut run = Running::start(ctx, config);

    run.send("alice", "first").await;
    sleep_ms(2000).await;
    assert_eq!(turns.count(), 1);

    let mut accepted = 0;
    for i in 0..200 {
        let message = run.prepared("alice", &format!("queued {i}"));
        if run.tx.try_send(message).is_err() {
            break;
        }
        accepted += 1;
        sleep_ms(1).await;
    }
    assert_eq!(accepted, 4 + 32, "4 held by the loop plus a full queue");

    gate.notify_one();
    run.finish().await;
    assert_eq!(turns.count(), 1 + accepted);
    for i in 0..accepted {
        assert!(turns.last_user(i + 1).ends_with(&format!("queued {i}")));
    }
}
