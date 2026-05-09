#!/usr/bin/env node
// Scan open PRs for CodeRabbit rate-limit comments and retrigger reviews
// once the stated wait window has elapsed.
//
// CodeRabbit's rate-limit comment looks like:
//   <!-- rate limited by coderabbit.ai -->
//   ...Please wait **46 seconds** before requesting another review.
// We parse the wait, add a small grace, and if `comment.created_at + wait`
// is in the past — and no one has already retriggered — we post
// `@coderabbitai review`.
//
// Pro plan limits CR to 5 PRs/hr, so cap retriggers per run.

import { execFileSync } from "node:child_process";

const RATE_LIMIT_MARKER = "rate limited by coderabbit.ai";
const RETRIGGER_BODY = "@coderabbitai review";
const CR_LOGINS = new Set(["coderabbitai", "coderabbitai[bot]"]);

function gh(args, { input } = {}) {
  return execFileSync("gh", args, {
    encoding: "utf8",
    input,
    stdio: input ? ["pipe", "pipe", "inherit"] : ["ignore", "pipe", "inherit"],
    maxBuffer: 32 * 1024 * 1024,
  });
}

function resolveRepo() {
  if (process.env.RABBIT_REPO) return process.env.RABBIT_REPO;
  for (const remote of ["upstream", "origin"]) {
    try {
      const url = execFileSync("git", ["remote", "get-url", remote], {
        encoding: "utf8",
      }).trim();
      const m = url.match(/github\.com[:/]([^/]+\/[^/.]+)(?:\.git)?$/);
      if (m) return m[1];
    } catch {
      // try next
    }
  }
  throw new Error("could not resolve repo (set RABBIT_REPO=owner/name)");
}

function parseArgs(argv) {
  const out = {
    cmd: argv[0] ?? "run",
    max: 5,
    dryRun: false,
    pr: null,
    graceSec: 30,
  };
  for (let i = 1; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--dry-run") out.dryRun = true;
    else if (a === "--max") out.max = Number(argv[++i]);
    else if (a === "--pr") out.pr = Number(argv[++i]);
    else if (a === "--grace") out.graceSec = Number(argv[++i]);
    else if (a === "-h" || a === "--help") {
      out.cmd = "help";
    } else {
      throw new Error(`unknown arg: ${a}`);
    }
  }
  return out;
}

// Convert "1 hour and 5 minutes and 30 seconds" / "46 seconds" / "5 minutes"
// to seconds. CR uses `**46 seconds**` style — strip markdown asterisks first.
function parseWaitSeconds(body) {
  const m = body.match(/Please wait\s+([^.<]+?)\s+before requesting/i);
  if (!m) return null;
  const raw = m[1].replace(/\*+/g, "").trim();
  const parts = raw.split(/\s*(?:,|and)\s*/i);
  let total = 0;
  for (const part of parts) {
    const pm = part.match(/^(\d+)\s*(second|minute|hour)s?$/i);
    if (!pm) return null;
    const n = Number(pm[1]);
    const unit = pm[2].toLowerCase();
    total += n * (unit === "second" ? 1 : unit === "minute" ? 60 : 3600);
  }
  return total > 0 ? total : null;
}

function fetchOpenPrs(repo) {
  const out = gh([
    "pr",
    "list",
    "-R",
    repo,
    "--state",
    "open",
    "--json",
    "number,title,author,isDraft",
    "--limit",
    "100",
  ]);
  return JSON.parse(out);
}

function fetchIssueComments(repo, pr) {
  const out = gh([
    "api",
    "--paginate",
    `repos/${repo}/issues/${pr}/comments?per_page=100`,
  ]);
  // gh --paginate concatenates JSON arrays; parse leniently.
  try {
    return JSON.parse(out);
  } catch {
    // Fallback: split on `][` boundary inserted between pages.
    return JSON.parse("[" + out.replace(/\]\s*\[/g, ",").slice(1, -1) + "]");
  }
}

function postComment(repo, pr, body) {
  gh(
    [
      "api",
      "-X",
      "POST",
      `repos/${repo}/issues/${pr}/comments`,
      "-f",
      `body=${body}`,
    ],
    {},
  );
}

