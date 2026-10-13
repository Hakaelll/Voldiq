import type { FileTypes } from './types.ts';

export function visibleFileTypes(types: FileTypes) {
  const items = types.items.slice(0, 8);
  return {
    items,
    otherBytes: types.total_bytes - items.reduce((sum, item) => sum + item.bytes, 0),
    otherFiles: types.total_files - items.reduce((sum, item) => sum + item.count, 0),
  };
}
