import test from "node:test";
import assert from "node:assert/strict";

import { handleLlmCompletions } from "../llm.mjs";
import { resetMockBehavior, setMockBehaviors } from "../../state.mjs";

function createMockResponse() {
  return {
    headers: {},
    statusCode: null,
    body: "",
    chunks: [],
    ended: false,
    setHeader(name, value) {
      this.headers[name] = value;
    },
    writeHead(status, headers = {}) {
      this.statusCode = status;
      Object.assign(this.headers, headers);
    },
    write(chunk) {
      const text = String(chunk);
      this.chunks.push(text);
      this.body += text;
    },
    end(chunk = "") {
      if (chunk) this.write(chunk);
      this.ended = true;
    },
  };
}

function makeCtx({
  method = "POST",
  url = "/chat/completions",
  parsedBody = { model: "gpt-oss", messages: [{ role: "user", content: "hello" }] },
  headers = {},
} = {}) {
  return {
    method,
    url,
    parsedBody,
    req: { headers },
    res: createMockResponse(),
  };
}

test.beforeEach(() => {
  resetMockBehavior();
});

test("handles root chat completions path with default fallback", () => {
  const ctx = makeCtx({ url: "/chat/completions" });

  const handled = handleLlmCompletions(ctx);

  assert.equal(handled, true);
  assert.equal(ctx.res.statusCode, 200);
  const body = JSON.parse(ctx.res.body);
  assert.equal(body.model, "gpt-oss");
  assert.equal(
    body.choices[0].message.content,
    "Hello from e2e mock agent",
  );
});

test("matches request rules against path and authorization header", () => {
  setMockBehaviors(
    {
      llmRequestRules: JSON.stringify([
        {
          path: "/v1/chat/completions",
          model: "gpt-4.1-mini",
          authorization: "Bearer sk-test",
          content: "matched via request rule",
        },
      ]),
    },
    "replace",
  );

  const ctx = makeCtx({
    url: "/v1/chat/completions",
    parsedBody: {
      model: "gpt-4.1-mini",
      messages: [{ role: "user", content: "hello" }],
    },
    headers: { authorization: "Bearer sk-test" },
  });

  const handled = handleLlmCompletions(ctx);

  assert.equal(handled, true);
  assert.equal(ctx.res.statusCode, 200);
  const body = JSON.parse(ctx.res.body);
  assert.equal(body.choices[0].message.content, "matched via request rule");
});

test("streams request-rule scripts for root chat completions path", async () => {
  setMockBehaviors(
    {
      llmRequestRules: JSON.stringify([
        {
          path: "/chat/completions",
          stream: true,
          streamScript: [{ text: "hello" }, { finish: "stop" }],
        },
      ]),
    },
    "replace",
  );

  const ctx = makeCtx({
    url: "/chat/completions",
    parsedBody: {
      model: "gpt-oss",
      stream: true,
      messages: [{ role: "user", content: "stream please" }],
    },
  });

  const handled = handleLlmCompletions(ctx);
  assert.equal(handled, true);

  await new Promise((resolve) => setTimeout(resolve, 80));

  assert.equal(ctx.res.statusCode, 200);
  assert.match(ctx.res.body, /data: .*hello/);
  assert.match(ctx.res.body, /data: \[DONE\]/);
  assert.equal(ctx.res.ended, true);
});

test("returns HTTP error for streaming rules with status >= 400", () => {
  setMockBehaviors(
    {
      llmRequestRules: JSON.stringify([
        {
          path: "/chat/completions",
          stream: true,
          status: 401,
          error: "unauthorized",
          type: "auth_error",
        },
      ]),
    },
    "replace",
  );

  const ctx = makeCtx({
    url: "/chat/completions",
    parsedBody: {
      model: "gpt-oss",
      stream: true,
      messages: [{ role: "user", content: "stream please" }],
    },
  });

  const handled = handleLlmCompletions(ctx);

  assert.equal(handled, true);
  assert.equal(ctx.res.statusCode, 401);
  assert.equal(ctx.res.headers["Content-Type"], "application/json");
  const body = JSON.parse(ctx.res.body);
  assert.equal(body.error.message, "unauthorized");
  assert.equal(body.error.type, "auth_error");
  assert.doesNotMatch(ctx.res.body, /^data:/m);
});

test("returns false for non-LLM routes", () => {
  const ctx = makeCtx({ method: "GET", url: "/chat/completions" });
  assert.equal(handleLlmCompletions(ctx), false);
});
