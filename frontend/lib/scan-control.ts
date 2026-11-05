export type ScanControlState = 'idle' | 'running' | 'pause_requested' | 'paused' | 'finished';
export interface ScanControlView {
  observed: ScanControlState | null;
  activity: 'query' | 'pause' | 'resume' | null;
  problem: string | null;
}
export interface ScanControlTransport {
  query(id: number): Promise<unknown>;
  pause(id: number): Promise<unknown>;
  resume(id: number): Promise<unknown>;
}
function readState(value: unknown): ScanControlState {
  switch (value) {
    case 'idle':
    case 'running':
    case 'pause_requested':
    case 'paused':
    case 'finished':
      return value;
    default:
      throw Error('Invalid scan control response.');
  }
}
export function createScanControl(
  id: number,
  transport: ScanControlTransport,
  onstate: (view: ScanControlView) => void,
) {
  let view: ScanControlView = { observed: null, activity: null, problem: null };
  let alive = true,
    epoch = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;
  function clear() {
    if (timer !== null) clearTimeout(timer);
    timer = null;
  }
  async function request(action: 'query' | 'pause' | 'resume'): Promise<void> {
    if (!alive || (view.activity !== null && (action === 'query' || view.activity !== 'query')))
      return;
    if (action === 'pause' && view.observed !== 'running') return;
    if (action === 'resume' && view.observed !== 'paused' && view.observed !== 'pause_requested')
      return;
    clear();
    const owner = ++epoch;
    view = { ...view, activity: action, problem: null };
    onstate(view);
    try {
      const response = await transport[action](id);
      if (!alive || owner !== epoch) return;
      view = { observed: readState(response), activity: null, problem: null };
      onstate(view);
      if (view.observed === 'idle' || view.observed === 'pause_requested')
        timer = setTimeout(() => {
          timer = null;
          void request('query');
        }, 150);
    } catch (error) {
      if (!alive || owner !== epoch) return;
      view = {
        observed: null,
        activity: null,
        problem: error instanceof Error ? error.message : String(error),
      };
      onstate(view);
    }
  }
  return {
    load: () => request('query'),
    pause: () => request('pause'),
    resume: () => request('resume'),
    dispose() {
      alive = false;
      epoch++;
      clear();
    },
  };
}
