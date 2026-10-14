import type { Snapshot } from './types';

type Capture = Pick<Snapshot, 'id' | 'root_key' | 'captured'>;

export function updateSnapshotPair(
  current: readonly Capture[],
  snapshot: Capture,
  selected: boolean,
): Capture[] | null {
  const next = current.filter((item) => item.id !== snapshot.id);
  if (selected) next.push(snapshot);
  if (next.length > 2 || next.some((item) => item.root_key !== next[0].root_key)) return null;
  return next.sort((a, b) => a.captured - b.captured || a.id - b.id);
}
