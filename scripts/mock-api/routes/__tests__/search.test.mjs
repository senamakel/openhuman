import assert from "node:assert/strict";
import test from "node:test";

import {
  resetMockBehavior,
  setMockBehaviors,
  startMockServer,
  stopMockServer,
} from "../../index.mjs";

test.beforeEach(async () => {
  await stopMockServer();
  resetMockBehavior();
});

test.afterEach(async () => {
  await stopMockServer();
});

async function post(baseUrl, path, body) {
  const response = await fetch(`${baseUrl}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  return { status: response.status, body: await response.json() };
}

test("serves managed Exa search with the backend request contract", async () => {
  const started = await startMockServer(18591, { retryIfInUse: true });
  const baseUrl = `http://127.0.0.1:${started.port}`;

  const ok = await post(baseUrl, "/agent-integrations/exa/search", {
    objective: "rust async",
    searchQueries: ["tokio select"],
  });
  assert.equal(ok.status, 200);
  assert.equal(ok.body.data.searchId, "exa-search-1");
  assert.deepEqual(ok.body.data.results[0], {
    url: "https://exa.example.com/0",
    title: "Exa result for tokio select",
    publish_date: "2026-09-01",
    excerpts: ["Objective: rust async; query: tokio select"],
  });

  const rejected = await post(baseUrl, "/agent-integrations/exa/search", {
    objective: "rust async",
    searchQueries: ["tokio select"],
    mode: "fast",
  });
  assert.equal(rejected.status, 400);
});

test("serves Exa contents, answer and findSimilar", async () => {
  const started = await startMockServer(18592, { retryIfInUse: true });
  const baseUrl = `http://127.0.0.1:${started.port}`;

  const contents = await post(baseUrl, "/agent-integrations/exa/contents", {
    urls: ["https://a.example"],
  });
  assert.equal(contents.body.data.results[0].url, "https://a.example");

  const answer = await post(baseUrl, "/agent-integrations/exa/answer", {
    query: "why",
  });
  assert.equal(answer.body.data.answer, "Mock Exa answer for why");
  assert.equal(answer.body.data.citations.length, 1);

  const similar = await post(baseUrl, "/agent-integrations/exa/findSimilar", {
    url: "https://a.example",
  });
  assert.equal(similar.status, 200);
});

test("answers insufficient credits for Exa when the balance behavior is set", async () => {
  setMockBehaviors({ exaInsufficientBalance: "1" });
  const started = await startMockServer(18593, { retryIfInUse: true });
  const baseUrl = `http://127.0.0.1:${started.port}`;

  const response = await post(baseUrl, "/agent-integrations/exa/search", {
    objective: "x",
    searchQueries: ["x"],
  });
  assert.equal(response.status, 400);
  assert.equal(response.body.errorCode, "USER_INSUFFICIENT_CREDITS");
});

test("serves Gemini grounded generate-content with grounding metadata", async () => {
  const started = await startMockServer(18594, { retryIfInUse: true });
  const baseUrl = `http://127.0.0.1:${started.port}`;

  const response = await post(
    baseUrl,
    "/agent-integrations/gemini/models/gemini-3.8-flash/generate-content",
    {
      contents: [{ role: "user", parts: [{ text: "who won" }] }],
      tools: [{ googleSearch: {} }],
    },
  );
  assert.equal(response.status, 200);
  const candidate = response.body.data.candidates[0];
  assert.equal(candidate.content.parts[0].text, "Mock grounded answer for who won");
  assert.equal(
    candidate.groundingMetadata.groundingChunks[0].web.uri,
    "https://gemini.example.com/source",
  );
  assert.equal(response.body.data.modelVersion, "gemini-3.8-flash");
});

test("serves TinyFish search and fetch", async () => {
  const started = await startMockServer(18595, { retryIfInUse: true });
  const baseUrl = `http://127.0.0.1:${started.port}`;

  const search = await post(baseUrl, "/agent-integrations/tinyfish/search", {
    query: "fish",
  });
  assert.equal(search.body.data.results[0].title, "TinyFish result for fish");

  const fetched = await post(baseUrl, "/agent-integrations/tinyfish/fetch", {
    urls: ["https://b.example"],
  });
  assert.equal(fetched.body.data.results[0].url, "https://b.example");
});
