import type { Page, Query } from './types';

export type QueryFamily =
  | 'file_list'
  | 'overview_children'
  | 'details'
  | 'folder_budget'
  | 'snapshot_visual'
  | 'snapshot_changes'
  | 'snapshot_capture'
  | 'snapshot_retention'
  | 'snapshot_calendar'
  | 'snapshot_calendar_entries'
  | 'snapshot_diary'
  | 'snapshot_trend'
  | 'snapshot_trend_changes'
  | 'space_account'
  | 'recovery_copies'
  | 'history_clip'
  | 'applications'
  | 'cleanup_tray';
export type QueryTransport<T = Page, Q = Query, F extends string = QueryFamily> = {
  run: (family: F, requestId: number, query: Q) => Promise<T | null>;
  cancel: (family: F, requestId: number) => Promise<void>;
};

let sequence = 0;
type Job<T, Q> = {
  id: number;
  query: Q;
  resolve: (page: T | null) => void;
  reject: (problem: unknown) => void;
  cancelled: boolean;
};

export function createQueryRunner<T = Page, Q = Query, F extends string = QueryFamily>(
  family: F,
  transport: QueryTransport<T, Q, F>,
) {
  let active: Job<T, Q> | null = null;
  let pending: Job<T, Q> | null = null;

  function cancelJob(job: Job<T, Q>): void {
    if (job.cancelled) return;
    job.cancelled = true;
    job.resolve(null);
    if (active === job) {
      // A failed IPC cancellation still suppresses stale output and bounds the queue.
      void transport.cancel(family, job.id).catch(() => undefined);
    }
  }

  function launch(job: Job<T, Q>): void {
    active = job;
    void Promise.resolve()
      .then(() => transport.run(family, job.id, job.query))
      .then(
        (page) => {
          if (!job.cancelled) job.resolve(page);
        },
        (problem: unknown) => {
          if (!job.cancelled) job.reject(problem);
        },
      )
      .finally(() => {
        active = null;
        const next = pending;
        pending = null;
        if (next && !next.cancelled) launch(next);
      });
  }

  function cancel(): void {
    if (pending) cancelJob(pending);
    pending = null;
    if (active) cancelJob(active);
  }

  function request(query: Q) {
    if (sequence >= Number.MAX_SAFE_INTEGER) throw new Error('Query request limit reached.');
    cancel();
    let cancelRequest = () => {};
    const result = new Promise<T | null>((resolve, reject) => {
      const job: Job<T, Q> = { id: ++sequence, query, resolve, reject, cancelled: false };
      cancelRequest = () => cancelJob(job);
      if (active) pending = job;
      else launch(job);
    });
    return { result, cancel: () => cancelRequest() };
  }

  return { request, cancel };
}
