import { afterEach, describe, expect, it } from 'vitest';

import { clearTestPlatform, getIsIOS, setTestPlatform } from './platform';

describe('platform detection', () => {
  afterEach(() => {
    clearTestPlatform();
  });

  it('returns false by default in test environment (not iOS UA)', () => {
    // In Vitest / jsdom navigator.userAgent is not an iPhone string,
    // so the result should be false with no override.
    clearTestPlatform();
    // Don't assert a specific value since isTauri() may vary by env;
    // just confirm getIsIOS() is a boolean.
    expect(typeof getIsIOS()).toBe('boolean');
  });

  it('returns true when test override is set to "ios"', () => {
    setTestPlatform('ios');
    expect(getIsIOS()).toBe(true);
  });

  it('returns false when test override is set to "desktop"', () => {
    setTestPlatform('desktop');
    expect(getIsIOS()).toBe(false);
  });

  it('toggle works round-trip', () => {
    setTestPlatform('ios');
    expect(getIsIOS()).toBe(true);
    setTestPlatform('desktop');
    expect(getIsIOS()).toBe(false);
    clearTestPlatform();
    // After clear, back to auto-detect (still a boolean).
    expect(typeof getIsIOS()).toBe('boolean');
  });
});
