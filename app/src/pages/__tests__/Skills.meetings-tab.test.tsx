import { fireEvent, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import '../../test/mockDefaultSkillStatusHooks';
import { renderWithProviders } from '../../test/test-utils';
import Skills from '../Skills';

vi.mock('../../components/skills/MeetingBotsCard', () => ({
  default: () => <div data-testid="meeting-bots-card">Meeting bot CTA</div>,
}));

vi.mock('../../hooks/useChannelDefinitions', () => ({
  useChannelDefinitions: () => ({ definitions: [], loading: false, error: null }),
}));

vi.mock('../../services/api/skillsApi', async () => {
  const actual = await vi.importActual<typeof import('../../services/api/skillsApi')>(
    '../../services/api/skillsApi'
  );
  return {
    ...actual,
    skillsApi: { ...actual.skillsApi, listSkills: vi.fn().mockResolvedValue([]) },
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
  useAgentReadyComposioToolkits: () => ({
    agentReady: new Set<string>(),
    loading: true,
    error: null,
  }),
}));

describe('Skills page — Meetings tab', () => {
  it('keeps the meeting bot CTA in its own Connections tab', () => {
    renderWithProviders(<Skills />, { initialEntries: ['/skills'] });

    expect(screen.queryByTestId('meeting-bots-card')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('tab', { name: 'Google Meet' }));

    expect(screen.getByTestId('meeting-bots-card')).toBeInTheDocument();
  });

  it('supports direct links to the Meetings tab', () => {
    renderWithProviders(<Skills />, { initialEntries: ['/skills?tab=meetings'] });

    expect(screen.getByRole('tab', { name: 'Google Meet' })).toHaveAttribute(
      'aria-selected',
      'true'
    );
    expect(screen.getByTestId('meeting-bots-card')).toBeInTheDocument();
  });
});
