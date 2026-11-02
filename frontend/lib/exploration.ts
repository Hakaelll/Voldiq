import type { IndexId } from './index-context.ts';

export type SelectionOperation = 'replace' | 'add' | 'toggle';
export type SelectMany = (
  ids: readonly number[],
  operation: SelectionOperation,
  expectedIndex: IndexId | null,
) => boolean;
export interface Exploration {
  readonly indexId: IndexId | null;
  readonly ids: ReadonlySet<number>;
}
export type ExplorationUpdate =
  { kind: 'accepted'; state: Exploration } | { kind: 'invalid' | 'limit' };
const empty: ReadonlySet<number> = new Set();

export function emptyExploration(indexId: IndexId | null): Exploration {
  return { indexId, ids: new Set() };
}
export function explorationIds(state: Exploration, indexId: IndexId | null): ReadonlySet<number> {
  return indexId !== null && state.indexId === indexId ? state.ids : empty;
}
export function updateExploration(
  state: Exploration,
  indexId: IndexId | null,
  ids: readonly number[],
  operation: SelectionOperation,
): ExplorationUpdate {
  if (indexId === null || ids.some((id) => !Number.isSafeInteger(id) || id < 0 || id > 0xffffffff))
    return { kind: 'invalid' };
  const next = new Set(operation === 'replace' ? [] : explorationIds(state, indexId));
  for (const id of new Set(ids)) {
    if (operation === 'toggle' && next.has(id)) next.delete(id);
    else next.add(id);
  }
  return next.size > 500 ? { kind: 'limit' } : { kind: 'accepted', state: { indexId, ids: next } };
}
export function selectionOperation(event: {
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}): SelectionOperation {
  return event.ctrlKey || event.metaKey ? 'toggle' : event.shiftKey ? 'add' : 'replace';
}

export function projectedSelection(
  frontier: readonly number[],
  parents: ReadonlyMap<number, number | null>,
  selected: ReadonlySet<number>,
  scopeAncestors: readonly number[],
): ReadonlySet<number> {
  if (scopeAncestors.some((id) => selected.has(id))) return new Set(frontier);
  return new Set(
    frontier.filter((id) => {
      const visited = new Set<number>();
      let current: number | null | undefined = id;
      while (current !== null && current !== undefined && !visited.has(current)) {
        if (selected.has(current)) return true;
        visited.add(current);
        current = parents.get(current);
      }
      return false;
    }),
  );
}
