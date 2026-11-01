import { bytes, type Entry } from './types.ts';

export function fileSizeLabel(
  entry: Pick<Entry, 'logical' | 'allocated' | 'allocation_known'>,
  allocated: boolean,
): string {
  if (!allocated) return bytes(entry.logical);
  if (entry.allocation_known) return bytes(entry.allocated);
  return entry.allocated > 0 ? `${bytes(entry.allocated)} known · partial` : 'Unknown';
}
