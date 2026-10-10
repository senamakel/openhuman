import test from 'node:test';
import assert from 'node:assert/strict';
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

function check(files) {
  const root = mkdtempSync(resolve(tmpdir(), 'embed-contract-boundary-'));
  try {
    mkdirSync(resolve(root, 'scripts/ci'), { recursive: true });
    mkdirSync(resolve(root, 'scripts/lib'), { recursive: true });
    const bridge = 'vendor/tinyagents/crates/tinyagents-harness/src/providers/claude_code/bridge.rs';
    mkdirSync(dirname(resolve(root, bridge)), { recursive: true });
    writeFileSync(resolve(root, bridge), 'struct ChatMessage { pub(crate) role: String, pub(crate) content: String, }\n');
    copyFileSync(new URL('../ci/check-agent-runtime-boundary.mjs', import.meta.url), resolve(root, 'scripts/ci/check-agent-runtime-boundary.mjs'));
    for (const helper of ['runtime-boundary-types.mjs', 'agent-sdk-contracts.mjs']) {
      copyFileSync(new URL(`../lib/${helper}`, import.meta.url), resolve(root, `scripts/lib/${helper}`));
    }
    writeFileSync(resolve(root, 'scripts/ci/agent-runtime-boundary-baseline.json'), '[]\n');
    for (const [path, source] of Object.entries(files)) {
      mkdirSync(dirname(resolve(root, path)), { recursive: true });
      writeFileSync(resolve(root, path), source);
    }
    const result = spawnSync(process.execPath, [resolve(root, 'scripts/ci/check-agent-runtime-boundary.mjs')], { encoding: 'utf8' });
    assert.equal(result.error, undefined);
    return { status: result.status, output: result.stdout + result.stderr };
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

const contracts = {
  'crates/openhuman-core/src/agent/host_overrides.rs': 'pub use tinyagents_harness::cancel::CancellationToken;\n',
  'crates/openhuman-embed/src/config.rs': 'pub use tinytools::{\n DefaultEffect, Patterns, RuleEffect, Surface, ToolMatcher, ToolRule, ToolRules,\n};\n',
  'crates/openhuman-embed/src/lib.rs': 'pub mod providers {\n pub use tinyinference_llm::message::MessageDelta;\n pub use tinyinference_llm::model::{\n ChatModel, DeferredHandle, DeferredStatus, ModelProfile, ModelRequest, ModelResponse,\n ModelStream, ModelStreamItem, ModelStreamMetadata,\n };\n pub use tinyinference_llm::{Error, Result};\n}\n',
};

test('Embed exposes the native provider and policy contracts without copying their owners', () => {
  const result = check(contracts);
  assert.equal(result.status, 0, result.output);
});

test('the public contract allowance does not admit another export in an approved file', () => {
  const result = check({ ...contracts, 'crates/openhuman-embed/src/lib.rs': contracts['crates/openhuman-embed/src/lib.rs'].replace('ModelStreamMetadata,', 'ModelStreamMetadata, UnreviewedType,') });
  assert.equal(result.status, 1, result.output);
  assert.match(result.output, /openhuman-upstream-reexport/);
});

test('the same native contract remains forbidden outside its owning adapter', () => {
  const result = check({ 'crates/openhuman-core/src/other/mod.rs': contracts['crates/openhuman-core/src/agent/host_overrides.rs'] });
  assert.equal(result.status, 1, result.output);
  assert.match(result.output, /openhuman-upstream-reexport/);
});
