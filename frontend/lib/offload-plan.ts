type BasicMetadata = {
  native_identity: string;
  created_windows_100ns: string | null;
  modified_windows_100ns: string;
  dos_attributes: number;
};

type OffloadPlan = {
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
    files: { id: number; metadata: BasicMetadata }[];
    limitations: string;
  };
};

export type PlanAction =
  | 'recycle'
  | { offload: OffloadPlan }
  | {
      restore: {
        transfer_plan_id: string;
        transfer_manifest_hash: string;
        required_available_bytes: string;
        limitations: string;
        groups: OffloadPlan[];
        files: {
          source_id: number;
          original_id: number;
          original_path: string;
          archive_identity: string;
          content_blake3: string;
          bytes: string;
          original_metadata: BasicMetadata;
        }[];
      };
    };
