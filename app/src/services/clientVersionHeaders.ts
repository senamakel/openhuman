import { getVersion } from '@tauri-apps/api/app';

import { APP_VERSION } from '../utils/config';
import { isTauri } from '../utils/tauriCommands/common';

const CLIENT_VERSION_MAX_LENGTH = 64;

let tauriVersionPromise: Promise<string | null> | null = null;

export function sanitizeClientVersion(raw: string | null | undefined): string | null {
  const sanitized = String(raw ?? '')
    .trim()
    .replace(/[^0-9A-Za-z._+-]+/g, '')
    .slice(0, CLIENT_VERSION_MAX_LENGTH);

  return sanitized.length > 0 ? sanitized : null;
}

async function getTauriClientVersion(): Promise<string | null> {
  if (!isTauri()) {
    return null;
  }

  if (!tauriVersionPromise) {
    tauriVersionPromise = getVersion()
      .then(version => sanitizeClientVersion(version))
      .catch(() => {
        tauriVersionPromise = null;
        return null;
      });
  }

  return tauriVersionPromise;
}

export async function getClientVersionHeaders(): Promise<Record<string, string>> {
  if (isTauri()) {
    const tauriVersion = await getTauriClientVersion();
    return tauriVersion ? { 'x-tauri-version': tauriVersion } : {};
  }

  const webVersion = sanitizeClientVersion(APP_VERSION);
  return webVersion ? { 'x-web-version': webVersion } : {};
}
