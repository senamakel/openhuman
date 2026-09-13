/**
 * Truncate `text` to at most `max` characters, appending an ellipsis (`…`)
 * when it was cut. The ellipsis itself counts against `max`, so the result
 * is never longer than `max` characters.
 *
 * Pass `trim: true` to trim whitespace before measuring/truncating (useful
 * for freeform text pulled from tool output or user input).
 */
export function truncateText(text: string, max: number, options?: { trim?: boolean }): string {
  const value = options?.trim ? text.trim() : text;
  return value.length > max ? `${value.slice(0, Math.max(0, max - 1))}…` : value;
}
