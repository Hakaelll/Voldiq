import type { IndexId } from './index-context';
import type { Query } from './types';
export type ReportFormat = 'csv' | 'xlsx' | 'pdf';

export interface ReportProgress {
  inspected: number;
  rows: number;
  total_entries: number;
}
export type ExportOwner = { kind: 'index'; id: IndexId } | { kind: 'history'; fingerprint: string };
export interface ReportContext {
  owner: ExportOwner;
  requestId: string;
}
export interface ReportSelection {
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
export interface ReportEvent<Progress = ReportProgress> {
  owner: ExportOwner;
  request_id: string;
  phase: { kind: 'choosing' } | { kind: 'running'; id: number; progress: Progress };
}
export interface ReportReceipt<Progress = ReportProgress> {
  owner: ExportOwner;
  request_id: string;
  outcome:
    | { kind: 'dialog_cancelled' }
    | { kind: 'cancelled'; progress: Progress }
    | { kind: 'complete'; progress: Progress; path: string };
}
export type ReportState<Progress = ReportProgress> =
  | { kind: 'idle' }
  | { kind: 'choosing'; cancelling: boolean }
  | { kind: 'running'; progress: Progress; cancelling: boolean }
  | { kind: 'finished'; receipt: ReportReceipt<Progress> }
  | { kind: 'failed'; message: string };

/** Complete reports recurse beneath the explicitly chosen folder, independently of UI paging/sort. */
export function reportSelection(
  query: Query,
  scope: number,
  items: ReportSelection['items'],
  filtered: boolean,
  includePaths: boolean,
): ReportSelection {
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

export function sameOwner(left: ExportOwner, right: ExportOwner): boolean {
  switch (left.kind) {
    case 'index':
      return right.kind === 'index' && left.id === right.id;
    case 'history':
      return right.kind === 'history' && left.fingerprint === right.fingerprint;
    default: {
      const exhaustive: never = left;
      return exhaustive;
    }
  }
}
export function indexedExport(context: ReportContext): { indexId: IndexId; requestId: string } {
  if (context.owner.kind !== 'index') throw new Error('This export requires a captured analysis.');
  return { indexId: context.owner.id, requestId: context.requestId };
}

interface Transport<Request, Format extends string, Progress> {
  save(context: ReportContext, request: Request, format: Format): Promise<ReportReceipt<Progress>>;
  read(context: ReportContext): Promise<ReportEvent<Progress> | null>;
  cancel(context: ReportContext): Promise<boolean>;
}
interface Run {
  context: ReportContext;
  timer: ReturnType<typeof setTimeout> | null;
  stopped: boolean;
  cancelAcknowledged: boolean;
  cancelPending: boolean;
  cancelling: boolean;
}

export function createReportExport<
  Request = ReportSelection,
  Format extends string = ReportFormat,
  Progress = ReportProgress,
>(
  transport: Transport<Request, Format, Progress>,
  changed: (state: ReportState<Progress>) => void,
  delayMs = 250,
) {
  let alive = true;
  let active: Run | null = null;
  let last: ReportState<Progress> = { kind: 'idle' };
  function emit(state: ReportState<Progress>): void {
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
        sameOwner(event.owner, run.context.owner) &&
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
    async save(context: ReportContext, request: Request, format: Format): Promise<void> {
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
        const pending = transport.save(context, request, format);
        void poll(run);
        const receipt = await pending;
        if (active !== run) return;
        if (!sameOwner(receipt.owner, context.owner) || receipt.request_id !== context.requestId)
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
