# Hosted Medulla Subconscious Implementation Plan

> Approved design: `docs/specs/2026-07-24-hosted-medulla-subconscious-design.md`

## Task 1: Remove the retired Rust steering read contract

**Files**

- Modify `src/openhuman/orchestration/cloud.rs`
- Modify `src/openhuman/orchestration/sync.rs`
- Modify `src/openhuman/orchestration/schemas.rs`
- Modify Rust tests colocated with those modules or
  `tests/orchestration_hosted_client.rs`

**TDD**

1. Add a regression assertion that a hosted sync pass never requests
   `/orchestration/v1/steering` and that status JSON has no `steering` member.
2. Run the focused test and confirm it fails against the stale contract.
3. Remove `STEERING_PATH`, `fetch_steering`, `STEERING_KEY`, steering cache
   writes/reads, `SteeringSummary`, and the status field/assembly.
4. Update module comments so the documented read surface is sessions/messages
   only.
5. Run:

   ```bash
   ulimit -n 4096
   cargo test --test orchestration_hosted_client -- --test-threads=1
   cargo test --lib orchestration:: -- --test-threads=1
   cargo fmt --check
   ```

6. Commit:

   ```bash
   atomic-commit "fix(orchestration): stop polling retired steering route" -- \
     src/openhuman/orchestration/cloud.rs \
     src/openhuman/orchestration/sync.rs \
     src/openhuman/orchestration/schemas.rs \
     tests/orchestration_hosted_client.rs
   ```

## Task 2: Remove steering-only renderer state and controls

**Files**

- Modify `app/src/lib/orchestration/orchestrationClient.ts`
- Modify `app/src/components/intelligence/TinyPlaceOrchestrationTab.tsx`
- Modify `app/src/components/intelligence/OrchestrationSidebar.tsx`
- Modify `app/src/components/intelligence/OrchestrationFocusPane.tsx`
- Modify `app/src/components/orchestration/AgentChatPanel.tsx`
- Modify their focused `*.test.tsx` files
- Modify `app/src/lib/i18n/*.ts`

**TDD**

1. Change focused tests to assert that neither orchestration presentation
   renders a steering chip/header nor exposes a manual review action.
2. Run the focused Vitest set and confirm the stale props/controls fail.
3. Remove `OrchestrationSteering`, the `steering` status member, steering props,
   local review state/callbacks, the `subconsciousTrigger` imports used only by
   steering controls, and steering-only translations.
4. Preserve pinned chat/message rendering and `lastTickAt`, which remain
   orchestration health/history fields.
5. Run:

   ```bash
   pnpm debug unit \
     app/src/components/intelligence/TinyPlaceOrchestrationTab.test.tsx \
     app/src/components/intelligence/OrchestrationSidebar.test.tsx \
     app/src/components/intelligence/OrchestrationFocusPane.test.tsx \
     app/src/components/orchestration/__tests__/AgentChatPanel.test.tsx
   pnpm typecheck
   pnpm format:check
   ```

6. Commit every independently validated renderer step if the test and i18n
   scopes separate cleanly; otherwise commit the cohesive renderer contract:

   ```bash
   atomic-commit "fix(orchestration): remove retired steering UI" -- \
     app/src/lib/orchestration/orchestrationClient.ts \
     app/src/components/intelligence/TinyPlaceOrchestrationTab.tsx \
     app/src/components/intelligence/TinyPlaceOrchestrationTab.test.tsx \
     app/src/components/intelligence/OrchestrationSidebar.tsx \
     app/src/components/intelligence/OrchestrationSidebar.test.tsx \
     app/src/components/intelligence/OrchestrationFocusPane.tsx \
     app/src/components/intelligence/OrchestrationFocusPane.test.tsx \
     app/src/components/orchestration/AgentChatPanel.tsx \
     app/src/components/orchestration/__tests__/AgentChatPanel.test.tsx \
     app/src/lib/i18n/ar.ts app/src/lib/i18n/bn.ts app/src/lib/i18n/de.ts \
     app/src/lib/i18n/en.ts app/src/lib/i18n/es.ts app/src/lib/i18n/fr.ts \
     app/src/lib/i18n/hi.ts app/src/lib/i18n/id.ts app/src/lib/i18n/it.ts \
     app/src/lib/i18n/ko.ts app/src/lib/i18n/pl.ts app/src/lib/i18n/pt.ts \
     app/src/lib/i18n/ru.ts app/src/lib/i18n/zh-CN.ts
   ```

