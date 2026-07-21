//! Hermetic, Rust-only library profiling workloads.
//!
//! This binary never enters shipped builds. It measures two production code
//! paths in fresh processes with network inference replaced by a deterministic
//! provider:
//! - `memory-ingest`: canonicalise and ingest 100 chat messages, then drain the
//!   real extraction/admission/tree queue.
//! - `subagents`: run one real orchestrator chat turn that spawns two real
//!   researcher subagents through the parallel-delegation tool.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use openhuman_core::core::event_bus::init_global;
use openhuman_core::openhuman::agent::harness::AgentDefinitionRegistry;
use openhuman_core::openhuman::config::schema::SubconsciousMode;
use openhuman_core::openhuman::config::Config;
use openhuman_core::openhuman::inference::provider::factory::test_provider_override;
use openhuman_core::openhuman::inference::provider::traits::{
    ChatRequest, ChatResponse, ProviderCapabilities, ToolCall,
};
use openhuman_core::openhuman::inference::provider::Provider;
use openhuman_core::openhuman::memory::ingest_pipeline::ingest_chat;
use openhuman_core::openhuman::memory_queue::drain_until_idle;
use openhuman_core::openhuman::memory_sync::canonicalize::chat::{ChatBatch, ChatMessage};
use openhuman_core::openhuman::proc_metrics::{self, ProcSample};
use openhuman_core::openhuman::subconscious::LongLivedSession;
use serde::Serialize;

const INGEST_MESSAGE_COUNT: usize = 100;
const SUBAGENT_A: &str = "LIB_PROFILE_SUBAGENT_A";
const SUBAGENT_B: &str = "LIB_PROFILE_SUBAGENT_B";

#[derive(Debug, Serialize)]
struct ProfileResult {
    schema_version: u32,
    scenario: &'static str,
    workload_units: usize,
    duration_ms: u128,
    baseline: ProcSample,
    settled: ProcSample,
    peak_rss_kib: u64,
    retained_delta_kib: i64,
    peak_delta_kib: u64,
}

struct EnvGuard {
    key: &'static str,
    old: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let old = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, old }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

struct Fixture {
    config: Config,
    _workspace_env: EnvGuard,
    _keyring_env: EnvGuard,
    _tmp: tempfile::TempDir,
}

fn fixture() -> Result<Fixture> {
    let tmp = tempfile::tempdir().context("create profile workspace")?;
    let root = tmp.path();
    let workspace = root.join("workspace");
    std::fs::create_dir_all(&workspace)?;

    let mut config_toml = r#"api_url = "http://127.0.0.1:9"
default_model = "profile-mock"
default_temperature = 0.0
chat_onboarding_completed = true

[secrets]
encrypt = false

[local_ai]
enabled = false
runtime_enabled = false

[runtime_python]
enabled = false

[memory_tree]
spacy_enabled = false
"#
    .to_string();
    if std::env::var_os("OPENHUMAN_PROFILE_DISABLE_MEMORY_WRITES").is_some() {
        config_toml.push_str(
            r#"
[memory]
auto_save = false

[learning]
episodic_capture_enabled = false
"#,
        );
    }
    std::fs::write(root.join("config.toml"), &config_toml)?;

    let workspace_env = EnvGuard::set("OPENHUMAN_WORKSPACE", &root.to_string_lossy());
    let keyring_env = EnvGuard::set("OPENHUMAN_KEYRING_BACKEND", "file");
    let mut config: Config = toml::from_str(&config_toml)?;
    config.workspace_dir = workspace;
    config.memory_tree.embedding_endpoint = None;
    config.memory_tree.embedding_model = None;
    config.memory_tree.embedding_strict = false;

    Ok(Fixture {
        config,
        _workspace_env: workspace_env,
        _keyring_env: keyring_env,
        _tmp: tmp,
    })
}

struct PeakSampler {
    peak: Arc<AtomicU64>,
    stop: Arc<AtomicU64>,
    task: tokio::task::JoinHandle<()>,
}

