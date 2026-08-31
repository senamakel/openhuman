---
name: flow-authoring
description: The tinyflows authoring reference — expression and jq syntax, node configuration for memory/dedup/trigger nodes, per-node error handling, how large a graph should be, and how to read a dry run honestly. Read a page before configuring the thing it covers.
metadata:
  version: "1.0.0"
  author: OpenHuman
  tags:
    - flows
    - workflows
    - authoring
    - reference
allowed-tools:
  - read_workflow_resource
  - get_node_kind_contract
  - list_node_kinds
---

# Authoring a tinyflows workflow

This is a **reference manual, not a procedure**. It holds the exact rules that
are too long to keep in a system prompt and too precise to reconstruct from
memory: an expression convention you half-remember produces a graph that
validates and then does the wrong thing at run time.

Read the one page that covers what you are about to configure.

| page | read it before you |
| --- | --- |
| `references/expressions.md` | write any `=` expression or jq filter, or attach a produced file to an outbound action |
| `references/node-config.md` | configure a `memory`, `dedup` or `trigger` node, or set per-node error handling |
| `references/graph-shape.md` | decide how many nodes a request actually needs |
| `references/dry-run.md` | report what a dry run did and did not prove |

Fetch one with:

```
read_workflow_resource { skill_id: "flow-authoring", relative_path: "references/expressions.md" }
```

## What is deliberately not here

**Per-kind configuration.** `get_node_kind_contract { kind }` returns a node
kind's config fields, ports, a worked example and its gotchas, and it is
generated from the same catalog the validator enforces — so it cannot go stale
the way this text can. Where the two disagree, the contract tool is right.
These pages cover the rules that span kinds, which is why they have nowhere
generated to live.

**The rules you must not break.** Propose rather than persist, ask before a
real run, ground every slug — those stay in the system prompt, because a rule
that only binds once someone chooses to read it is not a rule.
