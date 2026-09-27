/**
 * A mock TinyHumans backend for the life-scenario benchmark, serving the one
 * route the agent's `web_search_tool` needs.
 *
 * ## Why this exists
 *
 * `web_search_tool` is not a local tool: it posts to
 * `/agent-integrations/parallel/search` on the hosted backend, which resolves
 * the query against a paid search provider and bills the caller's team. The
 * benchmark has no session to spend — it runs BYOK inference on an offline
 * local token — so every call came back
 * `SESSION_EXPIRED: backend rejected session token … (401 Invalid token)`.
 *
 * The tool was advertised anyway (`tool spec filter: … web_search_tool`), so
 * the model spent calls discovering that it was dead and then routed around it
 * by hand: in the 2026-09-23 run `baggage-policy` burned two calls on the 401s
 * and then improvised a DuckDuckGo HTML scrape, guessing delta.com paths and
 * collecting four 404s. It reached the 15-call cap with the answer assembled
 * and the requested file unwritten, and graded 0/1.
 *
 * That is a defect in the benchmark rig, not in the product: the run offers a
 * capability its own configuration cannot serve. Removing the tool instead
 * would have been the other kind of dishonest — `baggage-policy` and
 * `fact-check-publish` are *about* finding things on the web.
 *
 * ## What is mocked, and what deliberately is not
 *
 * **Discovery is mocked. Retrieval is not.** This server ranks a fixture
 * corpus (`fixtures/search-index.json`) and returns real, live URLs. The agent
 * still fetches each page over the network with `web_fetch` and still has to
 * read what the page actually says. So `baggage-policy`'s `cites_delta_com`
 * and `states_carryon_dimensions` checks stay honest — what is gone is the
 * search engine the run cannot pay for, not the comprehension being measured.
 *
 * Ranking is deliberately crude (term overlap over title, URL, keywords and
 * excerpt). A cleverer ranker would start deciding the scenario's outcome,
 * which is the grader's job.
 *
 * ## Why it takes over the whole backend base URL
 *
 * `api_url` is the single base every backend caller resolves through
 * (`openhuman_tinyhumans::backend::url::effective_backend_api_url`), so pointing it here is what
 * routes the search. That also catches the other backend calls the run makes
 * and cannot authenticate — `/teams/me/usage`, the Composio toolkit list —
 * which previously logged a 401 per turn. They are answered with benign empty
 * stubs, so the log shows the run's own behaviour rather than a fixed auth
 * failure repeated once a turn. Composio itself is untouched: it is redirected
 * separately to `mock-composio.mjs` by `OPENHUMAN_COMPOSIO_DIRECT_BASE_V*`.
 *
 * Loopback with an ephemeral port is important. `effective_backend_api_url`
 * ignores an `api_url` that looks like an inference endpoint, and
 * `looks_like_local_ai_endpoint` treats loopback as a signal only when it is
 * paired with an LLM-ish port or path — so a bare `http://127.0.0.1:<random>`
 * is passed through as a backend override, which is exactly what this needs.
 *
 * Every unhandled route is recorded and answered 404, so a future capability
 * that starts reaching for the backend shows up in `search-requests.json`
 * rather than failing silently.
 */

import fs from "node:fs";
import http from "node:http";
import path from "node:path";

/** Words too common to carry a topic; dropped before scoring. */
const STOPWORDS = new Set([
  "a",
  "an",
  "and",
  "are",
  "as",
  "at",
  "be",
  "by",
  "current",
  "do",
  "does",
  "for",
  "from",
  "how",
  "i",
  "in",
  "is",
  "it",
  "its",
  "me",
  "my",
  "of",
  "on",
  "or",
  "s",
  "that",
  "the",
  "their",
  "there",
  "they",
  "this",
  "to",
  "was",
  "what",
  "when",
  "where",
  "which",
  "who",
  "will",
  "with",
  "you",
  "your",
]);

/** Lowercase alphanumeric terms, stopwords and one-character noise removed. */
function terms(text) {
  return (text || "")
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter((t) => t.length > 1 && !STOPWORDS.has(t));
}

/**
 * Does `term` match document term `h`?
 *
 * Exact, or a shared prefix of at least four characters so "fees" finds "fee"
 * and "baggage" finds "checked-baggage" without dragging in a stemmer. The
 * length floor is what stops a two-character fragment matching most of the
 * corpus.
 */
function termMatches(term, h) {
  if (term === h) return true;
  const shorter = term.length <= h.length ? term : h;
  if (shorter.length < 4) return false;
  return term.startsWith(h) || h.startsWith(term);
}

/**
 * Score one document against the query terms.
 *
 * Fields are weighted by how deliberate a match in them is: an explicit
 * `keywords` entry is curated, a title is the page's own claim about itself,
 * a URL segment is incidental, and an excerpt is the loosest signal of all.
 *
 * Returns the weighted total alongside the count of *distinct* query terms
 * that matched anywhere, because the two answer different questions: the total
 * orders the hits, and the distinct count is what tells an off-topic query
 * apart from a weak one. Scoring alone cannot — a single incidental term
 * repeated across four fields outscores a genuine two-term match — and a
 * search that always returns something would send the agent off to fetch
 * pages that have nothing to do with the task.
 */
function score(doc, queryTerms) {
  const fields = [
    [doc.keywords || [], 4],
    [[doc.title || ""], 3],
    [[doc.url || ""], 2],
    [doc.excerpts || [], 1],
  ];
  let total = 0;
  const matched = new Set();
  for (const [values, weight] of fields) {
    const haystack = terms(values.join(" "));
    if (haystack.length === 0) continue;
    for (const term of queryTerms) {
      if (haystack.some((h) => termMatches(term, h))) {
        total += weight;
        matched.add(term);
      }
    }
  }
  return { total, matched: matched.size };
}

