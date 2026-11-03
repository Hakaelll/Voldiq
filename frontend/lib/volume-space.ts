import type { Volume } from './types';
import { utcDate } from './history-activity';

export function volumeMeasurement(volume: Volume | null | undefined) {
  return volume?.space.kind === 'measured' ? volume.space.measurement : null;
}
export function volumeCaller(volume: Volume | null | undefined) {
  if (!volume) return null;
  return volume.space.kind === 'measured'
    ? {
        available: volume.space.measurement.caller_available,
        observed_at: volume.space.measurement.observed_at,
      }
    : volume.space.caller;
}
export function volumeObservationLabel(volume: Volume | null | undefined): string {
  const measurement = volumeMeasurement(volume);
  const observedAt = measurement?.observed_at ?? volumeCaller(volume)?.observed_at ?? null;
  if (observedAt === null) return 'Measurement date unavailable';
  return `${measurement ? 'Volume' : 'Availability'} measured ${utcDate(observedAt)}`;
}
