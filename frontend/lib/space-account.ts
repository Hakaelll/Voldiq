import type { Snapshot, VolumeSpace } from './types';
export type IndexedAllocation =
  | {
      kind: 'measured';
      known: string;
      complete: boolean;
      known_file_records: number;
      unknown_file_records: number;
    }
  | {
      kind: 'unavailable';
      reason:
        | 'accounting_unknown'
        | 'accounting_unsupported'
        | 'identity_unknown'
        | 'no_evidence'
        | 'inconsistent';
    };
export interface IndexedSpace {
  root: string;
  snapshot: number | null;
  observation: Snapshot['observation'];
  coverage: 'complete' | 'partial' | 'unknown';
  accounting: 'known' | 'unknown' | 'unsupported';
  exclusions: number;
  starts_at_volume_root: boolean;
  volume_serial: string | null;
  logical_paths: string;
  recorded_allocation: string;
  allocation: IndexedAllocation;
  file_paths: number;
  errors: number;
  hardlink_paths: number;
  cloud_file_paths: number;
  cloud_logical_paths: string;
  recall_required_file_paths: number;
  opaque_reparse_records: number;
}
export type SpaceVolume =
  | { kind: 'scoped_only' }
  | { kind: 'root_unavailable' }
  | { kind: 'observed'; observation: VolumeSpace };
export type SpaceResidual =
  | { kind: 'compared'; bytes: string; against_complete_scope: boolean }
  | {
      kind: 'unavailable';
      reason:
        | 'scoped_only'
        | 'root_unavailable'
        | 'volume_unavailable'
        | 'allocation_unavailable'
        | 'volume_mismatch';
    };
export interface SpaceReport {
  indexed: IndexedSpace;
  volume: SpaceVolume;
  residual: SpaceResidual;
}
export function spaceMeasurement(report: SpaceReport | null) {
  return report?.volume.kind === 'observed' && report.volume.observation.kind === 'measured'
    ? report.volume.observation.measurement
    : null;
}
export function spaceCaller(report: SpaceReport | null) {
  if (report?.volume.kind !== 'observed') return null;
  const observation = report.volume.observation;
  return observation.kind === 'measured'
    ? {
        available: observation.measurement.caller_available,
        observed_at: observation.measurement.observed_at,
      }
    : observation.caller;
}