// For one PR: returns { status, ... } describing what to do.
//   status: "no-cr" | "no-rate-limit" | "already-retriggered"
//         | "review-since" | "waiting" | "ready"
function analyzePr(comments, graceSec) {
  const crComments = comments.filter((c) => CR_LOGINS.has(c.user?.login));
  if (crComments.length === 0) return { status: "no-cr" };

  // Latest CR comment overall.
  const latestCr = crComments[crComments.length - 1];
  const latestRateLimit = [...crComments]
    .reverse()
    .find((c) => c.body.includes(RATE_LIMIT_MARKER));

  if (!latestRateLimit) return { status: "no-rate-limit" };

  // If CR has already posted a non-rate-limit comment AFTER the rate limit,
  // it has effectively recovered — skip.
  if (
    latestCr !== latestRateLimit &&
    new Date(latestCr.created_at) > new Date(latestRateLimit.created_at) &&
    !latestCr.body.includes(RATE_LIMIT_MARKER)
  ) {
    return { status: "review-since" };
  }

  // If anyone (us or otherwise) has already posted `@coderabbitai review`
  // after the rate limit, don't double-trigger.
  const retriggerSince = comments.find(
    (c) =>
      new Date(c.created_at) > new Date(latestRateLimit.created_at) &&
      c.body.trim().toLowerCase().startsWith(RETRIGGER_BODY),
  );
  if (retriggerSince) {
    return { status: "already-retriggered", at: retriggerSince.created_at };
  }

  const waitSec = parseWaitSeconds(latestRateLimit.body);
  if (waitSec == null) {
    return { status: "ready", reason: "unparseable wait — assuming elapsed" };
  }

  const expiresAt =
    new Date(latestRateLimit.created_at).getTime() +
    (waitSec + graceSec) * 1000;
  const now = Date.now();
  if (now < expiresAt) {
    return {
      status: "waiting",
      remainingSec: Math.ceil((expiresAt - now) / 1000),
      ratedLimitedAt: latestRateLimit.created_at,
    };
  }

  return {
    status: "ready",
    waitSec,
    ratedLimitedAt: latestRateLimit.created_at,
  };
}

function fmtAge(iso) {
  const sec = (Date.now() - new Date(iso).getTime()) / 1000;
  if (sec < 60) return `${Math.round(sec)}s ago`;
  if (sec < 3600) return `${Math.round(sec / 60)}m ago`;
  return `${(sec / 3600).toFixed(1)}h ago`;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.cmd === "help") {
    console.log("Usage: pnpm rabbit [run|list] [--max N] [--dry-run] [--pr N] [--grace SEC]");
    return;
  }

  const repo = resolveRepo();
  console.log(`[rabbit] repo: ${repo}`);

  let prs = fetchOpenPrs(repo);
  if (args.pr) prs = prs.filter((p) => p.number === args.pr);

  let triggered = 0;
  const summary = [];

  for (const pr of prs) {
    if (args.cmd === "run" && triggered >= args.max) {
      summary.push(`#${pr.number}  skipped (max ${args.max} reached)`);
      continue;
    }

    let comments;
    try {
      comments = fetchIssueComments(repo, pr.number);
    } catch (e) {
      summary.push(`#${pr.number}  error fetching comments: ${e.message}`);
      continue;
    }

    const result = analyzePr(comments, args.graceSec);
    const tag = `#${pr.number}`.padEnd(7);

    switch (result.status) {
      case "no-cr":
        summary.push(`${tag} no CodeRabbit comments`);
        break;
      case "no-rate-limit":
        summary.push(`${tag} no rate limit`);
        break;
      case "review-since":
        summary.push(`${tag} CR reviewed since rate-limit`);
        break;
      case "already-retriggered":
        summary.push(
          `${tag} already retriggered (${fmtAge(result.at)}) — waiting for CR`,
        );
        break;
      case "waiting": {
        const m = Math.floor(result.remainingSec / 60);
        const s = result.remainingSec % 60;
        summary.push(
          `${tag} rate-limited (${fmtAge(result.ratedLimitedAt)}); ${m}m${s}s left`,
        );
        break;
      }
      case "ready":
        if (args.cmd === "list" || args.dryRun) {
          summary.push(`${tag} READY — would retrigger`);
        } else {
          try {
            postComment(repo, pr.number, RETRIGGER_BODY);
            triggered += 1;
            summary.push(`${tag} retriggered (${triggered}/${args.max})`);
          } catch (e) {
            summary.push(`${tag} retrigger failed: ${e.message}`);
          }
        }
        break;
      default:
        summary.push(`${tag} unknown state`);
    }
  }

  console.log("\n" + summary.join("\n"));
  console.log(
    `\n[rabbit] ${args.cmd === "run" ? "retriggered" : "ready"}: ${
      args.cmd === "run" ? triggered : summary.filter((l) => l.includes("READY")).length
    }`,
  );
}

main().catch((e) => {
  console.error(`[rabbit] fatal: ${e.stack || e.message}`);
  process.exit(1);
});
