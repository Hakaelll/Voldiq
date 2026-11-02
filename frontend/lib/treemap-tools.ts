import { screenToWorld, type Camera, type Point } from './disk-city.ts';
import type { Rect } from './treemap.ts';

type Region = Pick<Rect, 'id' | 'x' | 'y' | 'width' | 'height'>;
export type LassoResult =
  { kind: 'selected'; ids: number[]; skippedGroups: number } | { kind: 'invalid'; message: string };
const MAX_POINTS = 256;
const equal = (a: Point, b: Point) => a.x === b.x && a.y === b.y;
const cross = (a: Point, b: Point, c: Point) =>
  (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);

export function appendLassoPoint(
  points: readonly Point[],
  point: Point,
  force = false,
): readonly Point[] {
  const last = points.at(-1);
  if (
    last &&
    (equal(last, point) || (!force && Math.hypot(last.x - point.x, last.y - point.y) < 3))
  )
    return points;
  const next = [...points, point];
  return next.length <= MAX_POINTS
    ? next
    : next.filter((_, i) => i % 2 === 0 || i === next.length - 1);
}

function intersects(a: Point, b: Point, c: Point, d: Point): boolean {
  const sign = (value: number) => (Math.abs(value) <= 1e-9 ? 0 : Math.sign(value));
  const abC = sign(cross(a, b, c)),
    abD = sign(cross(a, b, d));
  const cdA = sign(cross(c, d, a)),
    cdB = sign(cross(c, d, b));
  const within = (p: Point, start: Point, end: Point) =>
    p.x >= Math.min(start.x, end.x) - 1e-9 &&
    p.x <= Math.max(start.x, end.x) + 1e-9 &&
    p.y >= Math.min(start.y, end.y) - 1e-9 &&
    p.y <= Math.max(start.y, end.y) + 1e-9;
  return (
    (abC * abD < 0 && cdA * cdB < 0) ||
    (abC === 0 && within(c, a, b)) ||
    (abD === 0 && within(d, a, b)) ||
    (cdA === 0 && within(a, c, d)) ||
    (cdB === 0 && within(b, c, d))
  );
}

function area(points: readonly Point[]): number {
  if (points.length < 3) return 0;
  const origin = points[0];
  let twice = 0;
  for (let i = 1; i < points.length - 1; i++) twice += cross(origin, points[i], points[i + 1]);
  return Math.abs(twice) / 2;
}

function clip(points: readonly Point[], axis: 'x' | 'y', plane: number, sign: 1 | -1): Point[] {
  const output: Point[] = [];
  for (let i = 0; i < points.length; i++) {
    const a = points[i],
      b = points[(i + 1) % points.length];
    const distance = sign * (a[axis] - plane),
      next = sign * (b[axis] - plane);
    if (distance >= 0) output.push(a);
    if (distance >= 0 !== next >= 0) {
      const fraction = distance / (distance - next);
      const point = { x: a.x + fraction * (b.x - a.x), y: a.y + fraction * (b.y - a.y) };
      point[axis] = plane;
      output.push(point);
    }
  }
  return output;
}

export function selectLasso(rects: readonly Region[], outline: readonly Point[]): LassoResult {
  if (rects.length > 350 || outline.length > MAX_POINTS)
    return { kind: 'invalid', message: 'This selection exceeds the map limit. Use the item list.' };
  if (outline.some((p) => !Number.isFinite(p.x) || !Number.isFinite(p.y)))
    return {
      kind: 'invalid',
      message: 'The selection area is unavailable. Fit the map and try again.',
    };
  const points = outline.filter((p, i) => i === 0 || !equal(p, outline[i - 1]));
  if (points.length > 1 && equal(points[0], points[points.length - 1])) points.pop();
  if (points.length < 3) return { kind: 'invalid', message: 'Draw a loop with a visible area.' };
  for (let i = 0; i < points.length; i++) {
    const a = points[i],
      b = points[(i + 1) % points.length],
      c = points[(i + 2) % points.length];
    if (
      Math.abs(cross(a, b, c)) <= 1e-9 &&
      (a.x - b.x) * (c.x - b.x) + (a.y - b.y) * (c.y - b.y) > 0
    )
      return { kind: 'invalid', message: 'The outline crosses itself. Draw a simple loop.' };
    for (let j = i + 2; j < points.length; j++) {
      if (i === 0 && j === points.length - 1) continue;
      if (intersects(a, b, points[j], points[(j + 1) % points.length]))
        return { kind: 'invalid', message: 'The outline crosses itself. Draw a simple loop.' };
    }
  }
  if (area(points) === 0) return { kind: 'invalid', message: 'Draw a loop with a visible area.' };
  const minX = Math.min(...points.map((p) => p.x)),
    maxX = Math.max(...points.map((p) => p.x));
  const minY = Math.min(...points.map((p) => p.y)),
    maxY = Math.max(...points.map((p) => p.y));
  const ids: number[] = [];
  let skippedGroups = 0;
  for (const rect of rects) {
    if (
      rect.x >= maxX ||
      rect.y >= maxY ||
      rect.x + rect.width <= minX ||
      rect.y + rect.height <= minY
    )
      continue;
    let polygon = clip(points, 'x', rect.x, 1);
    polygon = clip(polygon, 'x', rect.x + rect.width, -1);
    polygon = clip(polygon, 'y', rect.y, 1);
    polygon = clip(polygon, 'y', rect.y + rect.height, -1);
    if (area(polygon) > 0) {
      if (rect.id < 0) skippedGroups++;
      else ids.push(rect.id);
    }
  }
  return { kind: 'selected', ids, skippedGroups };
}

export function lensCamera(camera: Camera, source: Point, center: Point): Camera {
  const world = screenToWorld(source, camera),
    scale = camera.scale * 2;
  return { x: center.x - world.x * scale, y: center.y - world.y * scale, scale };
}
