#!/usr/bin/env node
// capture-first-inference.mjs — loopback proxy that sits between the core and
// its inference backend, records the exact request bodies the harness sends,
// and summarises every inference response (which endpoint served it, time to
// first byte, prompt / cached tokens, status). Everything else on the backend
// (auth, sockets, integrations) is forwarded untouched.
//
// Run `node scripts/debug/capture-first-inference.mjs --help` (or
// `pnpm debug capture --help`) for the knobs; the tail of this file has the
// setup recipe.

import fs from "node:fs";
import http from "node:http";
import https from "node:https";
import path from "node:path";

const LOOPBACK_HOSTS = new Set(["127.0.0.1", "::1", "localhost"]);

function isLoopbackHost(host) {
  return LOOPBACK_HOSTS.has(host.toLowerCase());
}

const USAGE = `Usage: node scripts/debug/capture-first-inference.mjs [--help]

Loopback proxy between the OpenHuman core and its inference backend. Records the
exact request bodies the harness sends and prints one summary line per inference
response: serving endpoint, time to first byte, time to the first model token
(ttft, reasoning or text) and first visible text (ttfc), prompt / cached tokens,
stream flag, thread id, status.

Configure with environment variables:
  CAPTURE_PORT       listen port (default 18765)
  CAPTURE_HOST       listen host (default 127.0.0.1; see CAPTURE_ALLOW_REMOTE)
  CAPTURE_UPSTREAM   where to forward (default https://api.tinyhumans.ai;
                     https://openrouter.ai for a direct BYOK OpenRouter route)
  CAPTURE_OUTPUT     file for the first inference body
                     (default target/debug-logs/first-inference-request.json)
  CAPTURE_ALL=1      also record every inference request (and every non-2xx
                     response body) numbered under CAPTURE_ALL_DIR
  CAPTURE_RESPONSES=1  record every inference response body under CAPTURE_ALL_DIR
  CAPTURE_ALL_DIR    (default target/debug-logs/inference-sequence)
  CAPTURE_LOG        JSONL file receiving one record per inference response
                     (default target/debug-logs/inference-capture.jsonl)
  CAPTURE_ALLOW_REMOTE=1              bind a non-loopback CAPTURE_HOST
  CAPTURE_ALLOW_PLAINTEXT_UPSTREAM=1  forward the bearer to a non-loopback http: upstream

Request and response captures contain raw conversation content. They are
written as local owner-only files; remove the capture directory after diagnosis.

Point the core at it, then drive turns and read the summary lines:
  CAPTURE_ALL=1 pnpm debug capture
  api_url = "http://127.0.0.1:18765"           # in the user's config.toml, or
  BACKEND_URL=http://127.0.0.1:18765 ./target/debug/openhuman-core run --port 7799
`;

if (process.argv.includes("--help") || process.argv.includes("-h")) {
  process.stdout.write(USAGE);
  process.exit(0);
}

const listenHost = process.env.CAPTURE_HOST || "127.0.0.1";
const listenPort = Number.parseInt(process.env.CAPTURE_PORT || "18765", 10);
const upstream = new URL(
  process.env.CAPTURE_UPSTREAM || "https://api.tinyhumans.ai",
);
const outputPath = path.resolve(
  process.env.CAPTURE_OUTPUT ||
    "target/debug-logs/first-inference-request.json",
);
const summaryLogPath = path.resolve(
  process.env.CAPTURE_LOG || "target/debug-logs/inference-capture.jsonl",
);

// The proxy forwards the inbound `authorization` header verbatim. Binding to a
// non-loopback interface would expose that bearer to anything on the network
// that can reach this port with no auth of its own; forwarding it over a
// plaintext (`http:`) upstream that isn't itself loopback would expose it in
// transit. Both are opt-in escape hatches for a deliberate reason (a
// non-loopback CAPTURE_UPSTREAM pointed at a local mock server on `http:` is a
// normal debugging setup), gated by explicit env vars rather than silently
// allowed.
if (!isLoopbackHost(listenHost) && process.env.CAPTURE_ALLOW_REMOTE !== "1") {
  throw new Error(
    `refusing to bind CAPTURE_HOST=${listenHost}: not loopback. ` +
      "Set CAPTURE_ALLOW_REMOTE=1 to bind a non-loopback interface anyway.",
  );
}
if (
  upstream.protocol === "http:" &&
  !isLoopbackHost(upstream.hostname) &&
  process.env.CAPTURE_ALLOW_PLAINTEXT_UPSTREAM !== "1"
) {
  throw new Error(
    `refusing to forward the authorization header to CAPTURE_UPSTREAM=${upstream.origin} over plaintext http:. ` +
      "Use an https: upstream, point at a loopback host, or set CAPTURE_ALLOW_PLAINTEXT_UPSTREAM=1 to override.",
  );
}

