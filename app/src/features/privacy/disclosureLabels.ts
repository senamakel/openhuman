import type { PrivacyMode } from '../../store/privacySlice';

/**
 * Map the core's snake_case Privacy Mode to a friendly, human-readable i18n key
 * for the status pill. Tolerates unknown/future values by falling back to the
 * neutral "Standard" label.
 */
export function privacyModeLabelKey(mode: PrivacyMode | string): string {
  switch (mode) {
    case 'local_only':
      return 'privacy.mode.localOnly';
    case 'standard':
      return 'privacy.mode.standard';
    case 'sensitive':
      return 'privacy.mode.sensitive';
    // The pill is always-visible, so an unexpected/future mode string must
    // still resolve to a real i18n key — never `undefined`, which `t()` would
    // choke on. Fall back to the neutral "Standard" label.
    default:
      return 'privacy.mode.standard';
  }
}
