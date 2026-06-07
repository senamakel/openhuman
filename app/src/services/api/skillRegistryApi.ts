import debug from 'debug';

import { callCoreRpc } from '../coreRpcClient';

const log = debug('skillRegistryApi');

export interface CatalogEntry {
  id: string;
  name: string;
  description: string;
  source: string;
  category: string;
  author: string | null;
  version: string | null;
  tags: string[];
  platforms: string[];
  download_url: string;
  docs_path: string | null;
  commands: string[];
  env_vars: string[];
  license: string | null;
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

  search: async (query: string, source?: string, category?: string): Promise<CatalogEntry[]> => {
    log('search: query=%s source=%s category=%s', query, source, category);
    const response = await callCoreRpc<
      Envelope<{ entries: CatalogEntry[] }> | { entries: CatalogEntry[] }
    >({
      method: 'openhuman.skill_registry_search',
      params: { query, ...(source ? { source } : {}), ...(category ? { category } : {}) },
    });
    const result = unwrap(response);
    log('search: count=%d', result.entries.length);
    return result.entries;
  },

  sources: async (): Promise<string[]> => {
    log('sources: request');
    const response = await callCoreRpc<
      Envelope<{ sources: string[] }> | { sources: string[] }
    >({ method: 'openhuman.skill_registry_sources' });
    const result = unwrap(response);
    log('sources: count=%d', result.sources.length);
    return result.sources;
  },

  categories: async (): Promise<string[]> => {
    log('categories: request');
    const response = await callCoreRpc<
      Envelope<{ categories: string[] }> | { categories: string[] }
    >({ method: 'openhuman.skill_registry_categories' });
    const result = unwrap(response);
    log('categories: count=%d', result.categories.length);
    return result.categories;
  },

  install: async (
    entryId: string
  ): Promise<{ url: string; stdout: string; stderr: string; new_skills: string[] }> => {
    log('install: entryId=%s', entryId);
    const response = await callCoreRpc<
      | Envelope<{ url: string; stdout: string; stderr: string; new_skills: string[] }>
      | { url: string; stdout: string; stderr: string; new_skills: string[] }
    >({
      method: 'openhuman.skill_registry_install',
      params: { entry_id: entryId },
    });
    const result = unwrap(response);
    log('install: newSkills=%d', result.new_skills.length);
    return result;
  },
};
