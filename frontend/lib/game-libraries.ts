import type { NodeId } from './types';

export type GameLibraryReport =
  | { kind: 'recorded_scan' | 'unsupported' }
  | {
      kind: 'available';
      total: number;
      recorded: boolean;
      shared_bytes: string;
      unattributed_bytes: string;
      omitted_manifests: number;
      coverage_partial: boolean;
      manual: ({ id: NodeId; logical_bytes: string; partial: boolean } | null)[];
      items: {
        id: NodeId;
        provider: 'steam' | 'epic' | 'gog' | 'itch';
        product: string;
        title: string;
        path: string;
        installation_bytes: string | null;
        shaders_bytes: string | null;
        workshop_bytes: string | null;
        user_data_bytes: string | null;
        dlc_bytes: string | null;
        dlc_records: number;
        partial: boolean;
      }[];
    };
