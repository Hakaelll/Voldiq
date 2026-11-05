import type { PlanAction } from './offload-plan';

export type TransferSummary = { id: string } & (
  | {
      availability: 'ready';
      action: 'offload' | 'restore';
      state: string;
      created: number;
      connection_id: number;
      files: number;
      published_files: number;
      published_bytes: string;
      source_preview: string;
      destination_preview: string;
      destinations: number;
    }
  | { availability: 'unavailable'; reason: string }
);
export type TransferPage = { items: TransferSummary[]; next_before: string | null };
export type TransferRecord = {
  manifest: {
    id: string;
    connection_id: number;
    created: number;
    root: string;
    action: PlanAction;
    logical_bytes: string;
    entries: { id: number; path: string }[];
  };
  manifest_hash: string;
  state: string;
  last_observed_at: number;
  currently_connected: boolean | null;
  restore_available: boolean;
  limitations: string;
  next_after: number | null;
  files: {
    node: number;
    output_name: string;
    phase: string;
    private_name: string | null;
    destination_identity: string | null;
    content_blake3: string | null;
    verified_bytes: string | null;
    verified_at: number | null;
    published_at: number | null;
    interrupted_at: number | null;
  }[];
};
export type CatalogueState = {
  page: TransferPage | null;
  loading: boolean;
  error: string;
  before: string | null;
  selection:
    | { kind: 'none' }
    | { kind: 'loading'; id: string; after: number | null }
    | { kind: 'ready'; record: TransferRecord; after: number | null }
    | { kind: 'failed'; id: string; after: number | null; error: string };
};

export function createTransferCatalogue(source: {
  list: (before: string | null) => Promise<TransferPage>;
  read: (id: string, after: number | null) => Promise<TransferRecord>;
  onstate: (value: CatalogueState) => void;
}) {
  let state: CatalogueState = {
    page: null,
    loading: false,
    error: '',
    before: null,
    selection: { kind: 'none' },
  };
  let listRequest = 0,
    detailRequest = 0,
    disposed = false;
  function publish(next: CatalogueState): void {
    if (!disposed) {
      state = next;
      source.onstate(state);
    }
  }
  return {
    async load(before: string | null): Promise<void> {
      if (disposed) return;
      const request = ++listRequest;
      ++detailRequest;
      publish({
        ...state,
        page: null,
        loading: true,
        error: '',
        before,
        selection: { kind: 'none' },
      });
      try {
        const page = await source.list(before);
        if (!disposed && request === listRequest) publish({ ...state, page, loading: false });
      } catch (error) {
        if (!disposed && request === listRequest)
          publish({ ...state, loading: false, error: String(error) });
      }
    },
    async select(id: string, after: number | null = null): Promise<void> {
      if (disposed) return;
      const request = ++detailRequest;
      publish({ ...state, selection: { kind: 'loading', id, after } });
      try {
        const record = await source.read(id, after);
        if (record.manifest.id !== id)
          throw new Error(
            'The returned operation does not match this selection. Reload its record.',
          );
        if (!disposed && request === detailRequest)
          publish({ ...state, selection: { kind: 'ready', record, after } });
      } catch (error) {
        if (!disposed && request === detailRequest)
          publish({ ...state, selection: { kind: 'failed', id, after, error: String(error) } });
      }
    },
    dispose(): void {
      disposed = true;
      ++listRequest;
      ++detailRequest;
    },
  };
}
