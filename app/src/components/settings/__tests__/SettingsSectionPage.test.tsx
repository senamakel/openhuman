import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';

import SettingsSectionPage from '../SettingsSectionPage';

vi.mock('../hooks/useSettingsNavigation', () => ({
  useSettingsNavigation: () => ({
    navigateBack: vi.fn(),
    navigateToSettings: vi.fn(),
    breadcrumbs: [],
  }),
}));

vi.mock('../components/SettingsHeader', () => ({
  default: ({ title }: { title: string }) => <div data-testid="settings-header">{title}</div>,
}));

describe('SettingsSectionPage', () => {
  it('renders provided items and the optional footer', () => {
    render(
      <MemoryRouter>
        <SettingsSectionPage
          title="Account"
          items={[
            {
              id: 'recovery-phrase',
              title: 'Recovery phrase',
              description: 'desc',
              icon: <span data-testid="icon" />,
              route: 'recovery-phrase',
            },
          ]}
          footer={<div data-testid="account-footer">danger zone</div>}
        />
      </MemoryRouter>
    );

    expect(screen.getByTestId('settings-header')).toHaveTextContent('Account');
    expect(screen.getByText('Recovery phrase')).toBeInTheDocument();
    expect(screen.getByTestId('account-footer')).toBeInTheDocument();
  });

  it('omits the footer slot when none is provided', () => {
    render(
      <MemoryRouter>
        <SettingsSectionPage
          title="Account"
          items={[{ id: 'team', title: 'Team', icon: <span />, route: 'team' }]}
        />
      </MemoryRouter>
    );
    expect(screen.queryByTestId('account-footer')).not.toBeInTheDocument();
  });
});
