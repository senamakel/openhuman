import { screen } from '@testing-library/react';
import { useLocation } from 'react-router-dom';
import { describe, expect, test, vi } from 'vitest';

import { renderWithProviders } from '../../../../test/test-utils';
import IntegrationsPanel from '../IntegrationsPanel';

// Surfaces the current router location so we can assert legacy-hash redirects.
const LocationProbe = () => {
  const location = useLocation();
  return <div data-testid="location-probe">{`${location.pathname}${location.search}`}</div>;
};

// The panel body has its own test suite — stub it so these tests stay focused
// on the routing IntegrationsPanel owns.
vi.mock('../TaskSourcesPanel', () => ({
  default: ({ embedded }: { embedded?: boolean }) => (
    <div data-testid="stub-task-sources" data-embedded={String(embedded ?? false)} />
  ),
}));

vi.mock('../../hooks/useSettingsNavigation', () => ({
  useSettingsNavigation: () => ({
    navigateBack: vi.fn(),
    navigateToSettings: vi.fn(),
    breadcrumbs: [],
  }),
}));

describe('IntegrationsPanel', () => {
  test('renders the Task sources panel (webhooks tab retired)', () => {
    renderWithProviders(<IntegrationsPanel />, { initialEntries: ['/settings/integrations'] });

    expect(screen.getByTestId('stub-task-sources')).toBeInTheDocument();
  });

  test('legacy #composio hash redirects to Connections → API keys', () => {
    renderWithProviders(
      <>
        <IntegrationsPanel />
        <LocationProbe />
      </>,
      { initialEntries: ['/settings/integrations#composio'] }
    );

    expect(screen.getByTestId('location-probe')).toHaveTextContent('/connections?tab=composio-key');
  });
});
