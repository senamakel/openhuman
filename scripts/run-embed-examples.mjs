#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export function discoverExamples(directory = resolve(root, "crates/openhuman-embed/examples")) {
  return readdirSync(directory).filter((file) => file.endsWith(".rs")).sort().map((file) => {
    const source = readFileSync(resolve(directory, file), "utf8");
    const feature = source.match(/^\/\/! Feature: (.+)$/m)?.[1]?.trim();
    if (!source.includes("//! Title:") || !source.includes("//! Run:") || !source.includes("// ANCHOR:")) {
      throw new Error(`${file}: missing example metadata or documentation anchor`);
    }
    return { name: file.slice(0, -3), features: feature && feature !== "default" ? [feature] : [] };
  });
}
export function offlineEnvironment(environment) {
  return Object.fromEntries(Object.entries(environment).filter(([name]) =>
    !name.startsWith("OPENHUMAN_EXAMPLE_") && !name.startsWith("OPENHUMAN_BACKEND_") &&
    !name.startsWith("OPENHUMAN_KEYRING_") &&
    name !== "OPENHUMAN_STORAGE_URL" && name !== "OPENHUMAN_WORKSPACE"));
}
export function assertExampleOutput(name, result) {
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${name} exited ${result.status}\n${result.stdout}\n${result.stderr}`);
  if (!result.stdout.split(/\r?\n/).includes(`EXAMPLE_OK ${name}`)) {
    throw new Error(`${name}: successful exit without behavioral assertion marker\n${result.stdout}`);
  }
}
export function runExamples({ run = spawnSync, examples = discoverExamples(), environment = process.env } = {}) {
  // Keep the feature graph identical for every run so Cargo reuses one build.
  const features = [...new Set(examples.flatMap((example) => example.features))].sort();
  for (const example of examples) {
    const args = ["run", "--quiet", "-p", "openhuman-embed", "--example", example.name];
    if (features.length) args.push("--features", features.join(","));
    const result = run(resolve(root, "scripts/ci-cancel-aware.sh"), ["cargo", ...args], {
      cwd: root, encoding: "utf8", env: offlineEnvironment(environment), timeout: 1800000, maxBuffer: 16 * 1024 * 1024,
    });
    assertExampleOutput(example.name, result);
    console.log(`PASS ${example.name}`);
  }
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.includes("--list")) console.log(discoverExamples().map((example) => example.name).join("\n"));
  else runExamples();
}
