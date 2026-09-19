#!/usr/bin/env node
// Fold the per-target tinyanalyzer JSON reports written by
// scripts/dep-audit/run.sh into one Markdown report.
//
//   node scripts/dep-audit/report.mjs --reports target/dep-audit \
//        [--out target/dep-audit/REPORT.md] [--top 15] [--json summary.json]
//
// The report has four sections per the run.sh header: unused declared
// dependencies, crates resolved at several versions, the heaviest direct
// dependencies (by exclusive transitive crate count), and cross-repository
// version drift for crates that several targets depend on directly.
//
// Every unused-dependency flag is re-checked here with a textual scan of the
// package's own sources (see `textualUse`), because tinyanalyzer's check does
// not see crate names inside attributes (`#[tokio::test]`,
// `#[derive(thiserror::Error)]`). A flag whose crate *is* referenced as a path
// somewhere is reported as "keep" instead of "remove", so nothing is silently
// hidden but the reader knows which rows are real. Each "remove" row also says
// whether deleting the line shrinks the build ("graph win") or merely tidies
// the manifest because another workspace package still pulls the crate in.

import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const args = parseArgs(process.argv.slice(2));
const reportsDir = path.resolve(args.reports ?? "target/dep-audit");
const outFile = args.out ? path.resolve(args.out) : null;
const jsonOut = args.json ? path.resolve(args.json) : null;
const top = Number(args.top ?? 15);
const toolVersion = args["tinyanalyzer-version"] ?? "tinyanalyzer";

const targets = readTargets(path.join(reportsDir, "targets.tsv"));
const reports = [];
for (const target of targets) {
  const file = path.join(reportsDir, `${target.name}.json`);
  if (!fs.existsSync(file)) continue;
  const data = JSON.parse(fs.readFileSync(file, "utf8"));
  reports.push({ target, data });
}
if (reports.length === 0) {
  console.error(`dep-audit/report: no <target>.json reports in ${reportsDir}`);
  process.exit(1);
}

const summary = {
  generated_at: new Date().toISOString(),
  tinyanalyzer: toolVersion,
  targets: reports.map(({ target, data }) => ({
    name: target.name,
    path: target.path,
    commit: target.sha,
    remote: target.remote,
    packages: data.dependencies.packages.length,
    external_packages: data.dependencies.external_packages,
    max_depth: data.dependencies.max_depth,
    direct: data.dependencies.packages.filter((p) => p.is_direct).length,
    unused: unusedFor(target, data),
    duplicates: data.dependencies.duplicates
      .map((d) => ({
        name: d.name,
        versions: d.versions,
        via: Object.fromEntries(
          d.versions.map((v) => {
            const pkg = data.dependencies.packages.find((p) => p.name === d.name && p.version === v);
            return [v, pkg ? pulledInVia(data, pkg.id) : "?"];
          }),
        ),
      }))
      .sort((a, b) => b.versions.length - a.versions.length || a.name.localeCompare(b.name)),
    heavy: heavyFor(data, top),
  })),
};
summary.drift = driftAcross(reports);

const md = renderMarkdown(summary, top);
if (outFile) {
  fs.mkdirSync(path.dirname(outFile), { recursive: true });
  fs.writeFileSync(outFile, md);
} else {
  process.stdout.write(md);
}
if (jsonOut) {
  fs.mkdirSync(path.dirname(jsonOut), { recursive: true });
  fs.writeFileSync(jsonOut, JSON.stringify(summary, null, 2) + "\n");
}

// ---------------------------------------------------------------------------

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (!a.startsWith("--")) continue;
    const key = a.slice(2);
    const next = argv[i + 1];
    if (next !== undefined && !next.startsWith("--")) {
      out[key] = next;
      i += 1;
    } else {
      out[key] = true;
    }
  }
  return out;
}

function readTargets(file) {
  if (!fs.existsSync(file)) {
    console.error(`dep-audit/report: missing ${file}; run scripts/dep-audit/run.sh first`);
    process.exit(1);
  }
  return fs
    .readFileSync(file, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => {
      const [name, p, sha, remote] = line.split("\t");
      return { name, path: p, sha, remote: remote ?? "" };
    });
}

