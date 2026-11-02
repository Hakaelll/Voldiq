import type { NodeId, Query } from './types';

export function scopedFileQuery({
  scope,
  ranked,
  ...filters
}: Omit<Query, 'scope' | 'parent'> & { scope: NodeId; ranked: boolean }): Query {
  const recursive =
    ranked ||
    Boolean(filters.text) ||
    Boolean(filters.extension) ||
    filters.without_extension ||
    Boolean(filters.cloud);
  return { ...(recursive ? { scope } : { parent: scope }), ...filters };
}
