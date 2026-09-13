/**
 * Format a flow-run ISO timestamp for display: short month/day + time, in
 * the viewer's locale. Returns `null` for a missing or unparsable value so
 * callers can render nothing rather than "Invalid Date".
 *
 * Pass `withSeconds: true` for contexts (like a run inspector) precise
 * enough that second-level resolution helps distinguish nearby events.
 */
export function formatRunTimestamp(
  value: string | null | undefined,
  options?: { withSeconds?: boolean }
): string | null {
  if (!value) return null;
  const parsed = Date.parse(value);
  if (!Number.isFinite(parsed)) return null;
  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
    ...(options?.withSeconds ? { second: '2-digit' as const } : {}),
  }).format(new Date(parsed));
}
