import type { Query, CleanupRulePage } from './types';
import { detachedQuery } from './workbench.ts';
import { readReviewPriority } from './review-priority.ts';

export type ApplicationSource =
  'current_user64' | 'current_user32' | 'machine64' | 'machine32' | 'supplied_metadata';
export interface ApplicationFootprint {
  logical_bytes: string;
  files: number;
}
export type ApplicationCategory = 'installation' | 'data' | 'cache' | 'logs' | 'unknown';
export interface ApplicationCategoryMeasurement {
  category: ApplicationCategory;
  exclusive: ApplicationFootprint;
  overlapping: ApplicationFootprint;
}
export interface ApplicationItem {
  id: string;
  name: string;
  publisher: string | null;
  version: string | null;
  root: number;
  profile_scope: number | null;
  sources: ApplicationSource[];
  evidence: 'supplied_claim' | 'registered_location_claim' | 'registered_default_profile_claim';
  confidence: 'contextual' | 'supplied' | 'ambiguous';
  exclusive: ApplicationFootprint;
  overlapping: ApplicationFootprint;
  breakdown: ApplicationCategoryMeasurement[];
}
export interface ApplicationCoverage {
  source: ApplicationSource;
  complete: boolean;
  problem: string | null;
  visited: number;
  missing_location: number;
  rejected: number;
}
export interface ApplicationPage {
  attribution: 'current' | 'indexed' | 'captured';
  scope: number;
  indexed_at: number | null;
  registrations_observed_at: number;
  registrations_started_at: number;
  excluded_roots: number;
  partial_index: boolean;
  coverage: ApplicationCoverage[];
  unbound_records: number;
  duplicate_records: number;
  exclusive: ApplicationFootprint;
  overlapping: ApplicationFootprint;
  unattributed: ApplicationFootprint;
  total: ApplicationFootprint;
  applications: ApplicationItem[];
  application_count: number;
  chart: ApplicationItem[];
  chart_omitted: number;
}
export interface ApplicationQuery {
  scope: number;
  offset: number;
  limit: number;
  sort: 'size' | 'name' | 'files';
  filters: Query;
}
export interface ApplicationCleanupContext {
  scope: number;
  application_id: string;
}
export interface ApplicationCachePage {
  application: ApplicationItem;
  candidates: CleanupRulePage;
}
export function canReviewApplicationCache(app: ApplicationItem): boolean {
  return (
    app.profile_scope !== null &&
    app.confidence !== 'ambiguous' &&
    app.overlapping.files === 0 &&
    app.breakdown.some((part) => part.category === 'cache' && part.exclusive.files > 0)
  );
}
export function readApplicationCachePage(value: unknown): ApplicationCachePage {
  const response = record(value),
    page = record(response.candidates),
    rule = record(page.rule);
  if (rule.id !== 'old-browser-http-cache-files' || rule.policy !== 'recycle') return invalid();
  const items = list(page.items, 100, (value) => {
    const item = record(value);
    return {
      id: integer(item.id, 0xffffffff),
      path: text(item.path),
      logical: integer(item.logical),
      reclaimable: integer(item.reclaimable),
      hard_link: boolean(item.hard_link),
      modified: item.modified === null ? null : integer(item.modified),
      priority: readReviewPriority(item.priority),
    };
  });
  const total = integer(page.total),
    logical = integer(page.logical),
    reclaimable = integer(page.reclaimable);
  if (
    items.length > total ||
    new Set(items.map((item) => item.id)).size !== items.length ||
    items.reduce((sum, item) => sum + item.logical, 0) > logical ||
    items.reduce((sum, item) => sum + item.reclaimable, 0) > reclaimable
  )
    return invalid();
  return {
    application: application(response.application),
    candidates: {
      rule: {
        id: rule.id,
        policy: rule.policy,
        name: text(rule.name),
        description: text(rule.description),
        discovery: text(rule.discovery),
        selection: text(rule.selection),
        exclusions: text(rule.exclusions),
        risk: text(rule.risk),
        permissions: text(rule.permissions),
        verification: text(rule.verification),
      },
      root: text(page.root),
      indexed: boolean(page.indexed),
      complete: boolean(page.complete),
      unverified_branches: integer(page.unverified_branches),
      assessed_at: integer(page.assessed_at),
      items,
      total,
      logical,
      reclaimable,
    },
  };
}
export function applicationFilters(query: Query): Query {
  return { ...detachedQuery(query), sort: 'size' };
}
function invalid(): never {
  throw Error('Invalid application measurement response.');
}
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
function record(value: unknown): Record<string, unknown> {
  return isRecord(value) ? value : invalid();
}
function text(value: unknown): string {
  return typeof value === 'string' &&
    value.length > 0 &&
    value.length <= 1024 &&
    !/[\x00-\x1f\x7f]/.test(value)
    ? value
    : invalid();
}
function nullableText(value: unknown): string | null {
  return value === null ? null : text(value);
}
function integer(value: unknown, max = Number.MAX_SAFE_INTEGER): number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 && value <= max
    ? value
    : invalid();
}
function boolean(value: unknown): boolean {
  return typeof value === 'boolean' ? value : invalid();
}
function list<T>(value: unknown, max: number, read: (item: unknown) => T): T[] {
  return Array.isArray(value) && value.length <= max ? value.map(read) : invalid();
}
function source(value: unknown): ApplicationSource {
  switch (value) {
    case 'current_user64':
    case 'current_user32':
    case 'machine64':
    case 'machine32':
    case 'supplied_metadata':
      return value;
    default:
      return invalid();
  }
}
function footprint(value: unknown): ApplicationFootprint {
  const item = record(value),
    raw = item.logical_bytes;
  if (
    typeof raw !== 'string' ||
    !/^(0|[1-9][0-9]{0,38})$/.test(raw) ||
    BigInt(raw) > (1n << 128n) - 1n
  )
    return invalid();
  return { logical_bytes: raw, files: integer(item.files) };
}
function application(value: unknown): ApplicationItem {
  const item = record(value),
    id = text(item.id),
    evidence = item.evidence;
  if (
    !/^[0-9a-f]{64}$/.test(id) ||
    (evidence !== 'supplied_claim' &&
      evidence !== 'registered_location_claim' &&
      evidence !== 'registered_default_profile_claim')
  )
    return invalid();
  const sources = list(item.sources, 5, source);
  const confidence = item.confidence;
  if (confidence !== 'contextual' && confidence !== 'supplied' && confidence !== 'ambiguous')
    return invalid();
  if (
    !sources.length ||
    (evidence !== 'supplied_claim' && sources.includes('supplied_metadata')) ||
    (evidence === 'supplied_claim') !== sources.every((value) => value === 'supplied_metadata')
  )
    return invalid();
  const exclusive = footprint(item.exclusive),
    overlapping = footprint(item.overlapping);
  const breakdown = list(item.breakdown, 5, (value): ApplicationCategoryMeasurement => {
    const row = record(value),
      category = row.category;
    if (
      category !== 'installation' &&
      category !== 'data' &&
      category !== 'cache' &&
      category !== 'logs' &&
      category !== 'unknown'
    )
      return invalid();
    return {
      category,
      exclusive: footprint(row.exclusive),
      overlapping: footprint(row.overlapping),
    };
  });
  if (!breakdown.length || new Set(breakdown.map((row) => row.category)).size !== breakdown.length)
    return invalid();
  const measures: ('exclusive' | 'overlapping')[] = ['exclusive', 'overlapping'];
  for (const measure of measures) {
    const expected = measure === 'exclusive' ? exclusive : overlapping;
    if (
      breakdown.reduce((sum, row) => sum + BigInt(row[measure].logical_bytes), 0n) !==
        BigInt(expected.logical_bytes) ||
      breakdown.reduce((sum, row) => sum + row[measure].files, 0) !== expected.files
    )
      return invalid();
  }
  if (
    (confidence === 'ambiguous') !== breakdown.some((row) => row.category === 'unknown') ||
    (confidence !== 'ambiguous' && (confidence === 'supplied') !== (evidence === 'supplied_claim'))
  )
    return invalid();
  return {
    id,
    evidence,
    confidence,
    name: text(item.name),
    publisher: nullableText(item.publisher),
    version: nullableText(item.version),
    root: integer(item.root, 0xffffffff),
    profile_scope: item.profile_scope === null ? null : integer(item.profile_scope, 0xffffffff),
    sources,
    exclusive,
    overlapping,
    breakdown,
  };
}
function coverage(value: unknown): ApplicationCoverage {
  const item = record(value);
  return {
    source: source(item.source),
    complete: boolean(item.complete),
    problem: nullableText(item.problem),
    visited: integer(item.visited, 4096),
    missing_location: integer(item.missing_location, 4096),
    rejected: integer(item.rejected, 4096),
  };
}
export function readApplicationPage(value: unknown): ApplicationPage {
  const item = record(value);
  if (
    item.attribution !== 'current' &&
    item.attribution !== 'indexed' &&
    item.attribution !== 'captured'
  )
    return invalid();
  const page: ApplicationPage = {
    attribution: item.attribution,
    scope: integer(item.scope, 0xffffffff),
    indexed_at: item.indexed_at === null ? null : integer(item.indexed_at, 8640000000000),
    registrations_observed_at: integer(item.registrations_observed_at, 8640000000000),
    registrations_started_at: integer(item.registrations_started_at, 8640000000000),
    excluded_roots: integer(item.excluded_roots),
    partial_index: boolean(item.partial_index),
    coverage: list(item.coverage, 5, coverage),
    unbound_records: integer(item.unbound_records, 1024),
    duplicate_records: integer(item.duplicate_records, 1024),
    exclusive: footprint(item.exclusive),
    overlapping: footprint(item.overlapping),
    unattributed: footprint(item.unattributed),
    total: footprint(item.total),
    applications: list(item.applications, 100, application),
    application_count: integer(item.application_count, 1024),
    chart: list(item.chart, 200, application),
    chart_omitted: integer(item.chart_omitted, 1024),
  };
  const groups = [page.exclusive, page.overlapping, page.unattributed];
  if (
    groups.reduce((sum, group) => sum + BigInt(group.logical_bytes), 0n) !==
      BigInt(page.total.logical_bytes) ||
    groups.reduce((sum, group) => sum + group.files, 0) !== page.total.files ||
    page.applications.length > page.application_count ||
    page.chart.length !== Math.min(200, page.application_count) ||
    page.chart_omitted !== page.application_count - page.chart.length ||
    new Set(page.applications.map((app) => app.id)).size !== page.applications.length ||
    new Set(page.chart.map((app) => app.id)).size !== page.chart.length
  )
    return invalid();
  const rows = new Map(page.applications.map((app) => [app.id, app]));
  for (const app of [...page.applications, ...page.chart]) {
    if (
      BigInt(app.exclusive.logical_bytes) > BigInt(page.exclusive.logical_bytes) ||
      app.exclusive.files > page.exclusive.files ||
      BigInt(app.overlapping.logical_bytes) > BigInt(page.overlapping.logical_bytes) ||
      app.overlapping.files > page.overlapping.files
    )
      return invalid();
  }
  for (const app of page.chart) {
    const row = rows.get(app.id);
    if (row && JSON.stringify(row) !== JSON.stringify(app)) return invalid();
  }
  return page;
}
export function applicationBytes(value: string): string {
  const bytes = BigInt(value),
    units = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB', 'EiB', 'ZiB', 'YiB'];
  let unit = 0,
    scale = 1n;
  while (bytes >= scale * 1024n && unit < units.length - 1) {
    unit++;
    scale *= 1024n;
  }
  if (!unit) return `${bytes.toLocaleString('en')} B`;
  const tenths = (bytes * 10n + scale / 2n) / scale;
  return `${(tenths / 10n).toLocaleString('en')}${tenths % 10n ? '.' + (tenths % 10n) : ''} ${units[unit]}`;
}
export const APPLICATION_CHART = {
  width: 900,
  height: 280,
  left: 90,
  right: 880,
  top: 30,
  bottom: 235,
};
export function applicationPlot(apps: ApplicationItem[]) {
  const measured = apps.filter(
    (app) => app.exclusive.files > 0 && BigInt(app.exclusive.logical_bytes) > 0n,
  );
  const maxFiles = Math.max(0, ...measured.map((app) => app.exclusive.files));
  const maxBytes = measured.reduce(
    (max, app) =>
      BigInt(app.exclusive.logical_bytes) > BigInt(max) ? app.exclusive.logical_bytes : max,
    '0',
  );
  const xScale = Math.log1p(maxFiles) || 1,
    yScale = Math.log1p(Number(maxBytes)) || 1;
  return {
    maxFiles,
    maxBytes,
    omittedZero: apps.length - measured.length,
    points: measured.map((app) => ({
      app,
      x:
        APPLICATION_CHART.left +
        ((APPLICATION_CHART.right - APPLICATION_CHART.left) * Math.log1p(app.exclusive.files)) /
          xScale,
      y:
        APPLICATION_CHART.bottom -
        ((APPLICATION_CHART.bottom - APPLICATION_CHART.top) *
          Math.log1p(Number(app.exclusive.logical_bytes))) /
          yScale,
    })),
  };
}
