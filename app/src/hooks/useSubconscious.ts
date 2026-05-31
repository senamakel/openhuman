/**
 * useSubconscious — hook for the subconscious engine UI.
 *
 * Provides status, thoughts (reflections), and engine control actions
 * for the subconscious tab on the Intelligence page.
 */
import { useCallback, useEffect, useRef, useState } from 'react';

import {
  isTauri,
  subconsciousStatus,
  subconsciousTrigger,
} from '../utils/tauriCommands';
import type { SubconsciousStatus } from '../utils/tauriCommands/subconscious';

export interface UseSubconsciousResult {
  status: SubconsciousStatus | null;
  loading: boolean;
  triggering: boolean;
  refresh: () => Promise<void>;
  triggerTick: () => Promise<void>;
  error: string | null;
}

export function useSubconscious(): UseSubconsciousResult {
  const [status, setStatus] = useState<SubconsciousStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [triggering, setTriggering] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fetchingRef = useRef(false);

  const refresh = useCallback(async () => {
    if (!isTauri() || fetchingRef.current) return;
    fetchingRef.current = true;
    setLoading(true);
    setError(null);
    try {
      const statusRes = await withTimeout(subconsciousStatus());
      if (statusRes) setStatus(unwrap(statusRes) ?? null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load subconscious data');
    } finally {
      setLoading(false);
      fetchingRef.current = false;
    }
  }, []);

  const triggerTick = useCallback(async () => {
    if (!isTauri() || triggering) return;
    setTriggering(true);
    try {
      await subconsciousTrigger();
    } catch (err) {
      console.warn('[subconscious] trigger failed:', err);
    } finally {
      setTriggering(false);
    }
  }, [triggering]);

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, 3000);
    return () => {
      clearInterval(interval);
      fetchingRef.current = false;
    };
  }, [refresh]);

  return {
    status,
    loading,
    triggering,
    refresh,
    triggerTick,
    error,
  };
}

const RPC_TIMEOUT_MS = 2500;

function withTimeout<T>(promise: Promise<T>, ms: number = RPC_TIMEOUT_MS): Promise<T | null> {
  return Promise.race<T | null>([
    promise.catch(() => null),
    new Promise<null>(resolve => setTimeout(() => resolve(null), ms)),
  ]);
}

function unwrap<T>(response: unknown): T | null {
  if (!response || typeof response !== 'object') return null;
  const r = response as Record<string, unknown>;
  if ('result' in r) {
    return r.result as T;
  }
  return null;
}
