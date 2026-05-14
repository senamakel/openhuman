---
description: Reset the current branch to main, fetch upstream, and update submodules.
allowed-tools: Bash
---

Run the following steps in order. Stop and surface the error if any step fails — do not paper over a failure.

```bash
set -e
git fetch upstream --prune --tags
git checkout main
git reset --hard upstream/main
git submodule update --init --recursive
git status
```

Then report in one line: the new HEAD SHA of `main` and whether submodules changed.
