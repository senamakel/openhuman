import { beforeEach, describe, expect, it, vi } from 'vitest';

const mockFetchCurrentUser = vi.fn();

vi.mock('../../session/sessionOwner', () => ({
  fetchCurrentUser: (...args: unknown[]) => mockFetchCurrentUser(...args),
}));

const { userApi } = await import('../userApi');

function getMockUser() {
  return {
    _id: 'user-123',
    telegramId: 12345678,
    hasAccess: true,
    magicWord: 'alpha',
    firstName: 'Test',
    lastName: 'User',
    username: 'testuser',
    role: 'user',
    activeTeamId: 'team-1',
    referral: {},
    subscription: { hasActiveSubscription: false, plan: 'FREE' },
    settings: {
      dailySummariesEnabled: false,
      dailySummaryChatIds: [],
      autoCompleteEnabled: false,
      autoCompleteVisibility: 'always',
      autoCompleteWhitelistChatIds: [],
      autoCompleteBlacklistChatIds: [],
    },
    usage: {
      cycleBudgetUsd: 10,
      remainingUsd: 10,
      spentThisCycleUsd: 0,
      spentTodayUsd: 0,
      cycleStartDate: new Date().toISOString(),
    },
    autoDeleteTelegramMessagesAfterDays: 30,
    autoDeleteThreadsAfterDays: 30,
  };
}

describe('userApi.getMe', () => {
  beforeEach(() => {
    mockFetchCurrentUser.mockReset();
  });

  it('returns user data on success', async () => {
    mockFetchCurrentUser.mockResolvedValue({ user: getMockUser(), stale: false, staleSeconds: 0 });

    const user = await userApi.getMe();

    // A live answer: the session owner is asked to bypass its cache.
    expect(mockFetchCurrentUser).toHaveBeenCalledWith(true);
    expect(user._id).toBe('user-123');
    expect(user.firstName).toBe('Test');
    expect(user.username).toBe('testuser');
    expect(user.subscription.plan).toBe('FREE');
  });

  it('throws when the owner rejects the session', async () => {
    mockFetchCurrentUser.mockRejectedValue(new Error('REJECTED: Unauthorized'));

    await expect(userApi.getMe()).rejects.toThrow('REJECTED: Unauthorized');
  });

  it('throws when nobody is signed in', async () => {
    mockFetchCurrentUser.mockResolvedValue({ user: null, stale: false, staleSeconds: null });

    await expect(userApi.getMe()).rejects.toThrow('no signed-in user');
  });

  it('throws on network error', async () => {
    mockFetchCurrentUser.mockRejectedValue(new Error('TRANSIENT: Service unavailable'));

    await expect(userApi.getMe()).rejects.toBeDefined();
  });
});
