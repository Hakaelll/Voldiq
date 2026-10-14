export type NodeId = number;
export interface CleanupReport {
  items: { id: NodeId; path: string; removed: boolean; error: string | null; recorded: boolean }[];
  cancelled: boolean;
  journal_error: string | null;
}
export interface CleanupPreview {
  id: number;
  preview: {
    rule: string;
    risk: string;
    policy: string;
    items: { id: NodeId; path: string; logical: number; reclaimable: number; hard_link: boolean }[];
    logical: number;
    reclaimable: number;
    retained: string[];
  };
}

export interface CleanupRulePage {
  rule: {
    id: string;
    name: string;
    description: string;
    discovery: string;
    selection: string;
    exclusions: string;
    risk: string;
    permissions: string;
    policy: string;
    verification: string;
  };
  root: string;
  indexed: boolean;
  complete: boolean;
  items: CleanupPreview['preview']['items'];
  total: number;
  logical: number;
  reclaimable: number;
}
export interface DeveloperItem {
  id: NodeId;
  path: string;
  tool: string;
  category: 'dependencies' | 'build' | 'cache';
  logical: number;
  allocated: number;
  files: number;
  incomplete: boolean;
  allocation_known: boolean;
}
export interface DeveloperPage {
  items: DeveloperItem[];
  total: number;
  logical: number;
  allocated: number;
  categories: { category: DeveloperItem['category']; directories: number; logical: number }[];
}
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
export interface SelectedDetails extends Details {
  children: Page | null;
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
  extension: string | null;
  bytes: number;
  count: number;
}
export interface FileTypes {
  items: FileType[];
  total_bytes: number;
  total_files: number;
}
export interface Overview {
  directory: Details;
  children: Page;
  types: FileTypes;
}
export interface Issue {
  path: string;
  message: string;
}
export interface Progress {
  analyzed_at: number | null;
  previous: boolean;
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
  without_extension?: boolean;
  min_size?: number;
  modified_before?: number;
  files_only?: boolean;
  kind?: Entry['kind'];
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
  name: string;
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
  modified: number | null;
}

export interface Snapshot {
  id: number;
  root_key: number;
  root: string;
  captured: number;
  logical: number;
  allocated: number;
  files: number;
  entries: number;
  complete: boolean;
  errors: number;
}
export interface SnapshotChange {
  path: string;
  kind: 'added' | 'removed' | 'changed' | 'unobserved' | 'newly_observed';
  directory: boolean;
  before: number;
  after: number;
  delta: number;
}
export interface SnapshotComparison {
  before: number;
  after: number;
  delta: number;
  reliable: boolean;
  total: number;
  changes: SnapshotChange[];
  file_changes: {
    added: { files: number; logical: number };
    changed: { files: number; delta: number };
    removed: { files: number; logical: number };
    newly_observed: { files: number; logical: number };
    unobserved: { files: number; logical: number };
  };
}

export function difference(value: number): string {
  return `${value > 0 ? '+' : value < 0 ? '−' : ''}${bytes(Math.abs(value))}`;
}

export function bytes(value: number): string {
  if (value === 0) return '0 B';
  const unit = Math.min(4, Math.floor(Math.log(value) / Math.log(1024)));
  return `${(value / 1024 ** unit).toLocaleString('en', { maximumFractionDigits: unit > 0 ? 1 : 0 })} ${['B', 'KiB', 'MiB', 'GiB', 'TiB'][unit]}`;
}
export function date(value: number | null): string {
  return value === null ? 'Unavailable' : new Date(value * 1000).toLocaleString();
}
