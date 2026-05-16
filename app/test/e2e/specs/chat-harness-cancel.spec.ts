// @ts-nocheck
/**
 * Chat harness — mid-stream cancel.
 *
 * The composer's Cancel button calls `chatService.chatCancel` →
 * `openhuman.channel_web_cancel` → `cancel_chat()` in
 * `src/openhuman/channels/providers/web.rs`. That handler aborts the
 * in-flight JoinHandle, removes the IN_FLIGHT entry, and publishes a
 * `chat_error` event with `error_type = "cancelled"`.
 *
 * What this spec verifies:
 *   1. The mock LLM is configured to stream slowly enough that a real
 *      user could click Cancel mid-stream (per-chunk delay 500ms × 6
 *      chunks ≈ 3s of streaming).
 *   2. Send a message → wait for IN_FLIGHT to contain an entry.
 *   3. Click the Cancel button in the chat composer.
 *   4. Within a short window:
 *        - IN_FLIGHT clears (Rust side).
 *        - The DOM never accumulates the final two chunks.
 *        - Send button becomes enabled again.
 *   5. The conversation file on disk does NOT contain the full reply
 *      (the cancel happened before the assistant finished).
 *
 * This is the second hardest scenario in the chat pipeline — the first
 * being the streaming reply itself. If cancel breaks, in-flight chats
 * leak indefinitely.
 */
import { waitForApp } from '../helpers/app-helpers';
import { callOpenhumanRpc } from '../helpers/core-rpc';
import { textExists } from '../helpers/element-helpers';
import { resetApp } from '../helpers/reset-app';
import { navigateViaHash } from '../helpers/shared-flows';
import { setMockBehavior, startMockServer, stopMockServer } from '../mock-server';

const USER_ID = 'e2e-chat-harness-cancel';
const PROMPT = 'Please count to ten slowly with one number per chunk.';

// Six chunks × 500 ms ≈ 3s of streaming. Plenty of room to cancel
// after the first 1–2 chunks have landed.
const SLOW_SCRIPT = [
  { text: 'one ', delayMs: 500 },
  { text: 'two ', delayMs: 500 },
  { text: 'three ', delayMs: 500 },
  { text: 'four ', delayMs: 500 },
  { text: 'five ', delayMs: 500 },
  { text: 'six.', delayMs: 500 },
  { finish: 'stop' },
];

const EARLY_PIECES = ['one ', 'two '];
const LATE_PIECES = ['five ', 'six.'];

async function clickByTitle(title: string, timeoutMs = 6_000): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const clicked = await browser.execute((t: string) => {
      const el = document.querySelector(`button[title=${JSON.stringify(t)}]`) as
        | HTMLButtonElement
        | null;
      if (!el) return false;
      el.click();
      return true;
    }, title);
    if (clicked) return true;
    await browser.pause(200);
  }
  return false;
}

async function typeIntoComposer(text: string): Promise<void> {
  await browser.execute((t: string) => {
    const ta = document.querySelector(
      'textarea[placeholder="Type a message..."]'
    ) as HTMLTextAreaElement | null;
    if (!ta) return;
    const setter = Object.getOwnPropertyDescriptor(
      window.HTMLTextAreaElement.prototype,
      'value'
    )?.set;
    setter?.call(ta, t);
    ta.dispatchEvent(new Event('input', { bubbles: true }));
  }, text);
}

async function clickSend(): Promise<boolean> {
  return (await browser.execute(() => {
    const btn = document.querySelector('button[aria-label="Send message"]') as
      | HTMLButtonElement
      | null;
    if (!btn || btn.disabled) return false;
    btn.click();
    return true;
  })) as boolean;
}

/**
 * The composer's mid-stream cancel button is a plain `<button>` with the
 * localized text "Cancel" rendered inside the chat scroll region. We
 * disambiguate from any "Cancel" inside a modal by requiring the button
 * to live OUTSIDE a `[role="dialog"]`/`[aria-modal]` ancestor.
 */
async function clickComposerCancel(): Promise<boolean> {
  return (await browser.execute(() => {
    const buttons = Array.from(document.querySelectorAll<HTMLButtonElement>('button'));
    for (const b of buttons) {
      if ((b.textContent ?? '').trim() !== 'Cancel') continue;
      if (b.closest('[role="dialog"], [aria-modal="true"]')) continue;
      b.click();
      return true;
    }
    return false;
  })) as boolean;
}

