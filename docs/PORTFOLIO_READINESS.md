# Portfolio Readiness

This note is for engineering review and portfolio handoff. It does not change
the product surface or marketing README.

## What This Repo Demonstrates

- A large local-first desktop AI product with TypeScript UI, Rust/Tauri shell,
  and a Python/Rust-adjacent core surface.
- Integration-heavy product architecture: local memory, account connectors,
  desktop shell commands, native tools, and testable app services.
- Real validation breadth: TypeScript compile, ESLint, Vitest, and Rust
  mock-backed tests all run locally.

## Current Validation Evidence

Run from the repo root:

```bash
pnpm run typecheck
pnpm run lint
pnpm run test
pnpm run test:rust
git diff --check
```

Latest portfolio-readiness run:

- `pnpm run typecheck`: passed.
- `pnpm run lint`: passed with 33 warnings. The remaining warning family is
  React compiler `set-state-in-effect`.
- `pnpm run test`: passed, 214 test files, 1 skipped; 1,977 tests passed, 3
  skipped.
- `pnpm run test:rust`: passed through `scripts/test-rust-with-mock.sh`.
- `git diff --check`: passed.

## Cleanup Performed

- Removed a stale `eslint-disable-next-line no-console` directive in
  `app/src/services/meetCallService.ts`.
- Replaced an `any` cast in `app/src/lib/mcp/transport.ts` with a typed MCP
  event-handler map.
- Moved mnemonic/recovery-phrase mode resets into the explicit mode switch
  handlers.
- Removed dead sidebar label reset state now that conversation labels use a
  fixed tab model.
- Recorded validation evidence in `CODEX_WORKPAD.md`.

## Remaining Presentation Debt

- The lint output is not presentation-clean yet because 33 React compiler
  warnings remain.
- Most warnings are synchronous state updates inside effects. Some may be
  harmless legacy patterns, but they should be either refactored or explicitly
  accepted as a policy before this repo is used as a polished flagship example.
- Vitest currently emits repeated Node `localStorage is not available` warnings;
  tests pass, but the environment warning should be silenced or documented.

## Public Claim Boundary

Safe to claim:

- The repository has broad local validation across TypeScript, lint, JS tests,
  and Rust tests.
- The current cleanup reduced generic lint noise without changing product
  behavior.

Do not claim yet:

- "Lint-clean" or "warning-free."
- Full UI runtime readiness across every Tauri/desktop flow.
- That the remaining React compiler warnings have been reviewed and accepted.

## Next Slice

Create a narrow lint-policy slice:

1. Pick one warning family, starting with `react-hooks/set-state-in-effect`.
2. Classify warnings into real refactors vs accepted legacy patterns.
3. Fix the highest-risk components first.
4. Keep `pnpm run typecheck`, `pnpm run lint`, and the relevant Vitest tests
   green after each group.
