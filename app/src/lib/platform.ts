/**
 * Platform detection utilities.
 *
 * Uses navigator.userAgent for iOS detection plus isTauri() from
 * webviewAccountService to confirm we're inside the Tauri runtime.
 *
 * For tests: override via setTestPlatform() / clearTestPlatform().
 * Production code must not call the override functions.
 */
import { isTauri } from '../services/webviewAccountService';

// -- test override -----------------------------------------------------------

let _testOverride: 'ios' | 'desktop' | null = null;

/**
 * Override the detected platform in tests.
 * Call clearTestPlatform() in afterEach to restore.
 */
export function setTestPlatform(platform: 'ios' | 'desktop'): void {
  _testOverride = platform;
}

/** Restore automatic detection (call in afterEach). */
export function clearTestPlatform(): void {
  _testOverride = null;
}

// -- detection ---------------------------------------------------------------

function detectIOS(): boolean {
  if (_testOverride === 'ios') return true;
  if (_testOverride === 'desktop') return false;

  if (typeof navigator === 'undefined') return false;

  const ua = navigator.userAgent;
  const isMobileUA = /iPhone|iPad|iPod/i.test(ua);
  // Only treat as iOS when we're actually inside the Tauri runtime.
  // A web browser on an iPhone should not trigger iOS-specific Tauri flows.
  return isMobileUA && isTauri();
}

/**
 * True when the app is running on iOS (inside the Tauri iOS target).
 *
 * Evaluated lazily on first access and then cached for the lifetime of the
 * module — the platform never changes at runtime.
 */
let _isIOSCache: boolean | null = null;

export function getIsIOS(): boolean {
  if (_testOverride !== null) {
    // Always re-evaluate when a test override is active.
    return detectIOS();
  }
  if (_isIOSCache === null) {
    _isIOSCache = detectIOS();
  }
  return _isIOSCache;
}

/**
 * Convenience re-export as a constant.
 * Safe to import and use at module level — evaluated once on import.
 *
 * NOTE: if you need test overrides to work, call getIsIOS() instead,
 * since this is evaluated at module load time.
 */
export const isIOS: boolean = detectIOS();