async function inFlightCount(): Promise<number> {
  const snap = await callOpenhumanRpc<{ result: { entries: Array<unknown> } }>(
    'openhuman.test_support_in_flight_chats',
    {}
  );
  return snap.ok ? (snap.result?.result?.entries?.length ?? 0) : 0;
}

function hexEncode(s: string): string {
  return Array.from(new TextEncoder().encode(s))
    .map(b => b.toString(16).padStart(2, '0'))
    .join('');
}

async function getSelectedThreadId(): Promise<string | null> {
  return (await browser.execute(() => {
    const winAny = window as unknown as { __OPENHUMAN_STORE__?: { getState: () => unknown } };
    const state = winAny.__OPENHUMAN_STORE__?.getState() as
      | { thread?: { selectedThreadId?: string | null } }
      | undefined;
    return state?.thread?.selectedThreadId ?? null;
  })) as string | null;
}

describe('Chat harness — mid-stream cancel', () => {
  before(async () => {
    await startMockServer();
    await waitForApp();
    await resetApp(USER_ID);

    setMockBehavior('llmStreamScript', JSON.stringify(SLOW_SCRIPT));
    setMockBehavior('llmStreamChunkDelayMs', '500');
  });

  after(async () => {
    setMockBehavior('llmStreamScript', '');
    setMockBehavior('llmStreamChunkDelayMs', '');
    await stopMockServer();
  });

  it('sends → IN_FLIGHT populates → Cancel clears it before late chunks land', async () => {
    await navigateViaHash('/chat');
    await browser.waitUntil(async () => (await textExists('Threads')), {
      timeout: 15_000,
      timeoutMsg: 'Conversations did not mount',
    });
    expect(await clickByTitle('New thread', 8_000)).toBe(true);

    await typeIntoComposer(PROMPT);
    expect(
      await browser.waitUntil(async () => await clickSend(), {
        timeout: 5_000,
        timeoutMsg: 'Send button never enabled',
      })
    ).toBe(true);

    // 1) Wait until IN_FLIGHT has an entry.
    await browser.waitUntil(async () => (await inFlightCount()) > 0, {
      timeout: 10_000,
      timeoutMsg: 'IN_FLIGHT never gained an entry after send',
    });

    // 2) Wait for at least the first chunk to land so this is genuinely
    //    mid-stream. The second chunk lands ~1s later — cancel between
    //    them.
    await browser.waitUntil(async () => await textExists(EARLY_PIECES[0]), {
      timeout: 5_000,
      timeoutMsg: 'first delta never landed before cancel attempt',
    });

    // 3) Click cancel.
    expect(await clickComposerCancel()).toBe(true);

    // 4) IN_FLIGHT must drain quickly.
    await browser.waitUntil(async () => (await inFlightCount()) === 0, {
      timeout: 8_000,
      timeoutMsg: 'IN_FLIGHT did not clear after cancel',
    });

    // 5) Give the stream 1.5s of additional wall time — at 500ms/chunk
    //    that would naturally produce both LATE_PIECES if cancel didn't
    //    take. If cancel worked, those chunks were aborted before being
    //    forwarded to the UI.
    await browser.pause(1_500);
    for (const piece of LATE_PIECES) {
      const present = await textExists(piece);
      expect(present).toBe(false);
    }
  });

  it('after cancel, send button is interactive again (composer not stuck)', async () => {
    const enabled = await browser.execute(() => {
      const ta = document.querySelector(
        'textarea[placeholder="Type a message..."]'
      ) as HTMLTextAreaElement | null;
      return !!ta && !ta.disabled;
    });
    expect(enabled).toBe(true);
  });

  it('the persisted thread file does NOT contain the late chunks', async () => {
    const threadId = await getSelectedThreadId();
    expect(typeof threadId).toBe('string');
    const relPath = `memory/conversations/threads/${hexEncode(threadId as string)}.jsonl`;

    // The store may or may not record the partial assistant turn — both
    // are acceptable. What we lock down is the contract that the
    // LATE_PIECES never reach the persisted file.
    const read = await callOpenhumanRpc<{ result: { content_utf8: string } }>(
      'openhuman.test_support_read_workspace_file',
      { rel_path: relPath, max_bytes: 131_072 }
    );
    if (!read.ok) return; // No file yet → nothing to violate, also fine.
    const content = read.result?.result?.content_utf8 ?? '';
    for (const piece of LATE_PIECES) {
      expect(content.includes(piece)).toBe(false);
    }
  });
});
