// @ts-nocheck
/**
 * E2E spec: Interactive guided tour — gates and resume behaviour (#1215).
 *
 * Three scenarios are exercised:
 *
 *   1. Skills gate: start tour, reach the skills step, confirm skills UI is
 *      present. The tooltip advances via Next — the current implementation
 *      navigates to /skills and highlights the grid, but does NOT gate
 *      progression on actually connecting a skill. The gating assertion is
 *      skipped and the gap is called out explicitly.
 *
 *   2. Chat gate: reach the final step which pre-seeds a welcome message in
 *      the chat panel. The spec verifies the step title renders and that the
 *      Skip button is absent on the last step (per WalkthroughTooltip logic).
 *      Sending a message is not required to advance — gating is not
 *      implemented; skipped assertion calls out the gap.
 *
 *   3. Resume after reload: set walkthrough pending + a step-index cookie,
 *      reload the renderer, and assert the tour restarts. True mid-step
 *      resume (at last incomplete step) is NOT implemented — the step index
 *      is not persisted across sessions. The specific resume assertion is
 *      skipped with a comment pointing at the missing implementation.
 *
 * Product gaps surfaced (both skipped):
 *   - GP-1: No skill-connection gate on the /skills tour step.
 *   - GP-2: No step-index persistence — tour always restarts from step 0
 *           on reload rather than resuming at the last incomplete step.
 *
 * Implementation notes:
 *   - The walkthrough is driven by manipulating localStorage keys directly
 *     (`openhuman:walkthrough_pending`, `openhuman:walkthrough_completed`)
 *     rather than walking the full onboarding flow, because (a) resetApp
 *     already handles onboarding and (b) the Joyride component reads these
 *     keys on mount.
 *   - `data-walkthrough` attributes are queried to verify step targets are
 *     present without coupling to tooltip text that may be i18n-translated.
 *   - The spec uses `supportsExecuteScript()` guards so it degrades
 *     gracefully on Appium Mac2 (where `browser.execute` is unavailable in
 *     a WKWebView context).
 */
import { waitForApp } from '../helpers/app-helpers';
import { textExists, waitForText } from '../helpers/element-helpers';
import { supportsExecuteScript } from '../helpers/platform';
import { resetApp } from '../helpers/reset-app';
import {
  dismissWalkthroughIfVisible,
  navigateViaHash,
  waitForHomePage,
} from '../helpers/shared-flows';
import { startMockServer, stopMockServer } from '../mock-server';

const USER_ID = 'e2e-guided-tour-gates';

// localStorage keys mirrored from AppWalkthrough.tsx
const WALKTHROUGH_KEY = 'openhuman:walkthrough_completed';
const WALKTHROUGH_PENDING_KEY = 'openhuman:walkthrough_pending';

// ── helpers ──────────────────────────────────────────────────────────────────

/**
 * Arm the walkthrough: clear the completed flag, set the pending flag.
 * Equivalent to what resetWalkthrough() does in production code.
 * Returns false when execute() is unavailable (Mac2).
 */
async function armWalkthrough(): Promise<boolean> {
  if (!supportsExecuteScript()) return false;
  await browser.execute(
    ({ pendingKey, completedKey }: { pendingKey: string; completedKey: string }) => {
      try {
        localStorage.removeItem(completedKey);
        localStorage.setItem(pendingKey, 'true');
      } catch (_) {
        // swallow — mirrors AppWalkthrough try/catch
      }
    },
    { pendingKey: WALKTHROUGH_PENDING_KEY, completedKey: WALKTHROUGH_KEY }
  );
  return true;
}

/**
 * Mark walkthrough complete in localStorage so subsequent specs start clean.
 */
async function disarmWalkthrough(): Promise<void> {
  if (!supportsExecuteScript()) return;
  await browser.execute(
    ({ completedKey, pendingKey }: { completedKey: string; pendingKey: string }) => {
      try {
        localStorage.setItem(completedKey, 'true');
        localStorage.removeItem(pendingKey);
      } catch (_) {
        // ignore
      }
    },
    { completedKey: WALKTHROUGH_KEY, pendingKey: WALKTHROUGH_PENDING_KEY }
  );
}

