import { configureStore } from '@reduxjs/toolkit';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Provider } from 'react-redux';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import localeReducer from '../../../store/localeSlice';
import LogoutAndClearActions from '../LogoutAndClearActions';

function makeTestStore() {
  return configureStore({
    reducer: { locale: localeReducer },
    preloadedState: { locale: { current: 'en' as const } },
  });
}

vi.mock('../../../providers/CoreStateProvider', () => ({
  useCoreState: () => ({
    clearSession: vi.fn().mockResolvedValue(undefined),
    snapshot: { auth: { userId: null }, currentUser: null },
  }),
}));

const { mockClearAllAppData } = vi.hoisted(() => ({
  mockClearAllAppData: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('../../../utils/clearAllAppData', () => ({
  clearAllAppData: (...args: unknown[]) => mockClearAllAppData(...args),
}));

function renderActions() {
  return render(
    <Provider store={makeTestStore()}>
      <MemoryRouter>
        <LogoutAndClearActions />
      </MemoryRouter>
    </Provider>
  );
}

describe('LogoutAndClearActions', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockClearAllAppData.mockReset().mockResolvedValue(undefined);
  });

  it('renders the destructive actions row', () => {
    renderActions();
    expect(screen.getByText('Clear App Data')).toBeInTheDocument();
    expect(screen.getByText('Log out')).toBeInTheDocument();
  });

  it('passes the current snapshot user id + clearSession to clearAllAppData', async () => {
    const user = userEvent.setup();
    renderActions();

    await user.click(screen.getByText('Clear App Data').closest('button')!);
    // Confirm in the modal — the last button matching the label is the modal confirm.
    const confirmButtons = screen.getAllByRole('button', { name: /Clear App Data/i });
    await user.click(confirmButtons[confirmButtons.length - 1]);

    expect(mockClearAllAppData).toHaveBeenCalledTimes(1);
    const args = mockClearAllAppData.mock.calls[0][0];
    expect(args).toMatchObject({ userId: null });
    expect(typeof args.clearSession).toBe('function');
  });

  it('surfaces the core error message when clearAllAppData fails (Windows file-lock guidance)', async () => {
    const user = userEvent.setup();
    mockClearAllAppData.mockRejectedValueOnce(
      new Error(
        'Failed to remove C:\\Users\\me\\.openhuman because it is locked by another OpenHuman window or process. Close all OpenHuman windows and try again.'
      )
    );
    renderActions();

    await user.click(screen.getByText('Clear App Data').closest('button')!);
    const confirmButtons = screen.getAllByRole('button', { name: /Clear App Data/i });
    await user.click(confirmButtons[confirmButtons.length - 1]);

    expect(
      await screen.findByText(/locked by another OpenHuman window or process/)
    ).toBeInTheDocument();
  });

  it('falls back to the translated message when the error has no message', async () => {
    const user = userEvent.setup();
    mockClearAllAppData.mockRejectedValueOnce(new Error(''));
    renderActions();

    await user.click(screen.getByText('Clear App Data').closest('button')!);
    const confirmButtons = screen.getAllByRole('button', { name: /Clear App Data/i });
    await user.click(confirmButtons[confirmButtons.length - 1]);

    expect(await screen.findByText(/Failed to clear data and logout/)).toBeInTheDocument();
  });
});
