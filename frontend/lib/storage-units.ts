import { createSubscriber } from 'svelte/reactivity';

export type StorageUnits = 'binary' | 'decimal';

const labels = {
  binary: ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB', 'EiB', 'ZiB', 'YiB'],
  decimal: ['B', 'KB', 'MB', 'GB', 'TB', 'PB', 'EB', 'ZB', 'YB'],
};
const integer = new Intl.NumberFormat('en');
let preference: StorageUnits = 'binary';
let changed: (() => void) | undefined;
const subscribe = createSubscriber((update) => {
  changed = update;
  return () => {
    changed = undefined;
  };
});

export function readStorageUnits(value: unknown): StorageUnits {
  return value === 'decimal' ? 'decimal' : 'binary';
}

export function currentStorageUnits(): StorageUnits {
  subscribe();
  return preference;
}

export function setStorageUnits(value: StorageUnits): void {
  if (preference === value) return;
  preference = value;
  changed?.();
}

export function formatStorageBytes(
  value: string | number | bigint,
  units: StorageUnits = currentStorageUnits(),
): string {
  if (typeof value === 'number' && (!Number.isFinite(value) || value < 0)) return 'Unknown';
  if (typeof value === 'string' && !/^\d+$/.test(value)) return 'Unknown';
  const bytes = BigInt(typeof value === 'number' ? Math.round(value) : value);
  if (bytes < 0n) return 'Unknown';

  const base = units === 'decimal' ? 1000n : 1024n;
  const symbols = labels[units];
  let unit = 0;
  let scale = 1n;
  while (bytes >= scale * base && unit < symbols.length - 1) {
    unit++;
    scale *= base;
  }
  if (unit === 0) return integer.format(bytes) + ' B';
  let tenths = (bytes * 10n + scale / 2n) / scale;
  if (tenths === base * 10n && unit < symbols.length - 1) {
    unit++;
    tenths = 10n;
  }
  const fraction = tenths % 10n;
  return integer.format(tenths / 10n) + (fraction ? '.' + fraction : '') + ' ' + symbols[unit];
}
