import { useCoreState } from '../providers/CoreStateProvider';

/**
 * Hook to access the current core-owned user snapshot.
 */
export const useUser = () => {
  const { snapshot, refresh } = useCoreState();

  return {
    user: snapshot.currentUser,
    isLoading: !snapshot.auth.isAuthenticated && !snapshot.sessionToken ? false : false,
    error: null,
    refetch: refresh,
  };
};
