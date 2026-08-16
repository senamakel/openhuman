// Detects a crate that the build *claims* to consume from `vendor/` but has
// actually been inlined into this tree.
//
// WHY THIS EXISTS (#5559). On 2026-08-12, `3ee5a3cad` ("refactor: run tiny
// domains as TinyBus modules") removed the `vendor/tinywallet` submodule and
// inlined ~3,700 lines of crate source into `src/openhuman/web3/wallet/`,
// rewriting every `crate::` path. Nothing failed. The manifest comments and
// AGENTS.md kept describing a vendored crate, so code and docs contradicted
// each other and the contradiction was invisible to CI.
//
// An inlined copy of a shared crate is a SILENT FORK. Four fixes accrued in
// OpenHuman's copy that no other host ever saw, one of them a SLIP-10
// key-derivation bug: a path segment already carrying the hardening bit was
// OR-ed with it again, so `m/44'/501'/2147483648'` and `m/44'/501'/0'` derived
// the same key. Restored in #5533 / tinywallet#16 / tinywallet#17.
//
// The same failure was live again on `tinydocs` when this guard was written:
// four manifest comment blocks described it as vendored — including
// `git submodule update --init vendor/tinydocs`, an instruction that could not
// work — while there was no `vendor/tinydocs`, no `.gitmodules` entry and no
// dependency declaration. Its spec types sat inlined under
// `src/openhuman/tools/impl/document/`.
//
// THREE ASSERTIONS, applied to every claimed crate:
//
//   1. `.gitmodules` declares `vendor/<name>` as a submodule.
//   2. The git index records `vendor/<name>` as a gitlink (mode 160000). This
//      is the one that catches an inlining: replacing a submodule with real
//      files changes the index entry, and it is checked through the index
//      rather than the filesystem so the guard works on a runner that never
//      ran `git submodule update --init`.
//   3. Some manifest declares a `path = "…/vendor/<name>"` dependency. A
//      submodule nobody depends on is a checkout nobody compiles — which is
//      the state `3ee5a3cad` would have left behind had it deleted only the
//      dependency line.
//
// WHERE CLAIMS COME FROM — three independent sources, deliberately, because
// each covers a way the previous one can be edited away:
//
//   - Manifest comments that describe a crate as vendored. This is what
//     `3ee5a3cad` left behind untouched, so it is the source that would have
//     caught it on the PR that introduced it.
//   - `path = "…/vendor/<name>"` dependency declarations. Catches the reverse
//     shape: a real dependency on a vendor directory that is not a submodule.
//   - The compiled-in module registry (`src/openhuman/modules/registry.rs`).
//     A module-backed capability shares its wire contract with the host as a
//     crate; a module id with no vendored crate behind it means the host has
//     its own copy of the contract the module validates against, which is the
//     drift the extraction existed to prevent.
//
// Like `feature-forwarding.mjs`, this is a deliberately narrow scanner rather
// than a general TOML parser: the repo has no TOML dependency for Node and the
// shapes involved are well known.

/**
 * Crates that are named as vendored (or module-backed) but deliberately have
 * NO vendored source dependency, mapped to why.
 *
 * Adding an entry is a deliberate decision, not a way to silence the guard.
 * The reason string is the only thing that keeps "excluded on purpose"
 * distinguishable from "forgotten" — which is exactly the ambiguity that let
 * `3ee5a3cad`'s inlining sit in the tree with its documentation intact.
 *
 * A waiver here is checked for staleness: if the crate turns out to be
 * properly vendored after all, the entry fails as stale rather than sitting
 * around asserting something that stopped being true.
 */
export const INTENTIONALLY_NOT_VENDORED = {
  // 'some-crate': 'Reason this crate has no vendored source dependency.',
  tinyjuice:
    'Module-only: the host never links the crate. TinyJuice left the dependency graph in 4e4c8ffc1 ("load TinyJuice outside dependency graph"), which in the SAME commit re-declared the wire types host-side in src/openhuman/inference/tokenjuice/types.rs, saying so in that file\'s own doc comment. That is a recorded decision, not the silent contradiction this guard exists to catch — but it does mean two declarations of one contract, so if that pair ever drifts the fix is to share the crate for its types (the tinydocs shape) rather than to widen this waiver.',
  tinyvoice:
    'Module-only: the host never links the crate. src/openhuman/modules/voice.rs speaks to ai.tinyhumans.tinyvoice.Voice over the bus with local serde types and base64 framing; no crate-level contract is shared in either direction, so there is nothing for a vendored source dependency to provide.',
};

