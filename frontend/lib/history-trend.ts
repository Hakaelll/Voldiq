import type { Snapshot } from './types.ts';

export function historyTrend(captures: Pick<Snapshot, 'captured' | 'logical'>[], width: number) {
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
  const labelled = new Set(
    Array.from({ length: count }, (_, i) => Math.round((i * (ordered.length - 1)) / (count - 1))),
  );
  const points = ordered.map((capture, i) => ({
    ...capture,
    x:
      60 +
      (last === first ? 0.5 : (capture.captured - first) / (last - first)) *
        Math.max(0, width - 76),
    y: 180 - ((capture.logical - low) / (high - low)) * 156,
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
    ticks: [
      ...new Set(Array.from({ length: 4 }, (_, i) => Math.round(high - (i * (high - low)) / 3))),
    ].map((value) => ({
      y: 180 - ((value - low) / (high - low)) * 156,
      value,
    })),
    points,
  };
}
