# Loadable module boundary inventory

Status: migration incomplete. This inventory records the pinned source at
`ad89cddbca`; it does not assert that the host already uses only contracts.
The machine-readable dependency policy is
[`scripts/ci/module-boundaries.json`](../scripts/ci/module-boundaries.json).

Every owner below is the canonical upstream under
`https://github.com/tinyhumansai/`. Contract changes land there first. OpenHuman
host adapters switch only after compatible per-platform artifacts are released
and their digests pinned. Do not remove a dependency by removing its capability,
copying its implementation into a host, or accepting weaker isolation.

| Owner | Linked implementation packages | Contract | Host consumers | Required module work |
| --- | --- | --- | --- | --- |
| tinyjuice | tinyjuice | tinyjuice-bus | core tools, compression, CCR REPL | HTML extraction, query supplied artifacts against module CCR store, schema and tool declarations; retain turn-bound model callbacks |
| tinyconnectors | tinyconnectors, tinyconnectors-sync | tinyconnectors-bus | core integrations, credentials and triggers | Argument preparation, calendar defaults, task windows, structured provider errors, trigger archives; preserve sign-in/out reconciliation |
| tinymcp | tinymcp | tinymcp-bus | core MCP registry, supervisor, CLI stdio/HTTP server | Supervisor notifications, server protocol operations and callbacks for approved host tools/resources/prompts |
| tinychannels | tinychannels, tinychannels-runtime; runtime/crypto code in contract | tinychannels-bus | core channels and podcast email, TinyHumans host, CLI REPL | Move providers, signing and pairing out of contract; relay config, pairing, start/stop/send/status, inbound/status callbacks, bounded delivery/draining |
| tinyhosts | tinyhosts | tinyhosts-bus (missing at pin) | core hosting tools | Extract vocabulary and tool declarations; consume Execute/Providers after host validation/approval |
| tinywallet | tinywallet-crypto, tinywallet-web3, tinywallet-x402 via bus and direct imports | tinywallet-bus | core wallet/web3/x402 | Move behavioral re-exports out of bus; validation, transaction construction, quotes, swaps, payments, budgets, ledger; retain host custody/approval and confidential attestation |
| tinybox | tinybox-core, tinybox-jail, tinybox-docker, tinybox-host, tinybox-ssh | tinybox-bus (missing at pin) | core sandbox and security, desktop gateways | Discovery-only module needs handle-based sandbox/exec/streams/cancel/files/forward/status/close and shell-analysis facts; pre-core gateways use process loader |
| tinycomputer | tinycomputer-accessibility | tinycomputer-bus | core voice, paste/focus and permission checks | Permission, focus, paste, Globe listener vocabulary/operations; preserve native thread and target rules |
| tinyvoice | tinyvoice, cpal (also through accessibility probe) | tinyvoice-bus | core voice capture/hotkeys | Devices, recording/capture and hotkey lifecycle, bounded event batches; computer module permission decisions |
| tinyruntime | tinyruntime-pyserver | tinyruntime-bus | core Python worker, optional TinyJuice ML | Prepare/start/request/status/stop workers; module owns install, handshakes, retries/backoff and idle expiry |
| tinydocs / tinymemory | pdf-extract, calamine through tinymemory-integrations/documents-office | tinydocs-bus | core memory converter and file sources | Replace OfficeConverter with bus DocumentConverter; XLSX extraction plus existing PDF/DOCX/PPTX metadata/format coverage |
| tinysearch | none observed | tinysearch-bus | core module search proxy | Preserve existing bus adapter; enforce contract and host dependency graphs |

## Gate behavior

`pnpm rust:module-boundaries` checks the resolved normal and build dependency
closure of core, CLI, TUI and the excluded desktop workspace with all features.
Dev-only dependencies are excluded, including the test-only TinyWallet key
implementation. Package IDs, not dependency aliases, determine graph identity.
Target-specific dependencies are retained by Cargo metadata without a platform
filter. A new implementation package within an inventoried owner namespace
fails unless it is a registered contract.

Contracts are resolved separately in generated probe workspaces, once with
default features and once with every declared feature. Only serialization,
schema and error-derive closures are approved. This catches optional runtime or
implementation imports independently of host feature selection. Probes do not
compile native module code, and their manifests and lockfiles are kept under
`target/module-boundaries/` in the checkout.

Exceptions are exact package/scope pairs with reasons. Host exceptions still
traverse their descendants. Contract exceptions stop at the first unsafe edge:
its implementation closure is not approved, and that edge must be removed with
the owning migration. The channel and wallet contract migrations remain
explicitly outstanding. An unused exception fails, prompting removal instead
of leaving future regressions silently exempted.

