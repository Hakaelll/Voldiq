export interface DiaryLocation {
  snapshot: number;
  offset: number;
}
export interface HistoricalComparison {
  before: number;
  after: number;
  scope: string | null;
  returnTo: DiaryLocation | null;
}
export type HistoryWorkspace =
  | { kind: 'captures' }
  | ({ kind: 'comparison' } & HistoricalComparison)
  | { kind: 'playback' }
  | { kind: 'calendar' }
  | { kind: 'diary'; location: DiaryLocation };

export function utcDate(value: number | null): string {
  if (value === null) return 'Unavailable';
  const date = new Date(value * 1000);
  return Number.isFinite(date.getTime())
    ? date.toLocaleString(undefined, { timeZone: 'UTC' }) + ' UTC'
    : 'Date outside supported range';
}
export function validMonth(value: string): boolean {
  return /^\d{4}-(0[1-9]|1[0-2])$/.test(value) && Number(value.slice(0, 4)) >= 1970;
}
export function shiftMonth(value: string, step: number): string {
  if (!validMonth(value)) return value;
  const next = new Date(
    Date.UTC(Number(value.slice(0, 4)), Number(value.slice(5, 7)) - 1 + step, 1),
  )
    .toISOString()
    .slice(0, 7);
  return validMonth(next) ? next : value;
}