/** Directory of a workspace member from its cargo package id. */
function packageDir(data, packageName) {
  const pkg = data.dependencies.packages.find(
    (p) => p.name === packageName && (p.is_workspace_member || p.is_root_package),
  );
  if (!pkg) return null;
  const m = /^path\+file:\/\/(.+?)(#.*)?$/.exec(pkg.id);
  return m ? decodeURIComponent(m[1]) : null;
}

/**
 * Sources that belong to a package: its own directory (scanned recursively,
 * so an implicit `src/` or an implicit `build.rs` next to `Cargo.toml` are
 * covered) plus the exact file for every explicit `path = "..."` target in
 * its Cargo.toml (`[[test]]`, `[[example]]`, `[[bench]]`, `[[bin]]`, `[lib]`,
 * `[package] build = "..."`).
 *
 * Explicit targets are tracked as single *files*, not their parent
 * directory: OpenHuman's root crate declares its integration tests as
 * `path = "../../tests/<name>.rs"`, and `tests/` holds one file per package
 * (see AGENTS.md). Adding that whole directory would let an unrelated
 * sibling test file reference a dependency and flip an actually-unused
 * dependency to "keep".
 */
function packageSourceDirs(dir) {
  const dirs = new Set([dir]);
  const files = new Set();
  const manifest = path.join(dir, "Cargo.toml");
  if (!fs.existsSync(manifest)) return { dirs: [...dirs], files: [...files] };
  const toml = fs.readFileSync(manifest, "utf8");
  let section = "";
  for (const raw of toml.split("\n")) {
    const line = raw.trim();
    const head = /^\[\[?([a-zA-Z0-9_.-]+)\]?\]/.exec(line);
    if (head) {
      section = head[1];
      continue;
    }
    if (section === "package") {
      const b = /^build\s*=\s*"([^"]+)"/.exec(line);
      if (b) files.add(path.resolve(dir, b[1]));
      continue;
    }
    if (!/^(test|example|bench|bin|lib)$/.test(section)) continue;
    const m = /^path\s*=\s*"([^"]+)"/.exec(line);
    if (m) files.add(path.resolve(dir, m[1]));
  }
  return { dirs: [...dirs], files: [...files] };
}

/**
 * Alias -> real crate name for dependencies renamed with `package = "..."`,
 * covering both `alias = { package = "real", ... }` and
 * `[dependencies.alias]` / `package = "real"` table forms. tinyanalyzer's
 * `unused[].dependency` (and the graph's `packages[].name`) disagree for a
 * renamed dependency: the former is the manifest key (what code actually
 * imports), the latter is the real crate name (what the resolved package is
 * called), so callers matching a dependency against the graph need this map.
 */
function dependencyAliasMap(dir) {
  const map = new Map();
  const manifest = path.join(dir, "Cargo.toml");
  if (!fs.existsSync(manifest)) return map;
  const toml = fs.readFileSync(manifest, "utf8");
  let section = "";
  let tableDepKey = null;
  for (const raw of toml.split("\n")) {
    const line = raw.trim();
    const head = /^\[([a-zA-Z0-9_.-]+)\]/.exec(line);
    if (head) {
      section = head[1];
      const table = /^(dependencies|dev-dependencies|build-dependencies)\.([A-Za-z0-9_-]+)$/.exec(section);
      tableDepKey = table ? table[2] : null;
      continue;
    }
    if (tableDepKey) {
      const pkg = /^package\s*=\s*"([^"]+)"/.exec(line);
      if (pkg) map.set(tableDepKey, pkg[1]);
      continue;
    }
    if (!/^(dependencies|dev-dependencies|build-dependencies)$/.test(section)) continue;
    const inline = /^([A-Za-z0-9_-]+)\s*=\s*\{([^}]*)\}/.exec(line);
    if (inline) {
      const pkg = /package\s*=\s*"([^"]+)"/.exec(inline[2]);
      if (pkg) map.set(inline[1], pkg[1]);
    }
  }
  return map;
}

/**
 * Textual re-check of an unused flag over the package's sources.
 *
 * Returns:
 *   "path"  — the crate is referenced the way Rust code references a crate
 *             (`foo::`, `use foo`, `extern crate foo`, `#[foo`, `foo!`), which
 *             tinyanalyzer misses only when it sits inside an attribute or a
 *             macro body. Treat as used.
 *   "word"  — the bare name occurs (comment, string, doc) but never as a path.
 *   "none"  — nothing at all.
 */