/**
 * Vendor directories whose sources are checked into THIS repo rather than
 * pulled in as a submodule, mapped to why.
 *
 * These skip assertions 1 and 2 (no `.gitmodules` entry, no gitlink) but still
 * have to be tracked and depended on. The distinction matters: an in-tree
 * vendor directory is a fork by design and reviewed as this repo's own code,
 * whereas a submodule that quietly became one is the failure this guard is
 * here to catch. Also staleness-checked — an entry that has since become a
 * real submodule fails rather than silently exempting it.
 */
export const VENDORED_IN_TREE = {
  'motosan-ai-oauth':
    "Small OAuth helper carried as in-tree source (vendor/motosan-ai-oauth) rather than a submodule: it has no upstream repo of its own in the tinyhumansai org, so there is nothing to point a submodule at. Reviewed as this repo's own code.",
};

/**
 * Strip TOML `#` comments while respecting quoted strings, returning the code
 * and the comment text separately.
 *
 * Both halves are needed: claims come out of the comments, dependency
 * declarations out of the code. Splitting once here keeps a `#` inside a
 * quoted value from being read as a comment in either direction.
 */
export function splitTomlComments(text) {
  const code = [];
  const comments = [];
  for (const line of text.split(/\r?\n/)) {
    let quote = null;
    let cut = -1;
    for (let i = 0; i < line.length; i++) {
      const ch = line[i];
      if (quote) {
        if (ch === quote && line[i - 1] !== '\\') quote = null;
      } else if (ch === '"' || ch === "'") {
        quote = ch;
      } else if (ch === '#') {
        cut = i;
        break;
      }
    }
    if (cut === -1) {
      code.push(line);
    } else {
      code.push(line.slice(0, cut));
      comments.push(line.slice(cut + 1));
    }
  }
  return { code: code.join('\n'), comments };
}

/** Trailing punctuation a path picks up from prose: "vendor/tinydocs`." */
const NAME_RE = /vendor\/([A-Za-z0-9][A-Za-z0-9_.-]*)/g;

/**
 * A comment line only counts as a vendoring CLAIM when it says how the crate
 * is consumed — "submodule", or "vendored as". A passing mention of a path
 * (`see vendor/tauri-cef/.../tauri.rs`) is documentation of where something
 * lives, not an assertion that this build consumes it from there, and treating
 * it as one would make the guard fire on prose.
 */
const CLAIM_RE = /\bsubmodules?\b|\bvendored as\b/i;

/**
 * Vendoring claims made in a manifest's comments.
 *
 * Paths are resolved against the REPO ROOT, not the manifest's directory:
 * `vendor/` is a repo-root convention here, and the shell manifest's comments
 * say so explicitly ("repo-root vendor/tinyagents").
 *
 * @returns {Map<string, string>} crate name → the comment line that claimed it
 */
export function collectDocumentedClaims(tomlText) {
  const { comments } = splitTomlComments(tomlText);
  const claims = new Map();
  for (const comment of comments) {
    if (!CLAIM_RE.test(comment)) continue;
    for (const match of comment.matchAll(NAME_RE)) {
      if (!claims.has(match[1])) claims.set(match[1], comment.trim());
    }
  }
  return claims;
}

/**
 * Crates declared as `path = "…/vendor/<name>[/subpath]"` dependencies.
 *
 * Sub-crate paths (`vendor/tinymemory/core`, `vendor/tinyplace/sdk/rust`)
 * collapse onto their vendor root, which is the unit a submodule tracks.
 *
 * `[patch.crates-io]` entries count. A patch redirects the whole graph onto
 * the vendored source, so a patch pointing at a directory that is not a
 * submodule is the same silent fork by another route.
 *
 * @returns {Set<string>}
 */
export function collectDeclaredDependencies(tomlText) {
  const { code } = splitTomlComments(tomlText);
  const declared = new Set();
  for (const match of code.matchAll(/path\s*=\s*"([^"]*vendor\/[^"]*)"/g)) {
    const parts = match[1].split('vendor/');
    const tail = parts[parts.length - 1];
    const name = tail.split('/')[0];
    if (name) declared.add(name);
  }
  return declared;
}

/**
 * Module ids from the compiled-in registry (`modules::registry::ALL`).
 *
 * Read out of the source rather than a build artifact so the check costs
 * nothing and runs on a runner with no Rust toolchain.
 *
 * @returns {Set<string>}
 */
export function collectModuleIds(registrySource) {
  const ids = new Set();
  for (const match of registrySource.matchAll(/^\s*id:\s*"([^"]+)"\s*,/gm)) {
    ids.add(match[1]);
  }
  return ids;
}

/** Submodule paths declared in `.gitmodules`. */
export function parseGitmodulesPaths(gitmodulesText) {
  const paths = new Set();
  for (const match of gitmodulesText.matchAll(/^\s*path\s*=\s*(.+)$/gm)) {
    paths.add(match[1].trim());
  }
  return paths;
}

