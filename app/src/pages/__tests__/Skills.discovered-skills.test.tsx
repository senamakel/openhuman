import { fireEvent, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { SkillSummary } from '../../services/api/skillsApi';
import '../../test/mockDefaultSkillStatusHooks';
import { renderWithProviders } from '../../test/test-utils';
import Skills from '../Skills';

vi.mock('../../hooks/useChannelDefinitions', () => ({
  useChannelDefinitions: () => ({ definitions: [], loading: false, error: null }),
}));

vi.mock('../../services/api/skillsApi', async () => {
  const actual = await vi.importActual<typeof import('../../services/api/skillsApi')>(
    '../../services/api/skillsApi'
  );
  const seeded = (overrides: Partial<SkillSummary>): SkillSummary => ({
    id: 'skill-1',
    name: 'Skill 1',
    description: 'A discovered skill.',
    version: '0.1.0',
    author: null,
    tags: [],
    platforms: [],
    relatedSkills: [],
    sourceFormat: 'openhuman',
    tools: [],
    prompts: [],
    location: null,
    resources: [],
    scope: 'user',
    legacy: false,
    warnings: [],
    ...overrides,
  });
  return {
    ...actual,
    skillsApi: {
      ...actual.skillsApi,
      listSkills: vi
        .fn()
        .mockResolvedValue([
          seeded({ id: 'user-skill', name: 'User Skill', scope: 'user' }),
          seeded({ id: 'project-skill', name: 'Project Skill', scope: 'project' }),
          seeded({ id: 'legacy-skill', name: 'Legacy Skill', scope: 'user', legacy: true }),
          seeded({
            id: 'resource-skill',
            name: 'Resource Skill',
            resources: ['templates/rare-template.md'],
          }),
        ]),
    },
  };
});

vi.mock('../../lib/composio/hooks', () => ({
  useComposioIntegrations: () => ({
    toolkits: [],
    connectionByToolkit: new Map(),
    refresh: vi.fn(),
    loading: false,
    error: null,
  }),
  // Issue #2283: Skills.tsx also consumes useAgentReadyComposioToolkits.
  useAgentReadyComposioToolkits: () => ({
    agentReady: new Set<string>(),
    loading: true,
    error: null,
  }),
}));

describe('Skills page — discovered skill cards', () => {
  it('renders user / project / legacy scope labels and exposes Uninstall only for user-scope', async () => {
    renderWithProviders(<Skills />, { initialEntries: ['/skills'] });

    const otherHeading = await screen.findByRole('heading', { name: 'Other' });
    const otherCard = otherHeading.closest('.rounded-2xl') as HTMLElement;

    const userRow = within(otherCard).getByText('User Skill').closest('.rounded-xl');
    expect(userRow).not.toBeNull();
    expect(within(userRow as HTMLElement).getByText('User')).toBeInTheDocument();
    expect(within(userRow as HTMLElement).getByText('User').className).toMatch(/sage/);

    const projectRow = within(otherCard).getByText('Project Skill').closest('.rounded-xl');
    expect(within(projectRow as HTMLElement).getByText('Project').className).toMatch(/amber/);

    const legacyRow = within(otherCard).getByText('Legacy Skill').closest('.rounded-xl');
    expect(within(legacyRow as HTMLElement).getByText('Legacy').className).toMatch(/stone-600/);

    // Uninstall surfaces for user-scope, non-legacy only.
    expect(screen.queryByTestId('skill-uninstall-user-skill')).not.toBeInTheDocument();
    const userMore = within(userRow as HTMLElement).getByTitle('More actions');
    fireEvent.click(userMore);
    expect(await screen.findByTestId('skill-uninstall-user-skill')).toBeInTheDocument();
  });

  it('opens the detail drawer when the View CTA is clicked', async () => {
    renderWithProviders(<Skills />, { initialEntries: ['/skills'] });

    const otherHeading = await screen.findByRole('heading', { name: 'Other' });
    const userRow = within(otherHeading.closest('.rounded-2xl') as HTMLElement)
      .getByText('User Skill')
      .closest('.rounded-xl') as HTMLElement;
    const viewCta = within(userRow).getByRole('button', { name: 'View' });
    fireEvent.click(viewCta);

    expect(await screen.findByText('User Skill', { selector: 'h2' })).toBeInTheDocument();
  });

  it('filters discovered skills by bundled resource path', async () => {
    renderWithProviders(<Skills />, { initialEntries: ['/skills'] });

    expect(await screen.findByText('Resource Skill')).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText(/Search skills/i), {
      target: { value: 'rare-template' },
    });

    expect(screen.getByText('Resource Skill')).toBeInTheDocument();
    expect(screen.queryByText('User Skill')).not.toBeInTheDocument();
  });

  it('opens install and create dialogs from the Explorer header', async () => {
    renderWithProviders(<Skills />, { initialEntries: ['/skills'] });

    await screen.findByRole('heading', { name: 'Skills explorer' });

    fireEvent.click(screen.getByRole('button', { name: 'Install from URL' }));
    expect(
      await screen.findByRole('heading', { name: 'Install skill from URL' })
    ).toBeInTheDocument();

    fireEvent.keyDown(document, { key: 'Escape' });
    fireEvent.click(screen.getByRole('button', { name: 'New skill' }));
    expect(await screen.findByRole('dialog')).toHaveTextContent('New skill');
  });

  it('opens install dialog from the empty Explorer action', async () => {
    renderWithProviders(<Skills />, { initialEntries: ['/skills'] });

    await screen.findByRole('heading', { name: 'Skills explorer' });
    fireEvent.change(screen.getByPlaceholderText(/Search skills/i), {
      target: { value: 'no-such-skill' },
    });

    expect(screen.getByText('No skills found')).toBeInTheDocument();
    const installButtons = screen.getAllByRole('button', { name: 'Install from URL' });
    fireEvent.click(installButtons[installButtons.length - 1]);

    expect(
      await screen.findByRole('heading', { name: 'Install skill from URL' })
    ).toBeInTheDocument();
  });
});
