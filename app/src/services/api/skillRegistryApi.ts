import debug from 'debug';

import { callCoreRpc } from '../coreRpcClient';

const log = debug('skillRegistryApi');

export interface RegistrySource {
  id: string;
  name: string;
  url: string;
  kind: 'github_index' | 'http_catalog';
  enabled: boolean;
}

export interface CatalogEntry {
  id: string;
  name: string;
  description: string;
  format: string;
  author: string | null;
  version: string | null;
  tags: string[];
  download_url: string;
  source_id: string;
  stars: number | null;
  updated_at: string | null;
}

interface Envelope<T> {
  data?: T;
}

function unwrap<T>(response: Envelope<T> | T): T {
  if (response && typeof response === 'object' && 'data' in response) {
    const env = response as Envelope<T>;
    if (env.data !== undefined) return env.data as T;
  }
  return response as T;
}

export const skillRegistryApi = {
  browse: async (forceRefresh = false): Promise<CatalogEntry[]> => {
    log('browse: forceRefresh=%s', forceRefresh);
    const response = await callCoreRpc<
      Envelope<{ entries: CatalogEntry[] }> | { entries: CatalogEntry[] }
    >({ method: 'openhuman.skill_registry_browse', params: { force_refresh: forceRefresh } });
    const result = unwrap(response);
    log('browse: count=%d', result.entries.length);
    return result.entries;
  },

  search: async (query: string, format?: string, source?: string): Promise<CatalogEntry[]> => {
    log('search: query=%s format=%s source=%s', query, format, source);
    const response = await callCoreRpc<
      Envelope<{ entries: CatalogEntry[] }> | { entries: CatalogEntry[] }
    >({
      method: 'openhuman.skill_registry_search',
      params: { query, ...(format ? { format } : {}), ...(source ? { source } : {}) },
    });
    const result = unwrap(response);
    log('search: count=%d', result.entries.length);
    return result.entries;
  },

  sources: async (): Promise<RegistrySource[]> => {
    log('sources: request');
    const response = await callCoreRpc<
      Envelope<{ sources: RegistrySource[] }> | { sources: RegistrySource[] }
    >({ method: 'openhuman.skill_registry_sources' });
    const result = unwrap(response);
    log('sources: count=%d', result.sources.length);
    return result.sources;
  },

  addSource: async (params: {
    id: string;
    name: string;
    url: string;
    kind?: string;
  }): Promise<RegistrySource[]> => {
    log('addSource: id=%s', params.id);
    const response = await callCoreRpc<
      Envelope<{ sources: RegistrySource[] }> | { sources: RegistrySource[] }
    >({ method: 'openhuman.skill_registry_add_source', params });
    const result = unwrap(response);
    return result.sources;
  },

  removeSource: async (id: string): Promise<RegistrySource[]> => {
    log('removeSource: id=%s', id);
    const response = await callCoreRpc<
      Envelope<{ sources: RegistrySource[] }> | { sources: RegistrySource[] }
    >({ method: 'openhuman.skill_registry_remove_source', params: { id } });
    const result = unwrap(response);
    return result.sources;
  },

  install: async (
    entryId: string,
    sourceId: string
  ): Promise<{ url: string; stdout: string; stderr: string; new_skills: string[] }> => {
    log('install: entryId=%s sourceId=%s', entryId, sourceId);
    const response = await callCoreRpc<
      | Envelope<{ url: string; stdout: string; stderr: string; new_skills: string[] }>
      | { url: string; stdout: string; stderr: string; new_skills: string[] }
    >({
      method: 'openhuman.skill_registry_install',
      params: { entry_id: entryId, source_id: sourceId },
    });
    const result = unwrap(response);
    log('install: newSkills=%d', result.new_skills.length);
    return result;
  },
};
