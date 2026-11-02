import { computeVoronoi, type VoronoiReply } from './voronoi.ts';
import type { Rect } from './storage-map.ts';

self.onmessage = (event: MessageEvent<{ rects: Rect[]; total: number }>) => {
  let reply: VoronoiReply;
  try {
    reply = { kind: 'ready', layout: computeVoronoi(event.data.rects, event.data.total) };
  } catch (error) {
    reply = { kind: 'error', message: error instanceof Error ? error.message : String(error) };
  }
  self.postMessage(reply);
};
