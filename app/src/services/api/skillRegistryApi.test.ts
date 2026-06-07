import { beforeEach, describe, expect, it, vi } from 'vitest';

import { skillRegistryApi } from './skillRegistryApi';

const mockCallCoreRpc = vi.fn();
vi.mock('../coreRpcClient', () => ({ callCoreRpc: (...a: unknown[]) => mockCallCoreRpc(...a) }));

describe('skillRegistryApi', () => {
  beforeEach(() => {
    mockCallCoreRpc.mockReset();
  });

  it('normalizes install new_skills to newSkills', async () => {
    mockCallCoreRpc.mockResolvedValue({
      url: 'https://example.com/SKILL.md',
      stdout: 'ok',
      stderr: '',
      new_skills: ['demo'],
    });

    const result = await skillRegistryApi.install('demo');

    expect(mockCallCoreRpc).toHaveBeenCalledWith({
      method: 'openhuman.skill_registry_install',
      params: { entry_id: 'demo' },
    });
    expect(result.newSkills).toEqual(['demo']);
  });

  it('calls skill_registry_uninstall and normalizes removed_path', async () => {
    mockCallCoreRpc.mockResolvedValue({
      name: 'demo',
      removed_path: '/Users/test/.openhuman/skills/demo',
      scope: 'user',
    });

    const result = await skillRegistryApi.uninstall('demo');

    expect(mockCallCoreRpc).toHaveBeenCalledWith({
      method: 'openhuman.skill_registry_uninstall',
      params: { name: 'demo' },
    });
    expect(result.removedPath).toBe('/Users/test/.openhuman/skills/demo');
  });

  it('fetches skill_registry schemas for smoke script generation', async () => {
    mockCallCoreRpc.mockResolvedValue({
      schemas: [{ namespace: 'skill_registry', function: 'install', inputs: [], outputs: [] }],
    });

    const result = await skillRegistryApi.schemas();

    expect(mockCallCoreRpc).toHaveBeenCalledWith({ method: 'openhuman.skill_registry_schemas' });
    expect(result[0].function).toBe('install');
  });
});
