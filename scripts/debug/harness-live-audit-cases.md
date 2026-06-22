# Harness Live Audit Cases

These checks exercise the real harness through JSON-RPC with live model credentials. They are debug audits, not CI tests. They intentionally avoid printing prompt bodies, response bodies, tokens, or transcript contents.

Use an isolated spawned core when validating provider behavior without relying on a signed-in desktop session:

```bash
node scripts/debug/harness-subagent-rpc-audit.mjs --spawn-core --isolated-workspace --model gpt-4.1-mini --scenario all
```

`--isolated-workspace` writes a temporary direct-provider config using `OPENAI_API_KEY` or `OPENAI_KEY`, starts `openhuman-core`, and removes the temp workspace by default. Only pass `--keep-workspace` when you need to inspect artifacts locally.

## Cases

- `async-steer`: starts a durable async subagent with `spawn_subagent`, waits until the live registry shows it running, then sends `openhuman.subagent_steer` mid-run and verifies the steer was accepted.
- `parallel-research-code`: asks the orchestrator to call `spawn_parallel_agents` with two workers. One researches `https://example.com`; the other writes a small Python code snippet. The audit checks that the parent turn completed and at least two subagent transcripts changed.
- `all`: runs both cases against the same spawned core.

Useful focused commands:

```bash
node scripts/debug/harness-subagent-rpc-audit.mjs --spawn-core --isolated-workspace --model gpt-4.1-mini --scenario async-steer
node scripts/debug/harness-subagent-rpc-audit.mjs --spawn-core --isolated-workspace --model gpt-4.1-mini --scenario parallel-research-code
```