function textualUse(dir, dep) {
  if (!dir || !fs.existsSync(dir)) return "unknown";
  // Code imports a renamed dependency under its manifest alias (`dep`), not
  // under the crate's real name, so the alias is the correct identifier to
  // grep for here — see dependencyAliasMap's docstring for the name split.
  const ident = dep.replace(/-/g, "_");
  const { dirs, files } = packageSourceDirs(dir);
  const pathRe = `(\\b${ident}::|\\buse\\s+${ident}\\b|extern\\s+crate\\s+${ident}\\b|#\\[${ident}\\b|\\b${ident}!)`;
  if (grepAny(dirs, files, pathRe)) return "path";
  if (grepAny(dirs, files, `\\b${ident}\\b`)) return "word";
  return "none";
}

function grepAny(dirs, files, pattern) {
  const targets = [...dirs, ...files];
  if (targets.length === 0) return false;
  try {
    const out = execFileSync(
      "grep",
      ["-rlE", pattern, "--include=*.rs", "--exclude-dir=target", "--exclude-dir=vendor", ...targets],
      { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] },
    );
    return out.trim().length > 0;
  } catch (err) {
    // grep exits 1 when nothing matched (or a listed file does not exist);
    // anything else is a real failure we would rather surface as "used" than
    // as a false removal.
    return err.status !== 1;
  }
}

/**
 * Reverse adjacency (`to` id -> set of `from` ids) plus an id -> package index,
 * built once per target so the duplicate and unused sections can answer "who
 * pulls this in" without shelling out to `cargo tree`.
 */
function graphIndex(data) {
  if (data.__graph) return data.__graph;
  const byId = new Map(data.dependencies.packages.map((p) => [p.id, p]));
  const parents = new Map();
  for (const e of data.dependencies.edges) {
    if (!parents.has(e.to)) parents.set(e.to, new Set());
    parents.get(e.to).add(e.from);
  }
  data.__graph = { byId, parents };
  return data.__graph;
}

function shortName(id) {
  const m = /#(.+)@[^@]+$/.exec(id) ?? /\/([^/#]+)#[^#]*$/.exec(id);
  return m ? m[1] : id;
}

/**
 * The direct dependencies (external, `is_direct`) whose subtree contains the
 * package `id`, found by walking the reverse graph. Workspace members are
 * reported when the package is reached by them without any external direct
 * dependency in between (i.e. it *is* a direct dependency).
 */
function pulledInVia(data, id, limit = 6) {
  const { byId, parents } = graphIndex(data);
  const seen = new Set([id]);
  const queue = [id];
  const direct = new Set();
  const members = new Set();
  while (queue.length) {
    const cur = queue.shift();
    for (const from of parents.get(cur) ?? []) {
      if (seen.has(from)) continue;
      seen.add(from);
      const p = byId.get(from);
      if (!p) continue;
      if (p.is_workspace_member || p.is_root_package) {
        if (cur === id) members.add(p.name);
        continue;
      }
      if (p.is_direct) {
        direct.add(p.name);
        continue; // stop at the first direct dependency on each path
      }
      queue.push(from);
    }
  }
  const names = [...direct].sort();
  const label = names.length
    ? names.slice(0, limit).join(", ") + (names.length > limit ? `, +${names.length - limit}` : "")
    : "";
  const viaMembers = [...members].sort();
  if (viaMembers.length && !names.length) return `direct dep of ${viaMembers.join(", ")}`;
  if (viaMembers.length) return `direct dep of ${viaMembers.join(", ")}; also via ${label}`;
  return label || "(unreachable)";
}

/** Names of workspace packages other than `except` with a direct edge to any version of `depName`. */
function otherDependents(data, depName, except) {
  const { byId, parents } = graphIndex(data);
  const out = new Set();
  for (const p of data.dependencies.packages) {
    if (p.name !== depName) continue;
    for (const from of parents.get(p.id) ?? []) {
      const q = byId.get(from);
      if (q && q.name !== except && (q.is_workspace_member || q.is_root_package)) out.add(q.name);
    }
  }
  return [...out].sort();
}

/**
 * The package `depName` resolves to *for* workspace package `fromName`, found
 * through the edge list so a crate present at two versions reports the one
 * this package actually pulls, not the first by name.
 */
