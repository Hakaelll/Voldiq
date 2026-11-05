export type PlanAction =
  | 'recycle'
  | {
      offload: {
        source_volume: { guid_root: string; serial: string };
        destination: {
          path: string;
          volume: { guid_root: string; serial: string };
          filesystem: string;
          caller_available_bytes: string;
          observed_at: number;
        };
        originals: 'keep' | 'recycle_after_verification';
        outputs: { id: number; name: string }[];
        required_available_bytes: string;
        reserve_bytes: string;
        execution?: { maximum_seconds: number };
        metadata?: {
          profile: 'ntfs_basic_v1';
          files: {
            id: number;
            metadata: {
              native_identity: string;
              created_windows_100ns: string | null;
              modified_windows_100ns: string;
              dos_attributes: number;
            };
          }[];
          limitations: string;
        };
      };
    };
