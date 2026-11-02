import type { Rect } from './storage-map.ts';
import type { VoronoiReply } from './voronoi.ts';

export interface VoronoiPort {
  post(
    input: { rects: Rect[]; total: number },
    reply: (value: VoronoiReply) => void,
    failed: () => void,
  ): void;
  terminate(): void;
}
function browserPort(): VoronoiPort {
  const worker = new Worker(new URL('./voronoi.worker.ts', import.meta.url), { type: 'module' });
  return {
    post(input, reply, failed) {
      worker.onmessage = (event: MessageEvent<VoronoiReply>) => reply(event.data);
      worker.onerror = failed;
      worker.postMessage(input);
    },
    terminate: () => worker.terminate(),
  };
}

export function startVoronoiJob(
  rects: Rect[],
  total: number,
  onresult: (reply: VoronoiReply) => void,
  options: { create?: () => VoronoiPort; timeoutMs?: number } = {},
): () => void {
  let pending = true,
    port: VoronoiPort | undefined,
    timer: ReturnType<typeof setTimeout> | undefined;
  const close = () => {
    pending = false;
    clearTimeout(timer);
    port?.terminate();
    port = undefined;
  };
  const finish = (reply: VoronoiReply) => {
    if (!pending) return;
    close();
    onresult(reply);
  };
  try {
    port = (options.create ?? browserPort)();
    timer = setTimeout(
      () =>
        finish({
          kind: 'error',
          message: 'Voronoi worker reached its time limit. Use Treemap or List.',
        }),
      Math.max(0, Math.min(1500, options.timeoutMs ?? 1500)),
    );
    port.post({ rects, total }, finish, () =>
      finish({ kind: 'error', message: 'Voronoi worker is unavailable. Use Treemap or List.' }),
    );
  } catch {
    finish({ kind: 'error', message: 'Voronoi worker is unavailable. Use Treemap or List.' });
  }
  return () => {
    if (pending) close();
  };
}
