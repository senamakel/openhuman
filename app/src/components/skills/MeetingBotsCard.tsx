// Meeting bot integrations card. Lives on the Skills "Integrations"
// section — moved off the Intelligence Calls tab per product direction
// (#issue-mascot-meets).
//
// Wraps the backend mascot bot (PR tinyhumansai/backend#773): joining a
// Google Meet kicks off the Camoufox-driven mascot in the backend, which
// then streams the mascot's WebRTC video into the call as an anonymous
// guest. The user just supplies a meet URL. Zoom and Teams are shown
// as "coming soon" — the backend already routes them but returns 400
// "not yet supported".

import { useState } from 'react';

import {
  joinMeetingViaMascotBot,
  SERVER_OVERLOADED_MESSAGE,
  type MascotJoinMeetingError,
  type MascotMeetPlatform,
} from '../../services/meetCallService';

type Toast = { type: 'success' | 'error' | 'info'; title: string; message?: string };

interface Props {
  /** Optional toast sink — re-uses the Intelligence page's toast UI when mounted in /skills. */
  onToast?: (toast: Toast) => void;
}

interface PlatformDef {
  platform: MascotMeetPlatform;
  label: string;
  domainHint: string;
  comingSoon?: boolean;
}

const PLATFORMS: PlatformDef[] = [
  { platform: 'gmeet', label: 'Google Meet', domainHint: 'meet.google.com/abc-defg-hij' },
  { platform: 'zoom', label: 'Zoom', domainHint: 'zoom.us/j/…', comingSoon: true },
  { platform: 'teams', label: 'Microsoft Teams', domainHint: 'teams.microsoft.com/…', comingSoon: true },
];

function isMascotJoinMeetingError(err: unknown): err is MascotJoinMeetingError {
  return !!err && typeof err === 'object' && 'isCapacityGated' in err && 'message' in err;
}

export default function MeetingBotsCard({ onToast }: Props) {
  const [platform, setPlatform] = useState<MascotMeetPlatform>('gmeet');
  const [meetUrl, setMeetUrl] = useState('');
  const [displayName, setDisplayName] = useState('OpenHuman Mascot');
  const [submitting, setSubmitting] = useState(false);
  const [capacityGated, setCapacityGated] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const selected = PLATFORMS.find(p => p.platform === platform) ?? PLATFORMS[0];
  const isComingSoon = !!selected.comingSoon;

  const handleSubmit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setError(null);
    setCapacityGated(false);
    if (isComingSoon) {
      setError(`${selected.label} support is coming soon.`);
      return;
    }
    setSubmitting(true);
    try {
      await joinMeetingViaMascotBot({ platform, meetUrl, displayName });
      onToast?.({
        type: 'success',
        title: 'Mascot is joining the meeting',
        message: 'You should see it appear as a participant in a few seconds.',
      });
      setMeetUrl('');
    } catch (err) {
      if (isMascotJoinMeetingError(err)) {
        setCapacityGated(err.isCapacityGated);
        const message = err.isCapacityGated ? SERVER_OVERLOADED_MESSAGE : err.message;
        setError(message);
        onToast?.({
          type: 'error',
          title: err.isCapacityGated ? 'Mascot service is busy' : 'Could not start the mascot',
          message,
        });
      } else {
        const message = err instanceof Error ? err.message : 'Failed to start mascot.';
        setError(message);
        onToast?.({ type: 'error', title: 'Could not start the mascot', message });
      }
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div
      className="rounded-2xl border border-stone-200 bg-white p-3 shadow-soft animate-fade-up"
      data-testid="meeting-bots-card">
      <div className="px-1 pb-3 pt-1">
        <h2 className="text-sm font-semibold text-stone-900">Meeting bots</h2>
        <p className="mt-0.5 text-[11px] leading-relaxed text-stone-500">
          Send the mascot into a live meeting as an anonymous guest. The bot streams its WebRTC
          video into the call and listens / replies via the agent.
        </p>
      </div>

      <div className="flex flex-wrap gap-1.5 px-1 pb-3">
        {PLATFORMS.map(p => {
          const active = p.platform === platform;
          return (
            <button
              key={p.platform}
              type="button"
              onClick={() => {
                setPlatform(p.platform);
                setError(null);
              }}
              className={`rounded-full px-3 py-1 text-[11px] font-medium transition ${
                active
                  ? 'bg-primary-500 text-white'
                  : 'bg-stone-100 text-stone-600 hover:bg-stone-200'
              }`}>
              {p.label}
              {p.comingSoon && <span className="ml-1 opacity-70">· soon</span>}
            </button>
          );
        })}
      </div>

      <form onSubmit={handleSubmit} className="space-y-3 px-1 pb-1">
        <label className="block">
          <span className="text-[10px] font-medium uppercase tracking-wide text-stone-500">
            Meeting link
          </span>
          <input
            type="url"
            inputMode="url"
            autoComplete="off"
            spellCheck={false}
            value={meetUrl}
            onChange={e => setMeetUrl(e.target.value)}
            placeholder={selected.domainHint}
            disabled={isComingSoon || submitting}
            className="mt-1 w-full rounded-xl border border-stone-200 bg-white px-3 py-2 text-sm text-stone-900 placeholder:text-stone-400 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-100 disabled:cursor-not-allowed disabled:bg-stone-50"
            required
          />
        </label>

        <label className="block">
          <span className="text-[10px] font-medium uppercase tracking-wide text-stone-500">
            Display name
          </span>
          <input
            type="text"
            value={displayName}
            onChange={e => setDisplayName(e.target.value)}
            maxLength={64}
            disabled={isComingSoon || submitting}
            className="mt-1 w-full rounded-xl border border-stone-200 bg-white px-3 py-2 text-sm text-stone-900 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-100 disabled:cursor-not-allowed disabled:bg-stone-50"
          />
        </label>

        {error && (
          <div
            role="alert"
            className={`rounded-xl border px-3 py-2 text-xs ${
              capacityGated
                ? 'border-amber-200 bg-amber-50 text-amber-800'
                : 'border-coral-200 bg-coral-50 text-coral-700'
            }`}>
            {error}
          </div>
        )}

        <button
          type="submit"
          disabled={submitting || isComingSoon || !meetUrl.trim()}
          className="w-full rounded-xl bg-primary-500 px-3 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:bg-stone-200 disabled:text-stone-400">
          {isComingSoon
            ? `${selected.label} support coming soon`
            : submitting
              ? 'Starting mascot…'
              : `Send mascot to ${selected.label}`}
        </button>
      </form>
    </div>
  );
}
