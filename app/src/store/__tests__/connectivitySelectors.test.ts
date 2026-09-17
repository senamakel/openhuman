import { describe, expect, it } from 'vitest';

import { isHostedDegraded, selectBlockingState } from '../connectivitySelectors';
import type { ConnectivityState } from '../connectivitySlice';
import type { RootState } from '../index';

const make = (over: Partial<ConnectivityState>): RootState =>
  ({
    // The selector only reads `connectivity`. Cast through unknown so we don't
    // have to fabricate the rest of the root state.
    connectivity: {
      internet: 'online',
      core: 'reachable',
      backend: 'connected',
      hosted: 'connected',
      lastError: {},
      ...over,
    },
  }) as unknown as RootState;

describe('selectBlockingState', () => {
  it('returns ok when all three channels are healthy', () => {
    expect(selectBlockingState(make({}))).toBe('ok');
  });

  it('prioritises internet outage over everything else', () => {
    expect(
      selectBlockingState(
        make({ internet: 'offline', core: 'unreachable', backend: 'disconnected' })
      )
    ).toBe('internet-offline');
  });

  it('returns core-unreachable when only the sidecar is down', () => {
    expect(selectBlockingState(make({ core: 'unreachable' }))).toBe('core-unreachable');
  });

  it('returns backend-only when just the websocket is degraded', () => {
    expect(selectBlockingState(make({ backend: 'disconnected' }))).toBe('backend-only');
    expect(selectBlockingState(make({ backend: 'connecting' }))).toBe('backend-only');
  });

  it("returns hosted-degraded when only the core's hosted link is down while its loop is alive (#6256)", () => {
    expect(selectBlockingState(make({ hosted: 'connecting' }))).toBe('hosted-degraded');
    expect(selectBlockingState(make({ hosted: 'reconnecting' }))).toBe('hosted-degraded');
    // Server closed the namespace on a transport it left open: no events flow.
    expect(selectBlockingState(make({ hosted: 'disconnected' }))).toBe('hosted-degraded');
  });

  it('reports hosted-stopped when the core loop exited on an unusable session (#6270)', () => {
    expect(selectBlockingState(make({ hosted: 'stopped' }))).toBe('hosted-stopped');
    // Still below every other channel.
    expect(selectBlockingState(make({ backend: 'disconnected', hosted: 'stopped' }))).toBe(
      'backend-only'
    );
  });

  it('does not read a server error event on a live link as an outage', () => {
    // The core keeps the transport open and emits keep working after a server
    // `error` event, so "reconnecting" copy would be wrong here.
    expect(selectBlockingState(make({ hosted: 'error' }))).toBe('ok');
  });

  it('treats a hosted link that is not running as healthy, not degraded', () => {
    // Signed out, local session, early boot: no link was ever wanted.
    expect(selectBlockingState(make({ hosted: 'unknown' }))).toBe('ok');
    // A store hydrated before the channel existed reads the same way.
    expect(
      selectBlockingState(make({ hosted: undefined as unknown as ConnectivityState['hosted'] }))
    ).toBe('ok');
  });

  it('ranks the hosted link below every other channel', () => {
    expect(selectBlockingState(make({ backend: 'disconnected', hosted: 'reconnecting' }))).toBe(
      'backend-only'
    );
    expect(selectBlockingState(make({ core: 'unreachable', hosted: 'reconnecting' }))).toBe(
      'core-unreachable'
    );
    expect(selectBlockingState(make({ internet: 'offline', hosted: 'reconnecting' }))).toBe(
      'internet-offline'
    );
  });
});

describe('isHostedDegraded', () => {
  it('is true only for a link that is down and being retried', () => {
    expect(isHostedDegraded('connecting')).toBe(true);
    expect(isHostedDegraded('reconnecting')).toBe(true);
    expect(isHostedDegraded('disconnected')).toBe(true);
    expect(isHostedDegraded('stopped')).toBe(true);
    expect(isHostedDegraded('error')).toBe(false);
    expect(isHostedDegraded('connected')).toBe(false);
    expect(isHostedDegraded('unknown')).toBe(false);
    expect(isHostedDegraded(undefined)).toBe(false);
  });
});
