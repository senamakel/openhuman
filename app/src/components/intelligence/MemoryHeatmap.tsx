import { useMemo, useState } from 'react';

interface MemoryHeatmapProps {
  /** Array of document creation timestamps (unix epoch seconds). */
  timestamps: number[];
  loading?: boolean;
}

const WEEKS = 52;
const DAYS_PER_WEEK = 7;
const CELL_SIZE = 11;
const CELL_GAP = 2;
const DAY_LABELS = ['', 'Mon', '', 'Wed', '', 'Fri', ''];

const INTENSITY_COLORS = [
  'rgba(255,255,255,0.04)', // 0 events
  'rgba(74,131,221,0.25)',  // 1
  'rgba(74,131,221,0.45)',  // 2-3
  'rgba(74,131,221,0.65)',  // 4-6
  'rgba(74,131,221,0.85)',  // 7+
];

function getIntensity(count: number): number {
  if (count === 0) return 0;
  if (count === 1) return 1;
  if (count <= 3) return 2;
  if (count <= 6) return 3;
  return 4;
}

function dateToKey(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}

function formatDate(date: Date): string {
  return date.toLocaleDateString('en-US', {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  });
}

export function MemoryHeatmap({ timestamps, loading }: MemoryHeatmapProps) {
  const [hoveredCell, setHoveredCell] = useState<{ date: Date; count: number; x: number; y: number } | null>(null);

  const { grid, monthLabels, totalEvents, maxDailyCount } = useMemo(() => {
    // Build a date->count map from timestamps
    const countMap = new Map<string, number>();
    let total = 0;
    let maxCount = 0;

    for (const ts of timestamps) {
      const date = new Date(ts > 9999999999 ? ts : ts * 1000);
      const key = dateToKey(date);
      const count = (countMap.get(key) ?? 0) + 1;
      countMap.set(key, count);
      total++;
      if (count > maxCount) maxCount = count;
    }

    // Build grid: weeks x 7 days, ending today
    const today = new Date();
    today.setHours(0, 0, 0, 0);

    // Find the Sunday of the week that's WEEKS ago
    const startDate = new Date(today);
    startDate.setDate(startDate.getDate() - (WEEKS * 7) - startDate.getDay());

    const cells: { date: Date; count: number; weekIdx: number; dayIdx: number }[] = [];
    const months: { label: string; weekIdx: number }[] = [];
    let lastMonth = -1;

    const cursor = new Date(startDate);
    for (let w = 0; w <= WEEKS; w++) {
      for (let d = 0; d < DAYS_PER_WEEK; d++) {
        const cellDate = new Date(cursor);
        const key = dateToKey(cellDate);

        if (cellDate <= today) {
          cells.push({
            date: cellDate,
            count: countMap.get(key) ?? 0,
            weekIdx: w,
            dayIdx: d,
          });

          // Track month labels
          if (cellDate.getMonth() !== lastMonth && d === 0) {
            lastMonth = cellDate.getMonth();
            months.push({
              label: cellDate.toLocaleDateString('en-US', { month: 'short' }),
              weekIdx: w,
            });
          }
        }

        cursor.setDate(cursor.getDate() + 1);
      }
    }

    return { grid: cells, monthLabels: months, totalEvents: total, maxDailyCount: maxCount };
  }, [timestamps]);

  const svgWidth = (WEEKS + 1) * (CELL_SIZE + CELL_GAP) + 30;
  const svgHeight = DAYS_PER_WEEK * (CELL_SIZE + CELL_GAP) + 24;

  if (loading) {
    return (
      <div className="rounded-xl border border-white/10 bg-black/20 p-5">
        <h3 className="text-sm font-semibold text-white mb-3">Ingestion Activity</h3>
        <div className="h-28 rounded-lg bg-white/5 animate-pulse" />
      </div>
    );
  }

  return (
    <div className="rounded-xl border border-white/10 bg-black/20 p-5">
      <div className="flex items-center justify-between mb-3">
        <div>
          <h3 className="text-sm font-semibold text-white">Ingestion Activity</h3>
          <p className="text-xs text-stone-500 mt-0.5">
            {totalEvents} events over the last {WEEKS} weeks
            {maxDailyCount > 0 && <> · peak: {maxDailyCount}/day</>}
          </p>
        </div>
        <div className="flex items-center gap-1 text-[10px] text-stone-500">
          <span>Less</span>
          {INTENSITY_COLORS.map((color, i) => (
            <div
              key={i}
              className="w-[10px] h-[10px] rounded-[2px]"
              style={{ backgroundColor: color }}
            />
          ))}
          <span>More</span>
        </div>
      </div>

      <div className="overflow-x-auto">
        <svg
          width={svgWidth}
          height={svgHeight}
          viewBox={`0 0 ${svgWidth} ${svgHeight}`}
          className="block">
          {/* Day labels */}
          {DAY_LABELS.map((label, i) =>
            label ? (
              <text
                key={i}
                x={0}
                y={24 + i * (CELL_SIZE + CELL_GAP) + CELL_SIZE * 0.75}
                fontSize={9}
                fill="rgba(255,255,255,0.3)"
                style={{ userSelect: 'none' }}>
                {label}
              </text>
            ) : null
          )}

          {/* Month labels */}
          {monthLabels.map((m, i) => (
            <text
              key={i}
              x={30 + m.weekIdx * (CELL_SIZE + CELL_GAP)}
              y={12}
              fontSize={9}
              fill="rgba(255,255,255,0.3)"
              style={{ userSelect: 'none' }}>
              {m.label}
            </text>
          ))}

          {/* Cells */}
          {grid.map((cell, i) => {
            const x = 30 + cell.weekIdx * (CELL_SIZE + CELL_GAP);
            const y = 20 + cell.dayIdx * (CELL_SIZE + CELL_GAP);
            const intensity = getIntensity(cell.count);

            return (
              <rect
                key={i}
                x={x}
                y={y}
                width={CELL_SIZE}
                height={CELL_SIZE}
                rx={2}
                fill={INTENSITY_COLORS[intensity]}
                stroke={
                  hoveredCell?.date.getTime() === cell.date.getTime()
                    ? 'rgba(255,255,255,0.4)'
                    : 'transparent'
                }
                strokeWidth={1}
                style={{ cursor: 'pointer', transition: 'fill 0.1s' }}
                onMouseEnter={e => {
                  const rect = (e.target as SVGRectElement).getBoundingClientRect();
                  setHoveredCell({
                    date: cell.date,
                    count: cell.count,
                    x: rect.left + rect.width / 2,
                    y: rect.top,
                  });
                }}
                onMouseLeave={() => setHoveredCell(null)}
              />
            );
          })}
        </svg>
      </div>

      {/* Tooltip */}
      {hoveredCell && (
        <div
          className="fixed z-50 px-2 py-1 rounded-md bg-stone-800 border border-white/10 text-[11px] text-white shadow-lg pointer-events-none"
          style={{
            left: hoveredCell.x,
            top: hoveredCell.y - 32,
            transform: 'translateX(-50%)',
          }}>
          <span className="font-medium">
            {hoveredCell.count} event{hoveredCell.count !== 1 ? 's' : ''}
          </span>{' '}
          <span className="text-stone-400">on {formatDate(hoveredCell.date)}</span>
        </div>
      )}
    </div>
  );
}
