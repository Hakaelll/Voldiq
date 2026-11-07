import type { TrayOrigin } from './cleanup-tray';
export interface RestorationPreview {
  id: number;
  path: string;
  size: number;
}
export interface RestorationResult {
  path: string;
  recorded: boolean;
  journal_error: string | null;
}
export type NodeId = number;
export interface CleanupReport {
  items: { id: NodeId; path: string; removed: boolean; error: string | null; recorded: boolean }[];
  cancelled: boolean;
  journal_error: string | null;
}
export interface RecyclingReport {
  items: {
    id: NodeId;
    path: string;
    outcome: 'not_started' | 'unmoved' | 'returned' | 'recycled' | 'unknown';
    recorded: boolean;
    operation: number | null;
    bin_path: number[] | null;
    error: string | null;
    cleanup_error: string | null;
  }[];
  cancelled: boolean;
  journal_error: string | null;
  helper_error: string | null;
  preflight_error: string | null;
}
export interface CleanupPreview {
  id: number;
  origins: TrayOrigin[];
  preview: {
    rule: string;
    risk: string;
    policy: string;
    items: {
      id: NodeId;
      path: string;
      logical: number;
      reclaimable: number;
      hard_link: boolean;
      modified: number | null;
    }[];
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
  unverified_branches: number;
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
export type CloudState =
  | 'unknown'
  | 'not_placeholder'
  | 'file_no_recall_flag'
  | 'file_recall_required'
  | 'directory_no_recall_flag'
  | 'directory_recall_required';
export type CloudFilter =
  'placeholder' | 'recall_required' | 'no_recall_flag' | 'not_placeholder' | 'unknown';
export interface Entry {
  cloud: CloudState;
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
  sensitive: {
    kind:
      | 'credential_location'
      | 'credential_file'
      | 'key_material'
      | 'password_store'
      | 'private_application_state';
    reason: string;
  } | null;
}
export interface SelectedDetails extends Details {
  children: Page | null;
}
export interface Volume {
  path: string;
  name: string;
  filesystem: string;
  space: VolumeSpace;
  drive_type: string;
  accessible: boolean;
}
export interface VolumeIdentity {
  guid_root: string;
  serial: string;
}
export interface CallerSpace {
  volume: VolumeIdentity;
  observed_at: number | null;
  total: string;
  available: string;
}
export interface VolumeMeasurement {
  volume: VolumeIdentity;
  observed_at: number | null;
  total: string;
  free: string;
  used: string;
  caller_total: string;
  caller_available: string;
  reserved: string;
  storage_reserve: string;
  pool_unavailable: string;
}
export type VolumeSpace =
  | { kind: 'measured'; measurement: VolumeMeasurement }
  | {
      kind: 'unavailable';
      reason: 'unsupported' | 'metadata_unavailable' | 'native_failure' | 'invalid_counters';
      caller: CallerSpace | null;
    };
export interface Page {
  items: Entry[];
  total: number;
}
export interface FilePage extends Page {
  largest: number;
  paths: string[];
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
  cloud: CloudFilter | null;
  directory: Details;
  children: Page;
  types: FileTypes;
  branches: { parent: NodeId; items: Entry[] }[];
  map_metrics: {
    id: NodeId;
    logical: number;
    allocated: number;
    allocation_known: boolean;
    files: number;
    latest_modified: number | null;
    modified_known: number;
    modified_total: number;
    matches: { own: boolean; descendants: number } | null;
  }[];
}
export interface Issue {
  path: string;
  message: string;
}
export interface Progress {
  index_id: number | null;
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
  expression?: string;
  cloud?: CloudFilter;
  parent?: NodeId;
  scope?: NodeId;
  text?: string;
  extension?: string;
  without_extension?: boolean;
  min_size?: number;
  modified_before?: number;
  files_only?: boolean;
  kind?: Entry['kind'];
  sort?: 'size' | 'allocated' | 'name' | 'modified';
  offset?: number;
  limit?: number;
}

export type SearchInspection =
  { kind: 'valid'; conditions: number } | { kind: 'invalid'; offset: number; message: string };

export interface SearchTranslation {
  expression: string;
  conditions: number;
  unparsed: { start: number; end: number; text: string }[];
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
  unverified_branches: number;
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
  observation: {
    completed_at: number;
    complete: boolean;
    engine_version: string;
    accounting_version: number;
  } | null;
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
export interface FolderBudget {
  id: number;
  path: string;
  expression: string;
  limit_bytes: string;
  version: string;
}
export type ForecastReason =
  | 'not_requested'
  | 'no_budget'
  | 'unmeasured'
  | 'partial'
  | 'insufficient_history'
  | 'stale'
  | 'volatile'
  | 'not_growing'
  | 'beyond_horizon'
  | 'at_limit'
  | 'incompatible';
export interface BudgetReport {
  detailed: boolean;
  budget: FolderBudget | null;
  coverage: 'complete' | 'partial' | 'provisional';
  status: 'no_budget' | 'within' | 'at_limit' | 'exceeded' | 'unknown';
  logical_bytes: string;
  observed_percent: number | null;
  source_snapshot: number | null;
  observation: Snapshot['observation'];
  forecast:
    | { kind: 'unavailable'; reason: ForecastReason }
    | {
        kind: 'estimate';
        as_of: number;
        estimated_at: number;
        earliest_at: number;
        latest_at: number;
        bytes_per_day: number;
        samples: number;
        span_days: number;
      };
  image_equivalent: {
    sample_count: number;
    matched_images: number;
    mean_logical_bytes: string;
    approximate_images: string;
  } | null;
}
export interface SnapshotChange {
  path_id: string;
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
  coverage_complete: boolean;
  accounting: 'compatible' | 'unknown';
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

export interface SnapshotVisualEntry {
  path_id: string;
  name: string;
  kind: 'directory' | 'file' | 'link';
  logical: string;
  incomplete: boolean;
}
export interface SnapshotVisualFrame {
  snapshot: number;
  captured: number;
  observation: Snapshot['observation'];
  presence: 'present' | 'not_recorded' | 'not_observed';
  kind: SnapshotVisualEntry['kind'] | null;
  logical: string | null;
  incomplete: boolean;
  child_count: number;
  entries: SnapshotVisualEntry[];
  remainder: { logical: string; entries: number } | null;
}
export interface SnapshotVisualPair {
  path_id: string;
  parent_id: string | null;
  path: string;
  accounting: 'compatible' | 'unknown';
  before: SnapshotVisualFrame;
  after: SnapshotVisualFrame;
  delta: string | null;
  relative_percent: number | null;
  folders: {
    items: { path_id: string; name: string }[];
    previous: string | null;
    next: string | null;
  };
}

export interface ActivityCapture {
  id: number;
  captured: number;
  logical: string;
  observation: Snapshot['observation'];
  complete: boolean;
  accounting: 'supported' | 'unknown' | 'unsupported';
}
export interface CalendarTotals {
  files: number;
  logical: string;
}
export interface CalendarDay {
  day: number;
  date: string;
  totals: CalendarTotals;
}
export interface CalendarReport {
  capture: ActivityCapture;
  root: string;
  month: string;
  days: CalendarDay[];
  total: CalendarTotals;
  undated: CalendarTotals;
  outside: CalendarTotals;
}
export interface CalendarEntry {
  path_id: string;
  path: string;
  logical: string;
  modified: number;
  cloud: Entry['cloud'];
}
export interface CalendarPage {
  snapshot: number;
  date: string;
  offset: number;
  total: number;
  entries: CalendarEntry[];
  has_more: boolean;
}
export interface DiaryEvent {
  before: ActivityCapture | null;
  after: ActivityCapture;
  detail:
    | { kind: 'baseline' }
    | { kind: 'incompatible' }
    | { kind: 'compared'; delta: string; comparison: SnapshotComparison };
}
export interface DiaryPage {
  anchor: number;
  root: string;
  events: DiaryEvent[];
  has_more: boolean;
}

export interface SnapshotRetentionSettings {
  root_key: number;
  root: string;
  version: string;
  keep_latest: number | null;
  total: number;
}
export interface SnapshotRetentionPreview {
  settings: SnapshotRetentionSettings;
  revision: string;
  fingerprint: string;
  eligible: number;
  captures: { id: number; captured: number; logical: string; complete: boolean }[];
}
export interface SnapshotRetentionResult {
  removed: number;
  remaining_eligible: number;
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
