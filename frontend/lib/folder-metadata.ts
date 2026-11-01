export interface FolderNote {
  id: number | null;
  path: string;
  note: string;
  protected: boolean;
  version: string;
  updated: number | null;
}

export interface FolderMetadata {
  folder: FolderNote;
  protected_by_parent: boolean;
}

export interface ProtectedPage {
  items: FolderNote[];
  next_offset: number | null;
}

export interface FolderRead {
  source_snapshot: number | null;
  node_id: number;
  path: string;
  protected_from_cleanup: boolean;
  note: { kind: 'not_requested' } | { kind: 'plain_text'; text: string; updated: number | null };
}
