import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import { renderWithProviders } from '../../../test/test-utils';
import ThemeStudioPanel from './ThemeStudioPanel';

const themeState = {
  mode: 'system',
  tabBarLabels: 'hover',
  fontSize: 'medium',
  activeThemeId: 'system',
  customThemes: [],
};

describe('<ThemeStudioPanel />', () => {
  it('renders the family gallery', () => {
    renderWithProviders(<ThemeStudioPanel />, {
      preloadedState: { theme: themeState },
      initialEntries: ['/settings/theme'],
    });
    // Theme families (each with a Light/Dark/Auto variant toggle).
    expect(screen.getByText('Classic')).toBeInTheDocument();
    expect(screen.getByText('Ocean')).toBeInTheDocument();
    expect(screen.getByText('Matrix')).toBeInTheDocument();
    expect(screen.getByText('HAL 9000')).toBeInTheDocument();
  });

  it('duplicates the active preset into an editable custom theme', async () => {
    const user = userEvent.setup();
    const { store } = renderWithProviders(<ThemeStudioPanel />, {
      preloadedState: { theme: themeState },
      initialEntries: ['/settings/theme'],
    });

    expect(store.getState().theme.customThemes).toHaveLength(0);
    await user.click(screen.getByRole('button', { name: /duplicate/i }));

    const { customThemes, activeThemeId } = store.getState().theme;
    expect(customThemes).toHaveLength(1);
    expect(customThemes[0].builtIn).toBe(false);
    expect(activeThemeId).toBe(customThemes[0].id);
  });

  it('enables colour editing only when a custom theme is active', () => {
    // Built-in preset active → colour inputs are disabled.
    const { unmount } = renderWithProviders(<ThemeStudioPanel />, {
      preloadedState: { theme: themeState },
      initialEntries: ['/settings/theme'],
    });
    expect(document.querySelector('input[type="color"]:not([disabled])')).toBeNull();
    unmount();

    // Custom theme active → at least one colour input is editable.
    renderWithProviders(<ThemeStudioPanel />, {
      preloadedState: {
        theme: {
          ...themeState,
          activeThemeId: 'custom-1',
          customThemes: [
            { id: 'custom-1', name: 'Mine', isDark: false, builtIn: false, colors: {}, fonts: {} },
          ],
        },
      },
      initialEntries: ['/settings/theme'],
    });
    expect(document.querySelector('input[type="color"]:not([disabled])')).not.toBeNull();
  });
});
