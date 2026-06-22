#!/usr/bin/env node
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { once } from "node:events";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { homedir, tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const DEFAULT_RPC_URL = "http://127.0.0.1:7788/rpc";
const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));

function usage() {
  return `Usage: node scripts/debug/harness-subagent-rpc-audit.mjs [options]

Runs a live JSON-RPC harness turn, waits for the async subagent to register,
then steers it through openhuman.subagent_steer while the parent/core process is live.
No prompt, response, credential, or transcript bodies are printed.

Options:
  --core-url <url>          JSON-RPC endpoint (default: OPENHUMAN_CORE_RPC_URL or ${DEFAULT_RPC_URL})
  --token <token>           RPC bearer (default: OPENHUMAN_CORE_TOKEN or <workspace>/core.token)
  --workspace <path>        Workspace containing .openhuman/subagent_sessions.json
  --task-key <key>          Durable task key (default: audit-subagent-rpc-<timestamp>)
  --agent-id <id>           Subagent id to request (default: researcher)
  --model <model>           Optional model_override for openhuman.agent_chat
  --rpc-timeout-ms <n>      Parent agent_chat timeout (default: 600000)
  --spawn-wait-ms <n>       Time to wait for a running durable session (default: 120000)
  --settle-wait-ms <n>      Time to wait for final session status after parent returns (default: 60000)
  --spawn-core              Start openhuman-core run --jsonrpc-only for the audit
  --isolated-workspace      With --spawn-core, use a temp workspace and custom audit agent definitions
  --keep-workspace          Do not remove an isolated temp workspace after the run
  --verbose                 Print response char counts and spawned core logs
  -h, --help                Show this help

Examples:
  node scripts/debug/harness-subagent-rpc-audit.mjs
  node scripts/debug/harness-subagent-rpc-audit.mjs --spawn-core --isolated-workspace --model gpt-4.1-mini
`;
}

function parseArgs(argv) {
  const opts = {
    coreUrl: process.env.OPENHUMAN_CORE_RPC_URL || DEFAULT_RPC_URL,
    token: process.env.OPENHUMAN_CORE_TOKEN || "",
    workspace: process.env.OPENHUMAN_WORKSPACE || "",
    taskKey: `audit-subagent-rpc-${Date.now().toString(36)}`,
    agentId: "researcher",
    model: "",
    rpcTimeoutMs: 600_000,
    spawnWaitMs: 120_000,
    settleWaitMs: 60_000,
    spawnCore: false,
    isolatedWorkspace: false,
    keepWorkspace: false,
    verbose: false,
    coreUrlExplicit: Boolean(process.env.OPENHUMAN_CORE_RPC_URL),
    agentIdExplicit: false,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const next = () => {
      const value = argv[++i];
      if (!value) throw new Error(`missing value for ${arg}`);
      return value;
    };
    switch (arg) {
      case "--core-url":
        opts.coreUrl = next();
        opts.coreUrlExplicit = true;
        break;
      case "--token":
        opts.token = next();
        break;
      case "--workspace":
        opts.workspace = next();
        break;
      case "--task-key":
        opts.taskKey = next();
        break;
      case "--agent-id":
        opts.agentId = next();
        opts.agentIdExplicit = true;
        break;
      case "--model":
        opts.model = next();
        break;
      case "--rpc-timeout-ms":
        opts.rpcTimeoutMs = parsePositiveInt(next(), "--rpc-timeout-ms");
        break;
      case "--spawn-wait-ms":
        opts.spawnWaitMs = parsePositiveInt(next(), "--spawn-wait-ms");
        break;
      case "--settle-wait-ms":
        opts.settleWaitMs = parsePositiveInt(next(), "--settle-wait-ms");
        break;
      case "--spawn-core":
        opts.spawnCore = true;
        break;
      case "--isolated-workspace":
        opts.isolatedWorkspace = true;
        break;
      case "--keep-workspace":
        opts.keepWorkspace = true;
        break;
      case "--verbose":
        opts.verbose = true;
        break;
      case "-h":
      case "--help":
        console.log(usage());
        process.exit(0);
      default:
        throw new Error(`unknown option: ${arg}`);
    }
  }
  return opts;
}

