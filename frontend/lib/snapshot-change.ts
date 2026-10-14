import type { SnapshotComparison } from './types.ts';

export function snapshotChangeBars(changes: SnapshotComparison['file_changes']) {
  const rows = [
    { kind: 'added', label: 'Added', delta: changes.added.logical, files: changes.added.files },
    {
      kind: 'changed',
      label: 'Changed',
      delta: changes.changed.delta,
      files: changes.changed.files,
    },
    {
      kind: 'removed',
      label: 'Removed',
      delta: -changes.removed.logical,
      files: changes.removed.files,
    },
  ];
  const maximum = Math.max(...rows.map((row) => Math.abs(row.delta)));
  return rows.map((row) => {
    const width = maximum ? (Math.abs(row.delta) / maximum) * 50 : 0;
    return { ...row, width, start: row.delta < 0 ? 50 - width : 50 };
  });
}
