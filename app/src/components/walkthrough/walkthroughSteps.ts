import type { Step } from 'react-joyride';
import type { NavigateFunction } from 'react-router-dom';

import { TOUR_WELCOME_MESSAGE } from '../../constants/onboardingChat';
import { store } from '../../store';
import { addMessageLocal, createNewThread, setSelectedThread } from '../../store/threadSlice';
import type { ThreadMessage } from '../../types/thread';

/**
 * Polls via setTimeout until `[data-walkthrough="<selector>"]` appears in the
 * DOM, then resolves. Rejects after `timeout` ms (default 3000).
 *
 * Uses setTimeout (not rAF) so tests can advance time with fake timers.
 */
export function waitForTarget(selector: string, timeout = 3000): Promise<void> {
  const POLL_INTERVAL = 50;

  return new Promise<void>((resolve, reject) => {
    let elapsed = 0;

    function check() {
      if (document.querySelector(`[data-walkthrough="${selector}"]`)) {
        resolve();
        return;
      }
      elapsed += POLL_INTERVAL;
      if (elapsed >= timeout) {
        reject(
          new Error(`[walkthrough] waitForTarget timed out: [data-walkthrough="${selector}"]`)
        );
        return;
      }
      setTimeout(check, POLL_INTERVAL);
    }

    // Initial check — element may already be present.
    if (document.querySelector(`[data-walkthrough="${selector}"]`)) {
      resolve();
      return;
    }
    setTimeout(check, POLL_INTERVAL);
  });
}

/**
 * Factory that produces the post-onboarding walkthrough sequence.
 *
 * Steps that navigate to a different page receive a `before` async hook that
 * calls `navigate(path)` and then waits for the target element to appear in
 * the DOM via `waitForTarget`.
 *
 * All targets follow the `[data-walkthrough="<name>"]` convention — add the
 * attribute to the corresponding DOM element in the page/component.
 */
export function createWalkthroughSteps(navigate: NavigateFunction): Step[] {
  return [
    // ── Step 1 — /chat empty state ────────────────────────────────────────
    {
      target: '[data-walkthrough="home-card"]',
      title: 'Start in chat',
      content:
        'Chat is your starting point. New windows open with the same greeting and quick actions you saw after setup.',
      placement: 'bottom',
      skipBeacon: true,
    },

    // ── Step 2 — /chat empty state ────────────────────────────────────────
    {
      target: '[data-walkthrough="home-cta"]',
      title: 'Say hello',
      content: 'Tap here to start a conversation with your AI assistant anytime.',
      placement: 'bottom',
      skipBeacon: true,
    },

    // ── Step 3 — /chat ────────────────────────────────────────────────────
    {
      target: '[data-walkthrough="chat-agent-panel"]',
      title: 'Meet your AI',
      content:
        'This is where conversations happen. Ask questions, get summaries, or brainstorm. Everything stays searchable.',
      placement: 'bottom',
      skipBeacon: true,
      before: async () => {
        navigate('/chat');
        await waitForTarget('chat-agent-panel');
      },
    },

    // ── Step 4 — /connections (Apps tab) ──────────────────────────────────
    {
      target: '[data-walkthrough="skills-grid"]',
      title: 'Connect your world',
      content:
        'Gmail, Slack, WhatsApp, and more — each connection gives your assistant superpowers.',
      placement: 'top',
      skipBeacon: true,
      before: async () => {
        navigate('/connections');
        await waitForTarget('skills-grid');
      },
    },

    // ── Step 5 — /connections (Messaging tab) ────────────────────────────
    {
      target: '[data-walkthrough="skills-channels"]',
      title: 'Chat where you already are',
      content:
        'WhatsApp, Telegram, Slack, Discord — connect your messaging apps so your assistant can reach you anywhere.',
      placement: 'bottom',
      skipBeacon: true,
      before: async () => {
        await waitForTarget('skills-channels');
      },
    },

    // ── Step 6 — /settings ────────────────────────────────────────────────
    {
      target: '[data-walkthrough="settings-menu"]',
      title: 'Make it yours',
      content:
        'Preferences, privacy, notifications — everything is here. You can restart this tour anytime from this page.',
      placement: 'top',
      skipBeacon: true,
      before: async () => {
        navigate('/settings');
        await waitForTarget('settings-menu');
      },
    },

    // ── Step 7 — primary nav: Chat ────────────────────────────────────────
    {
      target: '[data-walkthrough="tab-chat"]',
      title: 'Jump back to chat',
      content: 'Use the Chat tab whenever you want to return to conversations.',
      placement: 'top',
      skipBeacon: true,
      before: async () => {
        await waitForTarget('tab-chat');
      },
    },

    // ── Step 8 — primary nav: Human ───────────────────────────────────────
    {
      target: '[data-walkthrough="tab-human"]',
      title: 'Meet your human profile',
      content:
        'Human is where your personal context, identity, and assistant-facing profile come together.',
      placement: 'top',
      skipBeacon: true,
    },

    // ── Step 9 — primary nav: Brain ───────────────────────────────────────
    {
      target: '[data-walkthrough="tab-brain"]',
      title: 'Open your Brain',
      content:
        'Brain is the memory graph: the place to inspect what OpenHuman knows and how ideas connect.',
      placement: 'top',
      skipBeacon: true,
    },

    // ── Step 10 — primary nav: Agent World ────────────────────────────────
    {
      target: '[data-walkthrough="tab-agent-world"]',
      title: 'Explore Agent World',
      content: 'Agent World is where reusable agents and shared automations live.',
      placement: 'top',
      skipBeacon: true,
    },

    // ── Step 11 — primary nav: Connections ────────────────────────────────
    {
      target: '[data-walkthrough="tab-connections"]',
      title: 'Manage connections',
      content:
        'Connections is always available from the main nav when you want to add or adjust services.',
      placement: 'top',
      skipBeacon: true,
    },

    // ── Step 12 — primary nav: Feedback ───────────────────────────────────
    {
      target: '[data-walkthrough="tab-feedback"]',
      title: 'Send feedback',
      content: 'Feedback gives you a direct place to report rough edges or ask for improvements.',
      placement: 'top',
      skipBeacon: true,
    },

    // ── Step 13 — /chat (pre-seeded welcome message) ──────────────────────
    {
      target: '[data-walkthrough="chat-agent-panel"]',
      title: "You're all set!",
      content:
        'Your assistant left you a welcome note — this is your space to chat, ask questions, or brainstorm. Have fun!',
      placement: 'bottom',
      skipBeacon: true,
      before: async () => {
        try {
          const thread = await store.dispatch(createNewThread()).unwrap();
          const welcomeMessage: ThreadMessage = {
            id: `msg_${crypto.randomUUID()}`,
            content: TOUR_WELCOME_MESSAGE,
            type: 'text',
            sender: 'agent',
            createdAt: new Date().toISOString(),
            extraMetadata: {},
          };
          await store
            .dispatch(addMessageLocal({ threadId: thread.id, message: welcomeMessage }))
            .unwrap();
          store.dispatch(setSelectedThread(thread.id));
          navigate('/chat');
        } catch (err) {
          console.debug('[walkthrough] step-9 before hook failed, falling back to /chat', err);
          navigate('/chat');
        }
        await waitForTarget('chat-agent-panel');
      },
    },
  ];
}
