# Embedding OpenHuman

`openhuman-embed` is a typed Rust library for running the OpenHuman core
in-process inside another product. It is the same core that ships in the
desktop app and the CLI, minus the Tauri shell and the RPC server: business
rules, agent turns, memory, tools and sandboxing, called directly from your
own binary.

## Adding the dependency

Use the default contributor feature set:

```toml
[dependencies]
openhuman-embed = { git = "https://github.com/tinyhumansai/openhuman", package = "openhuman-embed" }
```

Or select a narrower build:

```toml
[dependencies]
openhuman-embed = { git = "https://github.com/tinyhumansai/openhuman", package = "openhuman-embed", default-features = false, features = ["inference", "mcp"] }
```

Every feature on this crate forwards to the same-named feature on
`openhuman-core`: `default`, `http-server`, `inference`, `documents`, `hosting`, `modules`,
`voice`, `web3`, `runtime-node`, `media`, `flows`, `skills`, `mcp`,
`crash-reporting`, `channels`, `whatsapp-web`, `file-logging`,
`scheduler-gate`, and the capability features `tools-shell`,
`tools-fs-write`, `tools-exec`, `tools-system` and `composio` (see
[Capability features](#capability-features)). Two of them also gate
items on this crate's own surface: `mcp` gates `HttpHeader`,
`McpAuthConfig`, `McpServer`, `AgentSpec::mcp` and `HarnessBuilder::mcp`;
`skills` gates `AgentSpec::skills_dir` and `HarnessBuilder::skills_dir`.

Set your product's identity once at startup, before building any client:

```rust
use openhuman_embed::{set_product_identity, ProductIdentity};

if let Some(identity) = ProductIdentity::new("opencompany") {
    set_product_identity(identity);
}
```

## Capability features

Five default-ON features decide whether whole families of host-acting agent
tools exist in the binary. The desktop app ships all five. A product that
must not offer a family turns its feature off, and the tools are then never
constructed: they are absent from every agent's registry, from the
`tool_search` catalog, from delegation belts and from a flow's `oh:` tool
nodes. A host cannot re-enable them through config or RPC.

| Feature          | Tools it registers                                                                                                                                                                                  |
| ---------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tools-shell`    | `shell`                                                                                                                                                                                             |
| `tools-fs-write` | `file_write`, `edit`, `apply_patch`, `csv_export`, `curl`                                                                                                                                           |
| `tools-exec`     | `python_exec`, `run_tests`, `run_linter`, `git_operations`, `install_tool`, `detect_tools`                                                                                                          |
| `tools-system`   | `service_start`, `service_stop`, `service_restart`, `service_shutdown`, `service_install`, `service_uninstall`, `update_apply`, `proxy_config`, `daemon_host_prefs_set`, `workspace_update_persona` |
| `composio`       | `composio`, `composio_*`, the per-action `TOOLKIT_ACTION` tools built from connected integrations, and the flows `tool_call` backend for Composio slugs                                             |

Each gate covers only the tools listed. These stay in every build:

- the read-only neighbours: `file_read`, `grep`, `glob`, `list`,
  `read_diff`, `service_status`, `update_check`, `daemon_host_prefs_get` and
  `workspace_read_persona`;
- `node_exec` and `npm_exec`, which belong to `runtime-node`;
- `http_request` and `web_fetch`. They do not write to disk, and they follow
  `http_request.allowed_domains`.

`composio` does not remove the rest of the Composio domain. Its RPC
controllers, connection cache, catalogs, memory and task sources, and the
connected-integrations prompt section all stay in the build. None of them
lets the model act on a connected account, and they are too entangled with
flows, memory and session setup to split cleanly.

A build that lists its features explicitly leaves out every gate it does
not name. For example, a public persona bot with no host-acting tools at all:

```toml
[dependencies]
openhuman-embed = { git = "https://github.com/tinyhumansai/openhuman", package = "openhuman-embed", default-features = false, features = ["skills", "modules"] }
```

None of these features sheds a dependency. The tool types still compile:
most live in `tinytools-std`, and `shell.rs` holds helpers the exec tools
share. Nothing constructs them, though, so no code path reaches them.

### The runtime axis

The same families have `DomainGroup`s, so a runtime can also drop them with a
`DomainSet` without rebuilding:

| `DomainGroup` / `DomainSet` field | Tools                                                                                |
| --------------------------------- | ------------------------------------------------------------------------------------ |
| `Exec` / `exec`                   | `shell`, `run_tests`, `run_linter`, `git_operations`, `install_tool`, `detect_tools` |
| `Filesystem` / `filesystem`       | `file_write`, `edit`, `apply_patch`, `csv_export`, `curl`                            |
| `System` / `system`               | every `service_*`, `daemon_host_prefs_*` and `update_*` tool, plus `proxy_config`    |

`python_exec` stays in `Runtimes` and `workspace_update_persona` in `Config`,
the families they already belonged to. The `browser` and `browser_open`
tools are classified under `Modules`, since they exist only when the module
host is compiled in.

The presets:

- `full()` and `embedded()` keep all three groups on. These tools used to be
  `Platform`, which both presets enable, so both keep the tool surface they
  had.
- `harness()`, `kernel()` and `none()` turn all three off.

The runtime axis can only narrow what the build compiled in. To drop the
shell for one agent and keep it for another, set `exec: false` on that
agent's `AgentSpec::domains`.

## Two steps: a Runtime, then any number of Agents

The library API has two levels. First, one `Runtime` for the process:
features, background services, backend URL, the TinyHumans API key. Then
any number of independently configured agents on it, each with its own
provider, access tier, working directory, MCP servers and skills.

```rust,no_run
use openhuman_embed::{Access, AgentSpec, McpServer, Provider, Runtime, Workspace};

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let runtime = Runtime::builder()
    .workspace(Workspace::dir("/var/lib/my-product/openhuman"))
    .api_key("th_live_...")                    // the only credential in library mode
    .build()
    .await?;

let reviewer = runtime.agent(
    AgentSpec::new("reviewer")
        .system_prompt("You review pull requests and never edit files.")
        .access(Access::readonly())
        .skills_dir("./skills/review")         // copied into this agent's own skills root
        .action_dir("/srv/checkouts/pr-42"),
)?;

let fixer = runtime.agent(
    AgentSpec::new("fixer")
        .provider(Provider::openai_compatible("https://api.example/v1", "sk-...").model("gpt-5"))
        .access(Access::full())
        .mcp(McpServer::stdio("github", "gh-mcp", ["stdio"]))
        .action_dir("/srv/checkouts/pr-42"),
)?;

let review = reviewer.run("Summarise the risks in this change.").await?;
let fix = fixer
    .turn(format!("Address these findings:\n{}", review.reply))
    .send()
    .await?;
println!("{}", fix.reply);

// Continue a conversation with the same agent.
let again = fixer.turn("Now run the tests.").session(&fix.session_id).send().await?;
println!("{}", again.reply);
# Ok(())
# }
```

An agent owns its provider and model, its access tier and turn origin, its
`action_dir`, its MCP servers, its skills root
(`<workspace>/agents/<id>/skills/`), its system prompt, its tool scope and
sandbox mode, and a narrowed domain set or tool-group list. Every turn
dispatches under that agent's own context, so config, domain gating,
tool-group filtering and skill discovery all read that agent's settings and
never another agent's. Transcripts are keyed by agent id, and a turn resumes
only its own thread.

The runtime owns the workspace and credential store, the event bus, the
keyring, background services, the registered domain set (agents can only
narrow it, so enable `mcp` or `skills` at runtime build time if any agent
needs them), and the API key.

Layout under a runtime-owned root:

```text
<root>/config.toml, auth-profiles.json, core.token
<root>/workspace/session_db/, session_raw/<session_key>.<agent>[.g<n>].jsonl, agents/<agent>/skills/
<root>/agents/<agent>/action/                  default action_dir
```

## AgentSpec

`AgentSpec::new(id)` starts from the built-in orchestrator's definition
(prompt, tools, delegation) applied to a fresh id, except every registered
tool is visible by default (`ToolScopeSpec::Wildcard`) rather than the
desktop's own narrower set. Chain builder methods to override:

- `.system_prompt(text)` and `.definition(spec)` for the prompt, tool scope,
  disallowed tools, sandbox mode, iteration cap, temperature, display name
  and delegation blurb (`AgentDefinitionSpec`).
- `.provider(Provider)` / `.model(id)` for which model answers and where.
- `.access(Access)` for what the agent may do.
- `.tool_groups(ToolGroups)` and `.domains(DomainSet)` to narrow further,
  never wider than the runtime's own set.
- `.mcp(McpServer)` (repeatable) for MCP servers only this agent can call.
- `.skills_dir(path)` to copy skill bundles into this agent's own root.
- `.include_user_skills(bool)` to also let the agent see the operator's
  `~/.openhuman/skills` (off by default).
- `.action_dir(path)` for the agent's read/write root.
- `.trust(path, access)` to grant a directory outside `action_dir`.
- `.tools(factory)` to hand the agent real in-process tools built fresh per
  turn, rather than routing host callbacks through an MCP server.
- `.config(f)` as an escape hatch for config fields the spec does not model.
- `.lockdown()` to make the named tool belt a hard ceiling (see
  [Running a public agent](#running-a-public-agent)).

Agent ids must match `^[a-z0-9][a-z0-9_-]{0,63}$`; avoid the built-in ids
(`orchestrator`, `summarizer`, ...), which the runtime-wide delegation
catalog resolves to the shipped definitions.

## Access tiers

`Access` sets two things together on purpose: the autonomy tier that drives
the security policy, and the turn origin the approval gate checks. Setting
only the tier produces an agent whose shell, edit and exec tools all
silently refuse, which reads as a weak model rather than a missing scope.

- `Access::readonly()`: observe only, no writes, no shell, no network side
  effects. Safe default for an untrusted prompt.
- `Access::supervised()`: act, but park risky operations for a human
  decision. The approval gate stays on, so an unattended harness stalls here
  until an approval answers or the ten-minute TTL denies it.
- `Access::full()`: act autonomously, no approval pauses. Grants real shell
  and file access under `action_dir`; point it at a directory you are
  willing to have changed. Hard blocks still apply regardless of tier:
  credential stores (`~/.ssh`, `~/.gnupg`, `~/.aws`) and the workspace's own
  internal state stay off limits.
- `Access::public()`: untrusted public input. See
  [Running a public agent](#running-a-public-agent).

`.trust(path, access)` grants a directory outside the action root, and
`.allow_tool_install(bool)` opts into OS package installation, off in every
preset including `full()` because it reaches outside the action directory
that otherwise bounds the blast radius.

## Running a public agent

An agent that anyone on X or Telegram can put text in front of has to assume
prompt injection. It must only ever reach the tools its host named, including
through runs it starts. Four layers do this, from the build down to a single
tool call.

### 1. Leave whole families out of the build

The [capability features](#capability-features) decide whether `shell`, the
file-writing tools, the exec tools, the system tools and Composio exist in the
binary at all. A public bot with no host-acting tools compiles them out:

```toml
openhuman-embed = { git = "https://github.com/tinyhumansai/openhuman", package = "openhuman-embed", default-features = false, features = ["skills"] }
```

### 2. `AgentSpec::lockdown()`: deny by default

```rust,no_run
use openhuman_embed::{Access, AgentDefinitionSpec, AgentSpec, ToolScopeSpec};

let spec = AgentSpec::new("public-bot")
    .definition(
        AgentDefinitionSpec::new()
            .system_prompt("You answer questions about our product.")
            .tools(ToolScopeSpec::Named(vec!["web_fetch".into(), "memory".into()])),
    )
    .access(Access::public())
    .lockdown();
```

Lockdown needs a `ToolScopeSpec::Named` belt, and `Runtime::agent` refuses a
wildcard one. Then:

- The named belt becomes the session's **tool ceiling** (`[agent]
tool_ceiling` in core). Only those tools and the host's own (`.tools(…)`,
  `attach_tools`) are registered. Every other built-in and every `delegate_*`
  route the belt does not name do not exist for the agent's sessions.
- Every nested run inherits the ceiling, and nesting can only narrow it:
  - A sub-agent started by `spawn_subagent` or a delegate tool is intersected
    with it.
  - `run_workflow` refuses a workflow whose `skill.toml` belt or `SKILL.md`
    `allowed-tools` names a tool outside it, and builds the run with the
    same ceiling.
  - `cron_add` and `schedule` refuse a shell job unless `shell` is inside the
    ceiling. They refuse an agent job outright, because a scheduled run is
    built later from the stored job and cannot carry the ceiling.
  - `run_flow` and `resume_flow_run` refuse, because a flow's tool nodes
    dispatch outside the session.
  - `use_skill` cannot reach a pack tool that was never registered.
- MCP servers, the operator's user-scope skills and `install_tool` are off,
  whatever `.mcp(…)`, `.include_user_skills(true)` or the access tier said.
- Every tool group whose id or member tools the belt does not name is
  `GroupMode::Off`.
- The autonomy policy is enabled for the agent, so its access tier is
  enforced. With the policy off, which is the default, tiers are inert.

Lockdown is applied after the `.config(f)` escape hatch, so the escape hatch
cannot undo it. A refusal reaches the model as a tool error that contains
"tool ceiling".

The ceiling is a core mechanism, not an embed one. Any session built from a
config with `[agent] tool_ceiling` set gets the same behaviour.

### 3. `Access::public()`: untrusted input

`Access::public()` runs turns as an `AgentTurnOrigin::ExternalChannel`
(`channel: "public"`). It enables the autonomy policy at `ReadOnly` and caps
the session's tool permission at read-only. Every tool that writes, executes
or has an external effect is refused **immediately**, with a tool error the
model can read. Nothing parks waiting for an approval that no one on a public
channel could give. Read-only tools, including the host's own, keep working.

The tool-policy middleware reads the origin from the run context. It never
reads it from ambient task-local state.

### 4. Check the posture: `Agent::effective_tools`

```rust,no_run
# async fn check(agent: &openhuman_embed::Agent) -> Result<(), openhuman_embed::AgentError> {
let reachable = agent.effective_tools(None).await?;
assert!(!reachable.iter().any(|tool| tool == "shell" || tool == "file_write"));
# Ok(()) }
```

`effective_tools(origin)` builds the agent's session the way a turn does,
host tools included. It returns the tools a turn advertises, plus everything
reachable through a nested run (a sub-agent, a workflow, a `use_skill` pack,
a deferred `tool_search` hit). That set is bounded by the agent's registry
and its ceiling. Tools the policy refuses under `origin` are left out. `None`
means the origin the agent's `Access` states. The nested half is a deliberate
upper bound, so a posture test built on it fails safe.

## Provider: BYOK or managed

`Provider::inherit()` uses whatever inference the runtime or the machine is
already configured with: the account's managed backend, a local Ollama or
LM Studio, or a configured BYOK provider. `Provider::openai_compatible(url,
key)` points a turn at a specific OpenAI-compatible endpoint and bearer;
`/chat/completions` is appended to `url`, so pass the API root. `.model(id)`
pins the model id on either; it is advisory, since a model no configured
provider serves falls back rather than erroring.

An agent that names its own `Provider` never touches the runtime's API key.
One that names none inherits the runtime's default: managed inference on
the API key when one was supplied, or the machine's own configuration
otherwise.

## Workspace

`Workspace::Ephemeral` (the default) is a throwaway directory removed when
the harness or runtime drops; nothing here touches a real install, and
sessions do not survive the process. `Workspace::dir(path)` is a
caller-owned directory that persists across runs. `Workspace::Inherit`
reuses the machine's configured OpenHuman workspace, the same one the
desktop app and CLI use, resolved the usual way
(`OPENHUMAN_WORKSPACE`, `active_user.toml`, `~/.openhuman/...`); an inherited
workspace is the operator's, so the harness will not copy skills into it.

`Workspace::stateless()` keeps nothing durable on disk: every agent's
transcripts, turn journal, run status, goals and todos go through the
runtime's session store (`RuntimeBuilder::session_store`, required), a
`SessionStoreProvider` the host implements over its own database and scopes
by agent id. It is the shape for a cloud host serving many users from one
process; see the embed crate's README, "Conversations in a host store".

## Tool scopes and sandbox modes

`AgentDefinitionSpec::tools(ToolScopeSpec::Wildcard)` exposes every
registered tool (the default); `ToolScopeSpec::Named(vec![...])` restricts
an agent to exactly those tool names, dropping unknown ones at build time.
`.disallow_tools([...])` hides specific tools even when the scope would
include them.

`SandboxModeSpec` confines an agent's shell and file tools:

- `SandboxModeSpec::None` (default): only the access tier and path policy
  apply.
- `SandboxModeSpec::ReadOnly`: write and execute tools are filtered out
  entirely.
- `SandboxModeSpec::Sandboxed`: commands run under the platform jail or
  Docker backend.

## MCP servers

`McpServer::stdio(name, command, args)` launches a local subprocess
speaking newline-delimited JSON-RPC; `McpServer::http(name, endpoint)`
reaches a remote server over Streamable HTTP. Both support `.env(...)` or
`.auth(McpAuthConfig)`, `.allow_tools([...])` / `.deny_tools([...])` to
narrow which remote tools an agent sees (worth setting for a large server,
since every exposed tool costs prompt budget), `.timeout_secs(n)` and
`.description(text)`. Servers declared through `AgentSpec::mcp` are private
to that agent; other agents on the same runtime do not see them. Servers are
fixed at build time: there is no way to add one to a running harness.

## Skills

`AgentSpec::skills_dir(dir)` copies skill bundles into
`<workspace>/agents/<id>/skills/`, visible only to that agent. Bundles are
copied rather than symlinked because skill discovery rejects symlinked
bundles. `.include_user_skills(true)` additionally lets the agent discover
skills the operator installed under `~/.openhuman/skills`; the default
hides them, since an embedded agent should see what its host installed, not
what the machine's user did.

## Multi-turn sessions and progress streaming

`agent.turn(message)` returns a builder. `.session(id)` continues an
existing conversation; without it a fresh session id is minted and returned
on `TurnOutcome::session_id`. `.model(id)` and `.temperature(t)` override
those settings for one turn. `.cwd(dir)` roots that turn's filesystem and
shell tools somewhere other than the agent's configured `action_dir`.
`.route(Route)` sends one turn to a specific endpoint instead of the
account's configured route. `.origin(AgentTurnOrigin)` states the calling
authority explicitly, for a turn that is not a human at a terminal, such as
a workflow node or a scheduled job.

`.on_progress(sender)` streams tool calls, deltas and turn boundaries as
they happen. The core awaits every send, so the channel is real
backpressure: a receiver that stops draining stalls the turn.

```rust,no_run
# use openhuman_embed::Agent;
# async fn go(agent: &Agent) -> anyhow::Result<()> {
let (tx, mut rx) = tokio::sync::mpsc::channel(256);
let printer = tokio::spawn(async move {
    while let Some(progress) = rx.recv().await {
        eprintln!("[progress] {progress:?}");
    }
});
let outcome = agent.turn("go").on_progress(tx).send().await?;
let _ = tokio::time::timeout(std::time::Duration::from_secs(30), printer).await;
println!("{}", outcome.reply);
# Ok(()) }
```

`.seed(history)` replaces a session's history for one turn with the caller's
own rows (`(role, content)` pairs) rather than reading the transcript; only
a runtime-owned `Agent` can honour it, since it silently discards whatever
that session held, and it is meant to be paired with a session id the turn
is not sharing with turns that expect their history intact. `.meter(f)`
reports what a turn spent, including a turn that ran and then failed.

## Scheduling

OpenHuman's own cron can drive scheduled turns of a runtime's agents, with
their host tools, and run host code on a schedule. `runtime.cron()` is a typed
facade over the runtime's job store; jobs are named and `upsert` is idempotent
by that name, so a host can declare its schedule on every start:

```rust,no_run
# async fn demo(runtime: &openhuman_embed::Runtime) -> Result<(), Box<dyn std::error::Error>> {
use openhuman_embed::{JobSchedule, JobSpec};

let cron = runtime.cron();
// One turn of the runtime agent `teeny` every morning.
cron.upsert(
    JobSpec::agent("morning", "teeny", "Plan the day.", JobSchedule::Cron {
        expr: "0 8 * * *".into(),
        tz: Some("Europe/Berlin".into()),
    })
    .retries(0)          // exactly one attempt: the turn has side effects
    .single_flight(true) // skip (and record) a slot that comes due mid-run
)?;
// Host code on a schedule; its result is the run's recorded result.
runtime.on_system_job("digest", |_ctx| async { Ok(()) })?;
cron.upsert(JobSpec::system("digest", "digest", JobSchedule::Every { ms: 15 * 60_000 }))?;

let run = cron.run_now("morning").await?;   // same path as a scheduled run
println!("{} {}", run.success, run.output);
# Ok(()) }
```

- **Jobs fire only while the scheduler runs.** Build the runtime with
  `.services(ServiceSet { cron: true, ..ServiceSet::none() })`; `build()` then
  starts the selected services and dropping the runtime (and its agents) stops
  them. `Runtime::start_services` / `stop_services` give explicit control.
  Without the scheduler, jobs are stored and `run_now` still works.
- **An agent job runs as the agent.** When `agent_id` names an agent alive on
  the runtime, the turn uses its definition and system prompt, its
  `AgentSpec::tools` and attached tools, its provider and its own context
  (`agent::host_agents` resolves it for the core's cron and workflow drivers).
  Any other id resolves through the core's agent registries. The turn's origin
  is `TrustedAutomation { Cron }`, not the agent's own access origin. Agent jobs
  may not run more often than every five minutes.
- **A system job runs the host's handler.** `on_system_job(name, handler)`
  registers an async `Fn(SystemJobContext) -> Result<(), String>`; when the job
  comes due the scheduler still publishes `CronSystemJobDue` on the bus, then
  awaits the handler and records `ok` or `error` with its message.
- **Retries and overlap.** A failed run is retried `retries` times with
  backoff; unset keeps `reliability.scheduler_retries` (2), and `0` is one
  attempt. Two runs of one job never overlap: each due job is dispatched on
  its own task, so a long job no longer holds up the others, `run_now` is
  refused while a run is active, and a slot that comes due mid-run is skipped.
  With `single_flight` that skip is recorded as a `skipped` run.
- `cron.list()`, `cron.remove(name)` and `cron.runs(name, limit)` cover the
  rest. Shell jobs and workflow schedule triggers are not managed here.

## Channels

A runtime agent can answer a messaging channel. `runtime.channels()` starts a
channel listener bound to one agent, so every message on it is a turn of that
agent rather than of the orchestrator:

```rust,no_run
# async fn demo(runtime: &openhuman_embed::Runtime) -> Result<(), Box<dyn std::error::Error>> {
use openhuman_embed::{AgentSpec, TelegramChannelSpec};

let agent = runtime.agent(AgentSpec::new("teeny-chat").system_prompt("You are Teeny."))?;
let telegram = runtime.channels().telegram(
    TelegramChannelSpec::new("123456:bot-token", "teeny-chat")
        .allow_everyone()    // or .allowed_users(["alice", "4242"])
        .mention_only(true), // in groups, answer only when mentioned
)?;
// The bot answers until the listener is stopped or dropped.
# let _ = (agent, telegram);
# Ok(()) }
```

- **The agent answers as itself.** Each message runs with the agent's system
  prompt, tool scope, `AgentSpec::tools` belt and attachments, provider and
  context. History is per chat, kept by the channel the same way it is for the
  orchestrator. Typing indicators, streamed drafts and the reply work as for
  any channel. Call `telegram()` from inside the tokio runtime; it spawns the
  listener there.
- **Bind to an agent that exists.** `telegram()` refuses an id with no live
  agent on the runtime (`ChannelError::UnknownAgent`). If the agent is dropped
  while the bot is running, each message gets a short "not available" reply and
  an error is logged. The message is never handed to the orchestrator.
- **Turns are untrusted and read-only.** Anyone who can message the bot writes
  the turn's text, so it runs as `AgentTurnOrigin::ExternalChannel` and is
  capped at `PermissionLevel::ReadOnly`. A tool that writes, executes or has an
  external effect is withheld from the turn's tool list or refused at once with
  a tool error. It never waits on an approval, because the only person who
  could answer one is the sender. Give a public agent read-only host tools, and
  do anything with side effects outside the chat turn (a cron job, host code).
- **Under the hood** this is the core's channel runtime for that one bot,
  started under the runtime's context with
  `agent.channel_agents = { telegram = "<agent id>" }`. A core started from a
  config file can bind a channel the same way: any channel name works as the
  key, and the agent is resolved through `agent::host_agents`.

## Harness: the one-agent shorthand

`Harness` is a `Runtime` plus exactly one agent, built from one set of
inputs, for a host that only ever needs one:

```rust,no_run
use openhuman_embed::{Access, Harness, Provider, Workspace};

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let harness = Harness::builder()
    .provider(Provider::openai_compatible("https://api.example/v1", "sk-...").model("gpt-5"))
    .workspace(Workspace::Ephemeral)
    .access(Access::readonly())
    .build()
    .await?;

let first = harness.run("Summarize what you can see.").await?;
let second = harness
    .turn("Now list the risks.")
    .session(&first.session_id)
    .send()
    .await?;
println!("{}", second.reply);
# Ok(())
# }
```

`harness.runtime()` and `harness.agent()` hand out the underlying `Runtime`
and `Agent`, so a host that outgrows one agent adds more on the same
runtime rather than rebuilding.

## Connecting to the hosted backend

`openhuman-embed` alone installs no backend transport: agents, memory,
skills, tools and RPC all run, and every hosted-backend surface (billing,
integration tools, channel relay, cloud voice) answers with a typed
`BACKEND_UNAVAILABLE:` error. `openhuman-tinyhumans::RuntimeBuilder` mirrors
`openhuman_embed::RuntimeBuilder` method for method and installs the
SDK-backed transport on `build()`:

```rust
use openhuman_tinyhumans::{embed::Workspace, RuntimeBuilder};

let runtime = RuntimeBuilder::new()
    .workspace(Workspace::Ephemeral)
    .api_key("th_...")
    .build()
    .await?;
```

Hosts that boot the core themselves rather than through either builder (the
desktop shell, the TUI, the CLI, test fixtures) call `install` once before
the first backend-touching dispatch:

```rust
openhuman_tinyhumans::install(openhuman_tinyhumans::InstallOptions::default())?;
```

That call also registers the hosted RPC proxies (billing, team, referral,
announcements) into the core's controller registry. See
[One API key for everything](tinyhumans-api-key.md) for what a TinyHumans
API key unlocks once it is installed this way.

## Library mode has no user login

`RuntimeBuilder::api_key` installs a TinyHumans API key into the runtime's
credential store before the core boots. Managed inference then sends it as
`Authorization: Bearer <key>` to the TinyHumans endpoint, backend REST calls
send it as `x-api-key`, and the scheduler gate treats the runtime as signed
in. There is no `/auth/me` round trip, no session JWT, nothing to expire. An
agent that names its own `Provider` never touches the key at all.

## One runtime per process

The keyring master key, the RPC bearer, the global event bus and the
process's domain subscribers are process-scoped, so a second runtime would
silently share them while believing it had a separate workspace.
`RuntimeBuilder::build` returns `RuntimeError::AlreadyRunning` instead;
agents, not runtimes, are the unit of multiplicity within one process.

Build the tokio runtime yourself rather than using `#[tokio::main]`. A turn
is a large async state machine, and a sub-agent nesting inside it overflows
tokio's default 2 MiB worker stack:

```rust,no_run
use openhuman_core::core::runtime::{AGENT_WORKER_STACK_BYTES, MAX_BLOCKING_THREADS};

let runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .thread_stack_size(AGENT_WORKER_STACK_BYTES)
    .max_blocking_threads(MAX_BLOCKING_THREADS)
    .build()
    .expect("tokio runtime");
```

## A narrow build, measured

`docs/library-minimal-recipe.md` measures a headless recipe aimed at
100 to 1,000 live agents on a 2 GB RAM / 2 vCPU box:

```bash
cargo build --release \
  -p openhuman-embed \
  --no-default-features --features "skills,flows"
```

Stripped binary size: 51.0 MiB with no optional gates at all, about 60.4 MiB
with `skills,flows` enabled (the recipe above), against 115.9 MiB unstripped
with every gate on. See [Performance](performance.md) for how that build
size relates to per-agent memory and cold-start time when many agents share
one process.

## Examples to run

Against any OpenAI-compatible endpoint:

```bash
OPENHUMAN_EXAMPLE_BASE_URL=https://api.openai.com/v1 \
OPENHUMAN_EXAMPLE_API_KEY=sk-... \
OPENHUMAN_EXAMPLE_MODEL=gpt-5 \
  cargo run -p openhuman-embed --example run_turn -- "What can you see in this directory?"
```

Or against the machine's own configured inference, in its real workspace:

```bash
OPENHUMAN_EXAMPLE_INHERIT=1 cargo run -p openhuman-embed --example run_turn -- "Hello."
```

Two agents on one runtime, BYOK or managed:

```bash
OPENHUMAN_EXAMPLE_BASE_URL=https://api.openai.com/v1 \
OPENHUMAN_EXAMPLE_API_KEY=sk-... \
OPENHUMAN_EXAMPLE_MODEL=gpt-5 \
  cargo run -p openhuman-embed --example two_agents -- "Describe this directory."

OPENHUMAN_EXAMPLE_TINYHUMANS_API_KEY=th_... \
  cargo run -p openhuman-embed --example two_agents -- "Describe this directory."
```

`OPENHUMAN_EXAMPLE_BACKEND_URL` points non-inference backend calls
somewhere specific, and `OPENHUMAN_EXAMPLE_SKILLS_DIR` supplies skill
bundles. The repository-root `examples/embed_headless.rs` and
`examples/embed_kernel.rs` drive `CoreBuilder` from `openhuman-core`
directly, without this crate; run them with `cargo run --example
embed_headless`.

Tests worth reading alongside the examples: `tests/harness_embed.rs` proves
`Harness` runs a real turn against a mocked provider with nothing else
bound; `tests/runtime_agents.rs` runs three agents with different
providers, access tiers, skills, MCP servers and working directories on one
runtime; `tests/public_api.rs` pins the host-facing embedding contract at
compile time. Run them with:

```bash
cargo test -p openhuman-embed --features inference,mcp,skills
```

See also: [Jev](jev.md) for the decision model an embedded agent's tool
search can use, [Pluggable engines](engines.md) for the inference, memory
and search backends an agent chooses by config, and
[One API key for everything](tinyhumans-api-key.md) for what a TinyHumans
API key unlocks in library mode.