/** Load the corpus, tolerating the `_comment` key the fixture carries. */
function loadIndex(indexPath) {
  if (!fs.existsSync(indexPath)) return [];
  const parsed = JSON.parse(fs.readFileSync(indexPath, "utf8"));
  return Array.isArray(parsed) ? parsed : parsed.documents || [];
}

/**
 * Serve the backend routes the benchmark reaches.
 *
 * Resolves once the socket is listening, to `{ url, port, ctx, close }` in the
 * same shape as `startMockComposio` so `run.mjs` handles the two alike.
 */
export function startMockSearch({ indexPath, requestsPath, port = 0 }) {
  const ctx = {
    documents: loadIndex(indexPath),
    /** Every query served, with what it matched — the record a grader or a
     *  post-mortem reads to see whether discovery actually worked. */
    searches: [],
    /** Every request, including the unhandled ones. */
    requests: [],
    flush() {
      if (!requestsPath) return;
      fs.writeFileSync(
        requestsPath,
        JSON.stringify(
          { searches: ctx.searches, requests: ctx.requests },
          null,
          2,
        ),
      );
    },
  };

  const json = (res, code, body) => {
    const payload = JSON.stringify(body);
    res.writeHead(code, {
      "content-type": "application/json",
      "content-length": Buffer.byteLength(payload),
    });
    res.end(payload);
  };

  /**
   * The envelope every backend response is unwrapped from —
   * `integrations/client/errors.rs::parse_envelope` deserializes
   * `{ success, data, error }` and hands back `data`.
   *
   * Returning the bare payload instead fails as `missing field 'success'`, and
   * fails *late*: the tool reports a malformed-response error rather than an
   * empty result, so the agent concludes search is broken and abandons the
   * task. That is exactly what the first run of this mock did, six times over.
   */
  const ok = (res, data) => json(res, 200, { success: true, data });

  const search = (body) => {
    // The core sends Parallel's shape: an `objective` plus `searchQueries`,
    // with the result count under `excerpts.maxResults`.
    const queries = [
      ...(Array.isArray(body.searchQueries) ? body.searchQueries : []),
      body.objective,
      body.query,
    ].filter((q) => typeof q === "string" && q.trim());
    const queryTerms = [...new Set(queries.flatMap(terms))];
    const limit = Math.min(
      Math.max(Number(body?.excerpts?.maxResults) || 5, 1),
      10,
    );

    // An off-topic query must come back empty rather than with the least-bad
    // rows in the corpus: a confidently wrong result set is worse for the
    // agent than none, because it spends fetches on it. Two distinct matching
    // terms is the floor, relaxed to one when the query only has one to give.
    const floor = Math.min(2, queryTerms.length) || 1;
    const ranked = ctx.documents
      .map((doc) => ({ doc, ...score(doc, queryTerms) }))
      .filter((hit) => hit.total > 0 && hit.matched >= floor)
      .sort((a, b) => b.total - a.total || a.doc.url.localeCompare(b.doc.url))
      .slice(0, limit);

    ctx.searches.push({
      at: new Date().toISOString(),
      queries,
      hits: ranked.map((hit) => ({ url: hit.doc.url, score: hit.total })),
    });
    ctx.flush();

    return {
      searchId: `mock-${ctx.searches.length}`,
      // Zero rather than a plausible number: the benchmark's cost column must
      // report what the run actually spent, and this search cost nothing.
      costUsd: 0,
      provider: "LifeScenariosMock",
      results: ranked.map(({ doc }) => ({
        url: doc.url,
        title: doc.title || doc.url,
        publish_date: doc.publish_date ?? null,
        excerpts: doc.excerpts || [],
      })),
    };
  };

  const server = http.createServer((req, res) => {
    const url = new URL(req.url, "http://mock");
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      const raw = Buffer.concat(chunks).toString("utf8");
      let body = {};
      if (raw) {
        try {
          body = JSON.parse(raw);
        } catch {
          body = {};
        }
      }
      const p = url.pathname.replace(/\/+$/, "");
      ctx.requests.push({
        at: new Date().toISOString(),
        method: req.method,
        path: p,
      });

      if (
        p === "/agent-integrations/parallel/search" &&
        req.method === "POST"
      ) {
        try {
          return ok(res, search(body));
        } catch (e) {
          return json(res, 500, {
            success: false,
            data: null,
            error: `mock search failed: ${e.message}`,
          });
        }
      }

      // Benign stubs for the backend calls the run makes but has no session
      // for. Answering them keeps a fixed auth failure out of the log so what
      // remains is the run's own behaviour.
      if (p === "/teams/me/usage" && req.method === "GET") {
        return ok(res, { usage: {}, limits: {}, plan: "life-scenarios-mock" });
      }
      if (p === "/agent-integrations/composio/toolkits") {
        // Empty on purpose: Composio is served by `mock-composio.mjs` over the
        // direct base, so this list must not compete with it.
        return ok(res, { items: [], total_items: 0 });
      }

      ctx.flush();
      return json(res, 404, {
        success: false,
        data: null,
        error: `mock search backend: unhandled ${req.method} ${p}`,
      });
    });
  });

  return new Promise((resolve) => {
    server.listen(port, "127.0.0.1", () => {
      const actual = server.address().port;
      resolve({
        url: `http://127.0.0.1:${actual}`,
        port: actual,
        ctx,
        close: () =>
          new Promise((r) => {
            ctx.flush();
            server.close(r);
          }),
      });
    });
  });
}

/** Where the default corpus lives, for `run.mjs` and the self-test. */
export const DEFAULT_INDEX_PATH = path.join(
  path.dirname(new URL(import.meta.url).pathname),
  "fixtures",
  "search-index.json",
);
