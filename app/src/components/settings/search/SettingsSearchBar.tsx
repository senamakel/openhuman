// ---------------------------------------------------------------------------
// SettingsSearchBar
//
// A plain, full-width search field for the settings sidebar. It is purely a
// controlled text input — it does NOT render its own result list. The parent
// (SettingsSidebar) uses the query to filter the visible nav tabs in place.
// ---------------------------------------------------------------------------
import { useRef } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import Button from '../../ui/Button';
import { CloseIcon } from '../../ui/icons';
import TextField from '../../ui/TextField';

interface SettingsSearchBarProps {
  value: string;
  onValueChange: (next: string) => void;
}

const SearchIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={2}
      d="M21 21l-4.35-4.35M11 19a8 8 0 100-16 8 8 0 000 16z"
    />
  </svg>
);

const SettingsSearchBar = ({ value, onValueChange }: SettingsSearchBarProps) => {
  const { t } = useT();
  const inputRef = useRef<HTMLInputElement | null>(null);

  return (
    <div data-testid="settings-search" className="relative shrink-0">
      <span className="pointer-events-none absolute inset-y-0 left-3 flex items-center text-content-faint">
        <SearchIcon />
      </span>
      <TextField
        ref={inputRef}
        type="text"
        aria-label={t('settings.settingsSearch.ariaLabel')}
        autoComplete="off"
        spellCheck={false}
        value={value}
        onChange={event => onValueChange(event.target.value)}
        onKeyDown={event => {
          if (event.key === 'Escape' && value) {
            event.preventDefault();
            onValueChange('');
          }
        }}
        placeholder={t('settings.settingsSearch.placeholder')}
        data-testid="settings-search-input"
        className="rounded-none border-0 border-b border-line pl-10 pr-10 focus:ring-0"
      />
      {value && (
        <Button
          type="button"
          variant="tertiary"
          size="sm"
          iconOnly
          onClick={() => {
            onValueChange('');
            inputRef.current?.focus();
          }}
          aria-label={t('settings.settingsSearch.clear')}
          data-testid="settings-search-clear"
          className="absolute inset-y-0 right-2 my-auto h-7 w-7 text-content-faint hover:text-content-secondary hover:bg-transparent focus-visible:ring-offset-surface">
          <CloseIcon className="h-4 w-4" />
        </Button>
      )}
    </div>
  );
};

export default SettingsSearchBar;
