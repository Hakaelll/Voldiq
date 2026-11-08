export type SystemStorageCategory =
  'paging' | 'hibernation' | 'previous_windows' | 'component_store' | 'docker' | 'wsl';

export type SystemStorageObservation =
  | {
      kind: 'indexed';
      path: string;
      logical: string;
      allocated: string | null;
      incomplete: boolean;
    }
  | { kind: 'not_observed' | 'outside_scope' | 'location_unavailable' };

export type SystemStorageReport =
  | { kind: 'recorded_scan' }
  | {
      kind: 'live';
      root: string;
      indexed_at: number | null;
      scope_complete: boolean;
      docker_defaults_available: boolean;
      paging_registry: 'complete' | 'partial' | 'unavailable';
      wsl_registrations: 'complete' | 'partial' | 'unavailable';
      items: {
        category: SystemStorageCategory;
        label: string;
        observation: SystemStorageObservation;
      }[];
    };
