import type { IndexId } from './index-context.ts';
import type { Point } from './disk-city.ts';
import type { WeightedNode } from './treemap.ts';
import { VORONOI_GROUP_ID } from './voronoi.ts';

export type MapCommand =
  | { readonly kind: 'inspect'; readonly id: number }
  | { readonly kind: 'open'; readonly id: number }
  | { readonly kind: 'reveal'; readonly id: number }
  | { readonly kind: 'copy'; readonly id: number }
  | { readonly kind: 'list' };
export type MapTarget = { kind: 'entry' | 'contents'; id: number } | { kind: 'list' };
export interface MapMenuItem {
  readonly label: string;
  readonly command: MapCommand;
}
export interface MapMenuModel {
  readonly name: string;
  readonly indexId: IndexId;
  readonly items: readonly MapMenuItem[];
}
export type MapCommandHandler = (command: MapCommand, expectedIndex: IndexId) => void;
const nativeId = (id: number) => Number.isSafeInteger(id) && id >= 0 && id <= 0xffffffff;

export function mapTarget(id: number): MapTarget | null {
  if (id === -1 || id === VORONOI_GROUP_ID) return { kind: 'list' };
  if (id < -1) {
    const parent = -id - 2;
    return nativeId(parent) ? { kind: 'contents', id: parent } : null;
  }
  return nativeId(id) ? { kind: 'entry', id } : null;
}

export function createMapMenu(
  node: Pick<WeightedNode, 'id' | 'name' | 'directory'> | null,
  indexId: IndexId | null,
): MapMenuModel | null {
  if (indexId === null) return null;
  const list: MapMenuItem = { label: 'Open item list', command: { kind: 'list' } };
  if (!node) return { name: 'Storage map', indexId, items: [list] };
  const { name } = node;
  const target = mapTarget(node.id);
  if (!target) return null;
  if (target.kind === 'list') return { name, indexId, items: [list] };
  const id = target.id;
  if (target.kind === 'contents') {
    return {
      name,
      indexId,
      items: [{ label: 'Open containing folder', command: { kind: 'open', id } }, list],
    };
  }
  const items: MapMenuItem[] = [{ label: 'Inspect', command: { kind: 'inspect', id } }];
  if (node.directory) items.push({ label: 'Explore folder', command: { kind: 'open', id } });
  items.push(
    { label: 'Show in Explorer', command: { kind: 'reveal', id } },
    { label: 'Copy path', command: { kind: 'copy', id } },
    list,
  );
  return { name, indexId, items };
}

export function menuCommand(
  model: MapMenuModel,
  indexId: IndexId | null,
  command: MapCommand,
): MapCommand | null {
  return model.indexId === indexId && model.items.some((item) => item.command === command)
    ? command
    : null;
}

export function menuKeyIndex(key: string, index: number, length: number): number | null {
  if (!length) return null;
  switch (key) {
    case 'ArrowDown':
      return (index + 1) % length;
    case 'ArrowUp':
      return (index + length - 1) % length;
    case 'Home':
      return 0;
    case 'End':
      return length - 1;
    default:
      return null;
  }
}

export function menuPosition(
  anchor: Point,
  box: { width: number; height: number },
  viewport: { width: number; height: number },
): Point {
  return {
    x: Math.max(8, Math.min(anchor.x, viewport.width - box.width - 8)),
    y: Math.max(8, Math.min(anchor.y, viewport.height - box.height - 8)),
  };
}
