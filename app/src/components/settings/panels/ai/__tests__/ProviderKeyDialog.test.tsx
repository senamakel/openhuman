import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { renderWithProviders } from '../../../../../test/test-utils';
import { ProviderKeyDialog } from '../ProviderConnectControls';

describe('ProviderKeyDialog OAuth action', () => {
  // Regression (review on #6265): `onPersisting` locks the dialog while a key is
  // saved. A caller that succeeds without closing the dialog must not leave it
  // locked.
  it('releases the lock after a successful OAuth action that keeps the dialog open', async () => {
    renderWithProviders(
      <ProviderKeyDialog
        slug="openrouter"
        label="OpenRouter"
        isLocalRuntime={false}
        onCancel={vi.fn()}
        onSubmit={vi.fn()}
        oauthAction={{
          label: 'Sign in with OpenRouter',
          onClick: async ({ onPersisting }) => {
            onPersisting();
          },
        }}
      />
    );

    const dialog = await screen.findByRole('dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: /Sign in with OpenRouter/i }));

    // Every control the saving phase disables must come back, not just Cancel.
    await waitFor(() => {
      expect(within(dialog).getByRole('button', { name: /^Cancel$/i })).toBeEnabled();
      expect(within(dialog).getByRole('button', { name: /^Save$/i })).toBeEnabled();
      expect(
        within(dialog).getByRole('button', { name: /Sign in with OpenRouter/i })
      ).toBeEnabled();
    });
  });
});