/**
 * Fire the `walkthrough:restart` CustomEvent so a mounted AppWalkthrough
 * component picks up the armed localStorage state and shows the Joyride UI.
 */
async function dispatchWalkthroughRestart(): Promise<void> {
  if (!supportsExecuteScript()) return;
  await browser.execute(() => {
    window.dispatchEvent(new CustomEvent('walkthrough:restart'));
  });
}

/**
 * Wait up to `timeout` ms for the Joyride tooltip overlay to be visible.
 * Detection: the WalkthroughTooltip renders a `[role="tooltip"]` div.
 */
async function waitForTourTooltip(timeout = 15_000): Promise<boolean> {
  if (!supportsExecuteScript()) return false;
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const visible = await browser.execute(() => {
      return document.querySelector('[role="tooltip"]') !== null;
    });
    if (visible) return true;
    await browser.pause(400);
  }
  return false;
}

/**
 * Advance the tour by clicking the primary (Next/Let's go) button inside
 * the tooltip overlay. Returns true if the click landed, false if no button
 * was found within `timeout`.
 */
async function clickTourNext(timeout = 8_000): Promise<boolean> {
  if (!supportsExecuteScript()) return false;
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const clicked = await browser.execute(() => {
      const tooltip = document.querySelector('[role="tooltip"]');
      if (!tooltip) return false;
      // Primary button carries data-action="primary" (set by Joyride on primaryProps)
      const primary = tooltip.querySelector<HTMLButtonElement>('[data-action="primary"]');
      if (!primary) return false;
      primary.click();
      return true;
    });
    if (clicked) return true;
    await browser.pause(300);
  }
  return false;
}

/**
 * Advance the tour N times, pausing briefly between clicks.
 */
async function advanceTourSteps(count: number): Promise<void> {
  for (let i = 0; i < count; i++) {
    const clicked = await clickTourNext(6_000);
    if (!clicked) {
      console.warn(`[guided-tour-gates] clickTourNext: no primary button on advance ${i + 1}`);
      break;
    }
    // Allow the before() hook to navigate and the DOM to settle.
    await browser.pause(1_500);
  }
}

// ── suite ─────────────────────────────────────────────────────────────────────

