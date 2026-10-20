# doctor

Diagnostic and self-check domain for OpenHuman. The v2 framework registers stable-id checks, runs them asynchronously with tier budgets, bounded concurrency, and component gates, then returns the versioned `DoctorReport`. Existing v1 probes run through per-check blocking adapters until #506 ports them. The report is exposed through `doctor.report`; `doctor.models` returns bounded, read-only provider catalog inspection results.

## Responsibilities

- Define the check/report v2 contract, generic process and per-context check-source registrations, and a bounded asynchronous runner.
- Filter selected checks by exact id and tier; report disabled-component and above-tier checks as skipped.
- Keep the existing config, workspace, environment, memory, embeddings, and SDK probes working through a temporary v1 adapter.
- Preserve the `doctor.models` controller and the `doctor_health` / `doctor_models` tool names.

## Key files

| File | Role |
| --- | --- |
| `crates/openhuman-core/src/doctor/contract.rs` | Check trait, tier/component types, v2 findings, context, options, and report. |
| `crates/openhuman-core/src/doctor/registry.rs` | Built-in and host check sources; process and `CoreContext` registrations; id validation. |
| `crates/openhuman-core/src/doctor/runner.rs` | Exact-id filters, component/tier skips, timeouts, bounded concurrency, and v2 report assembly. |
| `crates/openhuman-core/src/doctor/legacy.rs` | Per-check adapter for the moved v1 probes until #506. |
| `crates/openhuman-core/src/doctor/ops.rs` | Async JSON-RPC surface; resolves async memory status and invokes the v2 runner. |
| `crates/openhuman-core/src/doctor/schemas.rs` | Controller schemas and handlers for `doctor.report` and `doctor.models`. |
| `crates/openhuman-core/src/doctor/runner_tests.rs` | Runner acceptance tests and `FakeCheck` usage. |
| `crates/openhuman-core/src/doctor/core_tests.rs` | Test suite for [`core.rs`](./core.rs) (via `#[path = "core_tests.rs"] mod tests`). |

## Public surface

From [`mod.rs`](./mod.rs):

- V2 types: `DoctorCheck`, `DoctorCheckSource`, `DoctorCtx`, `DoctorOptions`, `DoctorTier`, `Finding`, `FindingSeverity`, and `DoctorReport`.
- Registration: `register_check_source(source)` for the process or `register_check_source_in(&extensions, source)` for a `CoreContext` built with `CoreBuilder::extensions`.
- Legacy types remain available as `LegacyDoctorReport` / `LegacyDoctorSummary`; model-probe report types remain part of the `doctor.models` schema.

From `ops` (also re-exported as `doctor::rpc`):

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

None of its own (no `store.rs`). It only **reads** existing state owned by other domains: the memory engine's status (passed in as `MemoryEngineCheck`), config files, and `<workspace>/SYSTEM.md` / `<workspace>/memory/`. Its only writes are an ephemeral workspace probe file that is immediately deleted.

## Dependencies

- `crate::config::{Config, rpc}`: reads the live config for all probes; `config_rpc::load_config_with_timeout` in the handlers.
- `crate::memory::status`: the memory engine's status (`ok`/`degraded`/`down`/`off`), taken by `ops` before the blocking hop and passed to `run` as a `MemoryEngineCheck`.
- `crate::inference::embedding_host::effective_embedding_settings`: resolves the intended embedding provider/model.
- `crate::inference::local`: `ollama_base_url` (embedding probe).
- `config.resolved_endpoints` (filled by the host's config enricher) for the inference base URL; `crate::security::credentials::jwt::get_session_token` for sign-in state.
- `crate::core::all::{ControllerFuture, RegisteredController}`, `crate::core::{ControllerSchema, FieldSchema, TypeSchema}`: controller/schema plumbing.
- `crate::rpc::RpcOutcome`: handler return contract.
- External: `reqwest` (blocking client + URL parse), `serde`/`serde_json`, `chrono`, `anyhow`.

## Used by

- `crates/openhuman-core/src/core/all.rs`: registers the doctor controllers/schemas into the global RPC + CLI registry (the only in-tree consumer).

## Notes / gotchas

- The framework runs at most eight checks at once. A blocking adapter runs on a dedicated `spawn_blocking` worker; an async check stays on the runtime.
- Each detector has a tier-derived timeout (5 seconds static, 10 seconds connect, 60 seconds live). Timeout or worker failures are error findings and do not stop sibling checks.
- The legacy memory engine status remains an async input resolved by `ops` before the per-check blocking adapter. Do not call `Handle::block_on` from a check.
- The v1 adapter maps status and ids to v2 but does not forward unstructured legacy messages, which may contain resolved configuration values. Native v2 checks should provide safe, specific findings.
- An engine that is down is reported as an error and memory being off as a warning, so the doctor names what is wrong instead of showing an empty store.
- `doctor.models` uses an inspection-safe catalog hook and never falls back to the ordinary mutable catalog callback.
- The embedding probe is capped at a 3s timeout to avoid stalling on a slow Ollama daemon; non-ollama providers short-circuit to OK.
- `model_matches` treats `name` vs `name:tag` as a match only when at most one side is tagged; two differently-tagged names are not considered equal.
- Severity rollup: `DoctorSummary` counts `Ok`/`Warn`/`Error` items; checks are intentionally lenient (missing optional dirs/tools → `Warn`, not `Error`).
- OS-specific helpers (`available_disk_space_mb`, `check_command_available`, `check_claude_agent_sdk`) set `CREATE_NO_WINDOW` on Windows to avoid console flashes.

## Further reading

- [Platform services](../platform/README.md)
- [Platform and availability](../../../../docs/gitbooks/en/features/platform.md)
- [Recover a failed installation](../../../../../docs/gitbooks/en/guides/recover-failed-installation.md)
