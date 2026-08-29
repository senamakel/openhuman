import { type ReactNode } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import Button from './Button';
import { ModalShell } from './ModalShell';

export interface ConfirmDialogProps {
  title: ReactNode;
  body: ReactNode;
  titleId?: string;
  confirmLabel?: ReactNode;
  cancelLabel?: ReactNode;
  busy?: boolean;
  busyLabel?: ReactNode;
  confirmDisabled?: boolean;
  destructive?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * The three labels default through `useT()` rather than to English literals.
 *
 * They used to be hardcoded English defaults in the signature, which slipped
 * past `i18n:react:check` — that audit walks JSX for `aria-label`/`placeholder`
 * /`title`/`alt`/`label`, and a default parameter value is none of those. So
 * every confirm dialog that did not pass explicit labels rendered "Confirm" /
 * "Cancel" / "Working…" in English regardless of the user's locale.
 */
export function ConfirmDialog({
  title,
  body,
  titleId = 'confirm-dialog-title',
  confirmLabel,
  cancelLabel,
  busy = false,
  busyLabel,
  confirmDisabled = false,
  destructive = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const { t } = useT();
  const resolvedConfirmLabel = confirmLabel ?? t('common.confirm');
  const resolvedCancelLabel = cancelLabel ?? t('common.cancel');
  const resolvedBusyLabel = busyLabel ?? t('common.working');

  return (
    <ModalShell
      title={title}
      titleId={titleId}
      onClose={onCancel}
      maxWidthClassName="max-w-sm"
      closePolicy={busy ? { escape: false, backdrop: false, button: false } : undefined}
      footer={
        <div className="flex justify-end gap-2">
          <Button variant="secondary" size="sm" onClick={onCancel} disabled={busy}>
            {resolvedCancelLabel}
          </Button>
          <Button
            variant="primary"
            size="sm"
            tone={destructive ? 'danger' : undefined}
            data-testid="confirm-dialog-confirm"
            onClick={onConfirm}
            disabled={busy || confirmDisabled}>
            {busy ? resolvedBusyLabel : resolvedConfirmLabel}
          </Button>
        </div>
      }>
      <div className="text-sm text-content-secondary">{body}</div>
    </ModalShell>
  );
}