// `CAPTURE_ALL=1` records *every* inference request of the session, numbered, into
// `CAPTURE_ALL_DIR`. The single-shot default answers "what does the first turn
// cost"; only the sequence answers "does the cacheable prefix survive turn 2",
// which is a different question and the one a prefix cache is graded on.
const captureAll = process.env.CAPTURE_ALL === "1";
const captureResponses = process.env.CAPTURE_RESPONSES === "1";
const captureAllDir = path.resolve(
  process.env.CAPTURE_ALL_DIR || "target/debug-logs/inference-sequence",
);

if (upstream.protocol !== "https:" && upstream.protocol !== "http:") {
  throw new Error(`unsupported upstream protocol: ${upstream.protocol}`);
}

let captured = false;
let sequenceIndex = 0;

function isInferenceRequest(req) {
  return (
    req.method === "POST" &&
    (req.url?.includes("/openai/v1/chat/completions") ||
      req.url?.includes("/v1/chat/completions"))
  );
}

function upstreamPath(requestUrl) {
  const basePath = upstream.pathname.replace(/\/$/, "");
  return `${basePath}${requestUrl || "/"}`;
}

/**
 * The request-side facts worth one summary line: which model, how big, and the
 * routing/cache identifiers the backend or OpenRouter will key on.
 * `prompt_cache_key` is the harness's stable-prefix fingerprint and OpenRouter's
 * sticky-routing key — it must be identical across the turns of one thread, so
 * seeing it change call to call is a finding, not noise.
 */
function summarizeRequestBody(body) {
  try {
    const json = JSON.parse(body.toString("utf8"));
    return {
      model: json.model ?? null,
      messages: Array.isArray(json.messages) ? json.messages.length : null,
      tools: Array.isArray(json.tools) ? json.tools.length : 0,
      stream: json.stream === true,
      prompt_cache_key: json.prompt_cache_key ?? null,
      thread_id: json.thread_id ?? json.session_id ?? null,
    };
  } catch {
    return {
      model: null,
      messages: null,
      tools: 0,
      stream: false,
      prompt_cache_key: null,
    };
  }
}

/**
 * Folds an OpenAI-compatible response body (SSE stream or unary JSON) into the
 * response-side facts: the endpoint that served it (OpenRouter's `provider`
 * field), the final usage block, and any error object. Works on a partial body
 * too, so a stream cut off mid-way still reports what arrived.
 */
function summarizeResponseBody(text) {
  const out = {
    provider: null,
    prompt_tokens: null,
    cached_tokens: null,
    error: null,
  };
  const absorb = (json) => {
    if (json && typeof json === "object") {
      if (typeof json.provider === "string") out.provider = json.provider;
      if (json.usage && typeof json.usage === "object") {
        out.prompt_tokens = json.usage.prompt_tokens ?? out.prompt_tokens;
        out.cached_tokens =
          json.usage.prompt_tokens_details?.cached_tokens ?? out.cached_tokens;
      }
      if (json.error) {
        out.error =
          typeof json.error === "string"
            ? json.error
            : (json.error.message ?? JSON.stringify(json.error));
      }
    }
  };
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const payload = trimmed.startsWith("data:")
      ? trimmed.slice(5).trim()
      : trimmed;
    if (payload === "[DONE]" || !payload.startsWith("{")) continue;
    try {
      absorb(JSON.parse(payload));
    } catch {
      // partial or non-JSON line — keep scanning
    }
  }
  if (
    out.error === null &&
    !text.trimStart().startsWith("data:") &&
    text.includes("<html")
  ) {
    out.error = text
      .replace(/<[^>]+>/g, " ")
      .replace(/\s+/g, " ")
      .trim()
      .slice(0, 120);
  }
  return out;
}

// A streamed delta that carries model output: reasoning (OpenRouter's
// `reasoning`, DeepSeek's `reasoning_content`) or visible `content`.
const FIRST_TOKEN_RE = /"(?:content|reasoning|reasoning_content)":"[^"]/;
const FIRST_CONTENT_RE = /"content":"[^"]/;

/**
 * Tracks when a streamed response produced its first model token (reasoning
 * or content) and its first visible content, across arbitrary chunk
 * boundaries: each chunk is scanned together with the tail of the previous one.
 */