## Task 3: Generalize the backend Medulla tool-loop client

**Files**

- Modify `src/openhuman/orchestration/medulla.rs`
- Modify `src/openhuman/orchestration/schemas.rs` only if the existing public
  orchestration wrapper needs an explicit compatibility adapter

**TDD**

1. Add tests for a reusable run request that:
   - includes `flavor: "openhuman"`;
   - advertises exactly the caller-supplied tools;
   - distinguishes safe pre-submission failures from post-submission failures;
   - preserves the existing `orchestration.run` request shape when flavor is
     omitted.
2. Run focused tests and confirm failure.
3. Introduce caller options for flavor/tool list and a typed error carrying a
   fallback-safety phase (`Preflight` or `Submitted`).
4. Keep the current public `run` wrapper and RPC behavior source-compatible.
5. Ensure logging contains counts/phase only, never input, arguments, or backend
   bodies.
6. Run:

   ```bash
   ulimit -n 4096
   cargo test --lib openhuman::orchestration::medulla::tests -- --test-threads=1
   cargo fmt --check
   ```

7. Commit:

   ```bash
   atomic-commit "refactor(medulla): support scoped hosted tool loops" -- \
     src/openhuman/orchestration/medulla.rs \
     src/openhuman/orchestration/schemas.rs
   ```

## Task 4: Add the hosted memory-subconscious reflection adapter

**Files**

- Add `src/openhuman/subconscious/hosted.rs`
- Modify `src/openhuman/subconscious/mod.rs`
- Modify `src/openhuman/subconscious/profiles/memory.rs`
- Modify `src/openhuman/subconscious/instance.rs`
- Modify `src/openhuman/subconscious/instance_tests.rs`
- Modify `src/openhuman/subconscious/profiles/memory_tests.rs`

**TDD**

1. Add tests proving:
   - hosted is selected for an eligible configuration;
   - the advertised tools are exactly `notify_user`, `update_task`,
     `goals_list`, `goals_add`, `goals_edit`, and `spawn_subagent`;
   - the tool loop runs inside the expected trusted-automation origin, with
     tainted observations mapped to `SubconsciousTainted`;
   - sub-agent calls receive a root parent and are forced blocking;
   - a successful hosted terminal result returns `Reflection::Acted`;
   - a preflight failure invokes the local reflector exactly once;
   - a submitted failure never invokes local fallback.
2. Run the focused tests and confirm failure.
3. Build the explicit tool catalogue from the domain-owned constructors.
4. Add a hosted reflector that reuses the generalized Medulla client with the
   `openhuman` flavor and the existing subconscious decision guidance.
5. Wrap the hosted tool loop in `with_origin` and `with_root_parent`.
6. Add a selection layer to `MemoryProfile::reflect`; retain the current
   `run_agent` implementation as the fallback.
7. Ensure reflection success is the only changed-window path that reaches the
   existing graph commit node.
8. Run:

   ```bash
   ulimit -n 4096
   cargo test --lib openhuman::subconscious:: -- --test-threads=1
   cargo fmt --check
   ```

