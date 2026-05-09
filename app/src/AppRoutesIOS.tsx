/**
 * AppRoutesIOS — routes for the iOS app target.
 *
 * Two routes only:
 *   /pair    — QR scan login (public, always accessible)
 *   /mascot  — Full-screen mascot chat (requires a saved profile)
 *
 * All other desktop routes are not rendered on iOS. Any unmatched path
 * redirects to /pair or /mascot based on whether a profile is saved.
 */
import debug from 'debug';
import { type FC } from 'react';
import { Navigate, Route, Routes } from 'react-router-dom';

import { MascotScreen } from './pages/ios/MascotScreen';
import { PairScreen } from './pages/ios/PairScreen';
import { listProfiles } from './services/transport/profileStore';

const log = debug('ios:routes');

/** Redirect root / based on whether a profile is already saved. */
const IOSDefaultRedirect: FC = () => {
  const profiles = listProfiles();
  const hasPairing = profiles.length > 0;
  log('[ios] default redirect hasPairing=%s', hasPairing);
  return <Navigate to={hasPairing ? '/mascot' : '/pair'} replace />;
};

const AppRoutesIOS: FC = () => {
  return (
    <Routes>
      <Route path="/pair" element={<PairScreen />} />
      <Route path="/mascot" element={<MascotScreen />} />
      {/* Any unknown route goes to /pair or /mascot based on auth state */}
      <Route path="*" element={<IOSDefaultRedirect />} />
    </Routes>
  );
};

export default AppRoutesIOS;
