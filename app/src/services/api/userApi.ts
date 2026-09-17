import type { User } from '../../types/api';
import { fetchCurrentUser } from '../session/sessionOwner';

/**
 * User API endpoints
 */
export const userApi = {
  /**
   * Get current authenticated user information: a live `GET /auth/me`
   * through the session owner (the Tauri shell on the desktop).
   */
  getMe: async (): Promise<User> => {
    const current = await fetchCurrentUser(true);
    if (!current.user) {
      throw new Error('REJECTED: no signed-in user');
    }
    return current.user as User;
  },
};