impl PeakSampler {
    fn start(initial_kib: u64) -> Self {
        let peak = Arc::new(AtomicU64::new(initial_kib));
        let stop = Arc::new(AtomicU64::new(0));
        let task_peak = Arc::clone(&peak);
        let task_stop = Arc::clone(&stop);
        let task = tokio::spawn(async move {
            while task_stop.load(Ordering::Relaxed) == 0 {
                if let Ok(sample) = proc_metrics::sample_self() {
                    task_peak.fetch_max(sample.rss_kib, Ordering::Relaxed);
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });
        Self { peak, stop, task }
    }

    async fn stop(self) -> u64 {
        self.stop.store(1, Ordering::Relaxed);
        let _ = self.task.await;
        self.peak.load(Ordering::Relaxed)
    }
}

async fn measure<F, Fut>(
    scenario: &'static str,
    workload_units: usize,
    workload: F,
) -> Result<ProfileResult>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    tokio::time::sleep(Duration::from_millis(250)).await;
    let baseline = proc_metrics::sample_self()?;
    if let Some(seconds) = std::env::var("OPENHUMAN_PROFILE_HOLD_BEFORE_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
    {
        eprintln!(
            "library-profile pid={} holding at baseline for {seconds}s",
            std::process::id()
        );
        tokio::time::sleep(Duration::from_secs(seconds)).await;
    }
    let sampler = PeakSampler::start(baseline.rss_kib);
    let started = Instant::now();
    workload().await?;
    let duration_ms = started.elapsed().as_millis();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let settled = proc_metrics::sample_self()?;
    let peak_rss_kib = sampler.stop().await.max(settled.rss_kib);
    Ok(ProfileResult {
        schema_version: 1,
        scenario,
        workload_units,
        duration_ms,
        baseline,
        settled,
        peak_rss_kib,
        retained_delta_kib: settled.rss_kib as i64 - baseline.rss_kib as i64,
        peak_delta_kib: peak_rss_kib.saturating_sub(baseline.rss_kib),
    })
}

fn ingestion_batch() -> ChatBatch {
    let messages = (0..INGEST_MESSAGE_COUNT)
        .map(|index| ChatMessage {
            author: if index % 2 == 0 { "alice" } else { "bob" }.into(),
            timestamp: Utc
                .timestamp_millis_opt(1_700_000_000_000 + index as i64 * 60_000)
                .single()
                .expect("valid profile timestamp"),
            text: format!(
                "Phoenix migration update {index}: staging p99 is 12ms and error rate is 0.001%. \
                 Alice owns the rollback runbook, Bob owns on-call coordination, and the \
                 phoenix_v2_enabled flag ramps Friday after billing-ledger verification."
            ),
            source_ref: Some(format!("profile://message/{index}")),
        })
        .collect();
    ChatBatch {
        platform: "profile".into(),
        channel_label: "library-benchmark".into(),
        messages,
    }
}

async fn run_memory_ingest() -> Result<ProfileResult> {
    let fixture = fixture()?;
    let _ = init_global(256);
    measure("memory-ingest", INGEST_MESSAGE_COUNT, || async {
        let result = ingest_chat(
            &fixture.config,
            "profile:chat:100",
            "profile-user",
            vec!["profile".into()],
            ingestion_batch(),
        )
        .await?;
        anyhow::ensure!(result.chunks_written > 0, "ingestion wrote no chunks");
        drain_until_idle(&fixture.config).await?;
        Ok(())
    })
    .await
}

struct SubagentMock {
    prompts: Mutex<Vec<String>>,
}

impl SubagentMock {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            prompts: Mutex::new(Vec::new()),
        })
    }

    fn response(&self, joined: &str) -> ChatResponse {
        self.prompts
            .lock()
            .expect("mock prompt lock")
            .push(joined.into());
        if joined.contains("Finding A") && joined.contains("Finding B") {
            return response("Merged both researcher findings.");
        }
        if joined.contains(SUBAGENT_B) {
            return response("Finding B: rollback controls are complete.");
        }
        if joined.contains(SUBAGENT_A) {
            return response("Finding A: migration capacity is healthy.");
        }
        ChatResponse {
            text: Some("Delegating to two researchers.".into()),
            tool_calls: vec![ToolCall {
                id: "profile-parallel-call".into(),
                name: "spawn_parallel_agents".into(),
                arguments: serde_json::json!({
                    "tasks": [
                        {
                            "agent_id": "researcher",
                            "prompt": format!("{SUBAGENT_A}: inspect migration capacity"),
                            "ownership": "scope: capacity"
                        },
                        {
                            "agent_id": "researcher",
                            "prompt": format!("{SUBAGENT_B}: inspect rollback controls"),
                            "ownership": "scope: rollback"
                        }
                    ]
                })
                .to_string(),
                extra_content: None,
            }],
            usage: None,
            reasoning_content: None,
        }
    }
}

