import type { CleanupRulePage } from './types';

type Item = CleanupRulePage['items'][number];

export function updateCleanupSelection(
  current: ReadonlyMap<number, Item>,
  items: readonly Item[],
  select: boolean,
): Map<number, Item> | null {
  const next = new Map(current);
  for (const item of items) {
    if (select) next.set(item.id, item);
    else next.delete(item.id);
  }
  return next.size <= 500 ? next : null;
}
