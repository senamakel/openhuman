# Doctor v2 contract

This is the maintainer contract for the OpenHuman doctor framework and the
checks delivered by the doctor epic. It defines stable check ids and report
semantics before the legacy `platform/doctor` code moves. The core owns checks,
the registry, runner, and RPC surface; `openhuman-cli` owns command parsing,
rendering, and process exit codes; `openhuman-tinyhumans` owns hosted checks.

The intended user outcome is a read-only report that says whether the selected
configuration is ready to run, what was inspected, and what an operator can do
about each finding. This contract borrows OpenClaw's stable check identifiers,
structured findings, advisory JSON mode, and threshold-based lint behavior.
Wire fields remain snake case to match the existing RPC contract. This contract
does not adopt OpenClaw's repair operation: this epic adds no repair command.

## Goals and non-goals

The doctor validates configuration semantics and provenance, profile-specific
runtime readiness, configured local and hosted components, and outbound
connections. Checks are selectable by stable id and have an explicit cost tier.
The report is a versioned JSON API shared by RPC, CLI, the agent tool, and the
Settings diagnostics surface.

This epic does not add `--fix`, prompts, config rewrites, migrations, token
generation, or other repair side effects. A finding always has a human-readable
`fix_hint`, but that hint is guidance for a later explicit operator action.
Checks do not make billable inference or embedding calls, or write scratch
memory, unless the caller opts into the `live` tier.

## Check contract

The Rust API and serialized JSON use snake case. A check is
registered with a stable id, category, cost tier, and optional component gate.
Its detector is asynchronous and returns findings for that check:

```rust
DoctorCheck {
    id: "<area>/<name>",
    category: "<display category>",
    tier: DoctorTier::Static | DoctorTier::Connect | DoctorTier::Live,
    component_gate: Option<ComponentId>,
    async fn detect(&self, ctx: &DoctorCtx) -> Vec<Finding>,
}
```

Ids are lowercase ASCII `area/name` strings. They are API identifiers used by
`--only`, `--skip`, the RPC filters, and downstream consumers. Once released,
an id is never renamed or reused for different behavior; a replacement gets a
new id and the old one remains recognizable for at least one schema lifetime.
IDs are matched exactly. Prefixes, globs, and category names are not selectors.

`static` checks may read configuration and the local filesystem, but do not
open network connections or write files. `connect` checks may make bounded,
read-only network requests and are the default. `live` checks may make
billable model or embedding calls and may write disposable scratch memory; they
run only when explicitly requested. A requested tier is a ceiling: `static`
runs static checks, `connect` runs static and connect checks, and `live` runs
all three tiers.

The runner applies the active `ComponentSet` gate before calling a detector.
A gated-off component produces a `skipped` finding and is listed in
`checks_skipped`. Checks above the requested tier are also listed as skipped.
An id omitted by `--only` or named by a valid `--skip` is filtered out and is
not reported as a failed check. The runner uses bounded concurrency (at most
eight checks at once), a timeout for every detector (5 seconds for static,
10 seconds for connect, 60 seconds for live), and a stable registry order in
the report regardless of completion order. Timeout and detector failures are
`error` findings; one failure does not prevent other selected checks from
finishing. A timed-out detector is cancelled where the async operation supports
cancellation.

Every registered check emits at least one finding. A successful check emits a
`pass` finding. If a check finds multiple conditions, it emits one finding for
each condition; if it finds none, it emits one pass. This makes `checks_run`
and severity totals meaningful without treating an empty vector as an
unobserved check.

### Finding

```json
{
  "check_id": "config/file-integrity",
  "severity": "warn",
  "message": "The config file was recovered from its backup.",
  "fix_hint": "Review the backup and restore a valid config file.",
  "config_path": "config.toml",
  "value_source": "file",
  "duration_ms": 4,
  "affects": ["settings.configuration"]
}
```