function createFirstTokenTracker(startedAt) {
  let tail = "";
  const out = { first_token_ms: null, first_content_ms: null };
  return {
    observe(chunk, now = Date.now()) {
      if (out.first_token_ms !== null && out.first_content_ms !== null) return;
      const window = tail + chunk.toString("utf8");
      tail = window.slice(-256);
      if (out.first_token_ms === null && FIRST_TOKEN_RE.test(window)) {
        out.first_token_ms = now - startedAt;
      }
      if (out.first_content_ms === null && FIRST_CONTENT_RE.test(window)) {
        out.first_content_ms = now - startedAt;
      }
    },
    result: () => ({ ...out }),
  };
}

function formatSummaryLine(record) {
  const ms = (value) => (value == null ? "-" : `${(value / 1000).toFixed(2)}s`);
  return (
    `[capture] #${String(record.seq).padStart(3, "0")} ${record.status} ` +
    `model=${record.model ?? "?"} msgs=${record.messages ?? "?"} tools=${record.tools} ` +
    `served_by=${record.provider ?? "?"} ttfb=${ms(record.ttfb_ms)} total=${ms(record.total_ms)} ` +
    `prompt=${record.prompt_tokens ?? "?"} cached=${record.cached_tokens ?? "?"} ` +
    `cache_key=${record.prompt_cache_key ?? "-"} ` +
    `stream=${record.stream ? "yes" : "no"} ttft=${ms(record.first_token_ms)} ` +
    `ttfc=${ms(record.first_content_ms)} thread=${record.thread_id ?? "-"}` +
    (record.error ? ` error=${JSON.stringify(record.error)}` : "")
  );
}

function appendSummary(record) {
  fs.mkdirSync(path.dirname(summaryLogPath), { recursive: true });
  fs.appendFileSync(summaryLogPath, `${JSON.stringify(record)}\n`, {
    mode: 0o600,
  });
  fs.chmodSync(summaryLogPath, 0o600);
  process.stdout.write(`${formatSummaryLine(record)}\n`);
}

function writePrivateCapture(file, data) {
  fs.writeFileSync(file, data, { mode: 0o600 });
  fs.chmodSync(file, 0o600);
}

const server = http.createServer((req, res) => {
  const chunks = [];
  // A client that resets mid-request (e.g. the core dropping a pooled
  // connection) emits `error` on `req`; unhandled, it would crash the proxy.
  req.on("error", (error) => {
    process.stderr.write(`[capture] client request error: ${error.message}\n`);
  });
  req.on("data", (chunk) => chunks.push(chunk));
  req.on("end", () => {
    const body = Buffer.concat(chunks);
    const inference = isInferenceRequest(req);
    const seq = inference ? sequenceIndex++ : null;

    if (inference) {
      if (!captured) {
        fs.mkdirSync(path.dirname(outputPath), { recursive: true });
        writePrivateCapture(outputPath, body);
        captured = true;
        process.stdout.write(
          `[capture] wrote first inference body to ${outputPath}\n`,
        );
      }
      if (captureAll) {
        fs.mkdirSync(captureAllDir, { recursive: true });
        const name = `req-${String(seq).padStart(3, "0")}.json`;
        writePrivateCapture(path.join(captureAllDir, name), body);
        process.stdout.write(`[capture] wrote ${name} (${body.length} B)\n`);
      }
    }

    const headers = { ...req.headers, host: upstream.host };
    delete headers["content-length"];
    headers["content-length"] = String(body.length);

    const startedAt = Date.now();
    const transport = upstream.protocol === "https:" ? https : http;
    const upstreamReq = transport.request(
      {
        protocol: upstream.protocol,
        hostname: upstream.hostname,
        port: upstream.port || undefined,
        method: req.method,
        path: upstreamPath(req.url),
        headers,
      },
      (upstreamRes) => {
        res.writeHead(upstreamRes.statusCode || 502, upstreamRes.headers);
        if (!inference) {
          upstreamRes.pipe(res);
          return;
        }
        // Tee the response: stream it to the core untouched while folding a
        // copy into the summary. Inference bodies are small (a few hundred KB
        // at most), so buffering the copy is fine.
        let firstByteAt = null;
        const firstTokens = createFirstTokenTracker(startedAt);
        const pieces = [];
        upstreamRes.on("data", (chunk) => {
          if (firstByteAt === null) firstByteAt = Date.now();
          firstTokens.observe(chunk);
          pieces.push(chunk);
          res.write(chunk);
        });
        upstreamRes.on("end", () => {
          // Summarise before closing the client side, so anything waiting on
          // the response (a test, a script driving turns) can read the record
          // as soon as its call returns.
          const text = Buffer.concat(pieces).toString("utf8");
          const status = upstreamRes.statusCode || 502;
          const record = {
            seq,
            at: new Date(startedAt).toISOString(),
            status,
            ...summarizeRequestBody(body),
            ...summarizeResponseBody(text),
            ttfb_ms: firstByteAt === null ? null : firstByteAt - startedAt,
            ...firstTokens.result(),
            total_ms: Date.now() - startedAt,
          };
          if (
            captureResponses ||
            (captureAll && (status < 200 || status >= 300))
          ) {
            const name = `res-${String(seq).padStart(3, "0")}.txt`;
            fs.mkdirSync(captureAllDir, { recursive: true });
            writePrivateCapture(path.join(captureAllDir, name), text);
            record.response_body = path.join(captureAllDir, name);
          }
          appendSummary(record);
          res.end();
        });
        upstreamRes.on("error", (error) => {
          process.stderr.write(
            `[capture] upstream stream error: ${error.message}\n`,
          );
          res.end();
        });
      },
    );

    upstreamReq.on("error", (error) => {
      process.stderr.write(`[capture] upstream error: ${error.message}\n`);
      if (!res.headersSent)
        res.writeHead(502, { "content-type": "text/plain" });
      res.end("capture proxy upstream error");
      if (inference) {
        appendSummary({
          seq,
          at: new Date(startedAt).toISOString(),
          status: 502,
          ...summarizeRequestBody(body),
          provider: null,
          prompt_tokens: null,
          cached_tokens: null,
          error: `upstream: ${error.message}`,
          ttfb_ms: null,
          total_ms: Date.now() - startedAt,
        });
      }
    });

    upstreamReq.end(body);
  });
});