function resolvedDependency(data, fromName, depName) {
  const { byId } = graphIndex(data);
  const from = data.dependencies.packages.find(
    (p) => p.name === fromName && (p.is_workspace_member || p.is_root_package),
  );
  if (from) {
    for (const e of data.dependencies.edges) {
      if (e.from !== from.id) continue;
      const to = byId.get(e.to);
      if (to && to.name === depName) return to;
    }
  }
  return data.dependencies.packages.find((p) => p.name === depName) ?? null;
}

function unusedFor(target, data) {
  const rows = [];
  const seen = new Set();
  for (const u of data.dependencies.unused) {
    const key = `${u.package}\u0000${u.dependency}`;
    // The same dependency declared as both normal and dev shows up twice.
    const kinds = data.dependencies.unused
      .filter((v) => v.package === u.package && v.dependency === u.dependency)
      .map((v) => v.kind);
    if (seen.has(key)) continue;
    seen.add(key);
    const dir = packageDir(data, u.package);
    const evidence = textualUse(dir, u.dependency);
    // `u.dependency` is the manifest key; resolve a rename (`alias = {
    // package = "real" }`) to the crate name the graph indexes packages by
    // before looking anything up there.
    const realName = dir ? (dependencyAliasMap(dir).get(u.dependency) ?? u.dependency) : u.dependency;
    const pkg = resolvedDependency(data, u.package, realName);
    const others = otherDependents(data, realName, u.package);
    rows.push({
      package: u.package,
      dependency: u.dependency,
      kinds: [...new Set(kinds)],
      version: pkg?.version ?? null,
      exclusive_count: pkg?.exclusive_count ?? null,
      other_dependents: others,
      // Crates that actually leave the target's graph if this one edge is cut.
      graph_win: others.length ? 0 : (pkg?.exclusive_count ?? 0),
      evidence,
      verdict: evidence === "path" ? "keep" : "remove",
    });
  }
  return rows.sort(
    (a, b) =>
      (a.verdict === "remove" ? 0 : 1) - (b.verdict === "remove" ? 0 : 1) ||
      (a.evidence === "none" ? 0 : 1) - (b.evidence === "none" ? 0 : 1) ||
      b.graph_win - a.graph_win ||
      (b.exclusive_count ?? 0) - (a.exclusive_count ?? 0) ||
      a.package.localeCompare(b.package) ||
      a.dependency.localeCompare(b.dependency),
  );
}

function heavyFor(data, n) {
  return data.dependencies.packages
    .filter((p) => p.is_direct && !p.is_workspace_member && !p.is_root_package)
    .map((p) => ({
      name: p.name,
      version: p.version,
      kinds: p.kinds,
      exclusive_count: p.exclusive_count,
      transitive_count: p.transitive_count,
      source_bytes: p.source_bytes,
      features: p.features,
    }))
    .sort((a, b) => b.exclusive_count - a.exclusive_count || b.source_bytes - a.source_bytes)
    .slice(0, n);
}

/**
 * For every crate that is a *direct* dependency of two or more targets, the
 * resolved version in each. Only rows where the versions differ are kept.
 */
function driftAcross(reports) {
  const byCrate = new Map();
  for (const { target, data } of reports) {
    for (const p of data.dependencies.packages) {
      if (!p.is_direct || p.is_workspace_member || p.is_root_package) continue;
      if (!byCrate.has(p.name)) byCrate.set(p.name, new Map());
      const m = byCrate.get(p.name);
      if (!m.has(target.name)) m.set(target.name, new Set());
      m.get(target.name).add(p.version);
    }
  }
  const rows = [];
  let patchOnly = 0;
  for (const [name, perTarget] of byCrate) {
    if (perTarget.size < 2) continue;
    const versions = new Set([...perTarget.values()].flatMap((s) => [...s]));
    if (versions.size < 2) continue;
    // Patch-level drift (1.0.103 vs 1.0.104) is lockfile staleness; cargo
    // unifies it in the root build. Only semver-incompatible drift costs a
    // second copy, so only that is reported.
    const compat = new Set([...versions].map(compatKey));
    if (compat.size < 2) {
      patchOnly += 1;
      continue;
    }
    const byVersion = new Map();
    for (const [t, vs] of perTarget) {
      for (const v of vs) {
        if (!byVersion.has(v)) byVersion.set(v, []);
        byVersion.get(v).push(t);
      }
    }
    rows.push({
      name,
      versions: [...versions].sort(semverish),
      by_version: [...byVersion.entries()]
        .sort((a, b) => semverish(a[0], b[0]))
        .map(([v, ts]) => ({ version: v, targets: ts.sort() })),
    });
  }
  rows.sort((a, b) => b.versions.length - a.versions.length || a.name.localeCompare(b.name));
  rows.patch_only = patchOnly;
  return rows;
}

