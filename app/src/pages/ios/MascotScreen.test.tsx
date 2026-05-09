/**
 * MascotScreen tests — render, send message, disconnect.
 *
 * Mocks:
 *  - services/chatService: chatSend + subscribeChatEvents
 *  - services/transport/profileStore: listProfiles + deleteProfile
 *  - features/human/useHumanMascot: returns idle face
 *  - features/human/Mascot (YellowMascot): lightweight stub
 *  - react-router-dom: mock useNavigate
 */
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MascotScreen } from './MascotScreen';

// -- module mocks ------------------------------------------------------------

const mockNavigate = vi.fn();
vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof import('react-router-dom')>('react-router-dom');
  return { ...actual, useNavigate: () => mockNavigate };
});

const mockChatSend = vi.fn();
const mockUnsubscribe = vi.fn();
const mockSubscribeChatEvents = vi.fn((_listeners: unknown) => mockUnsubscribe);
vi.mock('../../services/chatService', () => ({
  chatSend: (args: unknown) => mockChatSend(args),
  subscribeChatEvents: (listeners: unknown) => mockSubscribeChatEvents(listeners),
}));

const mockListProfiles = vi.fn();
const mockDeleteProfile = vi.fn();
vi.mock('../../services/transport/profileStore', () => ({
  listProfiles: () => mockListProfiles(),
  deleteProfile: (...args: unknown[]) => mockDeleteProfile(...args),
  saveProfile: vi.fn(),
  getProfile: vi.fn(),
  listProfileIds: vi.fn(() => []),
}));

vi.mock('../../features/human/useHumanMascot', () => ({
  useHumanMascot: vi.fn(() => ({ face: 'idle', viseme: { aa: 0, E: 0, I: 0, O: 0, U: 0 } })),
}));

// Stub YellowMascot to avoid SVG / RAF complexity in tests.
vi.mock('../../features/human/Mascot', () => ({
  YellowMascot: ({ face }: { face: string }) => (
    <div data-testid="yellow-mascot" data-face={face} />
  ),
}));

// -- helpers -----------------------------------------------------------------

function renderMascotScreen() {
  return render(
    <MemoryRouter initialEntries={['/mascot']}>
      <MascotScreen />
    </MemoryRouter>
  );
}

// -- setup / teardown --------------------------------------------------------

beforeEach(() => {
  mockNavigate.mockReset();
  mockChatSend.mockReset();
  mockSubscribeChatEvents.mockClear();
  mockUnsubscribe.mockReset();
  mockListProfiles.mockReturnValue([{ id: 'chan1', label: 'Home desktop', kind: 'tunnel' }]);
  mockDeleteProfile.mockReset();
});

afterEach(() => {
  vi.clearAllMocks();
});

// -- tests -------------------------------------------------------------------

describe('MascotScreen', () => {
  it('renders mascot canvas and input', () => {
    renderMascotScreen();
    expect(screen.getByTestId('yellow-mascot')).toBeInTheDocument();
    expect(screen.getByPlaceholderText(/type a message/i)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /send message/i })).toBeInTheDocument();
  });

  it('shows paired desktop label in header', () => {
    renderMascotScreen();
    expect(screen.getByText('Home desktop')).toBeInTheDocument();
  });

  it('shows Disconnect button', () => {
    renderMascotScreen();
    expect(screen.getByRole('button', { name: /disconnect/i })).toBeInTheDocument();
  });

  it('PTT button is present and disabled', () => {
    renderMascotScreen();
    const pttBtn = screen.getByRole('button', { name: /push to talk/i });
    expect(pttBtn).toBeDisabled();
  });

  it('send button is disabled when input is empty', () => {
    renderMascotScreen();
    const sendBtn = screen.getByRole('button', { name: /send message/i });
    expect(sendBtn).toBeDisabled();
  });

  it('typing a message enables send button', async () => {
    renderMascotScreen();
    const input = screen.getByPlaceholderText(/type a message/i);
    await userEvent.type(input, 'Hello mascot');
    expect(screen.getByRole('button', { name: /send message/i })).not.toBeDisabled();
  });

  it('sending a message calls chatSend with the text', async () => {
    mockChatSend.mockResolvedValueOnce(undefined);
    renderMascotScreen();

    const input = screen.getByPlaceholderText(/type a message/i);
    await userEvent.type(input, 'Hello mascot');
    await userEvent.click(screen.getByRole('button', { name: /send message/i }));

    await waitFor(() => {
      expect(mockChatSend).toHaveBeenCalledOnce();
    });
    const call = mockChatSend.mock.calls[0][0];
    expect(call.message).toBe('Hello mascot');
    expect(typeof call.threadId).toBe('string');
  });

  it('sends on Enter key press', async () => {
    mockChatSend.mockResolvedValueOnce(undefined);
    renderMascotScreen();

    const input = screen.getByPlaceholderText(/type a message/i);
    await userEvent.type(input, 'Hi{Enter}');

    await waitFor(() => {
      expect(mockChatSend).toHaveBeenCalledOnce();
    });
  });

  it('clears input after sending', async () => {
    mockChatSend.mockResolvedValueOnce(undefined);
    renderMascotScreen();

    const input = screen.getByPlaceholderText(/type a message/i);
    await userEvent.type(input, 'Hello');
    await userEvent.click(screen.getByRole('button', { name: /send message/i }));

    await waitFor(() => {
      expect((input as HTMLInputElement).value).toBe('');
    });
  });

  it('subscribes to chat events on mount', () => {
    renderMascotScreen();
    expect(mockSubscribeChatEvents).toHaveBeenCalledOnce();
  });

  it('disconnect clears profiles and navigates to /pair', async () => {
    renderMascotScreen();
    await userEvent.click(screen.getByRole('button', { name: /disconnect/i }));

    expect(mockDeleteProfile).toHaveBeenCalledWith('chan1');
    expect(mockNavigate).toHaveBeenCalledWith('/pair', { replace: true });
  });

  it('shows error message in transcript on chatSend rejection', async () => {
    mockChatSend.mockRejectedValueOnce(new Error('Network error'));
    renderMascotScreen();

    const input = screen.getByPlaceholderText(/type a message/i);
    await userEvent.type(input, 'Test message');
    await userEvent.click(screen.getByRole('button', { name: /send message/i }));

    await waitFor(() => {
      expect(screen.getByText(/failed to send/i)).toBeInTheDocument();
    });
  });
});
