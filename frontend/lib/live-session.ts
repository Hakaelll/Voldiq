export interface LiveMeasurement {
  logical: string;
  files: number;
  measured_at: number | null;
  complete: boolean;
  accounting_version: number | null;
}
export interface LiveReport {
  reason: 'initial' | 'changes' | 'lost_events' | 'requested';
  before: LiveMeasurement;
  after: LiveMeasurement;
  net_change: string | null;
}
export type LiveObservation = {
  kind: 'observed';
  id: number;
  before_index: number;
  index_id: number;
  entries: number;
  report: LiveReport;
};
export type LiveEvent =
  | { kind: 'started'; id: number }
  | LiveObservation
  | { kind: 'ended'; id: number; error: string | null };
type Terminal =
  | { kind: 'idle'; reason: 'off' | 'stopped' | 'hidden'; report: LiveReport | null }
  | { kind: 'failed'; message: string; report: LiveReport | null };
export type LiveState =
  | Terminal
  | { kind: 'starting'; report: LiveReport | null }
  | { kind: 'watching'; report: LiveReport | null }
  | { kind: 'stopping'; report: LiveReport | null; problem: string | null };
export interface LiveTransport {
  start(
    index: number,
    concurrency: number | 'auto',
    receive: (event: LiveEvent) => Promise<void>,
  ): Promise<number>;
  stop(id: number): Promise<void>;
  acknowledge(id: number, index: number): Promise<void>;
  resynchronize(id: number): Promise<void>;
}
type Run = {
  activity: { kind: 'starting' | 'stopping'; id: number | null } | { kind: 'watching'; id: number };
  expected: number;
  report: LiveReport | null;
  outcome: { kind: 'idle'; reason: 'stopped' | 'hidden' } | { kind: 'failed'; message: string };
};

export function createLiveSession(
  transport: LiveTransport,
  publish: (state: LiveState) => void,
  observed: (event: LiveObservation) => Promise<void>,
) {
  let current: Run | null = null;
  function emit(run: Run): void {
    if (current !== run) return;
    publish(
      run.activity.kind === 'stopping'
        ? {
            kind: 'stopping',
            report: run.report,
            problem: run.outcome.kind === 'failed' ? run.outcome.message : null,
          }
        : { kind: run.activity.kind, report: run.report },
    );
  }
  async function requestStop(run: Run): Promise<void> {
    const id = run.activity.id;
    if (id === null) return;
    try {
      await transport.stop(id);
    } catch (problem) {
      if (current === run) {
        run.outcome = { kind: 'failed', message: String(problem) };
        emit(run);
      }
    }
  }
  async function fail(run: Run, problem: unknown): Promise<void> {
    if (current !== run) return;
    run.outcome = { kind: 'failed', message: String(problem) };
    run.activity = { kind: 'stopping', id: run.activity.id };
    emit(run);
    await requestStop(run);
  }
  async function receive(run: Run, event: LiveEvent): Promise<void> {
    if (current !== run) return;
    if (run.activity.id !== null && run.activity.id !== event.id) {
      await fail(run, 'Live session identity changed. Restart Live.');
      return;
    }
    if (event.kind === 'ended') {
      current = null;
      publish(
        event.error !== null && run.outcome.kind !== 'failed'
          ? { kind: 'failed', message: event.error, report: run.report }
          : { ...run.outcome, report: run.report },
      );
      return;
    }
    if (run.activity.kind === 'stopping') {
      run.activity = { kind: 'stopping', id: event.id };
      if (event.kind === 'started') {
        await requestStop(run);
        return;
      }
    } else run.activity = { kind: 'watching', id: event.id };
    if (event.kind === 'observed') {
      if (event.before_index !== run.expected) {
        await fail(run, 'Live analysis changed. Scan again before restarting Live.');
        return;
      }
      run.expected = event.index_id;
      run.report = event.report;
      emit(run);
      try {
        // Adopt a final publication even while stopping. Native acknowledgement is unnecessary after stop.
        await observed(event);
        if (current === run && run.activity.kind !== 'stopping')
          await transport.acknowledge(event.id, event.index_id);
      } catch (problem) {
        await fail(run, problem);
      }
    } else emit(run);
  }
  return {
    async start(index: number, concurrency: number | 'auto'): Promise<void> {
      if (current) return;
      const run: Run = {
        activity: { kind: 'starting', id: null },
        expected: index,
        report: null,
        outcome: { kind: 'idle', reason: 'stopped' },
      };
      current = run;
      emit(run);
      try {
        const id = await transport.start(index, concurrency, (event) => receive(run, event));
        if (current !== run) {
          await transport.stop(id);
          return;
        }
        if (run.activity.id !== null && run.activity.id !== id) {
          await fail(run, 'Live startup identity changed.');
          return;
        }
        run.activity = { ...run.activity, id };
        if (run.activity.kind === 'stopping') await requestStop(run);
      } catch (problem) {
        if (current === run) {
          current = null;
          publish({ kind: 'failed', message: String(problem), report: run.report });
        }
      }
    },
    async stop(reason: 'stopped' | 'hidden' = 'stopped'): Promise<void> {
      const run = current;
      if (!run || run.activity.kind === 'stopping') return;
      run.outcome = { kind: 'idle', reason };
      run.activity = { kind: 'stopping', id: run.activity.id };
      emit(run);
      await requestStop(run);
    },
    async resynchronize(): Promise<void> {
      const run = current;
      if (!run || run.activity.kind !== 'watching') return;
      try {
        await transport.resynchronize(run.activity.id);
      } catch (problem) {
        await fail(run, problem);
      }
    },
    dispose(): void {
      const run = current;
      current = null;
      if (run?.activity.id !== null && run?.activity.id !== undefined)
        void transport.stop(run.activity.id).catch(() => {});
    },
  };
}