/**
 * Semver compatibility bucket: `x.y.z` -> `x`, `0.x.y` -> `0.x`, and
 * `0.0.z` -> `0.0.z` (kept per-patch: Cargo treats every `0.0.z` as its own
 * incompatible version, so `0.0.1` and `0.0.2` must not collapse together).
 */
function compatKey(v) {
  const [major, minor, patch] = v.split(/[.+-]/);
  if (major !== "0") return major;
  if (minor !== "0") return `0.${minor}`;
  return `0.0.${patch}`;
}

function semverish(a, b) {
  const pa = a.split(/[.+-]/).map((x) => (Number.isNaN(Number(x)) ? x : Number(x)));
  const pb = b.split(/[.+-]/).map((x) => (Number.isNaN(Number(x)) ? x : Number(x)));
  for (let i = 0; i < Math.max(pa.length, pb.length); i += 1) {
    if (pa[i] === pb[i]) continue;
    if (pa[i] === undefined) return -1;
    if (pb[i] === undefined) return 1;
    return pa[i] < pb[i] ? -1 : 1;
  }
  return 0;
}

function mib(bytes) {
  return `${(bytes / 1048576).toFixed(1)} MiB`;
}

function code(s) {
  return `\`${s}\``;
}

function renderMarkdown(summary, n) {
  const lines = [];
  const push = (...xs) => lines.push(...xs);

  push(
    `# Dependency audit`,
    ``,
    `Generated ${summary.generated_at} by ${code("scripts/dep-audit/run.sh")} (${summary.tinyanalyzer}).`,
    `Re-run with ${code("pnpm dep:audit")}; see ${code("scripts/dep-audit/README.md")} for how to act on each section.`,
    ``,
    `## Targets`,
    ``,
    `| Target | Path | Commit | Direct deps | Crates in graph | Unused flags | Duplicate versions |`,
    `| --- | --- | --- | ---: | ---: | ---: | ---: |`,
  );
  for (const t of summary.targets) {
    push(
      `| ${t.name} | ${code(t.path)} | ${code(t.commit.slice(0, 10))} | ${t.direct} | ${t.external_packages} | ${t.unused.length} | ${t.duplicates.length} |`,
    );
  }

  // --- Unused --------------------------------------------------------------
  push(``, `## 1. Declared dependencies no source file names`, ``);
  push(
    `Verdict ${code("remove")}: nothing in the package's ${code("*.rs")} files (including ${code("[[test]]")}/${code("[[example]]")} targets declared by path) references the crate as ${code("crate::…")}, ${code("use crate")}, ${code("#[crate…")} or ${code("crate!")}. Delete the line from ${code("Cargo.toml")} and build; if it was an optional dependency, drop the ${code("dep:")} feature too. "name only" means the word occurs in a comment or string, which is not a use.`,
    `Verdict ${code("keep")}: tinyanalyzer saw no ${code("use")}/path, but one exists inside an attribute or macro body (${code("#[tokio::test]")}, ${code("#[derive(thiserror::Error)]")}) — a false positive of the tool, listed so the count is honest.`,
    `${code("Graph win")}: crates that leave this target's build if the line is deleted. It is 0 when another package in the same workspace still depends on the crate — the manifest still gets cleaner, the build does not get smaller.`,
    `Crates listed in ${code("[dependencies].ignore_unused")} of ${code("scripts/dep-audit/tinyanalyzer.toml")} are not reported at all.`,
    ``,
  );
  const unusedRows = summary.targets.flatMap((t) => t.unused.map((u) => ({ target: t.name, ...u })));
  if (unusedRows.length === 0) {
    push(`_None._`);
  } else {
    push(
      `| Target | Package | Dependency | Kind | Resolved | Graph win | Verdict |`,
      `| --- | --- | --- | --- | --- | ---: | --- |`,
    );
    for (const u of unusedRows) {
      push(
        `| ${u.target} | ${u.package} | ${code(u.dependency)} | ${u.kinds.join(", ")} | ${u.version ?? "—"} | ${u.version == null ? "—" : u.other_dependents.length ? `0 (kept by ${u.other_dependents.join(", ")})` : `${u.graph_win} crate${u.graph_win === 1 ? "" : "s"}`} | ${u.verdict === "remove" ? (u.evidence === "word" ? "**remove** (name only)" : "**remove**") : "keep (attribute/macro path)"} |`,
      );
    }
  }

  // --- Duplicates ----------------------------------------------------------
  push(``, `## 2. Crates resolved at more than one version`, ``);
  push(
    `Each version is compiled and linked separately. The ${code("root")} row is the one that costs the shipped build; submodule rows show where a pin should move so the root can unify.`,
    `${code("Pulled in via")} names the direct dependencies whose subtree carries that version (walked from tinyanalyzer's edge list; ${code("cargo tree -i <crate>@<version>")} gives the full chain). Unifying means raising whichever of those pins the older one, or dropping it.`,
    ``,
  );
  for (const t of summary.targets) {
    if (t.duplicates.length === 0) continue;
    push(`### ${t.name} — ${t.duplicates.length} crate(s)`, ``);
    push(`| Crate | Version | Pulled in via |`, `| --- | --- | --- |`);
    for (const d of t.duplicates) {
      d.versions.forEach((v, i) => {
        push(`| ${i === 0 ? code(d.name) : ""} | ${code(v)} | ${d.via[v]} |`);
      });
    }
    push(``);
  }
  const dupIndex = new Map();
  for (const t of summary.targets) {
    for (const d of t.duplicates) {
      if (!dupIndex.has(d.name)) dupIndex.set(d.name, []);
      dupIndex.get(d.name).push(t.name);
    }
  }
  const widespread = [...dupIndex.entries()].filter(([, ts]) => ts.length >= 3).sort((a, b) => b[1].length - a[1].length);
  if (widespread.length) {
    push(`### Duplicated in three or more targets`, ``, `| Crate | Targets |`, `| --- | --- |`);
    for (const [name, ts] of widespread) push(`| ${code(name)} | ${ts.length}: ${ts.join(", ")} |`);
    push(``);
  }

  // --- Heavy ---------------------------------------------------------------
  push(`## 3. Heaviest direct dependencies (top ${n} per target)`, ``);
  push(
    `${code("Exclusive")} is how many crates leave the graph if this dependency is dropped — the honest cost. ${code("Reaches")} is the transitive count, most of which something else pulls in anyway. ${code("Source")} is checked-out source size, not binary size.`,
    `A high exclusive count usually means default features pulling in a subtree you do not use: try ${code("default-features = false")} plus the two or three features needed.`,
    ``,
  );
  for (const t of summary.targets) {
    if (t.heavy.length === 0) continue;
    push(`### ${t.name}`, ``);
    push(`| Direct dependency | Version | Kind | Exclusive | Reaches | Source |`, `| --- | --- | --- | ---: | ---: | ---: |`);
    for (const h of t.heavy) {
      push(
        `| ${code(h.name)} | ${h.version} | ${h.kinds.join(", ")} | ${h.exclusive_count} | ${h.transitive_count} | ${mib(h.source_bytes)} |`,
      );
    }
    push(``);
  }

  // --- Drift ---------------------------------------------------------------
  push(`## 4. Version drift across repositories`, ``);
  push(
    `Crates that two or more targets depend on *directly* but resolve to semver-incompatible versions. When the root workspace ${code("[patch]")}-es a submodule in, both versions end up in the root build (section 2), so aligning the submodule's requirement with the root's removes a duplicate for free. Rows are ordered by how many distinct versions are in play.`,
    ``,
  );
  if (summary.drift.length === 0) {
    push(`_None._`);
  } else {
    push(`| Crate | Version | Targets |`, `| --- | --- | --- |`);
    for (const d of summary.drift) {
      d.by_version.forEach((bv, i) => {
        push(`| ${i === 0 ? code(d.name) : ""} | ${code(bv.version)} | ${bv.targets.join(", ")} |`);
      });
    }
    push(``, `_${summary.drift.patch_only} more crate(s) drift only at patch level (lockfile staleness; cargo unifies them) and are not listed._`);
  }
  push(``);
  return lines.join("\n");
}
