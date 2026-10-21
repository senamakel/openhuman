# doctor

Diagnostic and self-check domain for OpenHuman. The framework registers stable-id checks, runs them asynchronously with tier budgets, bounded concurrency, and component gates, then returns the versioned `DoctorReport`. Native checks inspect the selected configuration and environment directly; only selected connect checks make bounded read-only requests. The report is exposed through `doctor.report`; `doctor.models` returns bounded, read-only provider catalog inspection results.

## Responsibilities

- Define the check/report v2 contract, generic process and per-context check-source registrations, and a bounded asynchronous runner.
- Filter selected checks by exact id and tier; report disabled-component and above-tier checks as skipped.
- Keep check behavior in native modules under `checks/`, with one stable id and safe fix hint per result.
- Preserve the `doctor.models` controller and the `doctor_health` / `doctor_models` tool names.

## Key files

| File | Role |
| --- | --- |
| `crates/openhuman-core/src/doctor/contract.rs` | Check trait, tier/component types, v2 findings, context, options, and report. |
| `crates/openhuman-core/src/doctor/registry.rs` | Built-in and host check sources; process and `CoreContext` registrations; id validation. |
| `crates/openhuman-core/src/doctor/runner.rs` | Exact-id filters, component/tier skips, timeouts, bounded concurrency, and v2 report assembly. |
| `crates/openhuman-core/src/doctor/checks/` | Native config, workspace, environment, memory, embedding, and component checks. |
| `crates/openhuman-core/src/doctor/ops.rs` | Async JSON-RPC surface; resolves the selected profile and invokes the v2 runner. |
| `crates/openhuman-core/src/doctor/schemas.rs` | Controller schemas and handlers for `doctor.report` and `doctor.models`. |
| `crates/openhuman-core/src/doctor/runner_tests.rs` | Runner acceptance tests and `FakeCheck` usage. |

## Public surface

From [`mod.rs`](./mod.rs):

- V2 types: `DoctorCheck`, `DoctorCheckSource`, `DoctorCtx`, `DoctorOptions`, `DoctorTier`, `Finding`, `FindingSeverity`, and `DoctorReport`.
- Registration: `register_check_source(source)` for the process or `register_check_source_in(&extensions, source)` for a `CoreContext` built with `CoreBuilder::extensions`.
- The model-probe report types remain part of the `doctor.models` schema while catalog probing is unavailable.

From `ops`:

- `doctor_report(&Config) -> Result<Outcome<DoctorReport>, String>` (v2 report)
- `doctor_report_with_options(&Config, DoctorOptions) -> Result<Outcome<DoctorReport>, String>`
- `doctor_models(&Config, use_cache: bool) -> Result<Outcome<ModelProbeReport>, String>`

Schema pair re-exported as `all_doctor_controller_schemas` / `all_doctor_registered_controllers`.

## RPC / controllers

Namespace `doctor`, two functions:

| Method | Inputs | Output |
| --- | --- | --- |
| `doctor.report` | optional `tier`, exact-id `only` / `skip`, and `profile` | v2 `DoctorReport` ("Run diagnostics for workspace and runtime configuration.") |
| `doctor.models` | `use_cache: Option<bool>` (default `true`) | Returns per-provider results from bounded read-only model catalog requests. |

Both handlers load config through `DispatchCtx` and return `Outcome::single_log(...)`. The profile field is reserved for the host profile loaders in #490/#491. Controllers remain wired in `crates/openhuman-core/src/core/all.rs`.

## Agent tools

The `doctor_health` and `doctor_models` tools remain in [`tools.rs`](./tools.rs); their names and JSON-RPC method names stay stable.

## Events

None. No `bus.rs` / event-bus subscribers or publishers.

## Persistence

None of its own (no `store.rs`). Static checks read the selected configuration and workspace layout. The writable check creates a bounded temporary file in the selected workspace, writes a short probe, and removes it before returning. Connect checks use read-only memory health hooks or local Ollama's `/api/tags`; they never initialize storage, install tools, or write remote state.

