# Jev routing live proof

This example connects the merged TinyHiveMind Jev-first router to one shared
OpenHuman `Harness`. It is deliberately a dev-only proof adapter: none of its
dependencies enter the shipped OpenHuman graph.

The run records the exact message, candidate snapshot, System One questions and
typed answers, accepted routing plan, OpenHuman seats actually run, replies,
latency, tokens, and cost as formatted JSON. The TypeSafe credential is read
only by `tinyjevclient` from `TYPESAFE_API_KEY`; provider credentials are never
included in the report.

## Run

Supply an explicit calibrated routing policy. There are no built-in threshold
defaults:

```sh
export TYPESAFE_API_KEY=...
export OPENHUMAN_EXAMPLE_BASE_URL=https://provider.example/v1
export OPENHUMAN_EXAMPLE_API_KEY=...
export OPENHUMAN_EXAMPLE_MODEL=model-id
export JEV_ROUTING_POLICY_JSON='{
  "minimum_confidence": 650000,
  "high_impact_minimum_confidence": 800000,
  "collaboration_threshold": 650000,
  "contribution_threshold": 600000,
  "clarification_threshold": 700000,
  "high_impact_threshold": 700000,
  "round_width": 4,
  "choice_option_limit": 16
}'

# Optional and sensitive: write full evidence once to a new private file.
export JEV_PROOF_SECURE_OUTPUT=./jev-proof-private.json

cargo run --example jev_routing_live_proof -- \
  "Review this launch for engineering, financial, and compliance risk."
```

The numbers above only demonstrate the input shape. They are not recommended
thresholds; use values frozen from the labeled routing corpus.

The proof refuses a round wider than 25 seats. A normal desk makes one Jev
request. Large candidate sets may make the bounded screening-plus-Choice pair
defined by TinyHiveMind.

Stdout always redacts the user message, thread context, provider answers,
reasoning prompt/reply, and seat replies. `JEV_PROOF_SECURE_OUTPUT` writes the
full content-bearing evidence chain to a new file instead; it refuses to
overwrite an existing path and uses mode `0600` on Unix. Treat that file as
sensitive user data.

## Files

| File | Purpose |
| --- | --- |
| `main.rs` | Builds the request and shared Harness, runs selected seats, and prints the audit record. |
| `reasoning.rs` | Runs and records the single OpenHuman reasoning-router escalation. |
| `transport.rs` | Implements `SystemOneTransport` with `tinyjevclient` and records raw wire traces. |