9. Commit:

   ```bash
   atomic-commit "feat(subconscious): prefer hosted Medulla reflection" -- \
     src/openhuman/subconscious/hosted.rs \
     src/openhuman/subconscious/mod.rs \
     src/openhuman/subconscious/profiles/memory.rs \
     src/openhuman/subconscious/instance.rs \
     src/openhuman/subconscious/instance_tests.rs \
     src/openhuman/subconscious/profiles/memory_tests.rs
   ```

## Task 5: Migrate subconscious engine configuration

**Files**

- Modify `src/openhuman/config/schema/subconscious.rs`
- Modify `src/openhuman/config/schema/types.rs`
- Modify `src/openhuman/subconscious/instance.rs`
- Modify configuration/schema tests
- Modify `.env.example` or docs only where the setting is documented

**TDD**

1. Add serde/default tests:
   - omitted engine becomes `auto`;
   - `auto` and `local` round-trip;
   - legacy `medulla` deserializes to `auto`;
   - `local` bypasses every hosted preflight.
2. Confirm the tests fail.
3. Replace the old local-child selection with `Auto | Local`.
4. Delete `run_tick_medulla` and its subconscious-only tests. Keep the separate
   `medulla_local` domain and feature intact.
5. Update comments/docs that still describe `subconscious.engine = "medulla"`
   as a local child.
6. Run:

   ```bash
   ulimit -n 4096
   cargo test --lib openhuman::config::schema::subconscious::tests -- --test-threads=1
   cargo test --lib openhuman::subconscious:: -- --test-threads=1
   cargo fmt --check
   ```

7. Commit:

   ```bash
   atomic-commit "refactor(subconscious): default to hosted with local fallback" -- \
     src/openhuman/config/schema/subconscious.rs \
     src/openhuman/config/schema/types.rs \
     src/openhuman/subconscious/instance.rs \
     src/openhuman/subconscious/instance_tests.rs
   ```

## Task 6: Update domain documentation

**Files**

- Modify `src/openhuman/subconscious/README.md`
- Modify `src/openhuman/orchestration/mod.rs`
- Modify `docs/TEST-COVERAGE-MATRIX.md`
- Modify any directly stale architecture page discovered by targeted search

**Steps**

1. Document the hosted-preferred reflection boundary, safe fallback phases,
   retained local ownership, and retired steering route.
2. Remove statements that TinyPlace steering or a local `medulla-serve` child
   drives subconscious ticks.
3. Run source scans for stale live claims:

   ```bash
   rg -n "orchestration/v1/steering|STEERING_DIRECTIVE|run_tick_medulla|subconscious.engine = .medulla." \
     src/openhuman app/src gitbooks/developing docs/TEST-COVERAGE-MATRIX.md
   ```

4. Commit:

   ```bash
   atomic-commit "docs(subconscious): describe hosted Medulla reflection" -- \
     src/openhuman/subconscious/README.md \
     src/openhuman/orchestration/mod.rs \
     docs/TEST-COVERAGE-MATRIX.md
   ```

## Task 7: Full verification and review

1. Run changed-area frontend checks:

   ```bash
   pnpm typecheck
   pnpm lint
   pnpm format:check
   pnpm test -- --run
   ```

2. Run changed-area Rust checks with the host limits:

   ```bash
   ulimit -n 4096
   GGML_NATIVE=OFF cargo check --manifest-path Cargo.toml
   cargo test --lib openhuman::orchestration:: -- --test-threads=1
   cargo test --lib openhuman::subconscious:: -- --test-threads=1
   cargo test --test orchestration_hosted_client -- --test-threads=1
   ```

3. Because the shipped Tauri crate forwards `medulla-local`, verify both Cargo
   worlds and feature forwarding:

   ```bash
   GGML_NATIVE=OFF cargo check --manifest-path app/src-tauri/Cargo.toml
   node scripts/ci/check-feature-forwarding.mjs
   ```

4. Inspect `git diff --check`, `git status --short`, and the commit sequence.
5. Perform a code review against the approved spec.
6. Independently rerun the focused proof commands before claiming completion.
