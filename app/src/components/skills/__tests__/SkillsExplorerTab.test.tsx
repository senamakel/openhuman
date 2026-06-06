import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { CatalogEntry } from '../../../services/api/skillRegistryApi';
import type { SkillSummary } from '../../../services/api/skillsApi';
import SkillsExplorerTab from '../SkillsExplorerTab';

vi.mock('../../../services/api/skillsApi', () => ({
  skillsApi: {
    listSkills: vi.fn(),
    installSkillFromUrl: vi.fn(),
    uninstallSkill: vi.fn(),
  },
}));

vi.mock('../../../services/api/skillRegistryApi', () => ({
  skillRegistryApi: {
    browse: vi.fn(),
    search: vi.fn(),
    sources: vi.fn(),
    install: vi.fn(),
  },
}));

const MOCK_SKILL: SkillSummary = {
  id: 'test-skill',
  name: 'Test Skill',
  description: 'A test skill for unit testing',
  version: '1.0.0',
  author: 'Test Author',
  tags: ['test', 'automation'],
  platforms: [],
  relatedSkills: [],
  sourceFormat: 'hermes',
  tools: [],
  prompts: [],
  location: '/Users/test/.openhuman/skills/test-skill/SKILL.md',
  resources: [],
  scope: 'user',
  legacy: false,
  warnings: [],
};

const MOCK_PROJECT_SKILL: SkillSummary = {
  ...MOCK_SKILL,
  id: 'project-skill',
  name: 'Project Skill',
  sourceFormat: 'openhuman',
  scope: 'project',
};

const MOCK_CATALOG_ENTRY: CatalogEntry = {
  id: 'registry-skill-1',
  name: 'Registry Skill',
  description: 'A skill from the registry',
  format: 'hermes',
  author: 'Registry Author',
  version: '2.0.0',
  tags: ['registry', 'remote'],
  download_url: 'https://example.com/SKILL.md',
  source_id: 'openhuman-community',
  stars: 42,
  updated_at: '2026-01-01',
};

async function switchToInstalled() {
  const installedTab = screen.getByText('Installed', { selector: 'button' });
  await act(async () => {
    fireEvent.click(installedTab);
  });
}

