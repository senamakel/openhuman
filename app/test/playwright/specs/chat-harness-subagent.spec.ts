import { expect, type Page, test } from '@playwright/test';

import {
  bootAuthenticatedPage,
  dismissWalkthroughIfPresent,
  waitForAppReady,
} from '../helpers/core-rpc';

const MOCK_ADMIN_BASE = `http://127.0.0.1:${process.env.E2E_MOCK_PORT || '18473'}`;
const USER_ID = 'pw-chat-subagent';
const PROMPT = 'Research the answer to life and tell me a marker phrase.';
const CANARY_FINAL = 'subagent-canary-final-7afe2';
const RESEARCHER_REPLY = 'The researcher answer is 42.';
const KEYWORD_RESPONSES = [
  { keyword: "Search the user's memory tree", content: 'No relevant memory.' },
  {
    keyword: PROMPT,
    content: '',
    toolCalls: [
      {
        id: 'call_research_1',
        name: 'research',
        arguments: JSON.stringify({ prompt: 'Tell me a marker phrase' }),
      },
    ],
  },
  { keyword: 'Tell me a marker phrase', content: RESEARCHER_REPLY },
  { keyword: RESEARCHER_REPLY, content: `Done. The result is: ${CANARY_FINAL}` },
];

interface MockRequest {
  body?: string;
  method: string;
  url: string;
}

async function resetMock(): Promise<void> {
  await fetch(`${MOCK_ADMIN_BASE}/__admin/reset`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({}),
  });
}

async function setMockBehavior(key: string, value: string): Promise<void> {
  await fetch(`${MOCK_ADMIN_BASE}/__admin/behavior`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ key, value }),
  });
}

async function requests(): Promise<MockRequest[]> {
  const response = await fetch(`${MOCK_ADMIN_BASE}/__admin/requests`);
  const payload = (await response.json()) as { data?: MockRequest[] };
  return Array.isArray(payload.data) ? payload.data : [];
}

async function openChat(page: Page): Promise<void> {
  await bootAuthenticatedPage(page, USER_ID, '/chat');
  await page.goto('/#/chat');
  await waitForAppReady(page);
  await dismissWalkthroughIfPresent(page);
  await expect(page.getByTestId('send-message-button')).toBeVisible();
}

async function selectedThreadId(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    const store = (
      window as unknown as {
        __OPENHUMAN_STORE__?: {
          getState?: () => { thread?: { selectedThreadId?: string | null } };
        };
      }
    ).__OPENHUMAN_STORE__;
    return store?.getState?.().thread?.selectedThreadId ?? null;
  });
}

async function createNewThread(page: Page): Promise<string> {
  const before = await selectedThreadId(page);
  await dismissWalkthroughIfPresent(page);
  const sidebarButton = page.getByTestId('new-thread-sidebar-button');
  if (await sidebarButton.isVisible().catch(() => false)) {
    await sidebarButton.click({ force: true });
  } else {
    await page.getByTestId('new-thread-button').click({ force: true });
  }
  const changed = await expect
    .poll(
      async () => {
        const current = await selectedThreadId(page);
        return current && current !== before ? current : null;
      },
      { timeout: 10_000 }
    )
    .not.toBeNull()
    .then(
      () => true,
      () => false
    );
  const id = await selectedThreadId(page);
  if (changed && id) return id;
  if (id) return id;
  if (before) return before;
  throw new Error('selectedThreadId was not populated');
}

async function waitForSocketConnected(page: Page): Promise<void> {
  await expect
    .poll(
      async () =>
        page.evaluate(() => {
          const store = (
            window as unknown as {
              __OPENHUMAN_STORE__?: {
                getState?: () => { socket?: { byUser?: Record<string, { status?: string }> } };
              };
            }
          ).__OPENHUMAN_STORE__;
          const byUser = store?.getState?.().socket?.byUser ?? {};
          return Object.values(byUser).some(entry => entry?.status === 'connected');
        }),
      { timeout: 30_000 }
    )
    .toBe(true);
}

async function sendMessage(page: Page, prompt: string): Promise<void> {
  await waitForSocketConnected(page);
  await dismissWalkthroughIfPresent(page);
  await page.getByPlaceholder('How can I help you today?').fill(prompt);
  await dismissWalkthroughIfPresent(page);
  await expect(page.getByTestId('send-message-button')).toBeEnabled();
  await page.getByTestId('send-message-button').click();
}

test.describe('Chat Harness - Subagent', () => {
  test('delegates to a subagent through the harness', async ({ page }) => {
    test.setTimeout(150_000);

    await resetMock();
    await setMockBehavior('llmForcedResponses', '');
    await setMockBehavior('llmKeywordRules', JSON.stringify(KEYWORD_RESPONSES));
    await setMockBehavior('llmStreamChunkDelayMs', '10');

    await openChat(page);
    await createNewThread(page);
    await sendMessage(page, PROMPT);

    await expect
      .poll(
        async () => {
          const log = await requests();
          const llmRequests = log.filter(
            entry => entry.method === 'POST' && entry.url.includes('/openai/v1/chat/completions')
          );
          const bodies = llmRequests.map(entry => entry.body ?? '');
          const memoryIndex = bodies.findIndex(body =>
            body.includes(
              "Search the user's memory tree and return only context relevant to the next agent turn."
            )
          );
          const delegatedPromptIndex = bodies.findIndex(body =>
            body.includes('Tell me a marker phrase')
          );
          return memoryIndex >= 0 && delegatedPromptIndex > memoryIndex;
        },
        { timeout: 90_000 }
      )
      .toBe(true);
  });
});
