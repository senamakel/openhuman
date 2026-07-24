import createDebug from 'debug';
import { useCallback, useEffect, useRef, useState } from 'react';

import { type FlowRun, listAllFlowRuns, listFlowRuns } from '../services/api/flowsApi';

const log = createDebug('app:flows:runs-query');

export type FlowRunsQueryScope = { kind: 'flow'; flowId: string | null } | { kind: 'all' };

export interface UseFlowRunsQueryOptions {
  scope: FlowRunsQueryScope;
  enabled?: boolean;
}

export interface UseFlowRunsQueryResult {
  runs: FlowRun[];
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  refreshSilently: () => Promise<void>;
}

function normalizeError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function useFlowRunsQuery({
  scope,
  enabled = true,
}: UseFlowRunsQueryOptions): UseFlowRunsQueryResult {
  const [runs, setRuns] = useState<FlowRun[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(false);
  const requestGenerationRef = useRef(0);

  const scopeKind = scope.kind;
  const flowId = scope.kind === 'flow' ? scope.flowId : null;
  const canFetch = enabled && (scopeKind === 'all' || flowId !== null);

  const requestRuns = useCallback(
    () => (scopeKind === 'flow' ? listFlowRuns(flowId as string) : listAllFlowRuns()),
    [flowId, scopeKind]
  );

  const refresh = useCallback(async () => {
    if (!canFetch) return;

    const generation = ++requestGenerationRef.current;
    if (mountedRef.current) {
      setLoading(true);
      setError(null);
    }

    try {
      const result = await requestRuns();
      if (!mountedRef.current || generation !== requestGenerationRef.current) return;
      setRuns(result);
    } catch (requestError) {
      if (!mountedRef.current || generation !== requestGenerationRef.current) return;
      setError(normalizeError(requestError));
    } finally {
      if (mountedRef.current && generation === requestGenerationRef.current) {
        setLoading(false);
      }
    }
  }, [canFetch, requestRuns]);

  const refreshSilently = useCallback(async () => {
    if (!canFetch) return;

    const generation = ++requestGenerationRef.current;
    try {
      const result = await requestRuns();
      if (!mountedRef.current || generation !== requestGenerationRef.current) return;
      setRuns(result);
    } catch {
      if (!mountedRef.current || generation !== requestGenerationRef.current) return;
      log('silent refresh failed: scope=%s', scopeKind);
    } finally {
      if (mountedRef.current && generation === requestGenerationRef.current) {
        setLoading(false);
      }
    }
  }, [canFetch, requestRuns, scopeKind]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      requestGenerationRef.current += 1;
    };
  }, []);

  useEffect(() => {
    requestGenerationRef.current += 1;
    setRuns([]);
    setLoading(false);
    setError(null);

    if (canFetch) void refresh();

    return () => {
      requestGenerationRef.current += 1;
    };
  }, [canFetch, flowId, refresh, scopeKind]);

  return { runs, loading, error, refresh, refreshSilently };
}
