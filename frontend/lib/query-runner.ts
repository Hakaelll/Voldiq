import type { Page, Query } from './types';

export type QueryFamily = 'file_list' | 'overview_children';
export type QueryTransport = {
  run: (family: QueryFamily, requestId: number, query: Query) => Promise<Page | null>;
  cancel: (family: QueryFamily, requestId: number) => Promise<void>;
};

let sequence = 0;
type Job = {
  id: number;
  query: Query;
  resolve: (page: Page | null) => void;
  reject: (problem: unknown) => void;
  cancelled: boolean;
};

export function createQueryRunner(family: QueryFamily, transport: QueryTransport) {
  let active: Job | null = null;
  let pending: Job | null = null;

  function cancelJob(job: Job): void {
    if (job.cancelled) return;
    job.cancelled = true;
    job.resolve(null);
    if (active === job) {
      // A failed IPC cancellation still suppresses stale output and bounds the queue.
      void transport.cancel(family, job.id).catch(() => undefined);
    }
  }

  function launch(job: Job): void {
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

  function request(query: Query) {
    if (sequence >= Number.MAX_SAFE_INTEGER) throw new Error('Query request limit reached.');
    cancel();
    let cancelRequest = () => {};
    const result = new Promise<Page | null>((resolve, reject) => {
      const job: Job = { id: ++sequence, query, resolve, reject, cancelled: false };
      cancelRequest = () => cancelJob(job);
      if (active) pending = job;
      else launch(job);
    });
    return { result, cancel: () => cancelRequest() };
  }

  return { request, cancel };
}
