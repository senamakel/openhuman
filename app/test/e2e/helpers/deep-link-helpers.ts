/**
 * Deep-link trigger utilities for E2E tests.
 *
 * Uses macOS `open` command to fire the custom `openhuman://` URL scheme,
 * which the built .app bundle picks up via its registered CFBundleURLSchemes.
 */
import { exec } from 'child_process';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

function execCommand(command: string): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    exec(command, error => {
      if (error) reject(error);
      else resolve();
    });
  });
}

function resolveBuiltAppPath(): string | null {
  const helperDir = path.dirname(fileURLToPath(import.meta.url));
  const appDir = path.resolve(helperDir, '..', '..');
  const repoRoot = path.resolve(appDir, '..');
  const candidates = [
    path.join(appDir, 'src-tauri', 'target', 'debug', 'bundle', 'macos', 'OpenHuman.app'),
    path.join(repoRoot, 'target', 'debug', 'bundle', 'macos', 'OpenHuman.app'),
  ];

  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) return candidate;
  }

  return null;
}

/**
 * Trigger a deep link URL via the macOS `open` command.
 * Resolves once the OS has dispatched the URL (does NOT wait for the app to
 * finish handling it).
 *
 * @param {string} url
 * @returns {Promise<void>}
 */
export async function triggerDeepLink(url: string): Promise<void> {
  const appPath = resolveBuiltAppPath();

  // Primary path in E2E: ask Appium/mac2 to deep-link directly into this app.
  // This avoids relying on global OS URL-handler registration.
  if (typeof browser !== 'undefined') {
    try {
      await browser.execute('macos: deepLink', {
        url,
        bundleId: 'com.openhuman.app',
      } as Record<string, unknown>);
      return;
    } catch {
      // Fall through to OS-level dispatch.
    }
  }

  // Ensure the app receives a reopen event so hidden tray-mode windows are shown.
  if (appPath) {
    try {
      await execCommand(`open -a "${appPath}"`);
      await new Promise(resolve => setTimeout(resolve, 500));
    } catch {
      // Best effort; continue to URL dispatch.
    }
  }

  let openError: unknown = null;
  try {
    const command = appPath ? `open -a "${appPath}" "${url}"` : `open "${url}"`;
    await execCommand(command);
  } catch (err) {
    openError = err;
  }

  if (!openError) return;
  throw new Error(
    `Failed to trigger deep link: ${openError instanceof Error ? openError.message : openError}`
  );
}

/**
 * Convenience wrapper for auth deep links.
 *
 * @param {string} token - The login token to embed in the URL.
 * @returns {Promise<void>}
 */
export function triggerAuthDeepLink(token: string): Promise<void> {
  return triggerDeepLink(`openhuman://auth?token=${encodeURIComponent(token)}`);
}
