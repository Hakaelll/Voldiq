import type { NodeId } from './types';

export type MediaCategory =
  'source_images' | 'derivatives' | 'previews' | 'proxies' | 'catalog' | 'edit_profiles';

export type MediaLibraryReport =
  | { kind: 'recorded_scan' | 'unsupported' }
  | {
      kind: 'available';
      total: number;
      shared_bytes: string;
      unattributed_bytes: string;
      omitted_signatures: number;
      coverage_partial: boolean;
      manual: ({ id: NodeId; logical_bytes: string; partial: boolean } | null)[];
      items: {
        anchor: NodeId;
        folder: NodeId;
        provider: 'lightroom_classic' | 'darktable' | 'raw_therapee';
        name: string;
        path: string;
        sizes: { category: MediaCategory; logical_bytes: string }[];
        partial: boolean;
      }[];
    };