/**
 * Run the three assertions over every claimed crate.
 *
 * @param {object} input
 * @param {Map<string, {sources: string[], evidence: string}>} input.claims
 * @param {Set<string>} input.submodulePaths  paths from `.gitmodules`
 * @param {Set<string>} input.gitlinkPaths    index entries with mode 160000
 * @param {Set<string>} input.trackedVendorDirs vendor dirs with tracked files
 * @param {Set<string>} input.declaredDependencies
 * @param {object} [input.notVendored]  allow-list (name → reason)
 * @param {object} [input.inTree]       allow-list (name → reason)
 */
export function checkVendoredCrates({
  claims,
  submodulePaths,
  gitlinkPaths,
  trackedVendorDirs,
  declaredDependencies,
  notVendored = INTENTIONALLY_NOT_VENDORED,
  inTree = VENDORED_IN_TREE,
}) {
  const failures = [];
  const waived = [];
  const staleWaivers = [];
  const ok = [];

  for (const [name, claim] of claims) {
    const vendorPath = `vendor/${name}`;
    const isSubmodule = submodulePaths.has(vendorPath);
    const isGitlink = gitlinkPaths.has(vendorPath);
    const isTracked = trackedVendorDirs.has(vendorPath);
    const isDeclared = declaredDependencies.has(name);
    const where = claim.sources.join(', ');

    if (Object.hasOwn(notVendored, name)) {
      if (isSubmodule || isDeclared) {
        staleWaivers.push(
          `${name}: listed in INTENTIONALLY_NOT_VENDORED, but it IS vendored now ` +
            `(${isSubmodule ? '.gitmodules entry' : 'dependency declared'}). Remove the waiver.`,
        );
      } else {
        waived.push(`${name}: not vendored — ${notVendored[name]}`);
      }
      continue;
    }

    if (Object.hasOwn(inTree, name)) {
      if (isSubmodule) {
        staleWaivers.push(
          `${name}: listed in VENDORED_IN_TREE, but ${vendorPath} is a real submodule now. ` +
            'Remove the waiver so the normal assertions apply.',
        );
      } else if (!isTracked) {
        failures.push(
          `${name}: declared in VENDORED_IN_TREE but ${vendorPath} has no tracked files. ` +
            'An in-tree vendor directory must be committed to this repo.',
        );
      } else if (!isDeclared) {
        failures.push(
          `${name}: ${vendorPath} is committed in-tree but no manifest declares a ` +
            `path dependency on it. Vendored source nothing depends on is dead weight.`,
        );
      } else {
        waived.push(`${name}: vendored in-tree — ${inTree[name]}`);
      }
      continue;
    }

    const missing = [];
    if (!isSubmodule) missing.push(`no \`path = ${vendorPath}\` entry in .gitmodules`);
    if (!isGitlink)
      missing.push(
        `${vendorPath} is not a submodule in the git index` +
          (isTracked ? ' (it holds tracked files — the crate has been INLINED)' : ' (path absent)'),
      );
    if (!isDeclared) missing.push(`no manifest declares a \`path = "…/${vendorPath}"\` dependency`);

    if (missing.length === 0) {
      ok.push(name);
      continue;
    }
    failures.push(
      `${name}: claimed as vendored by ${where}, but ${missing.join('; ')}.\n` +
        `    evidence: ${claim.evidence}`,
    );
  }

  return {
    ok: failures.length === 0 && staleWaivers.length === 0,
    checked: ok,
    failures,
    waived,
    staleWaivers,
  };
}

/** Human-readable report for the CLI. */
export function formatReport(result) {
  const lines = [];
  if (result.checked.length) {
    lines.push(`Vendored crates verified: ${result.checked.sort().join(', ')}`);
  }
  for (const note of result.waived.sort()) lines.push(`WAIVED  ${note}`);
  for (const stale of result.staleWaivers.sort()) lines.push(`STALE   ${stale}`);
  for (const failure of result.failures.sort()) lines.push(`FAIL    ${failure}`);
  if (result.ok) {
    lines.push(
      '',
      'OK: every crate documented or registered as vendored is a real submodule with a path dependency.',
    );
  } else {
    lines.push(
      '',
      'A crate the build describes as vendored is not actually consumed from vendor/.',
      'That is how `3ee5a3cad` inlined tinywallet — a silent fork that hid a SLIP-10',
      'key-derivation bug from every other host (#5533).',
      '',
      'Fix by restoring the submodule and the path dependency, or — if the crate',
      'genuinely must not be vendored — add it to INTENTIONALLY_NOT_VENDORED (or',
      'VENDORED_IN_TREE) in scripts/lib/vendored-crates.mjs WITH A REASON.',
    );
  }
  return lines.join('\n');
}
