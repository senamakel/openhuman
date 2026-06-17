import { useLocation, useNavigate } from 'react-router-dom';

import { useT } from '../../../lib/i18n/I18nContext';
import type { Locale } from '../../../lib/i18n/types';
import { setActiveAccount } from '../../../store/accountsSlice';
import { useAppDispatch, useAppSelector } from '../../../store/hooks';
import { setLocale } from '../../../store/localeSlice';
import { createNewThread, loadThreadMessages, setSelectedThread } from '../../../store/threadSlice';
import { AGENT_ACCOUNT_ID } from '../../../utils/accountsFullscreen';
import { LOCALE_OPTIONS } from '../../LanguageSelect';
import { useRootSidebar } from './RootShellLayout';

const ICON_BTN =
  'flex h-7 w-7 flex-none items-center justify-center rounded-md text-stone-500 transition-colors hover:bg-stone-100 hover:text-stone-700 dark:text-neutral-400 dark:hover:bg-neutral-800/60 dark:hover:text-neutral-200';

/**
 * Thin utility header at the top of the root sidebar: collapse the sidebar,
 * jump to Settings, and switch language. The language control is a globe icon
 * with a transparent native <select> overlaid on top, so clicking the icon
 * opens the OS locale picker (reusing the shared LOCALE_OPTIONS + setLocale).
 */
export default function SidebarHeader() {
  const { t } = useT();
  const navigate = useNavigate();
  const location = useLocation();
  const dispatch = useAppDispatch();
  const { hide } = useRootSidebar();
  const locale = useAppSelector(state => state.locale.current);
  const threads = useAppSelector(state => state.thread.threads);

  // Home → the unified chat on a blank thread. When we're NOT already on chat,
  // just navigate and let the mounting Conversations page own blank-thread
  // landing (avoids a duplicate-create race). When already on chat (no remount),
  // reset to a blank thread here: reuse an existing empty one, else create.
  const handleHome = () => {
    // Switch back to the agent account first — otherwise a selected connected
    // app (WhatsApp/Slack/…) keeps Accounts rendering its webview instead of the
    // blank agent thread.
    dispatch(setActiveAccount(AGENT_ACCOUNT_ID));
    const onChat = location.pathname === '/chat' || location.pathname.startsWith('/chat/');
    if (!onChat) {
      navigate('/chat');
      return;
    }
    const empty = threads.find(thr => (thr.messageCount ?? 0) === 0);
    if (empty) {
      dispatch(setSelectedThread(empty.id));
      void dispatch(loadThreadMessages(empty.id));
      return;
    }
    void dispatch(createNewThread())
      .unwrap()
      .then(thr => {
        dispatch(setSelectedThread(thr.id));
        void dispatch(loadThreadMessages(thr.id));
      })
      .catch(() => {});
  };

  return (
    <div className="flex items-center justify-between gap-1 px-2 py-1.5">
      <button
        type="button"
        onClick={handleHome}
        className={ICON_BTN}
        data-analytics-id="sidebar-header-home"
        aria-label={t('nav.home')}
        title={t('nav.home')}>
        <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={1.8}
            d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-4 0a2 2 0 01-2-2v-4a2 2 0 012-2h2a2 2 0 012 2v4a2 2 0 01-2 2h-2z"
          />
        </svg>
      </button>

      <div className="flex items-center gap-0.5">
        {/* Language — globe icon with a transparent select overlay. `overflow-hidden`
          clips the native <select> to the icon box: a select won't shrink below
          its longest option's width, so without clipping it spills to the right
          and steals clicks from the Settings / collapse buttons. */}
        <label className={`relative overflow-hidden ${ICON_BTN}`} title={t('settings.language')}>
          <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={1.8}
              d="M21 12a9 9 0 11-18 0 9 9 0 0118 0zM3.6 9h16.8M3.6 15h16.8M12 3a15 15 0 010 18 15 15 0 010-18z"
            />
          </svg>
          <select
            value={locale}
            onChange={e => dispatch(setLocale(e.target.value as Locale))}
            aria-label={t('settings.language')}
            data-analytics-id="sidebar-header-language"
            className="absolute inset-0 h-full w-full cursor-pointer opacity-0">
            {LOCALE_OPTIONS.map(opt => (
              <option key={opt.value} value={opt.value}>
                {opt.flag} {opt.label}
              </option>
            ))}
          </select>
        </label>

        <button
          type="button"
          onClick={() => navigate('/settings')}
          className={ICON_BTN}
          data-analytics-id="sidebar-header-settings"
          aria-label={t('nav.settings')}
          title={t('nav.settings')}>
          <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={1.8}
              d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"
            />
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={1.8}
              d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
            />
          </svg>
        </button>

        {/* Collapse the sidebar — sits on the right, next to Settings. */}
        <button
          type="button"
          onClick={hide}
          className={ICON_BTN}
          data-analytics-id="sidebar-header-collapse"
          aria-label={t('chat.hideSidebar')}
          title={t('chat.hideSidebar')}>
          <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={1.8}
              d="M15 19l-7-7 7-7M20 5v14"
            />
          </svg>
        </button>
      </div>
    </div>
  );
}
