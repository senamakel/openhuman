import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { SkillSummary } from '../../../services/api/skillsApi';
import SkillsExplorerTab from '../SkillsExplorerTab';

vi.mock('../../../services/api/skillsApi', () => ({
  skillsApi: {
    listSkills: vi.fn(),
    installSkillFromUrl: vi.fn(),
    uninstallSkill: vi.fn(),
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

describe('SkillsExplorerTab', () => {
  beforeEach(async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    vi.mocked(skillsApi.listSkills).mockReset();
    vi.mocked(skillsApi.uninstallSkill).mockReset();
  });

  it('shows loading spinner then renders skills', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    vi.mocked(skillsApi.listSkills).mockResolvedValue([MOCK_SKILL, MOCK_PROJECT_SKILL]);

    render(<SkillsExplorerTab />);

    await waitFor(() => {
      expect(screen.getByText('Test Skill')).toBeInTheDocument();
    });
    expect(screen.getByText('Project Skill')).toBeInTheDocument();
    expect(screen.getByText('Hermes')).toBeInTheDocument();
    expect(screen.getByText('OpenHuman')).toBeInTheDocument();
  });

  it('shows empty state when no skills found', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    vi.mocked(skillsApi.listSkills).mockResolvedValue([]);

    render(<SkillsExplorerTab />);

    await waitFor(() => {
      expect(screen.getByText('No skills found')).toBeInTheDocument();
    });
  });

  it('shows error state on fetch failure', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    vi.mocked(skillsApi.listSkills).mockRejectedValue(new Error('Network error'));

    render(<SkillsExplorerTab />);

    await waitFor(() => {
      expect(screen.getByText('Network error')).toBeInTheDocument();
    });
    expect(screen.getByRole('button', { name: /Try again/ })).toBeInTheDocument();
  });

  it('filters skills by search query', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    vi.mocked(skillsApi.listSkills).mockResolvedValue([MOCK_SKILL, MOCK_PROJECT_SKILL]);

    render(<SkillsExplorerTab />);

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

  it('shows uninstall button only for user-scope skills on hover', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    vi.mocked(skillsApi.listSkills).mockResolvedValue([MOCK_SKILL, MOCK_PROJECT_SKILL]);

    render(<SkillsExplorerTab />);

    await waitFor(() => {
      expect(screen.getByTestId('skill-explorer-tile-test-skill')).toBeInTheDocument();
    });

    expect(screen.getByTestId('skill-uninstall-test-skill')).toBeInTheDocument();
    expect(screen.queryByTestId('skill-uninstall-project-skill')).not.toBeInTheDocument();
  });

  it('displays version and tags', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    vi.mocked(skillsApi.listSkills).mockResolvedValue([MOCK_SKILL]);

    render(<SkillsExplorerTab />);

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

    await waitFor(() => {
      expect(screen.getByText('Missing required field: author')).toBeInTheDocument();
    });
  });

  it('opens install dialog when install button clicked', async () => {
    const { skillsApi } = await import('../../../services/api/skillsApi');
    vi.mocked(skillsApi.listSkills).mockResolvedValue([]);

    render(<SkillsExplorerTab />);

    await waitFor(() => {
      expect(screen.getByTestId('skill-install-from-url-btn')).toBeInTheDocument();
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId('skill-install-from-url-btn'));
    });

    expect(screen.getByText('Install skill from URL')).toBeInTheDocument();
  });
});
