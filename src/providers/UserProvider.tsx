import { useEffect } from "react";
import { useAppSelector, useAppDispatch } from "../store/hooks";
import { fetchCurrentUser } from "../store/userSlice";
import { fetchTeams } from "../store/teamSlice";
import { clearToken } from "../store/authSlice";

/**
 * UserProvider automatically fetches user data when JWT token is available.
 * On fetch failure (e.g. expired token), logs out the user.
 */
const UserProvider = ({ children }: { children: React.ReactNode }) => {
  const dispatch = useAppDispatch();
  const token = useAppSelector((state) => state.auth.token);

  useEffect(() => {
    if (!token) return;
    dispatch(fetchCurrentUser()).then((result) => {
      if (fetchCurrentUser.fulfilled.match(result)) {
        dispatch(fetchTeams());
      } else if (fetchCurrentUser.rejected.match(result)) {
        dispatch(clearToken());
      }
    });
  }, [token, dispatch]);

  return <>{children}</>;
};

export default UserProvider;
