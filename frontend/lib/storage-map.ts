import { layout, type Rect as TreemapRect, type WeightedNode } from './treemap.ts';

export type HeightMode = 'modified' | 'files' | 'depth';
export type ColorMode = 'type' | 'size' | 'age';
export type StorageView = 'treemap' | 'city' | 'sunburst' | 'circles';
export const STORAGE_DEPTHS = [1, 2, 3, 4, 5, 6] as const;
export type StorageDepth = (typeof STORAGE_DEPTHS)[number];
export function parseStorageDepth(value: string): StorageDepth | null {
  return STORAGE_DEPTHS.find((depth) => String(depth) === value) ?? null;
}
export interface Rect extends TreemapRect {
  depth: number;
  modified: number | null;
  files: number;
  represented: number;
  aggregated: number;
  allocation_known: boolean;
}

export const STORAGE_WIDTH = 1000;
export const STORAGE_HEIGHT = 700;
const MAX_RECTS = 350;

function count(node: WeightedNode): number {
  return node.files ?? node.represented ?? (node.directory ? 0 : 1);
}

function modification(node: WeightedNode): number | null {
  const value = node.directory
    ? (node.modifiedKnown ?? 0) > 0
      ? node.latestModified
      : null
    : node.modified;
  return value != null && Number.isFinite(value) && value >= 0 ? value : null;
}

function toRect(node: TreemapRect, depth: number, total: number): Rect {
  const represented = count(node);
  return {
    ...node,
    depth,
    share: total ? node.value / total : 0,
    modified: modification(node),
    files: node.files ?? (node.directory ? 0 : 1),
    represented,
    aggregated: node.directory || node.id < 0 ? represented : 0,
    allocation_known: node.allocationKnown ?? false,
  };
}

export function createStorageLayout(
  nodes: WeightedNode[],
  maxDepth: StorageDepth = 6,
): {
  rects: Rect[];
  boundaries: Rect[];
  total: number;
  represented: number;
  zeroBytes: number;
  revision: string;
  parents: Map<number, number | null>;
} {
  const roots = nodes
    .filter((node) => Number.isFinite(node.value) && node.value > 0)
    .sort((a, b) => b.value - a.value || a.id - b.id);
  const total = roots.reduce((sum, node) => sum + node.value, 0);
  const rects: Rect[] = [];
  const boundaries: Rect[] = [];
  const parents = new Map<number, number | null>();
  let frontierCount = roots.length;
  let zeroBytes = 0;
  const countZero = (items: WeightedNode[]): void => {
    for (const item of items) {
      if (item.value === 0 && item.allocationKnown) zeroBytes += count(item);
      else if (item.children) countZero(item.children);
    }
  };
  countZero(nodes);
  const visit = (placed: TreemapRect, depth: number, parentId: number | null): void => {
    parents.set(placed.id, parentId);
    const parent = toRect(placed, depth, total);
    const children = (placed.children ?? [])
      .filter((node) => Number.isFinite(node.value) && node.value > 0)
      .sort((a, b) => b.value - a.value || a.id - b.id);
    const childTotal = children.reduce((sum, node) => sum + node.value, 0);
    const remainder = placed.value - childTotal;
    const nextCount = frontierCount - 1 + children.length + (remainder > 0 ? 1 : 0);
    if (
      !placed.directory ||
      !children.length ||
      remainder < 0 ||
      depth >= maxDepth ||
      placed.width < 90 ||
      placed.height < 65 ||
      placed.width * placed.height < 11000 ||
      nextCount > MAX_RECTS
    ) {
      rects.push(parent);
      return;
    }
    if (remainder > 0) {
      const represented = Math.max(
        0,
        count(placed) - children.reduce((sum, child) => sum + count(child), 0),
      );
      children.push({
        id: -(placed.id + 2),
        name: placed.incomplete ? 'Unscanned or unlisted contents' : 'Other contents',
        value: remainder,
        directory: true,
        files: Math.max(
          0,
          parent.files -
            children.reduce((sum, child) => sum + (child.files ?? (child.directory ? 0 : 1)), 0),
        ),
        represented,
        allocationKnown: placed.allocationKnown,
        incomplete: placed.incomplete,
        category: 'remainder',
      });
    }
    frontierCount = nextCount;
    boundaries.push(parent);
    children.sort((a, b) => b.value - a.value || a.id - b.id);
    for (const child of layout(children, placed.width, placed.height)) {
      visit({ ...child, x: child.x + placed.x, y: child.y + placed.y }, depth + 1, placed.id);
    }
  };
  for (const root of layout(roots, STORAGE_WIDTH, STORAGE_HEIGHT)) visit(root, 1, null);
  const revision = JSON.stringify(
    rects.map((rect) => [
      rect.id,
      rect.value,
      rect.x,
      rect.y,
      rect.width,
      rect.height,
      rect.depth,
      rect.modified,
      rect.files,
      rect.allocation_known,
      rect.modifiedKnown,
      rect.modifiedTotal,
      rect.category,
    ]),
  );
  return {
    rects,
    boundaries,
    total,
    represented: roots.reduce((sum, node) => sum + count(node), 0),
    zeroBytes,
    revision,
    parents,
  };
}