| Field          | Contract                                                                                                               |
| -------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `check_id`     | Required stable id of the check that produced the finding.                                                             |
| `severity`     | `pass`, `info`, `warn`, `error`, or `skipped`.                                                                         |
| `message`      | Short operator-facing result, with no secret or config value.                                                          |
| `fix_hint`     | Required next step. Use “No action required.” for `pass`; explain how to rerun or enable a skipped check.              |
| `config_path`  | Optional logical config path, never a resolved home-directory path.                                                    |
| `value_source` | Optional `cli`, `env`, `dotenv`, `file`, or `default`, when a finding concerns a resolved value.                       |
| `duration_ms`  | Elapsed detector time, repeated consistently on findings from one check. A check skipped before execution reports `0`. |
| `affects`      | Zero or more stable `about_app` capability ids, such as `channels.connect_platform` or `local_ai.configure_provider`.  |

The report must not contain credentials, tokens, secret material, raw config
values, request or response bodies, or user-authored content. Redact endpoint
credentials and query parameters before forming messages. A detector that
cannot safely summarize an error reports its class and a safe recovery hint,
not the original error string. `value_source` records provenance only; it
never carries the value.

## Report schema v2

The RPC method remains `openhuman.doctor_report`; the existing `doctor_models`
method and `doctor_health` / `doctor_models` tool names also remain stable.
`doctor_report` accepts `{ "tier", "only", "skip", "profile" }` and returns:

```json
{
  "schema_version": 2,
  "profile": "desktop-default",
  "config_source": "profile",
  "tier": "connect",
  "checks_run": ["config/file-integrity"],
  "checks_skipped": ["inference/role-completion"],
  "findings": [
    {
      "check_id": "config/file-integrity",
      "severity": "pass",
      "message": "The config file is valid.",
      "fix_hint": "No action required.",
      "duration_ms": 4,
      "affects": []
    },
    {
      "check_id": "inference/role-completion",
      "severity": "skipped",
      "message": "This live check was not requested.",
      "fix_hint": "Rerun doctor with --live to run this check.",
      "duration_ms": 0,
      "affects": []
    }
  ],
  "summary": { "pass": 1, "info": 0, "warn": 0, "error": 0, "skipped": 1 },
  "ok": true
}
```

`checks_run` and `checks_skipped` are ordered arrays of check ids. A check
selected by the caller but gated off or above the requested tier is in
`checks_skipped`; checks outside an `only` selection and checks named by `skip`
are absent. `summary` counts findings by severity. `ok` is false exactly when
the report contains an `error`; warnings remain advisory, and a skipped check
does not by itself make the report unhealthy. The CLI's lint threshold is
separate from this field.

`profile` is the effective profile id, when the host has one, and `null`
otherwise. `config_source` is the redacted source kind (`profile` or `tenant`),
not a filesystem path or tenant credential. The report describes the effective
configuration used for this run. It does not reveal a config path, except for
logical paths in individual findings.

The selection validator is a reserved runner check, `doctor/selection`. An
unknown id supplied to either `only` or `skip` adds an `error` finding under
`doctor/selection`, with a hint to use a registered exact id. Do not echo an
unknown selector into the message. Valid selected checks still run. The
reserved check is not itself selectable and does not count as a detector in
`checks_run`.

## Tier and selection inputs

The RPC accepts optional `tier`, `only`, `skip`, and `profile` fields. An
omitted tier means `connect`; omitted lists are empty. `only` and `skip` are
repeatable at the CLI and contain exact ids. If `only` is present, only those
registered checks run; `skip` then removes checks from that set. Duplicate ids
are collapsed. An unregistered id is a report finding rather than a transport
error, so callers receive useful results for the valid ids in the same request.

`--live` selects the `live` ceiling. It does not change the tier of each check
or imply repair. The doctor does not infer consent to billable work from a
profile setting.

## CLI contract

The command is implemented only in `crates/openhuman-cli`; the core contains
no CLI parser or renderer. It calls `openhuman.doctor_report` through the
`openhuman-rpc` facade and maps options to the RPC parameters above.

| Invocation                            | Output and exit behavior                                                                                                                    |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| `openhuman-core doctor`               | Human-readable table; exit `0` after a report is produced.                                                                                  |
| `openhuman-core doctor --json`        | JSON report; advisory mode exits `0` after a report is produced, regardless of findings.                                                    |
| `openhuman-core doctor --lint`        | Human-readable findings; exit `0` if none meet the threshold, `1` if any do, `2` if parsing or execution fails before a report is produced. |
| `openhuman-core doctor --lint --json` | Same threshold and exit behavior as lint, with JSON output.                                                                                 |

