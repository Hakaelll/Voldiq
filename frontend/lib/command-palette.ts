import type { Destination, RecordRef, Workspaces } from './workbench.ts';

export type PaletteAction =
  | { kind: 'navigate'; view: Destination }
  | { kind: 'search' }
  | { kind: 'choose' }
  | { kind: 'saved' }
  | { kind: 'load'; reference: RecordRef }
  | { kind: 'scan'; reference: RecordRef; path: string };
export interface PaletteCommand {
  id: string;
  label: string;
  detail: string;
  disabled: boolean;
  action: PaletteAction;
}
export function paletteCommands(
  catalog: Workspaces | null,
  ready: boolean,
  scanDisabled: boolean,
): PaletteCommand[] {
  const destinations: { view: Destination; label: string }[] = [
    { view: 'overview', label: 'Overview' },
    { view: 'explorer', label: 'Explorer' },
    { view: 'large', label: 'Large files' },
    { view: 'duplicates', label: 'Duplicates' },
    { view: 'cleanup', label: 'Cleanup' },
    { view: 'developer', label: 'Developer storage' },
    { view: 'history', label: 'History' },
    { view: 'settings', label: 'Settings' },
  ];
  const commands: PaletteCommand[] = [
    {
      id: 'search',
      label: 'Search current analysis',
      detail: 'Open the file search at the analysis root',
      disabled: !ready,
      action: { kind: 'search' },
    },
    {
      id: 'saved',
      label: 'Saved views and favorites',
      detail: 'Manage saved workspaces',
      disabled: false,
      action: { kind: 'saved' },
    },
    {
      id: 'choose',
      label: 'Choose a folder to scan',
      detail: 'Open the folder picker',
      disabled: scanDisabled,
      action: { kind: 'choose' },
    },
    ...destinations.map(({ view, label }) => ({
      id: `navigate:${view}`,
      label,
      detail: 'Go to page',
      disabled: false,
      action: { kind: 'navigate', view } satisfies PaletteAction,
    })),
  ];
  if (catalog) {
    for (const view of catalog.views)
      commands.push({
        id: `view:${view.id}`,
        label: view.name,
        detail: `Saved view · ${view.spec.representation} · ${view.spec.scope === 'analysis_root' ? 'whole analysis' : 'current folder'}`,
        disabled: !ready,
        action: { kind: 'load', reference: { id: view.id, revision: catalog.revision } },
      });
    for (const favorite of catalog.favorites)
      commands.push({
        id: `favorite:${favorite.id}`,
        label: `Scan ${favorite.name}`,
        detail: favorite.path,
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
export function filterCommands(
  commands: readonly PaletteCommand[],
  text: string,
): PaletteCommand[] {
  const words = text.trim().toLowerCase().split(/\s+/).filter(Boolean);
  return commands.filter((command) =>
    words.every((word) => `${command.label} ${command.detail}`.toLowerCase().includes(word)),
  );
}
export function paletteFocusIndex(
  key: string,
  current: number,
  commands: readonly PaletteCommand[],
): number | null {
  const enabled = commands.flatMap((command, i) => (command.disabled ? [] : [i]));
  if (!enabled.length) return null;
  if (key === 'Home') return enabled[0];
  if (key === 'End') return enabled[enabled.length - 1];
  if (key === 'ArrowDown') return enabled.find((index) => index > current) ?? enabled[0];
  if (key === 'ArrowUp') {
    for (let i = enabled.length - 1; i >= 0; i--) if (enabled[i] < current) return enabled[i];
    return enabled[enabled.length - 1];
  }
  return null;
}
