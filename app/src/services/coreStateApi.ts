import type { User } from '../types/api';
import type { TeamInvite, TeamMember, TeamWithRole } from '../types/team';
import { callCoreRpc } from './coreRpcClient';

interface AppStateSnapshotResult {
  auth: {
    isAuthenticated: boolean;
    userId: string | null;
    user: unknown | null;
    profileId: string | null;
  };
  sessionToken: string | null;
  currentUser: User | null;
  onboardingCompleted: boolean;
  analyticsEnabled: boolean;
  localState: {
    encryptionKey?: string | null;
    primaryWalletAddress?: string | null;
    onboardingTasks?: {
      accessibilityPermissionGranted: boolean;
      localModelConsentGiven: boolean;
      localModelDownloadStarted: boolean;
      enabledTools: string[];
      connectedSources: string[];
      updatedAtMs?: number;
    } | null;
  };
}

export async function fetchCoreAppSnapshot(): Promise<AppStateSnapshotResult> {
  const response = await callCoreRpc<{ result: AppStateSnapshotResult }>({
    method: 'openhuman.app_state_snapshot',
  });
  return response.result;
}

export async function updateCoreLocalState(params: {
  encryptionKey?: string | null;
  primaryWalletAddress?: string | null;
  onboardingTasks?: {
    accessibilityPermissionGranted: boolean;
    localModelConsentGiven: boolean;
    localModelDownloadStarted: boolean;
    enabledTools: string[];
    connectedSources: string[];
    updatedAtMs?: number;
  } | null;
}): Promise<void> {
  await callCoreRpc({
    method: 'openhuman.app_state_update_local_state',
    params,
  });
}

export async function listTeams(): Promise<TeamWithRole[]> {
  const response = await callCoreRpc<{ result: TeamWithRole[] }>({ method: 'openhuman.team_list_teams' });
  return response.result;
}

export async function getTeamMembers(teamId: string): Promise<TeamMember[]> {
  const response = await callCoreRpc<{ result: TeamMember[] }>({
    method: 'openhuman.team_list_members',
    params: { teamId },
  });
  return response.result;
}

export async function getTeamInvites(teamId: string): Promise<TeamInvite[]> {
  const response = await callCoreRpc<{ result: TeamInvite[] }>({
    method: 'openhuman.team_list_invites',
    params: { teamId },
  });
  return response.result;
}
