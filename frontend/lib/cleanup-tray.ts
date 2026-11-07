import type { ApplicationCleanupContext } from './applications';
import type { NodeId } from './types';

export type TraySource =
  | { kind: 'manual' | 'duplicates' | 'user_temp' | 'old_files' | 'project_temp' }
  | { kind: 'browser_cache'; application: ApplicationCleanupContext | null };

export interface TrayGroup {
  source: TraySource;
  selection: readonly NodeId[];
}

export type TrayMetric = 'logical' | 'allocated';
export interface TrayTarget {
  metric: TrayMetric;
  expression: string;
}

export interface TrayOrigin {
  id: string;
  label: string;
  description: string;
  risk: string;
}

export interface TrayItem {
  id: NodeId;
  parent: NodeId;
  name: string;
  folder: string;
  logical: string;
  allocated: string | null;
  hard_link: boolean;
}

export interface TraySummary {
  root: string;
  indexed_at: number | null;
  incomplete: boolean;
  root_logical: string;
  selected_logical: string;
  remaining_logical: string | null;
  known_allocated: string;
  allocation_complete: boolean;
  hard_link_paths: number;
  repeated_identity_paths: number;
  groups: { origin: TrayOrigin; items: TrayItem[] }[];
  target:
    | null
    | { kind: 'invalid'; message: string }
    | {
        kind: 'measured';
        metric: TrayMetric;
        bytes: string;
        selected_bytes: string;
        complete: boolean;
        reached: boolean;
      };
}

function sourceKey(source: TraySource): string {
  if (source.kind === 'browser_cache' && source.application) {
    return `browser_cache:${source.application.scope}:${source.application.application_id}`;
  }
  return source.kind;
}

type AddResult = { groups: readonly TrayGroup[] } | { problem: string };

export function addToTray(
  groups: readonly TrayGroup[],
  source: TraySource,
  ids: readonly NodeId[],
): AddResult {
  const key = sourceKey(source);
  const requested = new Set(ids);
  const matching = groups.find((group) => sourceKey(group.source) === key);
  if (
    groups.some((group) => group !== matching && group.selection.some((id) => requested.has(id)))
  ) {
    return {
      problem:
        'A file is already in the tray under another source. Remove it there to change its source.',
    };
  }
  const selection = [...new Set([...(matching?.selection ?? []), ...requested])];
  if (selection.length === (matching?.selection.length ?? 0)) return { groups };
  if (!matching && groups.length >= 16) return { problem: 'Review up to 16 source groups.' };
  const total = groups.reduce((sum, group) => sum + group.selection.length, 0);
  if (total + selection.length - (matching?.selection.length ?? 0) > 500) {
    return { problem: 'Review up to 500 files in total.' };
  }
  const replacement = { source, selection };
  return {
    groups: matching
      ? groups.map((group) => (group === matching ? replacement : group))
      : [...groups, replacement],
  };
}

export function trayFolders(items: readonly TrayItem[]) {
  const folders = new Map<NodeId, { path: string; items: TrayItem[] }>();
  for (const item of items) {
    const folder = folders.get(item.parent);
    if (folder) folder.items.push(item);
    else folders.set(item.parent, { path: item.folder, items: [item] });
  }
  return [...folders.values()];
}

export function trayPercent(value: string, maximum: string): number {
  const max = BigInt(maximum);
  return max === 0n ? 0 : Math.min(100, Number((BigInt(value) * 10000n) / max) / 100);
}