function parsePositiveInt(raw, label) {
  const value = Number(raw);
  if (!Number.isInteger(value) || value < 1) {
    throw new Error(`${label} must be a positive integer`);
  }
  return value;
}

function defaultOpenhumanDir() {
  return process.env.OPENHUMAN_APP_ENV === "staging"
    ? path.join(homedir(), ".openhuman-staging")
    : path.join(homedir(), ".openhuman");
}

async function defaultWorkspace() {
  if (process.env.OPENHUMAN_WORKSPACE) return process.env.OPENHUMAN_WORKSPACE;
  const openhumanDir = defaultOpenhumanDir();
  try {
    const active = await readFile(
      path.join(openhumanDir, "active_user.toml"),
      "utf8",
    );
    const match = active.match(/^\s*user_id\s*=\s*"([^"]+)"\s*$/m);
    if (match?.[1]) {
      return path.join(openhumanDir, "users", match[1], "workspace");
    }
  } catch {
    // Fall through to legacy root workspace.
  }
  return openhumanDir;
}

async function readToken(opts) {
  if (opts.token.trim()) return opts.token.trim();
  const tokenPath = path.join(opts.workspace, "core.token");
  try {
    return (await readFile(tokenPath, "utf8")).trim();
  } catch {
    throw new Error(
      `RPC token not provided and ${tokenPath} could not be read. Pass --token or set OPENHUMAN_CORE_TOKEN.`,
    );
  }
}

async function rpc(coreUrl, token, method, params, timeoutMs = 600_000) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);
  let res;
  try {
    res = await fetch(coreUrl, {
      method: "POST",
      signal: controller.signal,
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${token}`,
      },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: `subagent-audit-${Date.now()}-${Math.random().toString(16).slice(2)}`,
        method,
        params,
      }),
    });
  } catch (err) {
    if (err?.name === "AbortError") {
      throw new Error(`RPC ${method} timed out after ${timeoutMs}ms`);
    }
    throw err;
  } finally {
    clearTimeout(timeout);
  }

  const bodyText = await res.text();
  let body;
  try {
    body = JSON.parse(bodyText);
  } catch {
    throw new Error(`RPC ${method} returned non-JSON HTTP ${res.status}`);
  }
  if (!res.ok) throw new Error(`RPC ${method} HTTP ${res.status}`);
  if (body.error) {
    throw new Error(
      `RPC ${method} error: ${body.error.message || body.error.code || "unknown"}`,
    );
  }
  return body.result;
}

function sessionStorePath(workspace) {
  return path.join(workspace, ".openhuman", "subagent_sessions.json");
}

async function readSessions(workspace, taskKey) {
  let raw;
  try {
    raw = await readFile(sessionStorePath(workspace), "utf8");
  } catch {
    return [];
  }
  let sessions;
  try {
    sessions = JSON.parse(raw);
  } catch {
    return [];
  }
  return sessions
    .filter((session) => session?.taskKey === taskKey)
    .map((session) => ({
      subagentSessionId: String(session.subagentSessionId || ""),
      parentSession: String(session.parentSession || ""),
      workerThreadId: session.workerThreadId || null,
      agentId: String(session.agentId || ""),
      taskKey: String(session.taskKey || ""),
      currentTaskId: session.currentTaskId || null,
      status: String(session.status || ""),
      reusable: Boolean(session.reusable),
      updatedAt: String(session.updatedAt || ""),
      lastUsedAt: String(session.lastUsedAt || ""),
    }));
}

async function waitForRunningSession(
  workspace,
  taskKey,
  waitMs,
  parentPromise,
) {
  const deadline = Date.now() + waitMs;
  let last = [];
  while (Date.now() < deadline) {
    const parentState = await Promise.race([
      parentPromise.then(
        () => ({ done: true }),
        (err) => ({ error: err }),
      ),
      sleep(0).then(() => ({ pending: true })),
    ]);
    if (parentState.error) throw parentState.error;
    if (parentState.done) {
      throw new Error(
        `parent agent_chat completed before a running subagent session appeared; last_count=${last.length}`,
      );
    }

    last = await readSessions(workspace, taskKey);
    const running = last.find(
      (session) => session.currentTaskId && session.status === "running",
    );
    if (running) return running;
    await sleep(200);
  }
  throw new Error(
    `timed out waiting for running subagent session; last_count=${last.length}`,
  );
}

async function waitForSettledSessions(workspace, taskKey, waitMs) {
  const deadline = Date.now() + waitMs;
  let last = [];
  while (Date.now() < deadline) {
    last = await readSessions(workspace, taskKey);
    if (
      last.length > 0 &&
      last.some((session) => session.status !== "running")
    ) {
      return last;
    }
    await sleep(500);
  }
  return last;
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function spawnPrompt(opts) {
  return `Harness async subagent RPC audit.
Call spawn_subagent exactly once with agent_id \`${opts.agentId}\`, task_key \`${opts.taskKey}\`, blocking false, and fresh false.
Ask the sub-agent to produce a concise confirmation for audit marker \`${opts.taskKey}\`.
After the tool returns, reply with one short sentence saying the async worker was started.
Do not call wait_subagent.`;
}

function steerMessage(opts) {
  return `Mid-run RPC steering audit for marker \`${opts.taskKey}\`: acknowledge that this instruction arrived through the async steering queue, then keep the final answer concise.`;
}

function responseText(result) {
  if (typeof result === "string") return result;
  if (typeof result?.result === "string") return result.result;
  if (typeof result?.response === "string") return result.response;
  if (typeof result?.data === "string") return result.data;
  return JSON.stringify(result);
}

async function pickFreePort() {
  return await new Promise((resolve, reject) => {
    const server = createServer();
    server.unref();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : 0;
      server.close(() => resolve(port));
    });
  });
}

