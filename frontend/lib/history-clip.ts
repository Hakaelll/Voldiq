import type { SnapshotVisualFrame } from './types.ts';

export interface HistoryClipRequest {
  snapshot_ids: number[];
  include_paths: boolean;
}
export interface HistoryClip {
  schema_version: number;
  root_key: number;
  scope_fingerprint: string;
  root_path: { path: string; exact: boolean } | null;
  exclusions: { path: string; exact: boolean }[] | null;
  exclusion_count: number;
  paths_included: boolean;
  units: string;
  selection: string;
  accounting: 'compatible' | 'unknown';
  maximum_logical: string;
  partial_frames: number;
  legacy_frames: number;
  limitations: string;
  frames: SnapshotVisualFrame[];
  fingerprint: string;
}
export interface HistoryGifRequest {
  clip: HistoryClipRequest;
  fingerprint: string;
  width: number;
  height: number;
  delay_centiseconds: 50 | 100 | 200;
  looped: boolean;
}
export interface GifProgress {
  stage: 'preparing' | 'encoding' | 'revalidating' | 'finalizing';
  completed_frames: number;
  total_frames: number;
}
export type HistoryWebmRequest = Omit<HistoryGifRequest, 'looped'>;
export type HistoryExportProgress =
  | GifProgress
  | {
      stage: 'validating';
      inspected_bytes: number;
      total_bytes: number;
    };
