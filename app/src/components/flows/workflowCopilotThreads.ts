/**
 * Session-lived cache of each workflow's copilot chat thread id, keyed by flow
 * (a persisted flow id, or `'draft'` for an unsaved draft). The copilot panel
 * unmounts when closed and `FlowEditor` remounts when switching flows, so
 * without this the `workflow_builder` thread — and its transcript — would be
 * lost on every open/close. Persisting the thread id lets the panel reseed the
 * same thread (its messages live in the Redux `messagesByThreadId` store), so
 * reopening the copilot restores the conversation for that workflow.
 *
 * Module-level (session) scope is deliberate: it survives component remounts
 * without coupling to Redux, and a fresh session legitimately starts a new
 * authoring conversation.
 */
const copilotThreadByFlow = new Map<string, string>();

/** Cache key for a flow: its persisted id, or `'draft'` for an unsaved draft. */
export function copilotThreadKey(flowId: string | null): string {
  return flowId ?? 'draft';
}

export function getCopilotThreadId(flowId: string | null): string | null {
  return copilotThreadByFlow.get(copilotThreadKey(flowId)) ?? null;
}

export function setCopilotThreadId(flowId: string | null, threadId: string | null): void {
  const key = copilotThreadKey(flowId);
  if (threadId) copilotThreadByFlow.set(key, threadId);
  else copilotThreadByFlow.delete(key);
}
