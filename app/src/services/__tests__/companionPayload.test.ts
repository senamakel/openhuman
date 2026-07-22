import { describe, expect, it } from 'vitest';

import { parseCompanionStateChangedEvent } from '../companionEvents';

// The companion state change now arrives as a Tauri `companion://state_changed`
// event with a camelCase payload `{ sessionId, state, previousState }`.
describe('parseCompanionStateChangedEvent', () => {
  it('returns null for non-object inputs', () => {
    expect(parseCompanionStateChangedEvent(null)).toBeNull();
    expect(parseCompanionStateChangedEvent(undefined)).toBeNull();
    expect(parseCompanionStateChangedEvent(42)).toBeNull();
    expect(parseCompanionStateChangedEvent('listening')).toBeNull();
  });

  it('returns null when sessionId is missing or non-string', () => {
    expect(parseCompanionStateChangedEvent({ state: 'listening' })).toBeNull();
    expect(parseCompanionStateChangedEvent({ sessionId: 42, state: 'listening' })).toBeNull();
  });

  it('returns null when state is missing or not in the enum', () => {
    expect(parseCompanionStateChangedEvent({ sessionId: 's1' })).toBeNull();
    expect(parseCompanionStateChangedEvent({ sessionId: 's1', state: 'unknown' })).toBeNull();
    expect(parseCompanionStateChangedEvent({ sessionId: 's1', state: 7 })).toBeNull();
  });

  it('accepts a valid payload and round-trips all fields', () => {
    const event = parseCompanionStateChangedEvent({
      sessionId: 'sess-1',
      state: 'speaking',
      previousState: 'thinking',
    });
    expect(event).toEqual({ sessionId: 'sess-1', state: 'speaking', previousState: 'thinking' });
  });

  it("defaults previousState to 'idle' when missing or invalid", () => {
    const missing = parseCompanionStateChangedEvent({ sessionId: 's', state: 'listening' });
    expect(missing?.previousState).toBe('idle');

    const invalid = parseCompanionStateChangedEvent({
      sessionId: 's',
      state: 'listening',
      previousState: 'banana',
    });
    expect(invalid?.previousState).toBe('idle');
  });

  it('accepts every valid state value', () => {
    for (const state of ['idle', 'listening', 'thinking', 'speaking', 'pointing', 'error']) {
      const event = parseCompanionStateChangedEvent({ sessionId: 's', state });
      expect(event?.state).toBe(state);
    }
  });
});
