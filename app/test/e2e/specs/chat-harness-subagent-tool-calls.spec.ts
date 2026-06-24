/**
 * Chat harness — sub-agent TOOL CALLS save + render.
 *
 * Extends the orchestrator → researcher flow (see `chat-harness-subagent.spec.ts`)
 * to cover the specific thing users reported missing: a sub-agent's own tool
 * calls reaching the UI. This is a true full-stack run — the mock only forces
 * the LLM responses; the real core runs the agent harness a level deeper, so the
 * `subagent_tool_call` / `subagent_tool_result` socket events are produced by
 * the genuine pipeline, not synthesised in the test.
 *
 * Forced responses (popped in order across parent + sub-agent LLM calls):
 *   A) orchestrator   → emits a `research` delegated-archetype tool call
 *   B) researcher     → emits a `web_search` tool call (the child tool call)
 *   C) researcher     → final text answer
 *   D) orchestrator   → final synthesis (canary)
 *
 * Asserts:
 *   SAVE   — Redux `toolTimelineByThread[thread]` has a `subagent:researcher`
 *            row whose `subagent.toolCalls` / `subagent.transcript` records the
 *            `web_search` child call.
 *   RENDER — the inline sub-agent row opens the side panel on click, and the
 *            panel shows the child tool call with its enriched "Searching: …"
 *            label.
 */
import { waitForApp } from '../helpers/app-helpers';
import {
  chatMounted,
  clickByTitle,
  clickSend,
  getSelectedThreadId,
  typeIntoComposer,
  waitForSocketConnected,
} from '../helpers/chat-harness';
import { clickTestId, textExists, waitForTestId } from '../helpers/element-helpers';
import { resetApp } from '../helpers/reset-app';
import { navigateViaHash } from '../helpers/shared-flows';
import { setMockBehavior, startMockServer, stopMockServer } from '../mock-server';

const USER_ID = 'e2e-chat-harness-subagent-tool-calls';
const PROMPT = 'Research the marker phrase using a web search.';
const CANARY_FINAL = 'subagent-tool-canary-final-3b91c';
const RESEARCHER_REPLY = 'The researcher finished searching.';
const SEARCH_QUERY = 'answer to life the universe and everything';

const FORCED_RESPONSES = [
  // 1. Orchestrator: delegate to the researcher.
  {
    content: '',
    toolCalls: [
      {
        id: 'call_research_1',
        name: 'research',
        arguments: JSON.stringify({ prompt: 'Search the web for a marker phrase' }),
      },
    ],
  },
  // 2. Researcher sub-agent: invoke a child tool (web_search).
  {
    content: '',
    toolCalls: [
      {
        id: 'call_web_search_1',
        name: 'web_search',
        arguments: JSON.stringify({ query: SEARCH_QUERY }),
      },
    ],
  },
  // 3. Researcher sub-agent: final text answer (after the tool result).
  { content: RESEARCHER_REPLY },
  // 4. Orchestrator final synthesis containing the canary.
  { content: `Done. The result is: ${CANARY_FINAL}` },
];

interface SubagentSnapshot {
  rowId?: string;
  taskId?: string;
  toolNames: string[];
  transcriptToolNames: string[];
}

/** Pull the first sub-agent row's recorded child tool calls from the store. */
async function snapshotSubagent(threadId: string): Promise<SubagentSnapshot> {
  return (await browser.execute((tid: string) => {
    const winAny = window as unknown as { __OPENHUMAN_STORE__?: { getState: () => unknown } };
    const state = winAny.__OPENHUMAN_STORE__?.getState() as
      | {
          chatRuntime?: {
            toolTimelineByThread?: Record<
              string,
              Array<{
                id?: string;
                subagent?: {
                  taskId?: string;
                  toolCalls?: Array<{ toolName?: string }>;
                  transcript?: Array<{ kind?: string; toolName?: string }>;
                };
              }>
            >;
          };
        }
      | undefined;
    const timeline = state?.chatRuntime?.toolTimelineByThread?.[tid] ?? [];
    const row = timeline.find(e => e?.subagent);
    return {
      rowId: row?.id,
      taskId: row?.subagent?.taskId,
      toolNames: (row?.subagent?.toolCalls ?? []).map(c => c?.toolName ?? ''),
      transcriptToolNames: (row?.subagent?.transcript ?? [])
        .filter(i => i?.kind === 'tool')
        .map(i => i?.toolName ?? ''),
    };
  }, threadId)) as SubagentSnapshot;
}

describe('Chat harness — sub-agent tool calls save + render', () => {
  before(async function beforeSuite() {
    this.timeout(90_000);
    await startMockServer();
    await waitForApp();
    await resetApp(USER_ID, { clearAuthSession: true });
    setMockBehavior('llmForcedResponses', JSON.stringify(FORCED_RESPONSES));
    setMockBehavior('llmStreamChunkDelayMs', '10');
  });

  after(async () => {
    setMockBehavior('llmForcedResponses', '');
    setMockBehavior('llmStreamChunkDelayMs', '');
    await stopMockServer();
  });

  it('records the child tool call in Redux and renders it in the side panel', async function () {
    this.timeout(90_000);
    await navigateViaHash('/chat');
    await browser.waitUntil(async () => await chatMounted(), {
      timeout: 15_000,
      timeoutMsg: 'Conversations did not mount',
    });
    expect(await clickByTitle('New thread', 8_000)).toBe(true);

    const threadId = (await browser.waitUntil(async () => await getSelectedThreadId(), {
      timeout: 8_000,
      timeoutMsg: 'thread.selectedThreadId never populated',
    })) as string;

    await typeIntoComposer(PROMPT);
    await waitForSocketConnected(30_000);
    expect(
      await browser.waitUntil(async () => await clickSend(), {
        timeout: 5_000,
        timeoutMsg: 'Send button never enabled',
      })
    ).toBe(true);

    // SAVE: wait until the sub-agent's web_search child call is recorded in the
    // store (via the real subagent_tool_call socket event), on the transcript
    // (what the drawer renders) and/or the flat toolCalls list.
    let snap: SubagentSnapshot = { toolNames: [], transcriptToolNames: [] };
    await browser.waitUntil(
      async () => {
        snap = await snapshotSubagent(threadId);
        return (
          snap.toolNames.includes('web_search') || snap.transcriptToolNames.includes('web_search')
        );
      },
      { timeout: 45_000, timeoutMsg: 'sub-agent web_search call never reached Redux' }
    );
    expect(
      snap.toolNames.includes('web_search') || snap.transcriptToolNames.includes('web_search')
    ).toBe(true);

    // Let the run settle so the final canary lands (and the timeline persists).
    await browser.waitUntil(async () => await textExists(CANARY_FINAL), {
      timeout: 30_000,
      timeoutMsg: 'orchestrator never produced the final canary text',
    });

    // RENDER: the inline sub-agent row opens the side panel on click.
    // (`waitForTestId` throws if the element never appears.)
    await waitForTestId('subagent-open-panel', 10_000);
    await clickTestId('subagent-open-panel');
    await waitForTestId('subagent-drawer', 10_000);

    // The panel shows the child tool call with its args-enriched label.
    await waitForTestId('subagent-drawer-tool-call', 10_000);
    await browser.waitUntil(async () => await textExists('Searching'), {
      timeout: 10_000,
      timeoutMsg: 'enriched "Searching: …" tool-call label never rendered in the panel',
    });
  });
});
