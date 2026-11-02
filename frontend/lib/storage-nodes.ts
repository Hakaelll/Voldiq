import type { Entry, Overview, Page } from './types.ts';
import type { WeightedNode } from './treemap.ts';

export function storageNodes({
  directory,
  children,
  branches,
  metrics,
  allocated,
  filtered,
}: {
  directory: Entry | null;
  children: Page;
  branches: Overview['branches'];
  metrics: Overview['map_metrics'];
  allocated: boolean;
  filtered: boolean;
}): WeightedNode[] | null {
  if (!directory) return [];
  const measurements = new Map(metrics.map((metric) => [metric.id, metric]));
  const node = (entry: Entry, inherited = false): WeightedNode | null => {
    const metric = measurements.get(entry.id);
    if (filtered && !metric) return null;
    const source = filtered && metric ? metric : entry;
    return {
      id: entry.id,
      name: entry.name,
      directory: entry.kind === 'directory',
      value: allocated ? source.allocated : source.logical,
      files: source.files,
      represented: source.files,
      modified: entry.modified,
      allocationKnown: source.allocation_known,
      incomplete: entry.incomplete,
      latestModified: metric?.latest_modified,
      modifiedKnown: metric?.modified_known,
      modifiedTotal: metric?.modified_total,
      search: metric?.matches ? { ...metric.matches, inherited } : undefined,
    };
  };
  const root = node(directory);
  if (!root) return null;
  const entries: WeightedNode[] = [];
  for (const entry of children.items) {
    const item = node(entry, root.search?.own ?? false);
    if (!item) return null;
    const branch = branches.find((branch) => branch.parent === entry.id);
    if (branch) {
      item.children = [];
      for (const entry of branch.items) {
        const child = node(entry, Boolean(item.search?.own || item.search?.inherited));
        if (!child) return null;
        item.children.push(child);
      }
    }
    entries.push(item);
  }
  const childTotal = entries.reduce((sum, item) => sum + item.value, 0);
  if (filtered && childTotal > root.value) return null;
  const remainder = Math.max(0, root.value - childTotal);
  const remainingFiles = Math.max(
    0,
    (root.files ?? 0) - entries.reduce((sum, item) => sum + (item.files ?? 0), 0),
  );
  if (remainder > 0 || (filtered && remainingFiles > 0))
    entries.push({
      id: -1,
      name: filtered
        ? 'Other matching contents'
        : `${Math.max(0, children.total - children.items.length)} smaller items`,
      value: remainder,
      directory: true,
      files: remainingFiles,
      represented: remainingFiles,
      category: 'remainder',
      allocationKnown: root.allocationKnown,
      incomplete: root.incomplete,
      search: remainderSearch(root, entries),
    });
  return entries;
}

export function remainderSearch(
  parent: WeightedNode,
  children: readonly WeightedNode[],
): WeightedNode['search'] {
  if (!parent.search) return undefined;
  return {
    own: false,
    inherited: parent.search.own || parent.search.inherited,
    descendants: Math.max(
      0,
      parent.search.descendants -
        children.reduce(
          (sum, child) =>
            sum + (child.search ? Number(child.search.own) + child.search.descendants : 0),
          0,
        ),
    ),
  };
}

export function searchHighlightIds(nodes: readonly WeightedNode[]): Set<number> {
  return new Set(
    nodes
      .filter(
        (node) =>
          node.search && (node.search.own || node.search.inherited || node.search.descendants > 0),
      )
      .map((node) => node.id),
  );
}
