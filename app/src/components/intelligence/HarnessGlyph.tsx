/**
 * HarnessGlyph — a small brand mark for the agent harness driving an instance
 * (Claude / Codex / Gemini), plus an OpenHuman mark for internal windows. Used
 * as the leading glyph in roster rows.
 *
 * The colors here are deliberate brand/identity hues (not surface chrome), so
 * they use literal palette classes per the project's "meaningful content color"
 * guidance rather than semantic tokens.
 */
import type { HarnessType } from '../../lib/orchestration/orchestrationClient';

export type GlyphKind = HarnessType | 'openhuman';

interface HarnessGlyphProps {
  harness: GlyphKind;
  className?: string;
}

const GLYPH: Record<GlyphKind, { label: string; tone: string }> = {
  claude: { label: 'C', tone: 'bg-[#c96442] text-content-inverted' },
  codex: { label: 'Cx', tone: 'bg-content text-surface' },
  gemini: { label: 'G', tone: 'bg-primary-500 text-content-inverted' },
  // Literal hex (Tailwind's slate-800) rather than the `bg-slate-*` utility —
  // this is cursor's brand identity color, not a themed neutral surface.
  cursor: { label: 'Cu', tone: 'bg-[#1e293b] text-content-inverted' },
  windsurf: { label: 'Ws', tone: 'bg-teal-500 text-content-inverted' },
  openhuman: { label: 'OH', tone: 'bg-sage-500 text-content-inverted' },
};

export default function HarnessGlyph({
  harness,
  className,
}: HarnessGlyphProps): React.ReactElement {
  const { label, tone } = GLYPH[harness];
  return (
    <span
      aria-hidden
      data-testid="harness-glyph"
      data-harness={harness}
      className={`flex h-6 w-6 flex-none items-center justify-center rounded-md font-mono text-[11px] font-bold ${tone} ${className ?? ''}`}>
      {label}
    </span>
  );
}
