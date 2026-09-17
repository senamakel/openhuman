//! Standard model-tier identifiers and memory-sync cadence constants.

/// Standard model identifiers matching the backend model registry.
pub const MODEL_AGENTIC_V1: &str = "agentic-v1";
pub const MODEL_REASONING_V1: &str = "reasoning-v1";
/// Low-latency conversational tier.
pub const MODEL_CHAT_V1: &str = "chat-v1";
/// Legacy low-latency chat tier slug retained for older persisted configs.
pub const MODEL_REASONING_QUICK_V1: &str = "reasoning-quick-v1";
pub const MODEL_CODING_V1: &str = "coding-v1";
/// High-throughput "burst" tier served by the managed backend. Cheap, fast,
/// non-reasoning, text-only, 128k context, no prompt cache; used by fast
/// high-fanout workers. Managed-backend only (no BYOK knob).
pub const MODEL_BURST_V1: &str = "burst-v1";
pub const MODEL_SUMMARIZATION_V1: &str = "summarization-v1";
/// Multimodal (image-input) tier. Managed backend serves this with the vision
/// flag enabled; the vision sub-agent rides this tier via `hint:vision`.
pub const MODEL_VISION_V1: &str = "vision-v1";
/// Default model used when no explicit model is configured.
///
/// Set to `chat-v1`, the backend's low-latency conversational tier. The
/// orchestrator (user-facing front-line agent) rides on this tier by default
/// via `hint:chat`; reach for the slower `reasoning-v1` only when deep
/// reasoning is needed.
pub const DEFAULT_MODEL: &str = MODEL_CHAT_V1;

/// Effective default global memory-sync cadence (seconds) used when
/// [`Config::memory_sync_interval_secs`] is `None` — i.e. the user has not
/// explicitly picked a schedule. 24h, matching the "Sync every 24h" preset
/// surfaced in the Memory Sources UI. See issue #3302.
///
/// Defined in `tinymemory_api::host` and re-exported here: the extracted memory
/// subsystem applies this fallback too, and two `86_400`s that must agree is a
/// drift waiting to happen.
pub use tinymemory_api::host::DEFAULT_MEMORY_SYNC_INTERVAL_SECS;

/// Preset memory-sync cadences (seconds) offered in the UI: 4h / 12h / 24h.
/// "Manual only" is represented separately by `Some(0)`. See issue #3302.
pub const MEMORY_SYNC_INTERVAL_PRESETS_SECS: [u64; 3] = [14_400, 43_200, 86_400];