`--severity-min` accepts `info`, `warn`, or `error` and defaults to `error`.
The threshold controls both printed findings and the lint result. `pass` and
`skipped` never meet it. `--only <id>` and `--skip <id>` may be repeated;
`--live` selects the live tier, and `--profile <id>` selects the effective
profile where the host permits it. In a SaaS tenant, the tenant's forced
configuration always wins; a client cannot use `--profile` to inspect another
tenant or switch the active tenant. Argument syntax errors exit `2` before
the RPC call.

The CLI does not add fields to the RPC report. Human output is a rendering of
the same findings and includes the severity, check id, message, and fix hint.
JSON uses the same report in every surface.

## Profile semantics

On desktop, a requested profile id resolves through `ProfileLayout`. Doctor
loads that profile's configuration and credentials for inspection without
changing the active profile, writing profile state, or changing process-wide
configuration. With no explicit id, it uses the currently selected profile.
The report's `profile` is the effective id and `config_source` is `profile`.

In SaaS, doctor runs against the authenticated tenant's forced configuration.
The host supplies that context to `DoctorCtx`; a client-supplied profile cannot
override it. The report uses `config_source: "tenant"` and includes no tenant
secret or private config path. Profile-scoped credential and config access must
use the context provided for the run rather than process-global lookup helpers.

## Check registration and hosted checks

Built-in checks register in the core registry. The core also exposes the
generic `register_check_source` seam so a host can register a source of checks
without adding a dependency from core to that host. `openhuman-tinyhumans`
registers its backend, identity, socket, API URL, and managed-inference checks
through this seam. Sources register against the active core context, not a
process-global mutable singleton, so profiles and tests do not share state.
Duplicate ids fail registration with a startup error; hosted ids follow the
same stable `area/name` format and are unique across sources. A source may
register zero checks when its host capability is absent.

The doctor stays in the existing `DomainGroup::Platform` registration group.
Moving its Rust module from `platform/doctor` to the top-level `doctor` domain
does not add a new runtime domain group. `subsystems.status` remains a separate
wire projection: it reports driver bindings, health, and capabilities, while
doctor findings include stable check ids, severity, guidance, provenance, and
timing. A doctor check may consume subsystem status as evidence, but it does
not reuse `SubsystemStatus` as a finding.

## Check inventory

The paths below are the implementation-owned probe locations under
`crates/openhuman-core/src/doctor/checks/` (and `crates/openhuman-tinyhumans`
for hosted checks). The function names are stable implementation targets; line
numbers are intentionally left to each implementing leaf so this pre-code
contract does not publish stale line references. Each leaf must keep its check
id and function together and link the final function line in its implementation
PR. All findings use the fix hint shown here as a minimum, and may provide a
more specific safe action.

