/**
 * Unit tests for describeNode — the dynamic per-node card summary. Asserts the
 * config-driven text for representative kinds plus the generic fallbacks when
 * config isn't filled in.
 */
import { describe, expect, it } from 'vitest';

import { describeNode } from './nodeSummary';

describe('describeNode', () => {
  it('describes a schedule trigger via its cron', () => {
    expect(describeNode('trigger', { trigger_kind: 'schedule', schedule: '*/5 * * * 3' })).toBe(
      'Every 5 minutes on Wed'
    );
    expect(describeNode('trigger', { trigger_kind: 'manual' })).toBe('Runs on demand');
  });

  it('describes an http_request from method + url', () => {
    expect(describeNode('http_request', { method: 'POST', url: 'https://api.x.com/v1' })).toBe(
      'POST https://api.x.com/v1'
    );
    expect(describeNode('http_request', {})).toBe('GET request (set a URL)');
  });

  it('describes an agent by model hint', () => {
    expect(describeNode('agent', { model: 'hint:coding' })).toBe('Asks the coding');
    expect(describeNode('agent', { prompt: 'Summarize the thread', model: '' })).toContain(
      'Summarize the thread'
    );
  });

  it('describes branch nodes and reflects output routes', () => {
    expect(describeNode('condition', { field: 'status' })).toBe('If status → true / false');
    expect(describeNode('switch', { expression: 'item.type' }, ['a', 'b', 'default'])).toBe(
      'Routes by item.type (3 routes)'
    );
  });

  it('falls back for tool_call / transform with empty config', () => {
    expect(describeNode('tool_call', {})).toBe('Runs an app action (pick one)');
    expect(describeNode('transform', { set: { a: '=1', b: '=2' } })).toBe(
      'Sets 2 fields on each item'
    );
  });

  it('returns empty string for an unknown kind', () => {
    expect(describeNode('time_travel', {})).toBe('');
  });
});
