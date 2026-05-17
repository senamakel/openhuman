# Local AI

Local asset/runtime support for speech models and localhost-style integrations. This module no longer owns the public LLM inference contract. `src/openhuman/inference/` is the core namespace for prompt/chat/embed/status routing, while `local_ai` keeps the local speech/download/device-profile pieces and the lower-level helpers those inference adapters may delegate to.

## Public surface

- `pub struct LocalAiService` — `service/mod.rs` — singleton for Ollama/LM Studio health checks plus whisper/Piper helpers.
- `pub fn global(config: &Config) -> Arc<LocalAiService>` — `core.rs` — singleton accessor.
- `pub fn model_artifact_path(config: &Config) -> PathBuf` — `core.rs` — resolve on-disk model path.
- `pub struct DeviceProfile` — `device.rs` — RAM / VRAM / CPU classification used for preset selection.
- `pub struct ModelPreset` / `pub enum ModelTier` / `pub enum VisionMode` — `presets.rs` — bundled preset matrix.
- `pub struct SentimentResult` — `sentiment.rs` — internal sentiment result type used by inference delegates.
- Status / progress / result types: `pub struct LocalAiStatus`, `LocalAiAssetStatus`, `LocalAiAssetsStatus`, `LocalAiDownloadProgressItem`, `LocalAiDownloadsProgress`, `LocalAiEmbeddingResult`, `LocalAiSpeechResult`, `LocalAiTtsResult` — `types.rs`.
- `pub mod ops` (re-exported as `rpc`) — `ops.rs` — typed Rust wrappers. Public `local_ai.*` RPCs are limited to local speech/assets/device-profile flows; prompt/chat/embed/status helpers remain available for internal delegation from `inference`.
- RPC `local_ai.{agent_chat, agent_chat_simple, local_ai_transcribe, local_ai_transcribe_bytes, local_ai_tts, local_ai_assets_status, local_ai_downloads_progress, local_ai_download_asset, local_ai_device_profile, local_ai_presets, local_ai_apply_preset, local_ai_diagnostics, local_ai_install_whisper, local_ai_install_piper, local_ai_whisper_install_status, local_ai_piper_install_status}` — `schemas.rs`.

## Calls into

- `src/openhuman/config/` — provider selection, model IDs, localhost base URL override, device-profile inputs.
- Bundled binaries and assets for whisper.cpp and Piper.
- External Ollama / LM Studio endpoints for diagnostics and model-state checks.
- Filesystem under `~/.openhuman/local-ai/` for downloaded speech/model artifacts.

## Called by

- `src/openhuman/inference/` — delegates LLM/provider-facing status, prompt, chat, embed, reaction, and sentiment flows here as an implementation detail.
- `src/openhuman/voice/{streaming,postprocess,ops,types}.rs` — speech-to-text + text-to-speech.
- `src/openhuman/screen_intelligence/processing_worker.rs` — local multimodal helpers.
- `src/openhuman/autocomplete/core/engine.rs` — local completions.
- `src/openhuman/tree_summarizer/ops.rs` — summarisation backend.
- `src/openhuman/app_state/ops.rs` — runtime snapshot support.
- `src/core/all.rs` — registers `all_local_ai_*`.

## Tests

- Unit: `ops_tests.rs`, `schemas_tests.rs`, plus `service/ollama_admin_tests.rs`, `service/public_infer_tests.rs`.
- Domain mutex: `LOCAL_AI_TEST_MUTEX` (`mod.rs`) serializes tests that mutate the singleton or env vars.
- Routing: `agent/triage/routing_tests.rs` covers local-vs-remote escalation.

## Provider notes

OpenHuman does not ship or launch Ollama. The UI talks to the core, the core talks to `inference`, and `inference` can route to an external Ollama-compatible endpoint when configured. `local_ai` still exposes diagnostics and asset state so the UI can guide users through speech-model downloads and localhost runtime setup without treating Ollama as an app-managed runtime.