| Check id                                  | Tier    | Probe function                                       | Owning leaf | Minimum fix hint                                                                   |
| ----------------------------------------- | ------- | ---------------------------------------------------- | ----------- | ---------------------------------------------------------------------------------- |
| `config/file-integrity`                   | static  | `checks/config.rs::file_integrity`                   | #492        | Restore or repair the config file; review any recovered backup.                    |
| `config/provenance`                       | static  | `checks/config.rs::provenance`                       | #493        | Set the intended source explicitly and rerun doctor.                               |
| `config/inference-references`             | static  | `checks/config.rs::inference_references`             | #494        | Select a configured provider and model reference.                                  |
| `config/components-storage-paths-secrets` | static  | `checks/config.rs::components_storage_paths_secrets` | #495        | Correct the named component, storage scheme, path, or secret-source configuration. |
| `workspace/layout`                        | static  | `checks/workspace.rs::layout`                        | #506        | Create or repair the required workspace path and permissions.                      |
| `runtime/environment`                     | static  | `checks/runtime.rs::environment`                     | #506        | Install or configure the named local runtime prerequisite.                         |
| `runtime/agent-sdk`                       | connect | `checks/runtime.rs::agent_sdk`                       | #506        | Install or configure the agent SDK binary, then rerun doctor.                      |
| `memory/store`                            | connect | `checks/memory.rs::store`                            | #498        | Restore the configured memory driver's availability and rerun doctor.              |
| `embeddings/configured-model`             | connect | `checks/embeddings.rs::configured_model`             | #509        | Start the configured embedding service or correct its base URL and model.          |
| `inference/provider-models`               | connect | `checks/inference.rs::provider_models`               | #497        | Configure an available provider/model or resolve the reported access issue.        |
| `storage/connectivity`                    | connect | `checks/storage.rs::connectivity`                    | #498        | Make the configured storage driver reachable and retry the check.                  |
| `keyring/connectivity`                    | connect | `checks/keyring.rs::connectivity`                    | #498        | Unlock or configure the selected system keyring.                                   |
| `modules/readiness`                       | connect | `checks/modules.rs::readiness`                       | #499        | Install or enable the configured module source.                                    |
| `mcp/servers`                             | connect | `checks/mcp.rs::servers`                             | #499        | Start the configured MCP server or correct its safe connection settings.           |
| `skills/readiness`                        | static  | `checks/skills.rs::readiness`                        | #499        | Repair the named skill source or its metadata.                                     |
| `sandbox/readiness`                       | connect | `checks/sandbox.rs::readiness`                       | #499        | Install or enable the selected sandbox backend.                                    |
| `channels/readiness`                      | connect | `checks/channels.rs::readiness`                      | #500        | Reconnect or correct the named channel configuration.                              |
| `integrations/composio`                   | connect | `checks/integrations.rs::composio`                   | #500        | Reconnect the Composio integration or correct its endpoint configuration.          |
| `voice/readiness`                         | connect | `checks/voice.rs::readiness`                         | #500        | Configure an available voice provider or disable the unused voice feature.         |
| `search/readiness`                        | connect | `checks/search.rs::readiness`                        | #500        | Make the configured search provider reachable or correct its configuration.        |
| `wallet/readiness`                        | connect | `checks/wallet.rs::readiness`                        | #500        | Configure the required wallet safely or disable the wallet feature.                |
| `backend/reachable`                       | connect | `doctor/backend.rs::reachable`                       | #496        | Check the configured backend URL and network access.                               |
| `identity/session`                        | connect | `doctor/identity.rs::session`                        | #496        | Sign in again or refresh the session through the supported auth flow.              |
| `socket/reachable`                        | connect | `doctor/socket.rs::reachable`                        | #496        | Check backend socket availability and network policy.                              |
| `api/url`                                 | static  | `doctor/api.rs::url`                                 | #496        | Set a valid backend API URL for this profile.                                      |
| `inference/managed-catalog`               | connect | `doctor/inference.rs::managed_catalog`               | #496        | Check the managed inference catalog service and tenant access.                     |
| `inference/role-completion`               | live    | `checks/inference_live.rs::role_completion`          | #501        | Check the selected role's provider access and model configuration.                 |
| `embeddings/live-roundtrip`               | live    | `checks/embeddings_live.rs::roundtrip`               | #502        | Check embedding access and configured vector dimensions.                           |
| `memory/live-roundtrip`                   | live    | `checks/memory_live.rs::roundtrip`                   | #502        | Check the memory driver's write, fetch, and delete capabilities.                   |

`memory/live-roundtrip` uses disposable, uniquely scoped test content and must
delete it in a `finally`/drop path even when a later step fails. If cleanup
fails, the report names the cleanup failure without including test content.
No check may perform a write as part of `static` or `connect`.

## Implementation references

- Existing doctor being replaced: `crates/openhuman-core/src/platform/doctor/`
- Existing subsystem status projection: `crates/openhuman-core/src/core/subsystem/status.rs`
- Capability ids: `crates/openhuman-core/src/platform/about_app/`
- OpenClaw reference: [structured health-check contract](https://github.com/openclaw/openclaw/blob/main/docs/cli/doctor/health-contract.md), [lint mode and selectors](https://github.com/openclaw/openclaw/blob/main/docs/cli/doctor/lint.md), and [running doctor](https://github.com/openclaw/openclaw/blob/main/docs/cli/doctor/running.md)