// Socket.IO upgrades never enter the HTTP request callback. Relay the
// handshake and then pipe both raw sockets so desktop backend events work.
server.on("upgrade", (req, clientSocket, clientHead) => {
  // Attach before anything else: a reset on the client socket before the
  // upstream handshake completes (the long-lived Socket.IO tunnel dropping)
  // would otherwise be an unhandled `error` and take the whole proxy down.
  clientSocket.on("error", (error) => {
    process.stderr.write(`[capture] websocket client error: ${error.message}\n`);
    upstreamReq.destroy();
    upstreamSocket?.destroy();
  });
  const transport = upstream.protocol === "https:" ? https : http;
  const upstreamReq = transport.request({
    protocol: upstream.protocol,
    hostname: upstream.hostname,
    port: upstream.port || undefined,
    method: req.method,
    path: upstreamPath(req.url),
    headers: { ...req.headers, host: upstream.host },
  });

  let upstreamSocket;
  const writeResponseHead = (response) => {
    clientSocket.write(
      `HTTP/${response.httpVersion} ${response.statusCode} ${response.statusMessage}\r\n`,
    );
    for (let i = 0; i < response.rawHeaders.length; i += 2) {
      clientSocket.write(
        `${response.rawHeaders[i]}: ${response.rawHeaders[i + 1]}\r\n`,
      );
    }
    clientSocket.write("\r\n");
  };

  upstreamReq.on("upgrade", (response, socket, upstreamHead) => {
    upstreamSocket = socket;
    writeResponseHead(response);
    if (upstreamHead.length) clientSocket.write(upstreamHead);
    if (clientHead.length) socket.write(clientHead);
    socket.pipe(clientSocket);
    clientSocket.pipe(socket);
    socket.on("error", () => clientSocket.destroy());
  });
  upstreamReq.on("response", (response) => {
    writeResponseHead(response);
    response.pipe(clientSocket);
    response.on("error", () => clientSocket.destroy());
    response.on("end", () => clientSocket.end());
  });
  upstreamReq.on("error", (error) => {
    process.stderr.write(
      `[capture] websocket upstream error: ${error.message}\n`,
    );
    if (!clientSocket.destroyed) {
      clientSocket.end("HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n");
    }
  });
  clientSocket.on("close", () => {
    upstreamReq.destroy();
    upstreamSocket?.destroy();
  });
  upstreamReq.end();
});

server.listen(listenPort, listenHost, () => {
  // Report the bound port, not the configured one: CAPTURE_PORT=0 asks the OS
  // for a free port, which is how the self-test runs several proxies at once.
  const boundPort = server.address().port;
  process.stdout.write(
    `[capture] listening on http://${listenHost}:${boundPort}; forwarding to ${upstream.origin}` +
      `; summaries → ${summaryLogPath}` +
      `${captureAll ? `; recording every request under ${captureAllDir}` : ""}` +
      `${captureResponses ? `; recording every response under ${captureAllDir}` : ""}\n`,
  );
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => server.close(() => process.exit(0)));
}