async function writeAuditDefinitions(workspace) {
  const agentsDir = path.join(workspace, "agents");
  await mkdir(agentsDir, { recursive: true });
  await writeFile(
    path.join(agentsDir, "orchestrator.toml"),
    `id = "orchestrator"
display_name = "Subagent RPC Audit Orchestrator"
when_to_use = "Deterministic live harness async subagent RPC steering audit orchestrator."
temperature = 0.0
max_iterations = 4
sandbox_mode = "none"
agent_tier = "chat"
omit_identity = true
omit_memory_context = true
omit_safety_preamble = true
omit_skills_catalog = true
omit_profile = true
omit_memory_md = true

[system_prompt]
inline = """
You are the OpenHuman async subagent RPC audit orchestrator.
For every user message, call spawn_subagent exactly once with agent_id "async_audit_worker", blocking false, fresh false, and the task_key provided by the user.
After the tool returns, provide one sentence saying the async worker was started. Do not call wait_subagent. Do not call any other tools.
"""

[tools]
named = ["spawn_subagent"]

[subagents]
allowlist = ["async_audit_worker"]
`,
  );
  await writeFile(
    path.join(agentsDir, "async_audit_worker.toml"),
    `id = "async_audit_worker"
display_name = "Async Audit Worker"
delegate_name = "delegate_async_audit_worker"
when_to_use = "Tiny worker used only by harness async subagent RPC steering audit runs."
temperature = 0.0
max_iterations = 2
sandbox_mode = "none"
agent_tier = "worker"
omit_identity = true
omit_memory_context = true
omit_safety_preamble = true
omit_skills_catalog = true
omit_profile = true
omit_memory_md = true

[system_prompt]
inline = "Return one short sentence confirming the async audit worker ran and mention whether a steering instruction was received. Do not call tools."

[tools]
named = []
`,
  );
}

