import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import MedullaDemoChat from './MedullaDemoChat';

// Pass-through translator so assertions can target the i18n keys directly.
vi.mock('../../../lib/i18n/I18nContext', () => ({ useT: () => ({ t: (key: string) => key }) }));

describe('MedullaDemoChat', () => {
  it('renders the demo transcript with a disabled, labelled send control', () => {
    render(<MedullaDemoChat />);

    expect(screen.getByTestId('orch-demo-chat')).toBeInTheDocument();

    const send = screen.getByRole('button', { name: 'orchPage.demo.chat.composerDisabled' });
    expect(send).toBeDisabled();
    expect(send).toHaveAttribute('data-slot', 'button');
    expect(send).toHaveAttribute('data-variant', 'tertiary');
  });
});
