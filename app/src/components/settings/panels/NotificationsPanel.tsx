import { useT } from '../../../lib/i18n/I18nContext';
import { useAppDispatch, useAppSelector } from '../../../store/hooks';
import { type NotificationCategory, setPreference } from '../../../store/notificationSlice';
import { SettingsRow, SettingsSection, SettingsSwitch } from '../controls';
import SettingsPanel from '../layout/SettingsPanel';

interface NotificationsPanelProps {
  /** When embedded inside the tabbed Notifications page, the parent owns the
      `<SettingsHeader>` chrome and we render only the body. */
  embedded?: boolean;
}

const CATEGORIES: { id: NotificationCategory; title: string; description: string }[] = [
  {
    id: 'messages',
    title: 'Messages',
    description: 'New messages from embedded webview accounts (Slack, WhatsApp, …).',
  },
  {
    id: 'agents',
    title: 'Agent activity',
    description: 'Agent task completions and long-running responses.',
  },
  { id: 'skills', title: 'Skills', description: 'Skill sync events and OAuth status changes.' },
  {
    id: 'system',
    title: 'System',
    description: 'Connection issues, background process errors, updates.',
  },
  {
    id: 'meetings',
    title: 'Meetings',
    description: 'Upcoming meetings and calendar events detected by heartbeat.',
  },
  {
    id: 'reminders',
    title: 'Reminders',
    description: 'Upcoming reminders and scheduled tasks from cron jobs.',
  },
  {
    id: 'important',
    title: 'Important events',
    description: 'Urgent or time-sensitive events surfaced from connected sources.',
  },
];

const NotificationsPanel = ({ embedded = false }: NotificationsPanelProps = {}) => {
  const { t } = useT();
  const preferences = useAppSelector(s => s.notifications.preferences);
  const dispatch = useAppDispatch();
  const handleToggle = (category: NotificationCategory) => {
    dispatch(setPreference({ category, enabled: !preferences[category] }));
  };

  const body = (
    <>
      {/* Categories */}
      <SettingsSection title={t('settings.notifications.categories')}>
        {CATEGORIES.map(cat => {
          const enabled = preferences[cat.id];
          const switchId = `switch-notif-${cat.id}`;
          return (
            <SettingsRow
              key={cat.id}
              htmlFor={switchId}
              label={cat.title}
              description={cat.description}
              control={
                <SettingsSwitch
                  id={switchId}
                  checked={enabled}
                  onCheckedChange={() => handleToggle(cat.id)}
                  aria-label={`Toggle ${cat.title} notifications`}
                />
              }
            />
          );
        })}
      </SettingsSection>

      <p className="text-xs text-content-muted leading-relaxed px-1">
        {t('settings.notifications.categoryFooter')}
      </p>
    </>
  );

  // `embedded` renders the body with no page chrome, for a host that draws its
  // own header. The tabbed Notifications page that used it is gone — the
  // routing tab was removed with `NotificationRoutingPanel`, and a two-tab page
  // with one tab left is a control that cannot do anything — so this route now
  // renders the preferences directly. The prop is kept for the next host.
  if (embedded) return <div className="space-y-4">{body}</div>;

  return <SettingsPanel>{body}</SettingsPanel>;
};

export default NotificationsPanel;
