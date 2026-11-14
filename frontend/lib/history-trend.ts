import type { Snapshot } from './types.ts';

export interface TrendPoint {
  snapshot: number;
  measured_at: number;
  logical: string;
  complete: boolean;
  supported: boolean;
  previous: number | null;
  delta: string | null;
}

export interface TrendForecast {
  at: number;
  logical: string;
  low: string;
  high: string;
  samples: number;
  confidence: 'higher' | 'moderate';
}

export interface TrendReport {
  anchor: number;
  root: string;
  points: TrendPoint[];
  limited: boolean;
  missing_dates: boolean;
  forecast: TrendForecast | null;
}

export interface TrendChanges {
  before: number;
  after: number;
  grew: { name: string; delta: string }[];
  shrank: { name: string; delta: string }[];
}

export function historyTrend<T extends Pick<Snapshot, 'captured' | 'logical'>>(
  captures: T[],
  width: number,
  height = 180,
) {
  if (captures.length < 2) return null;
  const ordered = [...captures].sort((a, b) => a.captured - b.captured);
  let low = Math.min(...ordered.map((capture) => capture.logical));
  let high = Math.max(...ordered.map((capture) => capture.logical));
  if (low === high) {
    const padding = Math.max(1, high * 0.05);
    low = Math.max(0, low - padding);
    high += padding;
  }
  low = Math.floor(low);
  high = Math.ceil(high);
  const first = ordered[0].captured;
  const last = ordered[ordered.length - 1].captured;
  const count = Math.min(5, ordered.length);
  const y = (value: number) => height - ((value - low) / (high - low)) * (height - 24);
  const labelled = new Set(
    Array.from({ length: count }, (_, i) => Math.round((i * (ordered.length - 1)) / (count - 1))),
  );
  const points = ordered.map((capture, i) => ({
    ...capture,
    x:
      60 +
      (last === first ? 0.5 : (capture.captured - first) / (last - first)) *
        Math.max(0, width - 76),
    y: y(capture.logical),
    labelled: false,
  }));
  let previousLabel = -Infinity;
  const lastX = points[points.length - 1].x;
  for (const [i, point] of points.entries()) {
    if (last === first) {
      point.labelled = i === points.length - 1;
      continue;
    }
    if (
      i === 0 ||
      i === points.length - 1 ||
      (labelled.has(i) && point.x - previousLabel >= 72 && lastX - point.x >= 72)
    ) {
      point.labelled = true;
      previousLabel = point.x;
    }
  }
  return {
    y,
    ticks: [
      ...new Set(Array.from({ length: 4 }, (_, i) => Math.round(high - (i * (high - low)) / 3))),
    ].map((value) => ({
      y: y(value),
      value,
    })),
    points,
  };
}
