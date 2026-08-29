/**
 * Shared tiny.place orchestration API client.
 *
 * One client instance per app lifetime — the underlying calls go through
 * `callCoreRpc`, which manages its own connection lifecycle, so there is
 * nothing per-mount to own here.
 *
 * This singleton used to live in `agentworld/AgentWorldShell.tsx`. That surface
 * (the in-app Agent World section) was removed with #5424; the client itself
 * outlived it because the Brain orchestration sub-tab, the orchestration panels
 * and the pairing hook all still call tiny.place through it. It now sits next
 * to its factory so the surviving consumers do not depend on a deleted screen.
 */
import { createInvokeApiClient } from './invokeApiClient';

export const apiClient = createInvokeApiClient();