## Dependencies

- `crate::config::{Config, rpc}`: reads the live config for all probes; `config_rpc::load_config_with_timeout` in the handlers.
- `crate::memory::engine_registry::read_only_health_probe`: selected memory-engine connectivity without the normal cached/mutating resolver.
- `tinyinference_embeddings::catalog::effective_embedding_settings`: resolves the configured embedding provider and model.
- `tinyinference_local::ollama::ollama_base_url_from_override(config.local_ai.base_url)`: selected local Ollama URL for the configured-model readiness check.
- `sysinfo::Disks`: local free-space inspection without spawning a system command.
- `crate::core::all::{ControllerFuture, RegisteredController}`, `crate::core::{ControllerSchema, FieldSchema, TypeSchema}`: controller/schema plumbing.
- `crate::rpc::RpcOutcome`: handler return contract.
- External: `reqwest` (async read-only local probe + URL parsing), `serde_json`, `sysinfo`, `tokio::process` for the configured connect-tier SDK version probe.

## Used by

- `crates/openhuman-core/src/core/all.rs`: registers the doctor controllers/schemas into the global RPC + CLI registry (the only in-tree consumer).

## Notes / gotchas

- The framework runs at most eight checks at once. Async checks stay on the runtime; checks that declare synchronous work run on a dedicated `spawn_blocking` worker.
- Each detector has a tier-derived timeout (5 seconds static, 10 seconds connect, 60 seconds live). Timeout or worker failures are error findings and do not stop sibling checks.
- The `runtime/environment` static check uses PATH filesystem metadata only. It does not execute `git`, `curl`, or a shell to read version output.
- The `runtime/agent-sdk` check is `connect` tier and invokes only the configured CLI's `--version`, with output suppressed and the runner's bounded connect timeout.
- The selected `memory/health` check uses only a registered read-only health probe or a direct read-only credential snapshot. Do not call the normal cached memory resolver from doctor.
- `workspace/writable` may create one temporary local probe file, but it must remove it on every return path and report a cleanup failure safely.
- An unhealthy selected memory engine is reported as an error or warning from its typed `EngineHealth`; intentionally disabled memory is skipped.
- `doctor.models` uses an inspection-safe catalog hook and never falls back to the ordinary mutable catalog callback.
- The configured-model probe is capped at a 3s timeout to avoid stalling on a slow Ollama daemon; non-Ollama providers are skipped by this local Ollama readiness check.
- `model_matches` treats `name` vs `name:tag` as a match only when at most one side is tagged; two differently-tagged names are not considered equal.
- `workspace/layout` warns when `memory/` or `SYSTEM.md` is missing, or when the selected workspace volume has less than 512 MB available.
- `embeddings/configured-model` is a bounded connect check: non-Ollama providers are skipped, while Ollama is checked against the configured `local_ai.base_url` and model tags.

## Legacy behavior coverage

- The former config aggregate is split across `config/parse`, `config/source`, `config/env-override`, the inference and embedding checks, hosted `backend/api-url` / `backend/credential`, and `config/runtime-settings` for ignored fallback providers, incomplete model routes/delegate models, and the enabled-channel warning.
- The old workspace aggregate maps to `workspace/writable` and `workspace/layout`; its disk-space query uses OS filesystem data rather than a subprocess.
- The old environment and SDK checks map to `runtime/environment` and `runtime/agent-sdk` with explicit static/connect tiers.
- The old memory and embedding health checks map to `memory/health` and `embeddings/configured-model`. The unwritten daemon-state check was removed by #325 and has no replacement.

## Further reading

- [Platform services](../platform/README.md)
- [Platform and availability](../../../../docs/gitbooks/en/features/platform.md)
- [Recover a failed installation](../../../../../docs/gitbooks/en/guides/recover-failed-installation.md)
