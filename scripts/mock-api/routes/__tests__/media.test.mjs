import assert from "node:assert/strict";
import test from "node:test";

import { handleMedia, resetMediaMock } from "../media.mjs";

function createRes() {
  return {
    statusCode: 0,
    headers: {},
    body: "",
    writeHead(status, headers = {}) {
      this.statusCode = status;
      this.headers = headers;
    },
    setHeader(name, value) {
      this.headers[name] = value;
    },
    end(chunk = "") {
      this.body += Buffer.isBuffer(chunk)
        ? chunk.toString("latin1")
        : String(chunk);
    },
  };
}

function call(method, url, parsedBody) {
  const res = createRes();
  const handled = handleMedia({ method, url, parsedBody, res });
  return {
    handled,
    res,
    body:
      res.headers["Content-Type"] === "video/mp4"
        ? null
        : JSON.parse(res.body || "null"),
  };
}

test.beforeEach(() => resetMediaMock());

test("images return an enveloped OpenRouter body with base64 images", () => {
  const { handled, res, body } = call(
    "POST",
    "/agent-integrations/openrouter/images",
    { prompt: "x", n: 2 },
  );
  assert.equal(handled, true);
  assert.equal(res.statusCode, 200);
  assert.equal(body.success, true);
  assert.equal(body.data.data.length, 2);
  assert.equal(body.data.data[0].media_type, "image/png");
  assert.ok(body.data.data[0].b64_json.length > 0);
});

test("images reject a missing prompt", () => {
  const { res } = call("POST", "/agent-integrations/openrouter/images", {});
  assert.equal(res.statusCode, 400);
});

test("video jobs report completed-without-outputs before delivering", () => {
  const submit = call("POST", "/agent-integrations/openrouter/videos", {
    prompt: "x",
  });
  const id = submit.body.data.id;
  const poll = () =>
    call("GET", `/agent-integrations/openrouter/videos/${id}`).body.data;

  assert.equal(poll().status, "in_progress");
  const early = poll();
  assert.equal(early.status, "completed");
  assert.deepEqual(
    early.unsigned_urls,
    [],
    "completed before the output exists",
  );
  const done = poll();
  assert.equal(done.status, "completed");
  assert.equal(done.unsigned_urls.length, 1);

  const content = call(
    "GET",
    `/agent-integrations/openrouter/videos/${id}/content?index=0`,
  );
  assert.equal(content.res.headers["Content-Type"], "video/mp4");
});

test("unrelated routes fall through", () => {
  assert.equal(
    call("GET", "/agent-integrations/composio/tools").handled,
    false,
  );
});
