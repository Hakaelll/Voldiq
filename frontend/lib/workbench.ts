import type { IndexId } from './index-context.ts';
import type { Query } from './types.ts';
import type { StorageDepth, StorageView, ColorMode } from './storage-map.ts';

export type IndexedView = 'overview' | 'explorer' | 'large';
export type Destination =
  IndexedView | 'duplicates' | 'cleanup' | 'developer' | 'history' | 'settings';
export interface ViewSpec {
  query: Query;
  scope: 'current_folder' | 'analysis_root';
  view: IndexedView;
  representation: StorageView | 'table' | 'grid';
  metric: 'logical' | 'allocated';
  depth: StorageDepth;
  color: ColorMode;
}
export interface SavedView {
  id: number;
  name: string;
  spec: ViewSpec;
  updated: number;
}
export interface Favorite {
  id: number;
  name: string;
  path: string;
  updated: number;
}
export interface Workspaces {
  revision: string;
  views: SavedView[];
  favorites: Favorite[];
}
export interface RecordRef {
  id: number;
  revision: string;
}
export type CatalogState =
  { kind: 'loading' } | { kind: 'failed'; message: string } | { kind: 'ready'; value: Workspaces };

export function detachedQuery(query: Query): Query {
  const copy = { ...query };
  delete copy.parent;
  delete copy.scope;
  delete copy.offset;
  delete copy.limit;
  return copy;
}
export function isMapRepresentation(value: ViewSpec['representation']): value is StorageView {
  return value !== 'table' && value !== 'grid';
}
export function restoreWorkspace(spec: ViewSpec, currentFolder: number) {
  const q = spec.query;
  return {
    view: spec.view,
    scope: spec.scope === 'analysis_root' ? 0 : currentFolder,
    text: q.text ?? '',
    expression: q.expression ?? '',
    extension: q.extension ?? '',
    withoutExtension: q.without_extension ?? false,
    cloud: q.cloud ?? null,
    minSize: (q.min_size ?? 0) / 1048576,
    largeKind: q.kind === 'directory' ? 'directory' : 'file',
    before:
      q.modified_before === undefined
        ? ''
        : new Date(q.modified_before * 1000).toISOString().slice(0, 10),
    sort: q.sort === 'allocated' ? 'size' : (q.sort ?? 'size'),
    metric: spec.metric,
    depth: spec.depth,
    color: spec.color,
    mapMode: isMapRepresentation(spec.representation) ? spec.representation : null,
    explorerMap: isMapRepresentation(spec.representation),
    fileRepresentation: spec.representation === 'grid' ? 'grid' : 'table',
  } satisfies {
    largeKind: 'file' | 'directory';
    fileRepresentation: 'table' | 'grid';
    [key: string]: unknown;
  };
}
export interface WorkspaceContext {
  indexId: IndexId | null;
  scope: number;
  query: Query;
  owner: number;
  revision: string | null;
}
export function workspaceContextIsCurrent(
  request: WorkspaceContext,
  current: WorkspaceContext,
): boolean {
  return (
    request.indexId !== null &&
    request.indexId === current.indexId &&
    request.scope === current.scope &&
    request.query === current.query &&
    request.owner === current.owner &&
    request.revision !== null &&
    request.revision === current.revision
  );
}
