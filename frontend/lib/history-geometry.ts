import { layout, type WeightedNode } from './treemap.ts';
import { categoryPaint, visualPaint } from './storage-map.ts';
import { CHART_PAINTS } from './chart-colors.ts';
import { formatStorageBytes } from './storage-units.ts';
import type { SnapshotVisualFrame } from './types.ts';

export type HistoricalMapColor = 'size' | 'type';
export type ComparisonMapColor = HistoricalMapColor | 'change';

export const historicalChanges = {
  grew: { label: 'Grew', paint: CHART_PAINTS.video },
  shrank: { label: 'Shrank', paint: CHART_PAINTS.code },
  same: { label: 'Unchanged', paint: CHART_PAINTS.neutral },
  unavailable: { label: 'Change unavailable', paint: CHART_PAINTS.unknown },
} as const;

export function historicalChange(delta: string | undefined) {
  if (delta === undefined) return historicalChanges.unavailable;
  if (delta === '0') return historicalChanges.same;
  return delta.startsWith('-') ? historicalChanges.shrank : historicalChanges.grew;
}

export function formatHistoricalDelta(delta: string | undefined): string {
  if (delta === undefined) return 'Unavailable';
  if (delta === '0') return formatStorageBytes(delta);
  return delta.startsWith('-')
    ? `−${formatStorageBytes(delta.slice(1))}`
    : `+${formatStorageBytes(delta)}`;
}

export function historicalRects(
  frame: SnapshotVisualFrame,
  maximum: number,
  width: number,
  height = 320,
  color: HistoricalMapColor = 'type',
) {
  const nodes: WeightedNode[] = frame.entries.map((entry, index) => ({
    id: index + 1,
    name: entry.name,
    value: Number(entry.logical),
    directory: entry.kind === 'directory',
    incomplete: entry.incomplete,
  }));
  if (frame.remainder)
    nodes.push({
      id: -1,
      name: 'Other indexed contents',
      value: Number(frame.remainder.logical),
      directory: false,
      category: 'remainder',
    });
  return layout(
    nodes,
    maximum > 0 ? (width * Number(frame.logical ?? 0)) / maximum : 0,
    height,
  ).map((rect) => ({
    ...rect,
    paint:
      rect.id < 0 ? categoryPaint(rect) : visualPaint({ ...rect, modified: null }, color, maximum, 0),
    source: rect.id > 0 ? frame.entries[rect.id - 1] : null,
  }));
}
export function historicalAbsence(frame: SnapshotVisualFrame): string {
  return frame.presence === 'not_recorded'
    ? 'No entry recorded in this capture.'
    : frame.presence === 'not_observed'
      ? 'Not observed; capture coverage is incomplete.'
      : frame.incomplete
        ? 'No positive-size geometry in the partial records.'
        : 'Zero recorded logical bytes.';
}