describe('Guided tour — gates and resume behaviour (#1215)', function () {
  this.timeout(180_000);

  before(async () => {
    await startMockServer();
    await waitForApp();
    await resetApp(USER_ID);
  });

  afterEach(async () => {
    // Always disarm so the next scenario starts clean.
    await disarmWalkthrough();
    await dismissWalkthroughIfVisible(4_000);
  });

  after(async () => {
    await stopMockServer();
  });

  // ── Scenario 1: Skills gate ────────────────────────────────────────────────

  describe('Scenario 1 — skills gate', () => {
    it('tour starts and tooltip is visible at step 1 (home-card)', async () => {
      if (!supportsExecuteScript()) {
        console.log('[guided-tour-gates] skipping: execute() unsupported on this driver');
        return;
      }

      await navigateViaHash('/home');
      await armWalkthrough();
      await dispatchWalkthroughRestart();

      const tooltipVisible = await waitForTourTooltip(10_000);
      expect(tooltipVisible).toBe(true);

      const hasTitle = await textExists('Your command center');
      expect(hasTitle).toBe(true);
    });

    it('tour navigates to /skills and highlights skills-grid after 3 Next clicks', async () => {
      if (!supportsExecuteScript()) {
        console.log('[guided-tour-gates] skipping: execute() unsupported on this driver');
        return;
      }

      await navigateViaHash('/home');
      await armWalkthrough();
      await dispatchWalkthroughRestart();

      // Advance past step 1 (home-card), step 2 (home-cta) and step 3 (chat).
      // Step 4 (skills-grid) has a `before` hook that navigates to /skills.
      await waitForTourTooltip(10_000);
      await advanceTourSteps(3);

      // Route should now be /skills.
      const hash = await browser.execute(() => window.location.hash);
      expect(String(hash)).toContain('/skills');

      // The data-walkthrough target for step 4 should exist in the DOM.
      const skillsGridPresent = await browser.execute(() => {
        return document.querySelector('[data-walkthrough="skills-grid"]') !== null;
      });
      expect(skillsGridPresent).toBe(true);
    });

    // GP-1: Skills gate is not implemented in the current walkthrough.
    // The tour advances to the next step regardless of whether the user has
    // actually connected a skill. A real gating implementation would need to
    // hold the "Next" button disabled until a `openhuman.skills_list` RPC
    // call confirms at least one skill is connected, then re-enable it.
    it.skip(
      'GP-1 (NOT IMPLEMENTED): tour Next button is disabled until user connects a skill',
      async () => {
        // Expected product behaviour: the Next button on the /skills step
        // should remain disabled (`aria-disabled="true"` or `disabled`) while
        // no skill is connected, and become enabled only after the
        // `skills.skill_connected` event fires or a polling RPC returns ≥ 1
        // installed skill.
        //
        // Current state: the button is always enabled — clicking Next
        // immediately advances to the channels step without any skill check.
        //
        // File: app/src/components/walkthrough/AppWalkthrough.tsx
        //       app/src/components/walkthrough/walkthroughSteps.ts (step index 3)
        const primaryDisabled = await browser.execute(() => {
          const btn = document.querySelector<HTMLButtonElement>(
            '[role="tooltip"] [data-action="primary"]'
          );
          return btn?.disabled ?? btn?.getAttribute('aria-disabled') === 'true';
        });
        expect(primaryDisabled).toBe(true);
      }
    );
  });

  // ── Scenario 2: Chat gate (first message) ─────────────────────────────────

  describe('Scenario 2 — chat gate (first message)', () => {
    it('final tour step renders on /chat with a pre-seeded welcome note', async () => {
      if (!supportsExecuteScript()) {
        console.log('[guided-tour-gates] skipping: execute() unsupported on this driver');
        return;
      }

      await navigateViaHash('/home');
      await armWalkthrough();
      await dispatchWalkthroughRestart();

      // Advance 8 steps (steps 1-8) to reach the final step 9.
      // Step 9's before() hook creates a thread and seeds a welcome message.
      await waitForTourTooltip(10_000);
      await advanceTourSteps(8);

      // Final step tooltip should show "You're all set!"
      const hasLastStepTitle = await textExists("You're all set!");
      expect(hasLastStepTitle).toBe(true);

      // Skip button is hidden on the last step (WalkthroughTooltip renders it
      // only when !isLastStep) — this is verifiable via DOM inspection.
      const skipVisible = await browser.execute(() => {
        const tooltip = document.querySelector('[role="tooltip"]');
        if (!tooltip) return false;
        const skip = tooltip.querySelector<HTMLButtonElement>('[data-action="skip"]');
        return skip !== null && !skip.hidden;
      });
      expect(skipVisible).toBe(false);
    });

    it('chat panel target element is present when final step is active', async () => {
      if (!supportsExecuteScript()) {
        console.log('[guided-tour-gates] skipping: execute() unsupported on this driver');
        return;
      }

      // Navigate directly to /chat so we can check the target presence
      // independently of the full tour advance sequence (keeps the test fast).
      await navigateViaHash('/chat');
      const chatPanel = await browser.execute(() => {
        return document.querySelector('[data-walkthrough="chat-agent-panel"]') !== null;
      });
      // The data-walkthrough attribute must exist for Joyride to focus the step.
      expect(chatPanel).toBe(true);
    });

    // GP-1 (chat variant): No user-message gate on the final /chat step.
    // The final step should require the user to send at least one message
    // before the "Let's go!" button dismisses the tour and marks it complete.
    // Currently clicking "Let's go!" on the final step immediately calls
    // markWalkthroughComplete() without any check that a message was sent.
    it.skip(
      'GP-1 (chat, NOT IMPLEMENTED): Let\'s go! button is disabled until user sends first message',
      async () => {
        // Expected: the primary button text reads "Let's go!" AND is disabled
        // while the thread message count is 0.  After the user submits a
        // message to the chat panel the button should become enabled.
        //
        // Current state: always enabled — see AppWalkthrough.tsx handleEvent.
        const letsGoBtnDisabled = await browser.execute(() => {
          const btn = document.querySelector<HTMLButtonElement>(
            '[role="tooltip"] [data-action="primary"]'
          );
          return btn?.disabled ?? btn?.getAttribute('aria-disabled') === 'true';
        });
        expect(letsGoBtnDisabled).toBe(true);
      }
    );
  });

  // ── Scenario 3: Resume after relaunch ─────────────────────────────────────

  describe('Scenario 3 — resume after relaunch (close + reopen)', () => {
    it('walkthrough re-shows after renderer reload when pending flag is set', async () => {
      if (!supportsExecuteScript()) {
        console.log('[guided-tour-gates] skipping: execute() unsupported on this driver');
        return;
      }

      await navigateViaHash('/home');
      await armWalkthrough();

      // Simulate a "relaunch" by reloading the renderer without clearing
      // localStorage.  The walkthrough pending flag persists across a reload,
      // so the AppWalkthrough component should remount and start the tour.
      await browser.execute(() => {
        window.location.reload();
      });
      await browser.pause(2_000);

      // Wait for the app to stabilise after reload.
      const deadline = Date.now() + 15_000;
      while (Date.now() < deadline) {
        try {
          const ready = await browser.execute(() => document.readyState === 'complete');
          if (ready) break;
        } catch {
          // session recovering
        }
        await browser.pause(500);
      }

      // Home page must be reachable.
      await waitForHomePage(15_000);

      // Tour must auto-start because the pending flag survived the reload.
      const tooltipVisible = await waitForTourTooltip(12_000);
      expect(tooltipVisible).toBe(true);
    });

    // GP-2: Step-index persistence is not implemented.
    // Closing the app mid-tour and relaunching always restarts the walkthrough
    // from step 0 (home-card), regardless of which step was last active.
    // A proper implementation would persist the current step index to
    // localStorage (e.g. `openhuman:walkthrough_step_index`) and restore it
    // when AppWalkthrough mounts with `run=true`.
    it.skip(
      'GP-2 (NOT IMPLEMENTED): tour resumes at last incomplete step after reload',
      async () => {
        // Expected product behaviour:
        //   1. User advances to step 4 (/skills).
        //   2. App is closed (renderer reloaded) before the tour finishes.
        //   3. On reopen the tour shows step 4, not step 0.
        //
        // Current state: Joyride always starts from stepIndex=0 because
        // AppWalkthrough does not pass a `stepIndex` prop derived from
        // persisted state. The `openhuman:walkthrough_step_index` key does
        // not exist anywhere in the codebase.
        //
        // Files to modify:
        //   app/src/components/walkthrough/AppWalkthrough.tsx  (add stepIndex state + persistence)
        //   app/src/components/walkthrough/walkthroughSteps.ts (persist on STEP_AFTER events)

        // Arm walkthrough and advance 3 steps to simulate partial progress.
        await navigateViaHash('/home');
        await armWalkthrough();
        await dispatchWalkthroughRestart();
        await waitForTourTooltip(10_000);
        await advanceTourSteps(3);

        // Read the persisted step index (does not exist yet).
        const persistedStep = await browser.execute(() => {
          return localStorage.getItem('openhuman:walkthrough_step_index');
        });
        expect(persistedStep).toBe('3');

        // Reload the renderer — simulates app relaunch.
        await browser.execute(() => window.location.reload());
        await browser.pause(2_000);
        await waitForHomePage(15_000);

        // Verify the tour resumed at step 4, not step 0.
        const stepIndicator = await browser.execute(() => {
          const tooltip = document.querySelector('[role="tooltip"]');
          if (!tooltip) return null;
          // Step counter is rendered as "N of 10" inside the tooltip.
          return tooltip.textContent;
        });
        expect(stepIndicator).toContain('4 of 10');
      }
    );
  });
});
