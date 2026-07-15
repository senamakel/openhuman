import { describe, expect, it } from 'vitest';

import type { PrivacyMode } from '../../../store/privacySlice';
import { privacyModeLabelKey } from '../disclosureLabels';

describe('disclosureLabels', () => {
  it('maps every privacy mode to its label key', () => {
    const modes: PrivacyMode[] = ['local_only', 'standard', 'sensitive'];
    expect(modes.map(privacyModeLabelKey)).toEqual([
      'privacy.mode.localOnly',
      'privacy.mode.standard',
      'privacy.mode.sensitive',
    ]);
  });

  it('falls back to the standard mode key for an unexpected mode value', () => {
    // Guards the always-visible pill against `t(undefined)` if the core ever
    // emits a mode string the client does not yet know.
    expect(privacyModeLabelKey('some_future_mode')).toBe('privacy.mode.standard');
  });
});
