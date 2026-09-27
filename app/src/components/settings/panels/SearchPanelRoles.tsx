import { ChevronDownIcon, ChevronUpIcon, PlusIcon, XIcon } from 'lucide-react';

import type {
  SearchProviderInfo,
  SearchRole,
  SearchSettings,
  SearchSettingsUpdate,
} from '../../../utils/tauriCommands/config';
import Badge from '../../ui/Badge';
import Button from '../../ui/Button';
import Card from '../../ui/Card';

type Translate = (key: string) => string;

/** The roles the core exposes, in display order. */
export const SEARCH_ROLES: readonly SearchRole[] = ['search', 'answer', 'contents'];

const withProvider = (text: string, provider: string) => text.replace('{provider}', provider);

function roleTitle(role: SearchRole, t: Translate): string {
  if (role === 'answer') return t('settings.search.roleAnswer');
  if (role === 'contents') return t('settings.search.roleContents');
  return t('settings.search.roleSearch');
}

function roleDescription(role: SearchRole, t: Translate): string {
  if (role === 'answer') return t('settings.search.roleAnswerDesc');
  if (role === 'contents') return t('settings.search.roleContentsDesc');
  return t('settings.search.roleSearchDesc');
}

/** Move `order[index]` by `delta` places, returning a new array. */
export function moveProvider(order: string[], index: number, delta: -1 | 1): string[] {
  const target = index + delta;
  if (target < 0 || target >= order.length) return order;
  const next = [...order];
  [next[index], next[target]] = [next[target], next[index]];
  return next;
}

interface RowProps {
  role: SearchRole;
  settings: SearchSettings;
  saving: boolean;
  persist: (update: SearchSettingsUpdate) => Promise<boolean>;
  t: Translate;
}

/**
 * One role: its explanation, which provider serves it now, and the ordered
 * provider list the user can reorder, trim and reset. The list is built from
 * `settings.roles[role]`; providers that can serve the role but are not in the
 * list are offered as "add" buttons.
 */
const SearchRoleRow = ({ role, settings, saving, persist, t }: RowProps) => {
  const byId = new Map<string, SearchProviderInfo>(settings.providers.map(p => [p.id, p]));
  const order = (settings.roles[role] ?? []).filter(id => byId.get(id)?.roles.includes(role));
  const effective = settings.effective_roles[role] ?? [];
  const serving = effective[0] ? byId.get(effective[0]) : undefined;
  const addable = settings.providers.filter(p => p.roles.includes(role) && !order.includes(p.id));
  const testId = `search-role-${role}`;

  const saveOrder = (next: string[]) => void persist({ roles: { [role]: next } });

  return (
    <div data-testid={testId} className="space-y-2 px-4 py-3">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <p className="text-sm font-medium text-content">{roleTitle(role, t)}</p>
          <p className="text-xs text-content-muted">{roleDescription(role, t)}</p>
        </div>
        <Button
          type="button"
          variant="tertiary"
          size="xs"
          data-testid={`${testId}-reset`}
          disabled={saving}
          onClick={() => saveOrder([])}>
          {t('settings.search.roleReset')}
        </Button>
      </div>

      <p
        data-testid={`${testId}-serving`}
        className={
          serving ? 'text-xs text-content-secondary' : 'text-xs text-amber-700 dark:text-amber-300'
        }>
        {serving
          ? withProvider(t('settings.search.roleServedBy'), serving.label)
          : t('settings.search.roleNoProvider')}
      </p>

      <ol className="divide-y divide-line-subtle rounded-lg border border-line-subtle bg-surface-subtle">
        {order.map((id, index) => {
          const provider = byId.get(id);
          if (!provider) return null;
          const usable = effective.includes(id);
          return (
            <li
              key={id}
              data-testid={`${testId}-provider-${id}`}
              data-serving={serving?.id === id ? 'true' : undefined}
              className="flex items-center gap-2 px-3 py-1.5">
              <span className="w-4 text-xs tabular-nums text-content-muted">{index + 1}</span>
              <span className="min-w-0 flex-1 truncate text-sm text-content">{provider.label}</span>
              {!usable && <Badge variant="neutral">{t('settings.search.roleUnavailable')}</Badge>}
              <Button
                type="button"
                variant="tertiary"
                size="xs"
                iconOnly
                aria-label={withProvider(t('settings.search.roleMoveUp'), provider.label)}
                disabled={saving || index === 0}
                onClick={() => saveOrder(moveProvider(order, index, -1))}>
                <ChevronUpIcon className="size-3.5" aria-hidden="true" />
              </Button>
              <Button
                type="button"
                variant="tertiary"
                size="xs"
                iconOnly
                aria-label={withProvider(t('settings.search.roleMoveDown'), provider.label)}
                disabled={saving || index === order.length - 1}
                onClick={() => saveOrder(moveProvider(order, index, 1))}>
                <ChevronDownIcon className="size-3.5" aria-hidden="true" />
              </Button>
              <Button
                type="button"
                variant="tertiary"
                tone="danger"
                size="xs"
                iconOnly
                aria-label={withProvider(t('settings.search.roleRemove'), provider.label)}
                disabled={saving || order.length <= 1}
                onClick={() => saveOrder(order.filter(other => other !== id))}>
                <XIcon className="size-3.5" aria-hidden="true" />
              </Button>
            </li>
          );
        })}
      </ol>

      {addable.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {addable.map(provider => (
            <Button
              key={provider.id}
              type="button"
              variant="secondary"
              size="xs"
              data-testid={`${testId}-add-${provider.id}`}
              disabled={saving}
              leadingIcon={<PlusIcon className="size-3" aria-hidden="true" />}
              onClick={() => saveOrder([...order, provider.id])}>
              {withProvider(t('settings.search.roleAdd'), provider.label)}
            </Button>
          ))}
        </div>
      )}
    </div>
  );
};

interface Props {
  settings: SearchSettings;
  saving: boolean;
  persist: (update: SearchSettingsUpdate) => Promise<boolean>;
  t: Translate;
}

/** The Roles card: one row per capability role (Search, Answer, Contents). */
const SearchPanelRoles = ({ settings, saving, persist, t }: Props) => (
  <Card
    data-testid="search-roles"
    title={t('settings.search.rolesTitle')}
    description={t('settings.search.rolesDesc')}>
    {SEARCH_ROLES.map(role => (
      <SearchRoleRow
        key={role}
        role={role}
        settings={settings}
        saving={saving}
        persist={persist}
        t={t}
      />
    ))}
  </Card>
);

export default SearchPanelRoles;
