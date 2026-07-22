# TinyAgents tool-model boundary design (2026-07-22)

**Status:** proposed; implementation of the public contract changes requires
review. This resolves the decision questions in
`tinyagents-migration-plan-2026-07-22.md` WP-4 without starting a high-blast-
radius type migration.

## Decision

Keep two tool traits and make `SharedToolAdapter` a supported boundary:

- TinyAgents owns the framework-facing `tinyagents::harness::tool::Tool<State>`
  contract, harness execution, timeouts, retries, workspace descriptors, and
  portable builtin tools.
- OpenHuman owns its product-facing `openhuman::tools::Tool` contract, tool
  registry, channel/RPC visibility, approval and command classification,
  generated-tool context, UI metadata, and MCP/runtime result shape.
- Generic tools that move to TinyAgents implement the crate trait natively.
  Product tools stay on the OpenHuman trait and cross the harness boundary via
  `SharedToolAdapter`. There is no goal to make all product tools implement the
  crate trait directly.

This is a permanent adapter, not a temporary compatibility layer. It prevents
product policy from becoming public SDK API while still allowing the host
implementation tree to shrink family by family.

## Why trait unification is rejected

The traits overlap at name, description, schema, execution, display metadata,
and timeout policy, but their remaining responsibilities are intentionally
different:

| OpenHuman-only concern | Why it stays host-side |
| --- | --- |
| `PermissionLevel` and `permission_level_with_args` | Ordered channel permissions and argument-sensitive command classification are product security policy. |
| `ToolScope` and `ToolCategory` | They control OpenHuman agent definitions, explicit runtime/RPC exposure, integrations subagents, and legacy wire labels. |
| `external_effect_with_args` | It drives the interactive approval gate using product semantics. |
| `GeneratedToolRuntimeContext` | It binds generated tools to OpenHuman policy and runtime state. |
| `ToolCallOptions::prefer_markdown` | It is an OpenHuman result-rendering optimization tied to the host result type. |

TinyAgents' `ToolPolicy` remains useful as a portable, serializable declaration,
but it cannot replace per-call host authorization. The adapter may project a
conservative static policy for registry introspection; OpenHuman middleware must
still evaluate the original tool and arguments immediately before execution.

## Result types stay in OpenHuman

`ToolResult` and `ToolContent` remain defined in the inert, dependency-light
`skills::types` carve-out and continue to be re-exported from `tools`. They are
used by MCP, the Node runtime, product RPC, and every host tool implementation.
Moving them would turn TinyAgents into the owner of an OpenHuman wire and UI
contract and would create a coordinated migration across hundreds of consumer
files.

The adapter should stop discarding structure. Its conversion contract is:

- TinyAgents `ToolResult.content` receives `output_for_llm(true)`.
- TinyAgents `ToolResult.error` mirrors `is_error`.
- TinyAgents `ToolResult.raw` receives the serialized OpenHuman `ToolResult`,
  including content blocks and `markdownFormatted`.
- Conversion failure is treated as an adapter error, not silently replaced by
  `None`.

This uses the crate's existing `raw: Option<serde_json::Value>` escape hatch and
does not change either public result type. A future crate-native structured-
content API can be evaluated independently if more than one host needs it.

## Scope semantics

Keep all three OpenHuman variants and enforce them at every entry point:

| Scope | Autonomous agent | Explicit CLI/RPC/runtime |
| --- | --- | --- |
| `All` | allow | allow |
| `AgentOnly` | allow | deny |
| `CliRpcOnly` | deny | allow |

`AgentOnly` is not dead: `agent_memory`, `subconscious`, and `rhai_workflows`
already declare it. The current agent builders and TinyAgents middleware deny
`CliRpcOnly`, but the generic Node/RPC execution bridge does not yet reject
`AgentOnly`. The first implementation slice must centralize the matrix above
in a small host helper and test both execution surfaces. Scope does not belong
in the TinyAgents trait because it describes OpenHuman entry points, not SDK
execution safety.

## Security boundary

TinyAgents owns execution mechanics; OpenHuman remains the policy decision
point. In particular:

- `SecurityPolicy::classify_command`, `gate_decision`, approval requests,
  credential/system-directory denial, and workspace-internal path denial stay
  in OpenHuman.
- Crate-native filesystem/network tools depend only on crate abstractions such
  as `ToolAccess` and `WorkspaceDescriptor`.
- The host injects allowed roots and adapters/hooks. A crate declaration may
  narrow access, but it must never widen a host decision.
- Argument-sensitive authorization runs after argument-recovery middleware and
  before the tool body. A static `ToolPolicy` projection is not authorization.

## Migration slices

1. Add and test the centralized `ToolScope` entry-point matrix; close the
   existing `AgentOnly` explicit-execution gap.
2. Preserve the full OpenHuman result in TinyAgents `ToolResult.raw`; add
   mixed text/JSON, markdown, and error conversion tests.
3. Move generic time tools to crate-native registrations as the adapter-free
   pilot; retain their host-facing wrappers only where RPC compatibility needs
   them.
4. Design the host-injected access/classification hook before moving filesystem
   or command tools. No filesystem family moves until fail-closed tests cover
   trusted roots, workspace-internal paths, per-call classification, and
   approval.
5. Port generic filesystem and network families incrementally. Delete a host
   implementation only after its crate-native tool passes the host security and
   wire-compatibility suites.

## Acceptance gates

- No new product vocabulary or backend-specific policy in TinyAgents.
- Disabled-feature builds keep `skills::types` available.
- Every tool is denied by default at an entry point whose scope does not allow
  it.
- Host authorization receives the recovered final arguments and cannot be
  bypassed by a crate policy declaration.
- Structured content and markdown survive the adapter in `raw` while the
  model-facing string remains stable.
- `SharedToolAdapter` is deleted only if no OpenHuman-native tool remains; its
  existence is not migration debt by itself.

