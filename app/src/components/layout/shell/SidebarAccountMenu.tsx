import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { AVATAR_MENU_ITEMS } from '../../../config/navConfig';
import { useT } from '../../../lib/i18n/I18nContext';
import { useCoreState } from '../../../providers/CoreStateProvider';
import { trackEvent } from '../../../services/analytics';
import { BILLING_DASHBOARD_URL } from '../../../utils/links';
import { isLocalSessionToken } from '../../../utils/localSession';
import { openUrl } from '../../../utils/openUrl';
import { resolveUserName } from '../../../utils/userName';

const getInitials = (name: string): string => {
  const words = name.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return 'OH';
  return words
    .slice(0, 2)
    .map(word => word[0]?.toUpperCase() ?? '')
    .join('');
};

/**
 * Account / avatar control pinned to the bottom of the root-shell sidebar.
 * Relocated from the former bottom tab bar: shows the signed-in user's initials
 * and opens an upward popover with Account / Billing / Rewards / Invites /
 * Wallet (cloud-only entries hidden for local sessions).
 */
export default function SidebarAccountMenu() {
  const { t } = useT();
  const navigate = useNavigate();
  const { snapshot } = useCoreState();
  const [open, setOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  const isLocalSession = isLocalSessionToken(snapshot.sessionToken);
  const userName = resolveUserName(snapshot.currentUser);
  const userInitials = getInitials(userName);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (menuRef.current?.contains(event.target as Node)) return;
      setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [open]);

  const handleItemClick = (itemId: string, kind: string, target: string) => {
    setOpen(false);
    if (kind === 'openUrl') {
      openUrl(target).catch(() => {});
    } else {
      navigate(target);
    }
    trackEvent('avatar_menu_item_click', { item_id: itemId });
  };

  return (
    <div
      ref={menuRef}
      className="relative border-t border-stone-200 p-1.5 dark:border-neutral-800">
      <button
        type="button"
        onClick={() => setOpen(o => !o)}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={t('nav.avatarMenu.account')}
        title={userName}
        className={`flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-[13px] transition-colors cursor-pointer ${
          open
            ? 'bg-white text-stone-900 shadow-sm dark:bg-neutral-800 dark:text-neutral-100'
            : 'text-stone-600 hover:bg-stone-200/70 hover:text-stone-800 dark:text-neutral-300 dark:hover:bg-neutral-800/60 dark:hover:text-neutral-100'
        }`}>
        <span className="flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-full bg-primary-500 text-[10px] font-semibold leading-none text-white">
          {userInitials}
        </span>
        <span className="min-w-0 flex-1 truncate text-left">{userName}</span>
      </button>

      {open && (
        <div
          role="menu"
          aria-label={t('nav.avatarMenu.account')}
          className="absolute bottom-full left-2 right-2 mb-2 overflow-hidden rounded-lg border border-stone-300 bg-white shadow-soft dark:border-neutral-700 dark:bg-neutral-900">
          <div className="p-1">
            {AVATAR_MENU_ITEMS.filter(item => !item.cloudOnly || !isLocalSession).map(item => {
              const target = item.id === 'billing' ? BILLING_DASHBOARD_URL : item.target;
              return (
                <button
                  key={item.id}
                  type="button"
                  role="menuitem"
                  onClick={() => handleItemClick(item.id, item.kind, target)}
                  className="flex w-full items-center rounded-md px-2 py-2 text-left text-sm text-stone-700 transition-colors hover:bg-stone-100 dark:text-neutral-200 dark:hover:bg-neutral-800">
                  {t(item.labelKey)}
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