export function metricValue(rect: Rect, mode: HeightMode, now: number): number | null {
  switch (mode) {
    case 'modified':
      return rect.modified === null ? null : Math.max(0, now - rect.modified);
    case 'files':
      return rect.files;
    case 'depth':
      return rect.depth;
  }
}

export function metricMaximum(rects: Rect[], mode: HeightMode, now: number): number {
  return Math.max(0, ...rects.map((rect) => metricValue(rect, mode, now) ?? 0));
}

export function metricHeight(rect: Rect, mode: HeightMode, now: number, maximum: number): number {
  const value = metricValue(rect, mode, now);
  if (value === null) return 5;
  const fraction =
    mode === 'modified'
      ? Math.log1p(Math.min(value / 86400, 3652.5)) / Math.log1p(3652.5)
      : maximum <= 0
        ? 0
        : mode === 'files'
          ? Math.log1p(Math.max(0, value)) / Math.log1p(maximum)
          : value / maximum;
  return 5 + 125 * Math.min(1, Math.max(0, fraction));
}

export function heightDescription(rect: Rect, mode: HeightMode, now: number): string {
  switch (mode) {
    case 'modified': {
      if (rect.modified === null)
        return rect.directory
          ? 'Descendant modification time unknown'
          : 'Modification time unknown';
      const days = Math.floor(Math.max(0, now - rect.modified) / 86400);
      const coverage =
        rect.directory && rect.modifiedTotal
          ? ` (${rect.modifiedKnown ?? 0}/${rect.modifiedTotal} known)`
          : '';
      return `${rect.directory ? 'Latest known descendant modification' : 'Modified'} ${days} days ago${coverage}`;
    }
    case 'files':
      return `${rect.files.toLocaleString()} ${rect.directory ? 'descendant files' : 'file'}`;
    case 'depth':
      return `Depth ${rect.depth} from this folder`;
  }
}

const paints = {
  amber: { fill: '#d5a94e', ink: '#171a18' },
  terracotta: { fill: '#ad563d', ink: '#ffffff' },
  olive: { fill: '#8b9859', ink: '#11160e' },
  teal: { fill: '#397c7c', ink: '#ffffff' },
  violet: { fill: '#796b97', ink: '#ffffff' },
  neutral: { fill: '#9b9d97', ink: '#171a18' },
  unknown: { fill: '#6b706b', ink: '#ffffff' },
};

export function categoryLabel(node: WeightedNode): string {
  if (node.category === 'remainder' || node.id < 0) return 'Other contents';
  if (node.directory) return 'Folder';
  const extension = node.name.toLowerCase().split('.').at(-1) ?? '';
  const category = node.category?.toLowerCase() ?? '';
  if (
    category === 'image' ||
    category === 'images' ||
    /^(png|jpe?g|gif|webp|heic|svg|avif|raw)$/.test(extension)
  )
    return 'Image';
  if (category === 'video' || category === 'videos' || /^(mp4|mkv|mov|avi|webm)$/.test(extension))
    return 'Video';
  if (category === 'audio' || /^(mp3|wav|flac|ogg|m4a|aac)$/.test(extension)) return 'Audio';
  if (
    category === 'code' ||
    /^(ts|tsx|js|jsx|rs|py|go|css|html|json|c|cpp|h|java|swift)$/.test(extension)
  )
    return 'Code';
  if (
    category === 'archive' ||
    category === 'archives' ||
    /^(zip|7z|rar|gz|tar|bz2|xz|iso)$/.test(extension)
  )
    return 'Archive';
  if (
    category === 'document' ||
    category === 'documents' ||
    /^(pdf|docx?|xlsx?|pptx?|txt|md|csv)$/.test(extension)
  )
    return 'Document';
  return 'Other file';
}

export function categoryPaint(node: WeightedNode): { fill: string; ink: string } {
  switch (categoryLabel(node)) {
    case 'Folder':
    case 'Image':
      return paints.amber;
    case 'Video':
      return paints.terracotta;
    case 'Audio':
    case 'Document':
      return paints.olive;
    case 'Code':
      return paints.teal;
    case 'Archive':
      return paints.violet;
    case 'Other contents':
      return paints.unknown;
    default:
      return paints.neutral;
  }
}

const sequentialPaints = [
  { fill: '#b6d2c6', ink: '#10231f' },
  { fill: '#91b9aa', ink: '#10231f' },
  { fill: '#679b8e', ink: '#10231f' },
  { fill: '#3e7c73', ink: '#ffffff' },
  { fill: '#23625f', ink: '#ffffff' },
];

export function visualPaint(
  rect: WeightedNode & { modified: number | null },
  mode: ColorMode,
  maximum: number,
  now: number,
): { fill: string; ink: string } {
  if (mode === 'type') return categoryPaint(rect);
  if (mode === 'age') {
    if (rect.modified === null) return paints.unknown;
    const days = Math.max(0, now - rect.modified) / 86400;
    const index = days < 30 ? 0 : days < 180 ? 1 : days < 365 ? 2 : days < 3652 ? 3 : 4;
    return sequentialPaints[index] ?? paints.unknown;
  }
  const fraction = maximum > 0 ? Math.log1p(Math.max(0, rect.value)) / Math.log1p(maximum) : 0;
  const index = Math.min(4, Math.max(0, Math.floor(fraction * 5)));
  return sequentialPaints[index] ?? paints.unknown;
}
