import { useEffect } from 'react';
import { useLocation, useParams } from 'react-router-dom';

import { handleDeepLinkUrls, registerAuthDeepLinkState } from '../utils/desktopDeepLinkListener';

function buildSyntheticDeepLink(
  kind: string | undefined,
  status: string | undefined,
  search: string
): string | null {
  if (kind === 'auth') {
    return `openhuman://auth${search}`;
  }

  if (kind === 'oauth' && status) {
    return `openhuman://oauth/${status}${search}`;
  }

  return null;
}

export default function WebCallbackPage() {
  const { kind, status } = useParams();
  const location = useLocation();

  useEffect(() => {
    let search = location.search;

    // Web build: the OAuth button stashed the in-app `state` nonce in
    // sessionStorage before the full-page navigation (module memory is lost on
    // reload). That app-written nonce is the CSRF proof for this same-origin
    // callback (an attacker cannot write the victim's sessionStorage), so we
    // accept it whether or not the backend echoed `state` back — but we reject
    // a backend-echoed `state` that does NOT match it (a forged callback). When
    // the backend did not echo `state`, inject the stored nonce into the
    // synthetic deep link so `handleAuthDeepLink`'s guard (finding C3) matches.
    if (kind === 'auth') {
      try {
        const params = new URLSearchParams(location.search);
        const echoed = params.get('state');
        const stored = window.sessionStorage.getItem('openhuman:auth-deep-link-state');
        if (stored) {
          window.sessionStorage.removeItem('openhuman:auth-deep-link-state');
        }
        if (stored && (!echoed || echoed === stored)) {
          registerAuthDeepLinkState(stored);
          if (!echoed) {
            params.set('state', stored);
            search = `?${params.toString()}`;
          }
        }
      } catch {
        // sessionStorage unavailable — handleDeepLinkUrls will reject below.
      }
    }

    const synthetic = buildSyntheticDeepLink(kind, status, search);
    if (!synthetic) return;

    void handleDeepLinkUrls([synthetic]);
  }, [kind, status, location.search]);

  return (
    <div className="flex min-h-[60vh] items-center justify-center px-6 text-center">
      <div className="max-w-md space-y-3">
        <h1 className="text-2xl font-semibold text-slate-900">Completing sign-in</h1>
        <p className="text-sm text-slate-600">
          OpenHuman is processing your callback and will continue automatically.
        </p>
      </div>
    </div>
  );
}
