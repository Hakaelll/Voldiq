import type { IndexId } from './index-context';
import type { Query } from './types';

export interface CsvProgress {
  inspected: number;
  rows: number;
  total_entries: number;
}
export interface CsvContext {
  indexId: IndexId;
  requestId: string;
}
export interface CsvRequest {
  items: 'files' | 'folders';
  filter: Pick<
    Query,
    | 'scope'
    | 'text'
    | 'expression'
    | 'extension'
    | 'without_extension'
    | 'min_size'
    | 'modified_before'
    | 'cloud'
  >;
  include_paths: boolean;
}
export interface CsvEvent {
  index_id: IndexId;
  request_id: string;
  phase: { kind: 'choosing' } | { kind: 'running'; id: number; progress: CsvProgress };
}
export interface CsvReceipt {
  index_id: IndexId;
  request_id: string;
  outcome:
    | { kind: 'dialog_cancelled' }
    | { kind: 'cancelled'; progress: CsvProgress }
    | { kind: 'complete'; progress: CsvProgress; path: string };
}
export type CsvState =
  | { kind: 'idle' }
  | { kind: 'choosing'; cancelling: boolean }
  | { kind: 'running'; progress: CsvProgress; cancelling: boolean }
  | { kind: 'finished'; receipt: CsvReceipt }
  | { kind: 'failed'; message: string };

/** Complete reports recurse beneath the explicitly chosen folder, independently of UI paging/sort. */
export function csvRequest(
  query: Query,
  scope: number,
  items: CsvRequest['items'],
  filtered: boolean,
  includePaths: boolean,
): CsvRequest {
  const { text, expression, extension, without_extension, min_size, modified_before, cloud } =
    query;
  return {
    items,
    filter: filtered
      ? { scope, text, expression, extension, without_extension, min_size, modified_before, cloud }
      : { scope },
    include_paths: includePaths,
  };
}

interface Transport {
  save(context: CsvContext, request: CsvRequest): Promise<CsvReceipt>;
  read(context: CsvContext): Promise<CsvEvent | null>;
  cancel(context: CsvContext): Promise<boolean>;
}
interface Run {
  context: CsvContext;
  timer: ReturnType<typeof setTimeout> | null;
  stopped: boolean;
  cancelAcknowledged: boolean;
  cancelPending: boolean;
  cancelling: boolean;
}

export function createCsvExport(
  transport: Transport,
  changed: (state: CsvState) => void,
  delayMs = 250,
) {
  let alive = true;
  let active: Run | null = null;
  let last: CsvState = { kind: 'idle' };
  function emit(state: CsvState): void {
    last = state;
    if (alive) changed(state);
  }
  async function requestCancel(run: Run): Promise<void> {
    if (run.stopped || run.cancelAcknowledged || run.cancelPending) return;
    run.cancelPending = true;
    try {
      run.cancelAcknowledged = await transport.cancel(run.context);
    } catch {
      /* The next owned progress read retries cancellation; completion still settles normally. */
    } finally {
      run.cancelPending = false;
    }
  }
  async function poll(run: Run): Promise<void> {
    try {
      const event = await transport.read(run.context);
      if (run.stopped || active !== run) return;
      if (
        event &&
        event.index_id === run.context.indexId &&
        event.request_id === run.context.requestId
      ) {
        emit(
          event.phase.kind === 'running'
            ? { kind: 'running', progress: event.phase.progress, cancelling: run.cancelling }
            : { kind: 'choosing', cancelling: run.cancelling },
        );
        if (run.cancelling) await requestCancel(run);
      }
    } catch {
      /* Progress is optional; the command's final receipt determines publication. */
    }
    if (!run.stopped && active === run) run.timer = setTimeout(() => void poll(run), delayMs);
  }
  return {
    async save(context: CsvContext, request: CsvRequest): Promise<void> {
      if (!alive || active) return;
      const run: Run = {
        context,
        timer: null,
        stopped: false,
        cancelAcknowledged: false,
        cancelPending: false,
        cancelling: false,
      };
      active = run;
      emit({ kind: 'choosing', cancelling: false });
      try {
        const pending = transport.save(context, request);
        void poll(run);
        const receipt = await pending;
        if (active !== run) return;
        if (receipt.index_id !== context.indexId || receipt.request_id !== context.requestId)
          throw new Error('Export response belongs to another request.');
        emit({ kind: 'finished', receipt });
      } catch (problem) {
        if (active === run) emit({ kind: 'failed', message: String(problem) });
      } finally {
        run.stopped = true;
        if (run.timer !== null) clearTimeout(run.timer);
        if (active === run) active = null;
      }
    },
    cancel(): void {
      if (active && !active.stopped) {
        active.cancelling = true;
        if (last.kind === 'choosing' || last.kind === 'running')
          emit({ ...last, cancelling: true });
        void requestCancel(active);
      }
    },
    dispose(): void {
      alive = false;
      // Keep the one existing pull alive until its cancelled native command settles.
      if (active) {
        active.cancelling = true;
        void requestCancel(active);
      }
    },
  };
}
