import type { Rect } from './treemap';

export type Box = Pick<Rect, 'x' | 'y' | 'width' | 'height'>;
export type Zoom = { direction: 'in' | 'out'; anchor: Box };

export function resolveZoom(from: number, to: number, before: Rect[], after: Rect[]): Zoom | null {
  if (from === to) return null;
  const entering = before.find((rect) => rect.id === to);
  if (entering) return { direction: 'in', anchor: entering };
  const leaving = after.find((rect) => rect.id === from);
  return leaving ? { direction: 'out', anchor: leaving } : null;
}

export function zoomEase(progress: number): number {
  return 1 - (1 - Math.max(0, Math.min(1, progress))) ** 3;
}

export function zoomBox(zoom: Zoom, width: number, height: number, progress: number): Box {
  const amount = zoom.direction === 'in' ? zoomEase(progress) : 1 - zoomEase(progress);
  return {
    x: zoom.anchor.x * (1 - amount),
    y: zoom.anchor.y * (1 - amount),
    width: zoom.anchor.width + (width - zoom.anchor.width) * amount,
    height: zoom.anchor.height + (height - zoom.anchor.height) * amount,
  };
}

export function createTreemapAnimator(
  clock: {
    now: () => number;
    request: (callback: FrameRequestCallback) => number;
    cancel: (id: number) => void;
  },
  render: (progress: number) => void,
) {
  let pending: number | null = null;
  let generation = 0;
  function stop(): void {
    generation += 1;
    if (pending !== null) {
      clock.cancel(pending);
      pending = null;
      render(1);
    }
  }
  return {
    stop,
    running: () => pending !== null,
    start(): void {
      if (pending !== null) clock.cancel(pending);
      const owner = ++generation;
      const started = clock.now();
      render(0);
      const frame: FrameRequestCallback = (time) => {
        if (owner !== generation) return;
        const progress = Math.max(0, Math.min(1, (time - started) / 160));
        pending = null;
        render(progress);
        if (progress < 1 && owner === generation) pending = clock.request(frame);
      };
      pending = clock.request(frame);
    },
  };
}