async function writeIsolatedDirectProviderConfig(workspace, model) {
  const apiKey =
    process.env.OPENAI_API_KEY?.trim() || process.env.OPENAI_KEY?.trim() || "";
  if (!apiKey) {
    throw new Error(
      "--isolated-workspace requires OPENAI_API_KEY or OPENAI_KEY for direct OpenAI provider routing",
    );
  }
  const providerModel = model?.trim() || "gpt-4.1-mini";
  const providerRoute = `openai:${providerModel}`;
  await writeFile(
    path.join(workspace, "config.toml"),
    `api_key = ${JSON.stringify(apiKey)}
inference_url = "https://api.openai.com/v1"
default_model = ${JSON.stringify(providerModel)}
chat_provider = ${JSON.stringify(providerRoute)}
reasoning_provider = ${JSON.stringify(providerRoute)}
agentic_provider = ${JSON.stringify(providerRoute)}
coding_provider = ${JSON.stringify(providerRoute)}
memory_provider = "openhuman"
embedding_provider = "none"

[[cloud_providers]]
id = "audit_openai"
slug = "openai"
label = "OpenAI"
endpoint = "https://api.openai.com/v1"
auth_style = "bearer"
default_model = ${JSON.stringify(providerModel)}
`,
    { mode: 0o600 },
  );
}

async function startCore(opts) {
  const token = opts.token || `audit-${randomBytes(24).toString("hex")}`;
  const env = { ...process.env, OPENHUMAN_CORE_TOKEN: token };
  if (opts.workspace) env.OPENHUMAN_WORKSPACE = opts.workspace;
  if (opts.isolatedWorkspace) env.OPENHUMAN_AGENTBOX_MODE = "1";
  const port = new URL(opts.coreUrl).port || "7788";
  env.OPENHUMAN_CORE_PORT = port;
  env.OPENHUMAN_CORE_RPC_URL = opts.coreUrl;
  const child = spawn(
    "cargo",
    [
      "run",
      "--quiet",
      "--bin",
      "openhuman-core",
      "--",
      "run",
      "--host",
      "127.0.0.1",
      "--port",
      port,
      "--jsonrpc-only",
    ],
    {
      cwd: path.resolve(SCRIPT_DIR, "../.."),
      env,
      stdio: opts.verbose
        ? ["ignore", "inherit", "inherit"]
        : ["ignore", "ignore", "pipe"],
    },
  );
  let stderr = "";
  if (child.stderr) {
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
      if (stderr.length > 8000) stderr = stderr.slice(-8000);
    });
  }
  await waitForCore(opts.coreUrl, token, child, () => stderr);
  return { child, token };
}

async function waitForCore(coreUrl, token, child, stderrFn) {
  const deadline = Date.now() + 120_000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) {
      throw new Error(
        `spawned core exited with ${child.exitCode}\n${stderrFn()}`,
      );
    }
    try {
      await rpc(coreUrl, token, "core.ping", {}, 10_000);
      return;
    } catch {
      await sleep(750);
    }
  }
  throw new Error(`timed out waiting for spawned core\n${stderrFn()}`);
}

async function stopChild(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  child.kill("SIGTERM");
  const exited = await Promise.race([
    once(child, "exit").then(() => true),
    sleep(5_000).then(() => false),
  ]);
  if (exited || child.exitCode !== null || child.signalCode !== null) return;
  child.kill("SIGKILL");
  await Promise.race([once(child, "exit"), sleep(2_000)]);
}

