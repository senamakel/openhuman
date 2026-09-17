//! The [`Default`] impl for [`Config`], anchored at the OpenHuman root dir.

use directories::UserDirs;
use std::collections::HashMap;
use std::path::PathBuf;

use super::config::default_temperature_unsupported_models;
use super::config::DEFAULT_TEMPERATURE;
use super::model_ids::DEFAULT_MODEL;
use crate::config::schema::*;

impl Default for Config {
    fn default() -> Self {
        let openhuman_dir = crate::config::default_root_openhuman_dir().unwrap_or_else(|_| {
            let home =
                UserDirs::new().map_or_else(|| PathBuf::from("."), |u| u.home_dir().to_path_buf());
            let dir_name = if crate::api::config::is_staging_app_env(
                crate::api::config::app_env_from_env().as_deref(),
            ) {
                ".openhuman-staging"
            } else {
                ".openhuman"
            };
            home.join(dir_name)
        });

        Self {
            workspace_dir: openhuman_dir.join("workspace"),
            action_dir: crate::config::default_action_dir(),
            action_dir_override: None,
            config_path: openhuman_dir.join("config.toml"),
            cli_inference_snapshot: None,
            recovered_from_corruption: false,
            schema_version: 0,
            api_url: None,
            api_key: None,
            inference_url: None,
            ephemeral_route: None,
            default_model: Some(DEFAULT_MODEL.to_string()),
            default_temperature: DEFAULT_TEMPERATURE,
            output_language: None,
            temperature_unsupported_models: default_temperature_unsupported_models(),
            observability: ObservabilityConfig::default(),
            dashboard: DashboardConfig::default(),
            autonomy: AutonomyConfig::default(),
            hooks: HooksConfig::default(),
            privacy: PrivacyConfig::default(),
            sandbox: SandboxConfig::default(),
            runtime: RuntimeConfig::default(),
            shell: ShellConfig::default(),
            reliability: ReliabilityConfig::default(),
            scheduler: SchedulerConfig::default(),
            scheduler_gate: SchedulerGateConfig::default(),
            agent_activity_level: AgentActivityLevel::default(),
            memory_sync_interval_secs: None,
            agent: AgentConfig::default(),
            orchestrator: OrchestratorModelConfig::default(),
            teams: HashMap::new(),
            context: ContextConfig::default(),
            model_routes: Vec::new(),
            embedding_routes: Vec::new(),
            heartbeat: HeartbeatConfig::default(),
            subconscious: crate::config::schema::SubconsciousConfig::default(),
            cron: CronConfig::default(),
            task_sources: TaskSourcesConfig::default(),
            channels_config: ChannelsConfig::default(),
            memory: MemoryConfig::default(),
            memory_tree: MemoryTreeConfig::default(),
            storage: StorageConfig::default(),
            subsystems: SubsystemsConfig::default(),
            composio: ComposioConfig::default(),
            secrets: SecretsConfig::default(),
            browser: BrowserConfig::default(),
            http_request: HttpRequestConfig::default(),
            curl: CurlConfig::default(),
            gitbooks: GitbooksConfig::default(),
            mcp_client: McpClientConfig::default(),
            modules: ModulesConfig::default(),
            capability_providers: Vec::new(),
            multimodal: MultimodalConfig::default(),
            multimodal_files: MultimodalFileConfig::default(),
            seltz: SeltzConfig::default(),
            searxng: SearxngConfig::default(),
            web_search: WebSearchConfig::default(),
            search: SearchConfig::default(),
            proxy: ProxyConfig::default(),
            cost: CostConfig::default(),
            memory_sources: Vec::new(),
            agent_registry: crate::agent::registry::types::AgentRegistryConfig::default(),
            agents: HashMap::new(),
            local_ai: LocalAiConfig::default(),
            claude_agent_sdk: ClaudeAgentSdkConfig::default(),
            cloud_providers: Vec::new(),
            primary_cloud: None,
            chat_provider: None,
            reasoning_provider: None,
            agentic_provider: None,
            coding_provider: None,
            vision_provider: None,
            memory_provider: None,
            embeddings_provider: None,
            custom_embeddings: None,
            heartbeat_provider: None,
            learning_provider: None,
            subconscious_provider: None,
            node: NodeConfig::default(),
            runtime_python: RuntimePythonConfig::default(),
            runtime_pool: RuntimePoolConfig::default(),
            tokenjuice: TokenjuiceConfig::default(),
            hosting: HostingConfig::default(),
            voice_server: VoiceServerConfig::default(),
            voice_providers: Vec::new(),
            stt_provider: None,
            tts_provider: None,
            integrations: IntegrationsConfig::default(),
            learning: LearningConfig::default(),
            update: UpdateConfig::default(),
            dictation: DictationConfig::default(),
            onboarding_completed: false,
            chat_onboarding_completed: false,
            model_registry: Vec::new(),
            composio_source_caps_migration_version: 0,
        }
    }
}