describe('SkillsExplorerTab', () => {
  beforeEach(async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    const { skillRegistryApi } = await import(
      '../../../services/api/skillRegistryApi'
    );
    vi.mocked(skillsApi.listSkills).mockReset();
    vi.mocked(skillsApi.uninstallSkill).mockReset();
    vi.mocked(skillRegistryApi.browse).mockReset();
    vi.mocked(skillRegistryApi.install).mockReset();
    vi.mocked(skillRegistryApi.browse).mockResolvedValue([]);
  });

  it('defaults to registry view and shows catalog entries', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    const { skillRegistryApi } = await import(
      '../../../services/api/skillRegistryApi'
    );
    vi.mocked(skillsApi.listSkills).mockResolvedValue([]);
    vi.mocked(skillRegistryApi.browse).mockResolvedValue([MOCK_CATALOG_ENTRY]);

    render(<SkillsExplorerTab />);

    await waitFor(() => {
      expect(screen.getByText('Registry Skill')).toBeInTheDocument();
    });
    expect(screen.getByText('Hermes')).toBeInTheDocument();
    expect(screen.getByText('openhuman-community')).toBeInTheDocument();
  });

  it('shows installed skills when switching to installed tab', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    vi.mocked(skillsApi.listSkills).mockResolvedValue([MOCK_SKILL, MOCK_PROJECT_SKILL]);

    render(<SkillsExplorerTab />);

    await waitFor(() => {
      expect(screen.getByText('Installed')).toBeInTheDocument();
    });

    await switchToInstalled();

    await waitFor(() => {
      expect(screen.getByText('Test Skill')).toBeInTheDocument();
    });
    expect(screen.getByText('Project Skill')).toBeInTheDocument();
  });

  it('shows empty state when no installed skills', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    vi.mocked(skillsApi.listSkills).mockResolvedValue([]);

    render(<SkillsExplorerTab />);
    await switchToInstalled();

    await waitFor(() => {
      expect(screen.getByText('No skills found')).toBeInTheDocument();
    });
  });

  it('shows error state on registry fetch failure', async () => {
    const { skillRegistryApi } = await import(
      '../../../services/api/skillRegistryApi'
    );
    const { skillsApi } = await import('../../../services/api/skillsApi');
    vi.mocked(skillsApi.listSkills).mockResolvedValue([]);
    vi.mocked(skillRegistryApi.browse).mockRejectedValue(new Error('Network error'));

    render(<SkillsExplorerTab />);

    await waitFor(() => {
      expect(screen.getByText('Network error')).toBeInTheDocument();
    });
    expect(screen.getByRole('button', { name: /Try again/ })).toBeInTheDocument();
  });

  it('filters installed skills by search query', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    vi.mocked(skillsApi.listSkills).mockResolvedValue([MOCK_SKILL, MOCK_PROJECT_SKILL]);

    render(<SkillsExplorerTab />);
    await switchToInstalled();

    await waitFor(() => {
      expect(screen.getByText('Test Skill')).toBeInTheDocument();
    });

    const searchInput = screen.getByPlaceholderText('Search skills...');
    fireEvent.change(searchInput, { target: { value: 'project' } });

    expect(screen.queryByText('Test Skill')).not.toBeInTheDocument();
    expect(screen.getByText('Project Skill')).toBeInTheDocument();
  });

  it('shows install from URL button', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    vi.mocked(skillsApi.listSkills).mockResolvedValue([]);

    render(<SkillsExplorerTab />);

    await waitFor(() => {
      expect(screen.getByTestId('skill-install-from-url-btn')).toBeInTheDocument();
    });
  });

  it('shows uninstall button only for user-scope skills', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    vi.mocked(skillsApi.listSkills).mockResolvedValue([MOCK_SKILL, MOCK_PROJECT_SKILL]);

    render(<SkillsExplorerTab />);
    await switchToInstalled();

    await waitFor(() => {
      expect(screen.getByTestId('skill-explorer-tile-test-skill')).toBeInTheDocument();
    });

    expect(screen.getByTestId('skill-uninstall-test-skill')).toBeInTheDocument();
    expect(screen.queryByTestId('skill-uninstall-project-skill')).not.toBeInTheDocument();
  });

  it('displays version and tags in installed view', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    vi.mocked(skillsApi.listSkills).mockResolvedValue([MOCK_SKILL]);

    render(<SkillsExplorerTab />);
    await switchToInstalled();

    await waitFor(() => {
      expect(screen.getByText('v1.0.0')).toBeInTheDocument();
    });
    expect(screen.getByText('test')).toBeInTheDocument();
    expect(screen.getByText('automation')).toBeInTheDocument();
  });

  it('displays scope badges', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    vi.mocked(skillsApi.listSkills).mockResolvedValue([MOCK_SKILL, MOCK_PROJECT_SKILL]);

    render(<SkillsExplorerTab />);
    await switchToInstalled();

    await waitFor(() => {
      expect(screen.getByText('Test Skill')).toBeInTheDocument();
    });
    expect(screen.getAllByText('User').length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText('Project').length).toBeGreaterThanOrEqual(1);
  });

  it('shows skill warnings when present', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    const skillWithWarning = {
      ...MOCK_SKILL,
      warnings: ['Missing required field: author'],
    };
    vi.mocked(skillsApi.listSkills).mockResolvedValue([skillWithWarning]);

    render(<SkillsExplorerTab />);
    await switchToInstalled();

    await waitFor(() => {
      expect(screen.getByText('Missing required field: author')).toBeInTheDocument();
    });
  });

  it('shows "Installed" badge for already-installed catalog entries', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    const { skillRegistryApi } = await import(
      '../../../services/api/skillRegistryApi'
    );
    const installedSkill = { ...MOCK_SKILL, id: 'registry-skill-1' };
    vi.mocked(skillsApi.listSkills).mockResolvedValue([installedSkill]);
    vi.mocked(skillRegistryApi.browse).mockResolvedValue([MOCK_CATALOG_ENTRY]);

    render(<SkillsExplorerTab />);

    await waitFor(() => {
      expect(screen.getByText('Registry Skill')).toBeInTheDocument();
    });
    expect(screen.getByTestId('registry-tile-registry-skill-1')).toBeInTheDocument();
  });

  it('has an install from URL button', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    vi.mocked(skillsApi.listSkills).mockResolvedValue([]);

    render(<SkillsExplorerTab />);

    await waitFor(() => {
      expect(screen.getByTestId('skill-install-from-url-btn')).toBeInTheDocument();
    });
    expect(screen.getByTestId('skill-install-from-url-btn')).toHaveTextContent(
      'Install from URL'
    );
  });
});
