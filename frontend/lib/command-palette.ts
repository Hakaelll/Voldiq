import type { Destination, RecordRef, Workspaces } from './workbench.ts';
import { STORAGE_VIEWS, type StorageView } from './storage-map.ts';

export type PaletteAction =
  | { kind: 'navigate'; view: Destination }
  | { kind: 'chart'; view: StorageView }
  | { kind: 'search' }
  | { kind: 'choose' }
  | { kind: 'saved' }
  | { kind: 'help' }
  | { kind: 'load'; reference: RecordRef }
  | { kind: 'scan'; reference: RecordRef; path: string };
export interface PaletteCommand {
  id: string;
  label: string;
  detail: string;
  group: PaletteGroup;
  icon: string;
  keywords?: string;
  disabled: boolean;
  action: PaletteAction;
}

export const paletteGroups = {
  actions: 'Actions',
  views: 'Go to',
  charts: 'Storage charts',
  saved: 'Saved views',
  favorites: 'Favorite folders',
};

type PaletteGroup = keyof typeof paletteGroups;

export const destinations: { id: Destination; label: string; icon: string; keywords?: string }[] = [
  { id: 'overview', label: 'Overview', icon: 'overview', keywords: 'dashboard drive space' },
  { id: 'explorer', label: 'Explorer', icon: 'folder', keywords: 'browse grid list' },
  { id: 'large', label: 'Large files', icon: 'large', keywords: 'largest folders size' },
  { id: 'duplicates', label: 'Duplicates', icon: 'duplicate', keywords: 'similar images copies' },
  { id: 'cleanup', label: 'Cleanup', icon: 'trash', keywords: 'recycle review tray' },
  { id: 'developer', label: 'Developer storage', icon: 'code', keywords: 'caches projects git' },
  { id: 'history', label: 'History', icon: 'history', keywords: 'snapshots trends compare' },
  {
    id: 'settings',
    label: 'Settings',
    icon: 'settings',
    keywords: 'preferences theme startup ajustes',
  },
];

export function paletteCommands(
  catalog: Workspaces | null,
  ready: boolean,
  scanDisabled: boolean,
): PaletteCommand[] {
  const commands: PaletteCommand[] = [
    {
      id: 'search',
      label: 'Search current analysis',
      detail: ready ? 'At the scan root' : 'Run a scan first',
      group: 'actions',
      icon: 'search',
      keywords: 'find files analysis root',
      disabled: !ready,
      action: { kind: 'search' },
    },
    {
      id: 'saved',
      label: 'Saved views and favorites',
      detail: 'Manage workspaces',
      group: 'actions',
      icon: 'history',
      disabled: false,
      action: { kind: 'saved' },
    },
    {
      id: 'choose',
      label: 'Choose a folder to scan',
      detail: scanDisabled ? 'Unavailable right now' : '',
      group: 'actions',
      icon: 'folder',
      disabled: scanDisabled,
      action: { kind: 'choose' },
    },
    {
      id: 'help',
      label: 'Help & shortcuts',
      detail: 'F1',
      group: 'actions',
      icon: 'help',
      keywords: 'getting started guide keyboard ayuda atajos',
      disabled: false,
      action: { kind: 'help' },
    },
    ...destinations.map(({ id: view, label, icon, keywords }): PaletteCommand => ({
      id: `navigate:${view}`,
      label,
      detail: '',
      group: 'views',
      icon,
      keywords,
      disabled: false,
      action: { kind: 'navigate', view },
    })),
  ];
  if (ready)
    for (const view of STORAGE_VIEWS)
      commands.push({
        id: `chart:${view.id}`,
        label: view.label,
        detail: 'Expanded chart',
        group: 'charts',
        icon: 'overview',
        keywords: 'map chart visualization gráfica',
        disabled: false,
        action: { kind: 'chart', view: view.id },
      });
  if (catalog) {
    for (const view of catalog.views)
      commands.push({
        id: `view:${view.id}`,
        label: view.name,
        detail: ready
          ? `${view.spec.representation} · ${view.spec.scope === 'analysis_root' ? 'Whole analysis' : 'Current folder'}`
          : 'Run a scan first',
        group: 'saved',
        icon: 'history',
        disabled: !ready,
        action: { kind: 'load', reference: { id: view.id, revision: catalog.revision } },
      });
    for (const favorite of catalog.favorites)
      commands.push({
        id: `favorite:${favorite.id}`,
        label: `Scan ${favorite.name}`,
        detail: favorite.path,
        group: 'favorites',
        icon: 'folder',
        disabled: scanDisabled,
        action: {
          kind: 'scan',
          reference: { id: favorite.id, revision: catalog.revision },
          path: favorite.path,
        },
      });
  }
  return commands;
}

function normalized(text: string): string {
  return text.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase().trim().replace(/\s+/g, ' ');
}

export function filterCommands(
  commands: readonly PaletteCommand[],
  text: string,
): PaletteCommand[] {
  const query = normalized(text);
  if (!query) return [...commands];
  const words = query.split(' ');
  return commands
    .flatMap((command) => {
      const label = normalized(command.label);
      const searchable = `${label} ${normalized(command.detail)} ${normalized(command.keywords ?? '')}`;
      if (!words.every((word) => searchable.includes(word))) return [];
      let score = 0;
      if (label === query) score = 3;
      else if (label.startsWith(query)) score = 2;
      else if (words.every((word) => label.includes(word))) score = 1;
      return [{ command, score }];
    })
    .sort((left, right) => right.score - left.score)
    .map(({ command }) => command);
}

export function groupCommands(
  commands: readonly PaletteCommand[],
): { group: PaletteGroup; commands: PaletteCommand[] }[] {
  const groups = new Map<PaletteGroup, PaletteCommand[]>();
  for (const command of commands) {
    const group = groups.get(command.group);
    if (group) group.push(command);
    else groups.set(command.group, [command]);
  }
  return Array.from(groups, ([group, commands]) => ({ group, commands }));
}
export function paletteFocusIndex(
  key: 'ArrowUp' | 'ArrowDown',
  current: number,
  commands: readonly PaletteCommand[],
): number | null {
  const enabled = commands.flatMap((command, i) => (command.disabled ? [] : [i]));
  if (!enabled.length) return null;
  if (key === 'ArrowDown') return enabled.find((index) => index > current) ?? enabled[0];
  const reversed = enabled.reverse();
  return reversed.find((index) => index < current) ?? reversed[0];
}
