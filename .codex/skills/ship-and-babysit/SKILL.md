---
name: ship-and-babysit
description: "End-to-end PR shipping workflow for tinyhumansai/openhuman: commit local changes, push to the user's fork, open or reuse a PR against main, then babysit CI and CodeRabbit feedback until the PR is green and clean. Use when the user asks to ship, open a PR, monitor CI, address review comments, or 'babysit' a branch."
---

# Ship and Babysit

Use this skill for `tinyhumansai/openhuman` when the user wants a branch shipped end to end:

- commit the local changes
- push the branch to the user's fork
- open or reuse a PR against `tinyhumansai/openhuman:main`
- monitor CI and review feedback
- address actionable review comments
- stop only when the PR is green and clean

## Preconditions

- Work from the repository root.
- Follow repo rules from `AGENTS.md`, including validation and PR checklist requirements.
- Assume `origin` is the user's writable fork and `upstream` points to `tinyhumansai/openhuman`.
- Never push directly to `main`.
- Never amend or rewrite commits that are already pushed unless the user explicitly asks for it.
- Never bypass hooks for breakage introduced by your own changes.

## Workflow

1. Inspect the branch before changing anything:
   - `git status --short`
   - `git diff --stat`
   - `git log --oneline --decorate -n 12`
2. Validate what changed and run the smallest meaningful local checks for the touched area.
3. Create a focused conventional commit message.
4. Push the current branch to the user's fork on `origin`.
5. Detect the fork owner:
   - `gh repo view --json owner,name`
6. Open or reuse a PR against `tinyhumansai/openhuman:main`:
   - check for an existing PR with `gh pr list --head <fork-owner>:<branch> --state all`
   - if missing, create one with `gh pr create`
7. Babysit the PR until it is healthy:
   - inspect CI with `gh pr checks <pr-number> --watch`
   - inspect review comments with:
     - `gh api repos/tinyhumansai/openhuman/pulls/<pr-number>/comments --paginate`
     - `gh api repos/tinyhumansai/openhuman/issues/<pr-number>/comments --paginate`
8. For actionable feedback:
   - make the smallest correct fix
   - rerun targeted validation
   - commit
   - push again
9. For incorrect or stale feedback:
   - reply with concrete technical reasoning
   - resolve or dismiss only when the platform supports it and the reasoning is explicit
10. Exit only when:
   - required checks are green
   - there are no unresolved actionable review comments left
   - the branch is pushed and the PR is up to date

## Useful Checks

- `pnpm typecheck`
- `pnpm lint`
- `pnpm format:check`
- `pnpm test:unit`
- `cargo check --manifest-path Cargo.toml`
- `cargo check --manifest-path app/src-tauri/Cargo.toml`
- `pnpm test:rust`

Prefer targeted test commands when the touched area is narrow, but do not claim validation passed if a command was not run.

## Notes

- Do not merge the PR unless the user explicitly asks.
- If CI or review surfaces reveal unrelated pre-existing breakage, call it out clearly and avoid masking it as fixed.
- If GitHub auth, remotes, or branch protection do not allow the workflow, report the exact blocker and stop at the first blocked step.

## Invocation Hints

- `Use $ship-and-babysit for this branch`
- `Ship this and babysit the PR`
- `Open the PR and stay on CI until it is green`
