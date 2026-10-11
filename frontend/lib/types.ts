export type NodeId = number;
export interface Entry {
  id: NodeId;
  parent: NodeId | null;
  name: string;
  kind: 'directory' | 'file' | 'link';
  logical: number;
  allocated: number;
  allocation_known: boolean;
  modified: number | null;
  files: number;
  incomplete: boolean;
  hard_link: boolean;
}
export interface Details {
  entry: Entry;
  path: string;
  ancestors: Entry[];
  path_exact: boolean;
}
export interface Volume {
  path: string;
  name: string;
  filesystem: string;
  total: number;
  free: number;
  drive_type: string;
  accessible: boolean;
}
export interface Page {
  items: Entry[];
  total: number;
}
export interface FileType {
  extension: string;
  bytes: number;
  count: number;
}
export interface Issue {
  path: string;
  message: string;
}
export interface Progress {
  phase: 'scanning' | 'complete' | 'cancelled' | 'failed';
  entries: number;
  files: number;
  logical: number;
  elapsed_ms: number;
  errors: number;
  method: string;
}
export interface Query {
  parent?: NodeId;
  scope?: NodeId;
  text?: string;
  extension?: string;
  min_size?: number;
  modified_before?: number;
  files_only?: boolean;
  sort?: string;
  offset?: number;
  limit?: number;
}

export interface DuplicateProgress {
  phase: 'sampling' | 'hashing' | 'complete' | 'cancelled' | 'failed';
  examined: number;
  candidates: number;
  bytes_read: number;
  cache_hits: number;
  errors: number;
  elapsed_ms: number;
  message: string;
}
export interface DuplicateGroup {
  key: number;
  logical: number;
  recoverable: number;
  copies: number;
  digest: string;
}
export interface DuplicatePage {
  groups: DuplicateGroup[];
  total: number;
  recoverable: number;
  errors: number;
  cancelled: boolean;
  bytes_read: number;
  cache_hits: number;
}
export interface DuplicateItem {
  id: NodeId;
  name: string;
  path: string;
  logical: number;
  allocated: number;
  links: number;
}

export function bytes(value: number): string {
  if (value === 0) return '0 B';
  const unit = Math.min(4, Math.floor(Math.log(value) / Math.log(1024)));
  return `${(value / 1024 ** unit).toLocaleString('en', { maximumFractionDigits: unit > 0 ? 1 : 0 })} ${['B', 'KiB', 'MiB', 'GiB', 'TiB'][unit]}`;
}
export function date(value: number | null): string {
  return value === null ? 'Unavailable' : new Date(value * 1000).toLocaleString();
}
