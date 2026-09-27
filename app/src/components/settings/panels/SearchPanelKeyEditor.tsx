import { ExternalLink, Eye, EyeOff } from 'lucide-react';
import { useId } from 'react';

import Badge from '../../ui/Badge';
import Button from '../../ui/Button';
import { InputGroupButton, InputGroupInput, InputGroupRoot } from '../../ui/InputGroup';

export interface KeyEditorProps {
  label: string;
  placeholder: string;
  show: boolean;
  onToggleShow: () => void;
  value: string;
  onChange: (v: string) => void;
  onSave: () => void;
  onClear: () => void;
  configured: boolean;
  /** Where to get a key; the link is hidden when the provider has none. */
  docUrl?: string | null;
  disabled?: boolean;
  testId?: string;
  t: (key: string) => string;
}

/**
 * One API-key row inside a provider row: the key label, a "Stored" badge and
 * the doc link on the left; a maskable input with a show/hide toggle, Save
 * and (when stored) Clear on the right.
 */
const KeyEditor = ({
  label,
  placeholder,
  show,
  onToggleShow,
  value,
  onChange,
  onSave,
  onClear,
  configured,
  docUrl,
  disabled = false,
  testId,
  t,
}: KeyEditorProps) => {
  const inputId = useId();

  return (
    <div
      role="group"
      aria-labelledby={inputId}
      data-testid={testId}
      className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
      <div className="min-w-0 space-y-0.5">
        <div className="flex items-center gap-2">
          <label id={inputId} htmlFor={`${inputId}-input`} className="text-sm text-content">
            {label}
          </label>
          {configured && <Badge variant="success">{t('settings.search.keyStored')}</Badge>}
        </div>
        {docUrl && (
          <a
            href={docUrl}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-1 text-xs text-primary-600 hover:underline dark:text-primary-400">
            {t('settings.search.getApiKey')}
            <ExternalLink className="h-3 w-3" aria-hidden />
          </a>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <InputGroupRoot size="sm" className="w-full md:w-72">
          <InputGroupInput
            id={`${inputId}-input`}
            type={show ? 'text' : 'password'}
            autoComplete="off"
            mono
            value={value}
            onChange={e => onChange(e.target.value)}
            placeholder={placeholder}
          />
          <InputGroupButton
            type="button"
            variant="secondary"
            onClick={onToggleShow}
            leadingIcon={
              show ? (
                <EyeOff className="h-3.5 w-3.5" aria-hidden />
              ) : (
                <Eye className="h-3.5 w-3.5" aria-hidden />
              )
            }>
            {show ? t('settings.search.hide') : t('settings.search.show')}
          </InputGroupButton>
        </InputGroupRoot>
        <Button
          type="button"
          variant="primary"
          size="sm"
          onClick={onSave}
          disabled={disabled || value.trim().length === 0}>
          {t('settings.search.save')}
        </Button>
        {configured && (
          <Button
            type="button"
            variant="secondary"
            tone="danger"
            size="sm"
            onClick={onClear}
            disabled={disabled}>
            {t('settings.search.clear')}
          </Button>
        )}
      </div>
    </div>
  );
};

export default KeyEditor;