fn response(text: &str) -> ChatResponse {
    ChatResponse {
        text: Some(text.into()),
        tool_calls: Vec::new(),
        usage: None,
        reasoning_content: None,
    }
}

#[async_trait]
impl Provider for SubagentMock {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_tool_calling: true,
            vision: false,
        }
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        _model: &str,
        _temperature: f64,
    ) -> Result<String> {
        Ok(self
            .response(&format!("{}\n{message}", system_prompt.unwrap_or("")))
            .text
            .unwrap_or_default())
    }

    async fn chat(
        &self,
        request: ChatRequest<'_>,
        _model: &str,
        _temperature: f64,
    ) -> Result<ChatResponse> {
        let joined = request
            .messages
            .iter()
            .map(|message| format!("{}: {}", message.role, message.content))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(self.response(&joined))
    }
}

async fn run_subagents() -> Result<ProfileResult> {
    let fixture = fixture()?;
    let _ = init_global(256);
    openhuman_core::openhuman::agent::bus::register_agent_handlers();
    let _ = AgentDefinitionRegistry::init_global_builtins();
    let mock = SubagentMock::new();
    let provider: Arc<dyn Provider> = mock.clone();
    let _provider = test_provider_override::install(provider);
    if std::env::var_os("OPENHUMAN_PROFILE_PREWARM_SUBAGENTS").is_some() {
        let warmup = LongLivedSession::with_thread(
            fixture.config.workspace_dir.clone(),
            SubconsciousMode::Aggressive,
            "profile:warmup".into(),
        );
        let outcome = warmup
            .process_promoted("Please research the Phoenix migration.", false)
            .await
            .map_err(anyhow::Error::msg)?;
        anyhow::ensure!(!outcome.response.is_empty(), "empty warmup response");
        mock.prompts.lock().expect("mock prompt lock").clear();
    }
    let session = LongLivedSession::with_thread(
        fixture.config.workspace_dir.clone(),
        SubconsciousMode::Aggressive,
        "profile:orchestrator".into(),
    );
    measure("subagents", 2, || async {
        let outcome = session
            .process_promoted("Please research the Phoenix migration.", false)
            .await
            .map_err(anyhow::Error::msg)?;
        anyhow::ensure!(!outcome.response.is_empty(), "empty orchestrator response");
        let prompts = mock.prompts.lock().expect("mock prompt lock");
        anyhow::ensure!(prompts.iter().any(|p| p.contains(SUBAGENT_A)));
        anyhow::ensure!(prompts.iter().any(|p| p.contains(SUBAGENT_B)));
        Ok(())
    })
    .await
}

#[tokio::main]
async fn main() -> Result<()> {
    let scenario = std::env::args()
        .nth(1)
        .context("usage: library-profile <memory-ingest|subagents>")?;
    let result = match scenario.as_str() {
        "memory-ingest" => run_memory_ingest().await?,
        "subagents" => run_subagents().await?,
        _ => anyhow::bail!("unknown scenario: {scenario}"),
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    if let Some(seconds) = std::env::var("OPENHUMAN_PROFILE_HOLD_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
    {
        eprintln!(
            "library-profile pid={} holding for {seconds}s",
            std::process::id()
        );
        tokio::time::sleep(Duration::from_secs(seconds)).await;
    }
    Ok(())
}