function unwrapData(result) {
  return result?.data && typeof result.data === "object" ? result.data : result;
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (!opts.workspace) opts.workspace = await defaultWorkspace();

  let tempWorkspace = "";
  let spawned;
  if (opts.isolatedWorkspace) {
    if (!opts.spawnCore) {
      throw new Error("--isolated-workspace requires --spawn-core");
    }
    tempWorkspace = await mkdtemp(
      path.join(tmpdir(), "openhuman-harness-subagent-rpc-audit-"),
    );
    opts.workspace = path.join(tempWorkspace, "workspace");
    await mkdir(opts.workspace, { recursive: true });
    await writeAuditDefinitions(opts.workspace);
    await writeAuditDefinitions(path.join(opts.workspace, "workspace"));
    await writeIsolatedDirectProviderConfig(opts.workspace, opts.model);
    opts.sessionWorkspace = path.join(opts.workspace, "workspace");
    if (!opts.agentIdExplicit) opts.agentId = "async_audit_worker";
    if (!opts.model) opts.model = "gpt-4.1-mini";
  }

  if (opts.spawnCore) {
    if (!opts.coreUrlExplicit) {
      const port = await pickFreePort();
      opts.coreUrl = `http://127.0.0.1:${port}/rpc`;
    }
    spawned = await startCore(opts);
    opts.token = spawned.token;
  } else {
    opts.token = await readToken(opts);
  }

  console.log("[harness-subagent-rpc-audit] starting live audit");
  console.log(`  rpc: ${opts.coreUrl}`);
  console.log(`  workspace: ${opts.workspace}`);
  if (opts.sessionWorkspace) {
    console.log(`  session_workspace: ${opts.sessionWorkspace}`);
  }
  console.log(`  task_key: ${opts.taskKey}`);
  console.log(`  agent_id: ${opts.agentId}`);
  console.log(`  mode: ${opts.spawnCore ? "spawned-core" : "attached-core"}`);
  if (opts.isolatedWorkspace) {
    console.log("  definitions: isolated audit overrides enabled");
  }

  let parentResult;
  let runningSession;
  let steerResult;
  let sessions = [];
  try {
    const params = { message: spawnPrompt(opts) };
    if (opts.model) params.model_override = opts.model;

    const parentStarted = Date.now();
    const parentPromise = rpc(
      opts.coreUrl,
      opts.token,
      "openhuman.agent_chat",
      params,
      opts.rpcTimeoutMs,
    );

    runningSession = await waitForRunningSession(
      opts.sessionWorkspace || opts.workspace,
      opts.taskKey,
      opts.spawnWaitMs,
      parentPromise,
    );
    console.log(
      `[harness-subagent-rpc-audit] running session task_id=${runningSession.currentTaskId} subagent_session_id=${runningSession.subagentSessionId}`,
    );

    steerResult = unwrapData(
      await rpc(
        opts.coreUrl,
        opts.token,
        "openhuman.subagent_steer",
        {
          taskId: runningSession.currentTaskId,
          message: steerMessage(opts),
          mode: "steer",
        },
        30_000,
      ),
    );
    console.log(
      `[harness-subagent-rpc-audit] steer result steered=${Boolean(steerResult.steered)} reason=${steerResult.reason || "none"}`,
    );

    parentResult = await parentPromise;
    const response = responseText(parentResult);
    console.log(
      `[harness-subagent-rpc-audit] parent turn completed in ${Date.now() - parentStarted}ms${
        opts.verbose ? ` response_chars=${response.length}` : ""
      }`,
    );

    sessions = await waitForSettledSessions(
      opts.sessionWorkspace || opts.workspace,
      opts.taskKey,
      opts.settleWaitMs,
    );
  } finally {
    if (spawned?.child) await stopChild(spawned.child);
    if (tempWorkspace && !opts.keepWorkspace) {
      await rm(tempWorkspace, { recursive: true, force: true });
    }
  }

  console.log("[harness-subagent-rpc-audit] sessions");
  if (sessions.length === 0) {
    console.log("  none");
  } else {
    for (const session of sessions) {
      console.log(
        `  subagent_session_id=${session.subagentSessionId} task_id=${session.currentTaskId || "none"} status=${session.status} reusable=${session.reusable} updated_at=${session.updatedAt}`,
      );
    }
  }

  const failures = [];
  if (!runningSession?.currentTaskId)
    failures.push("no running subagent task observed");
  if (!steerResult?.steered) {
    failures.push(
      `subagent steer was not accepted (${steerResult?.reason || "unknown"})`,
    );
  }
  const uniqueSessions = new Set(
    sessions.map((session) => session.subagentSessionId),
  );
  if (uniqueSessions.size !== 1) {
    failures.push(
      `expected one durable session for task key, observed ${uniqueSessions.size}`,
    );
  }
  if (sessions.length === 0)
    failures.push("no durable session remained after audit");

  if (failures.length > 0) {
    console.error("\n[harness-subagent-rpc-audit] FAIL");
    for (const failure of failures) console.error(`  - ${failure}`);
    process.exit(1);
  }
  if (tempWorkspace && opts.keepWorkspace) {
    console.log(
      `[harness-subagent-rpc-audit] kept isolated workspace: ${opts.workspace}`,
    );
  }
  console.log("\n[harness-subagent-rpc-audit] PASS");
}

main().catch((err) => {
  console.error(`[harness-subagent-rpc-audit] ERROR: ${err.message}`);
  process.exit(1);
});
