import { buildCity } from './disk-city';
import type { Rect } from './storage-map';

self.onmessage = (event: MessageEvent<{ rects: Rect[]; heights: number[] }>) => {
  try {
    const { rects, heights } = event.data;
    const values = new Map(rects.map((rect, index) => [rect.id, heights[index]]));
    self.postMessage({ scene: buildCity(rects, (rect) => values.get(rect.id) ?? 5) });
  } catch (error) {
    self.postMessage({ error: error instanceof Error ? error.message : String(error) });
  }
};
