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
