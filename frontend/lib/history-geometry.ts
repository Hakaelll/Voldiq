import { layout, type WeightedNode } from './treemap.ts';
import { categoryPaint } from './storage-map.ts';
import type { SnapshotVisualFrame } from './types.ts';

export function historicalRects(
  frame: SnapshotVisualFrame,
  maximum: number,
  width: number,
  height = 320,
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
    paint: categoryPaint(rect),
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