`pnpm rust:module-boundaries:complete` also fails while any exception or pending
contract exists. Passing the transitional gate means no unlisted boundary
violation was observed; it does not mean this plan is complete.

## Remaining acceptance work

The shared `openhuman_rpc::embed::modules::ModuleClient` takes explicit runtime
configuration and uses the same process-wide loader before or after core
startup. It has no linked fallback. Resolution reports and client errors use a
closed reason vocabulary, registry metadata and a bounded deduplication cache;
raw loader paths and remote fault prose are absent from these Sentry events.
Existing domain adapters still need to migrate through this reporting path.

Upstream operations and artifacts, gateway integration, frozen tool restoration,
lifecycle regressions and coverage of all migrated adapter failures are still
required. No capability migration has landed in this change. The minimal/default/product/individual-feature build matrix,
platform compilation, runtime bus fixtures and shed measurements remain
separate acceptance checks. Dependency counts alone do not demonstrate a
build-time or binary-size improvement.

## Upstream migration work

Owner changes are independently reviewable; host dependencies and artifact pins
remain unchanged until compatible upstream releases are available.

| Change | Canonical PR | Local verification |
| --- | --- | --- |
| Async HTML extraction seam, deadlines and generic tool-metadata sanitization in TinyTools | [tinytools#60](https://github.com/tinyhumansai/tinytools/pull/60) | 1,183 workspace tests and six doctests; clippy/build; independent sanitizer review accepted; all 113 source files at least 90% coverage |
| TinyJuice typed CCR/content queries, HTML extraction, pure schemas and declarations | [tinyjuice#59](https://github.com/tinyhumansai/tinyjuice/pull/59) | 737 tests; dynamic artifact E2E; module input/limit guards |
| Complete TinyDocs Markdown conversion for memory ingestion | [tinydocs#31](https://github.com/tinyhumansai/tinydocs/pull/31) | 183 tests; dynamic artifact E2E; per-file coverage at least 90% |
| Minimal TinyHosts vocabulary and recorded tool declarations | [tinyhosts#21](https://github.com/tinyhumansai/tinyhosts/pull/21) | 195 tests; dynamic Execute/Providers fixture; pure contract audit; per-file coverage at least 90% |
| TinyComputer native permissions, confidential focus/paste and opaque Globe listeners | [tinycomputer#87](https://github.com/tinyhumansai/tinycomputer/pull/87) | Full workspace checks; 88-member dynamic artifact verification; pure contract audit; native bridge 100% line coverage |
| TinyConnectors argument preparation, task filtering, structured provider errors and leased archives | [tinyconnectors#46](https://github.com/tinyhumansai/tinyconnectors/pull/46) | 423 tests; dynamic artifact calls; user archive lifecycle fixtures; pure contract audit; per-file coverage at least 90% |
| TinyChannels contract vocabulary separated from provider, relay, pairing and runtime behavior | [tinychannels#56](https://github.com/tinyhumansai/tinychannels/pull/56) | 1,174 default and 1,180 all-feature tests; independent contract audit and relocation review accepted; bus files at least 98.65% coverage; legacy provider/worker coverage gaps disclosed |
| TinyWallet contract dependency cut and module-side address validation | [tinywallet#56](https://github.com/tinyhumansai/tinywallet/pull/56) | 735 tests; four-chain signing fixtures; dynamic artifact E2E; pure contract audit; all 94 source files at least 90% coverage |
| TinyVoice device enumeration, reserved recording/continuous capture handles, bounded output and acknowledged shutdown | [tinyvoice#23](https://github.com/tinyhumansai/tinyvoice/pull/23) | 188 tests and one doctest; 26-member compiled artifact verification; pure contract audit; covered files at least 90%, with the existing physical-device exclusion; independent lifecycle review accepted |
| TinyMCP supervisor observations, module server callbacks, replayable replies and acknowledged cancellation | [tinymcp#54](https://github.com/tinyhumansai/tinymcp/pull/54) | 1,480 all-feature tests; dynamic 40-member artifact verification; pure dependency audit; all 83 source files at least 90% coverage; independent lifecycle review accepted |

The TinyDocs and TinyJuice operations need new published module artifacts.
TinyHosts keeps existing member arities and wire forms. No local build digest
has been used as a release pin, and these PRs do not yet remove any host exception.

TinyChannels preserves its serialized vocabulary and moves behavioral APIs to
implementation crates, using compatibility extension traits where needed. Its
relay/pairing/delivery bus operations and OpenHuman adapters remain outstanding.
