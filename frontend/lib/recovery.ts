import type { VolumeIdentity } from './types';

export interface ShadowCopy {
  id: string;
  provider_id: string | null;
  installed_at: number | null;
  previous_versions: boolean | null;
  persistent: boolean | null;
}
export type RecoveryDiscovery =
  | {
      kind: 'available';
      copies: ShadowCopy[];
      coverage: 'complete' | 'item_limit' | 'time_limit';
    }
  | {
      kind: 'unavailable';
      reason:
        | 'unsupported'
        | 'root_unavailable'
        | 'access_denied'
        | 'provider_unavailable'
        | 'native_failure'
        | 'invalid_metadata';
    };
export interface RecoveryReport {
  root: string;
  volume: VolumeIdentity | null;
  observed_at: number | null;
  copy_limit: number;
  enumeration_window_ms: number;
  shadows: RecoveryDiscovery;
}
