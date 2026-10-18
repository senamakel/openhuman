// Pure helpers for scripts/modules/release-and-repin.mjs
// (tinyhumansai/openhuman#7615, epic #7523).
//
// Every digest written here is copied VERBATIM from a published release's
// checksum.toml. Nothing in this file hashes anything: a digest computed from a
// local build agrees with itself no matter what was served, which defeats the
// pin. A test pins that by scanning this file for any hashing import.
import { rustStructBodyAfter } from "./rust-struct-body.mjs";

/** `[sha256]` table of `"archive" = "hex"` pairs -> Map(archive -> hex). */
export function parseChecksumToml(text) {
  const sums = new Map();
  sums.librarySha256 = new Map();
  let inTable = false;
  let tableName = "";
  for (const raw of String(text).split("\n")) {
    const line = raw.replace(/#.*$/, "").trim();
    if (!line) continue;
    const table = line.match(/^\[([^\]]+)\]$/);
    if (table) {
      tableName = table[1].trim();
      inTable = tableName === "sha256" || tableName === "library_sha256";
      continue;
    }
    if (!inTable) continue;
    const pair = line.match(/^"([^"]+)"\s*=\s*"([^"]*)"$/);
    if (!pair) throw new Error(`checksum.toml: cannot parse line: ${line}`);
    if (!/^[0-9a-f]{64}$/.test(pair[2])) {
      throw new Error(
        `checksum.toml: ${pair[1]} in [${tableName}] is not a 64-hex lowercase sha256`,
      );
    }
    (tableName === "sha256" ? sums : sums.librarySha256).set(pair[1], pair[2]);
  }
  if (sums.size === 0) throw new Error("checksum.toml: no [sha256] entries");
  return sums;
}

/** `X.Y.Z` bumped by patch|minor|major. */
export function bumpVersion(version, bump) {
  const m = String(version).match(/^(\d+)\.(\d+)\.(\d+)$/);
  if (!m) throw new Error(`not a plain X.Y.Z version: ${version}`);
  let [major, minor, patch] = m.slice(1).map(Number);
  if (bump === "major") [major, minor, patch] = [major + 1, 0, 0];
  else if (bump === "minor") [minor, patch] = [minor + 1, 0];
  else if (bump === "patch") patch += 1;
  else throw new Error(`--bump must be patch, minor or major, got ${bump}`);
  return `${major}.${minor}.${patch}`;
}

const RECORD_RE =
  /(const [A-Z0-9_]+: ModuleRecord = ModuleRecord \{)([\s\S]*?)(\n\};)/g;
const ASSET_RE =
  /(ReleaseAsset|PlatformAsset) \{\s*host_key: "([^"]+)",\s*archive: "([^"]+)",\s*sha256: "([^"]+)",\s*(?:library_sha256: (?:Some\("[^"]*"\)|None),\s*)?\}/g;

function releaseBody(body) {
  return rustStructBodyAfter(body, "release: ReleaseRecord") ?? body;
}

/**
 * Rewrite one record in a records_*.rs source to release `newVersion`.
 *
 * `version`, `release_url` and each asset's `archive` and `sha256` change;
 * the archive name is the old one with the version swapped and must exist in
 * `checksums`. Returns `{ src, changed }`, or `{ src, changed: false }` when
 * the file does not hold the record. Throws when an asset has no checksum.
 */
export function rewriteRecord(src, id, newVersion, checksums) {
  let changed = false;
  const out = src.replace(RECORD_RE, (whole, head, body, tail) => {
    if (!new RegExp(`^\\s*id: "${id}",`, "m").test(releaseBody(body)))
      return whole;
    const oldVersion = releaseBody(body).match(/^\s*version: "([^"]+)"/m)?.[1];
    if (!oldVersion) throw new Error(`${id}: record has no version`);
    changed = true;
    const missing = [];
    let next = body
      .replace(/^(\s*version: ")[^"]+(")/m, `$1${newVersion}$2`)
      .replace(
        /^(\s*release_url: ")([^"]*)(")/m,
        (_, a, url, c) =>
          `${a}${url.replace(/\/tag\/v[^/]+$/, `/tag/v${newVersion}`)}${c}`,
      );
    next = next.replace(ASSET_RE, (asset, kind, hostKey, archive) => {
      const renamed = archive.replace(oldVersion, newVersion);
      const sha = checksums.get(renamed);
      if (!sha) {
        missing.push(renamed);
        return asset;
      }
      const librarySha = checksums.librarySha256?.get(renamed);
      const libraryField =
        kind === "ReleaseAsset"
          ? `\n            library_sha256: ${librarySha ? `Some("${librarySha}")` : "None"},`
          : "";
      return `${kind} {\n            host_key: "${hostKey}",\n            archive: "${renamed}",\n            sha256: "${sha}",${libraryField}\n        }`;
    });
    if (missing.length > 0) {
      throw new Error(
        `${id}: v${newVersion} checksum.toml has no entry for ${missing.join(", ")}`,
      );
    }
    return `${head}${next}${tail}`;
  });
  return { src: out, changed };
}

/** Archive names in `checksums` that no asset of the record uses. */
export function unusedChecksums(src, id, checksums) {
  const used = new Set();
  for (const record of src.matchAll(RECORD_RE)) {
    const release = releaseBody(record[2]);
    if (!new RegExp(`^\\s*id: "${id}",`, "m").test(release)) continue;
    for (const a of release.matchAll(ASSET_RE)) used.add(a[3]);
  }
  return [...checksums.keys()].filter((name) => !used.has(name));
}

/**
 * Update module-provider-pins.json for a provider record: its release version
 * and the commit it was built against. Other entries are left untouched.
 */
export function updateProviderPins(json, id, version, builtAgainst) {
  const providers = json.providers ?? {};
  if (!providers[id]) return { json, changed: false };
  return {
    changed: true,
    json: {
      ...json,
      providers: {
        ...providers,
        [id]: { ...providers[id], version, built_against: builtAgainst },
      },
    },
  };
}

/** Drop an exemption for `id` once its pins agree. */
export function dropExemption(json, id) {
  const exemptions = json.exemptions ?? [];
  const kept = exemptions.filter((e) => e.id !== id);
  return {
    json: { ...json, exemptions: kept },
    changed: kept.length !== exemptions.length,
  };
}
