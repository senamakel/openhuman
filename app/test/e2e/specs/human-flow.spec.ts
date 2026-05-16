// @ts-nocheck
/**
 * Rich /human E2E — exercises the mascot stage + embedded chat sidebar.
 *
 * The /human page composes three things:
 *   - YellowMascot (svg-driven face)
 *   - A "Push to Talk" toggle that persists to `localStorage`
 *     under the `human.speakReplies` key
 *   - A `<Conversations variant="sidebar" composer="mic-cloud" />` panel
 *     anchored to the right of the mascot. The mic-cloud variant
 *     replaces the textarea composer with a mic button — voice-only
 *     send. A real voice round-trip needs a microphone (not available
 *     in headless CEF), so this spec asserts shape, not transcript.
 *
 * What this spec proves end-to-end:
 *   1. /human route mounts and the mascot stage renders.
 *   2. Push-to-Talk toggle round-trips through localStorage —
 *      flipping the checkbox writes `human.speakReplies` and re-reading
 *      reflects the new value.
 *   3. The Conversations sidebar mounts in `variant="sidebar"` shape:
 *      thread sidebar header is present AND the mic-cloud composer
 *      (no textarea) is rendered instead of the text composer.
 *   4. Creating a new thread via the "+" button works on this surface
 *      too — same primitive as the unified /chat spec, but exercised
 *      inside the sidebar variant.
 */
import { waitForApp } from '../helpers/app-helpers';
import { textExists, waitForText } from '../helpers/element-helpers';
import { resetApp } from '../helpers/reset-app';
import { navigateViaHash } from '../helpers/shared-flows';
import { startMockServer, stopMockServer } from '../mock-server';

const USER_ID = 'e2e-human-flow';
const SPEAK_REPLIES_KEY = 'human.speakReplies';

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
    await browser.pause(250);
  }
  return false;
}

async function readSpeakRepliesFromStorage(): Promise<string | null> {
  return (await browser.execute(
    (key: string) => window.localStorage.getItem(key),
    SPEAK_REPLIES_KEY
  )) as string | null;
}

async function togglePushToTalk(): Promise<boolean> {
  return (await browser.execute(() => {
    // The toggle is a checkbox inside a <label> that says "Push to Talk".
    const labels = Array.from(document.querySelectorAll<HTMLLabelElement>('label'));
    const ptt = labels.find(l => /push to talk/i.test(l.textContent ?? ''));
    const cb = ptt?.querySelector('input[type="checkbox"]') as HTMLInputElement | null;
    if (!cb) return false;
    cb.click();
    return true;
  })) as boolean;
}

describe('Human flow — /human mascot + sidebar chat', () => {
  before(async () => {
    await startMockServer();
    await waitForApp();
    await resetApp(USER_ID);
  });

  after(async () => {
    await stopMockServer();
  });

  it('mounts the mascot stage and the Push to Talk control', async () => {
    await navigateViaHash('/human');

    const hash = await browser.execute(() => window.location.hash);
    expect(String(hash)).toContain('/human');

    // Mascot is an inline SVG — its presence is the strongest "the page
    // really rendered" signal.
    const hasMascot = await browser.waitUntil(
      async () =>
        Boolean(
          await browser.execute(() => {
            // YellowMascot renders an <svg> inside the mascot stage.
            return document.querySelectorAll('svg').length > 0;
          })
        ),
      { timeout: 15_000, timeoutMsg: '/human did not render any SVG (mascot missing)' }
    );
    expect(hasMascot).toBe(true);

    await waitForText('Push to Talk', 10_000);
    expect(await textExists('Push to Talk')).toBe(true);
  });

  it('Push to Talk toggle persists to localStorage (human.speakReplies)', async () => {
    const before = await readSpeakRepliesFromStorage();
    // HumanPage defaults the checkbox to checked → "1" in storage on first
    // mount. Accept any pre-state; we only care that the toggle flips.
    const toggled = await togglePushToTalk();
    expect(toggled).toBe(true);

    const after = await browser.waitUntil(
      async () => {
        const v = await readSpeakRepliesFromStorage();
        return v !== null && v !== before;
      },
      {
        timeout: 5_000,
        timeoutMsg: `${SPEAK_REPLIES_KEY} did not change after toggling Push to Talk`,
      }
    );
    expect(after).toBe(true);
  });

  it('embedded chat sidebar mounts in mic-cloud (voice-only) variant', async () => {
    // The sidebar variant of Conversations still shows the thread list
    // header so the user can see + pick threads.
    await waitForText('Threads', 10_000);
    expect(await textExists('Threads')).toBe(true);

    // mic-cloud composer = NO textarea (the text composer is replaced
    // entirely by MicComposer). That's the structural difference between
    // /chat and /human and the contract this spec locks down.
    const noTextarea = await browser.execute(
      () => document.querySelectorAll('textarea[placeholder="Type a message..."]').length === 0
    );
    expect(noTextarea).toBe(true);
  });

  it('creating a new thread on /human works the same way it does on /chat', async () => {
    const clicked = await clickByTitle('New thread', 8_000);
    expect(clicked).toBe(true);

    // After creation, the active thread changes; reuse the threadId-via-
    // redux check the slack/whatsapp specs use. `thread.selectedThreadId`
    // should now be set.
    const selected = await browser.waitUntil(
      async () =>
        Boolean(
          await browser.execute(() => {
            const winAny = window as unknown as {
              __OPENHUMAN_STORE__?: { getState: () => unknown };
            };
            const state = winAny.__OPENHUMAN_STORE__?.getState() as
              | { thread?: { selectedThreadId?: string | null } }
              | undefined;
            return Boolean(state?.thread?.selectedThreadId);
          })
        ),
      {
        timeout: 8_000,
        timeoutMsg: 'thread.selectedThreadId never populated after clicking New thread',
      }
    );
    expect(selected).toBe(true);
  });
});
