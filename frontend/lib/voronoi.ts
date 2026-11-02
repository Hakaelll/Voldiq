import type { Point } from './disk-city.ts';
import type { Rect } from './storage-map.ts';

export const VORONOI_GROUP_ID = Number.MIN_SAFE_INTEGER;
export const VORONOI_CELL_LIMIT = 120;
const AREA = 0.7;
type Vertex = Point & { neighbor: number };
type Site = Point & { weight: number };
export interface VoronoiCell {
  node: Rect;
  members: number[];
  polygon: Point[];
  anchor: Point;
  area: number;
  relativeError: number;
}
export interface VoronoiLayout {
  cells: VoronoiCell[];
  totalError: number | null;
  maximumError: number | null;
  groupedEntries: number;
  sourceEntries: number;
  iterations: number;
  clips: number;
  stop: 'converged' | 'budget' | 'stalled' | 'empty';
}
export interface VoronoiBudget {
  iterations?: number;
  clips?: number;
  expired?: () => boolean;
}
export type VoronoiReply =
  { kind: 'ready'; layout: VoronoiLayout } | { kind: 'error'; message: string };
type Item = { node: Rect; members: number[] };
type Diagram = { polygons: Vertex[][]; areas: number[]; anchors: Point[] };
type Edge = { i: number; j: number; value: number };
class BudgetReached extends Error {}

function workLimit(value: number | undefined, maximum: number): number {
  if (value === undefined) return maximum;
  if (!Number.isSafeInteger(value) || value < 0) throw new Error('Invalid Voronoi work budget.');
  return Math.min(value, maximum);
}

function prepare(rects: Rect[], total: number): { items: Item[]; grouped: number } {
  if (!rects.length && total === 0) return { items: [], grouped: 0 };
  const ids = new Set(rects.map((node) => node.id));
  if (
    rects.length > 350 ||
    ids.size !== rects.length ||
    ids.has(VORONOI_GROUP_ID) ||
    !Number.isFinite(total) ||
    total <= 0 ||
    rects.some((node) => !Number.isFinite(node.value) || node.value <= 0) ||
    Math.abs(rects.reduce((sum, node) => sum + node.value, 0) / total - 1) > 1e-9
  )
    throw new Error('This frontier cannot be drawn consistently. Use Treemap or List.');
  const sorted = [...rects].sort((a, b) => b.value - a.value || a.id - b.id);
  const head = sorted.filter(
    (node, index) => index < VORONOI_CELL_LIMIT - 1 && node.value / total >= 1e-5,
  );
  const tail = sorted.slice(head.length);
  const items: Item[] = head.map((node) => ({ node, members: [node.id] }));
  if (tail.length < 2) {
    for (const node of tail) items.push({ node, members: [node.id] });
    return { items, grouped: 0 };
  }
  const value = tail.reduce((sum, node) => sum + node.value, 0);
  const files = tail.reduce((sum, node) => sum + node.files, 0);
  const represented = tail.reduce((sum, node) => sum + node.represented, 0);
  const node: Rect = {
    id: VORONOI_GROUP_ID,
    name: `${tail.length} grouped frontier entries`,
    value,
    directory: true,
    x: 0,
    y: 0,
    width: 0,
    height: 0,
    share: value / total,
    depth: 0,
    modified: null,
    files,
    represented,
    aggregated: represented,
    allocation_known: tail.every((node) => node.allocation_known),
    category: 'remainder',
  };
  items.push({ node, members: tail.map((node) => node.id) });
  return { items, grouped: tail.length };
}

function clip(polygon: Vertex[], a: number, b: number, c: number, neighbor: number): Vertex[] {
  const output: Vertex[] = [];
  for (let k = 0; k < polygon.length; k++) {
    const end = polygon[k],
      start = polygon[(k + polygon.length - 1) % polygon.length];
    const s = a * start.x + b * start.y - c,
      e = a * end.x + b * end.y - c;
    if (s <= 0 !== e <= 0) {
      const t = s / (s - e);
      output.push({
        x: start.x + t * (end.x - start.x),
        y: start.y + t * (end.y - start.y),
        neighbor: s <= 0 ? end.neighbor : neighbor,
      });
    }
    if (e <= 0) output.push(end);
  }
  return output;
}

function stats(polygon: Point[]): { area: number; anchor: Point } {
  if (polygon.length < 3) return { area: 0, anchor: { x: 0, y: 0 } };
  const origin = polygon[0];
  let twice = 0,
    x = 0,
    y = 0;
  for (let k = 1; k < polygon.length - 1; k++) {
    const a = { x: polygon[k].x - origin.x, y: polygon[k].y - origin.y };
    const b = { x: polygon[k + 1].x - origin.x, y: polygon[k + 1].y - origin.y };
    const cross = a.x * b.y - a.y * b.x;
    twice += cross;
    x += (a.x + b.x) * cross;
    y += (a.y + b.y) * cross;
  }
  return {
    area: twice / 2,
    anchor: twice > 0 ? { x: origin.x + x / (3 * twice), y: origin.y + y / (3 * twice) } : origin,
  };
}

function diagram(sites: Site[], tick: () => void): Diagram {
  const polygons: Vertex[][] = [],
    areas: number[] = [],
    anchors: Point[] = [];
  for (let i = 0; i < sites.length; i++) {
    const p = sites[i];
    let polygon: Vertex[] = [
      { x: 0, y: 0, neighbor: -1 },
      { x: 1, y: 0, neighbor: -1 },
      { x: 1, y: 0.7, neighbor: -1 },
      { x: 0, y: 0.7, neighbor: -1 },
    ];
    for (let j = 0; j < sites.length && polygon.length; j++) {
      if (i === j) continue;
      tick();
      const q = sites[j];
      polygon = clip(
        polygon,
        2 * (q.x - p.x),
        2 * (q.y - p.y),
        q.x * q.x + q.y * q.y - p.x * p.x - p.y * p.y + p.weight - q.weight,
        j,
      );
    }
    const value = stats(polygon);
    polygons.push(polygon);
    areas.push(value.area);
    anchors.push(value.anchor);
  }
  return { polygons, areas, anchors };
}

function edges(current: Diagram, sites: Site[]): Edge[] {
  const values = new Map<number, Edge>();
  for (let i = 0; i < sites.length; i++) {
    const polygon = current.polygons[i];
    for (let k = 0; k < polygon.length; k++) {
      const end = polygon[k],
        start = polygon[(k + polygon.length - 1) % polygon.length],
        j = end.neighbor;
      if (j < 0 || j === i) continue;
      const low = Math.min(i, j),
        high = Math.max(i, j),
        key = low * sites.length + high;
      const value =
        Math.hypot(end.x - start.x, end.y - start.y) /
        (4 * Math.hypot(sites[i].x - sites[j].x, sites[i].y - sites[j].y));
      const prior = values.get(key);
      if (prior) prior.value += value;
      else values.set(key, { i: low, j: high, value });
    }
  }
  return [...values.values()];
}

function newton(current: Diagram, sites: Site[], residual: number[]): number[] | null {
  const graph = edges(current, sites),
    n = sites.length - 1;
  const diagonal = Array<number>(n).fill(0);
  for (const edge of graph) {
    if (edge.i < n) diagonal[edge.i] += edge.value;
    if (edge.j < n) diagonal[edge.j] += edge.value;
  }
  if (diagonal.some((value) => !(value > 0) || !Number.isFinite(value))) return null;
  const apply = (p: number[]): number[] => {
    const result = Array<number>(n).fill(0);
    for (const edge of graph) {
      const d = edge.value * ((p[edge.i] ?? 0) - (p[edge.j] ?? 0));
      if (edge.i < n) result[edge.i] += d;
      if (edge.j < n) result[edge.j] -= d;
    }
    return result;
  };
  const dot = (a: number[], b: number[]): number =>
    a.reduce((sum, value, i) => sum + value * b[i], 0);
  const x = Array<number>(n).fill(0),
    r = residual.slice(0, n);
  let z = r.map((value, i) => value / diagonal[i]),
    p = [...z],
    rz = dot(r, z);
  const tolerance = dot(r, r) * 1e-16 + 1e-28;
  for (
    let iteration = 0;
    iteration < Math.min(240, n * 2 + 8) && dot(r, r) > tolerance;
    iteration++
  ) {
    const ap = apply(p),
      denominator = dot(p, ap);
    if (!(denominator > 0)) return null;
    const alpha = rz / denominator;
    for (let i = 0; i < n; i++) {
      x[i] += alpha * p[i];
      r[i] -= alpha * ap[i];
    }
    z = r.map((value, i) => value / diagonal[i]);
    const next = dot(r, z),
      beta = next / rz;
    p = z.map((value, i) => value + beta * p[i]);
    rz = next;
  }
  return x.every(Number.isFinite) ? [...x, 0] : null;
}

function quality(
  current: Diagram,
  targets: number[],
): { total: number; maximum: number; norm: number } {
  let total = 0,
    maximum = 0,
    norm = 0;
  current.areas.forEach((area, i) => {
    const difference = Math.abs(area - targets[i]);
    total += difference;
    maximum = Math.max(maximum, difference / targets[i]);
    norm += difference * difference;
  });
  return { total: total / (2 * AREA), maximum, norm };
}

export function computeVoronoi(
  rects: Rect[],
  total: number,
  budget: VoronoiBudget = {},
): VoronoiLayout {
  const prepared = prepare(rects, total),
    items = prepared.items;
  if (!items.length)
    return {
      cells: [],
      totalError: null,
      maximumError: null,
      groupedEntries: 0,
      sourceEntries: 0,
      iterations: 0,
      clips: 0,
      stop: 'empty',
    };
  const targets = items.map((item) => (AREA * item.node.value) / total);
  if (targets.some((value) => value / AREA < 1e-14))
    throw new Error('This size range exceeds stable Voronoi precision. Use Treemap or List.');
  const columns = Math.ceil(Math.sqrt(items.length / AREA)),
    rows = Math.ceil(items.length / columns);
  let sites: Site[] = items.map((item, i) => {
    const seed = ((Math.imul(i + 1, 2654435761) ^ item.node.id) >>> 0) / 4294967296;
    return {
      x: ((i % columns) + 0.35 + 0.3 * seed) / columns,
      y: ((Math.floor(i / columns) + 0.35 + 0.3 * (1 - seed)) / rows) * 0.7,
      weight: 0,
    };
  });
  let clips = 0,
    iterations = 0,
    stop: VoronoiLayout['stop'] = 'stalled';
  const deadline = performance.now() + 700;
  const expired = budget.expired ?? (() => performance.now() > deadline);
  const maximumClips = workLimit(budget.clips, 1_500_000);
  const maximumIterations = workLimit(budget.iterations, 48);
  const tick = () => {
    if (clips >= maximumClips || (clips % 128 === 0 && expired())) throw new BudgetReached();
    clips++;
  };
  let current: Diagram;
  try {
    current = diagram(sites, tick);
  } catch (error) {
    if (error instanceof BudgetReached)
      throw new Error(
        'Voronoi calculation reached its budget before a complete diagram. Use Treemap or List.',
      );
    throw error;
  }
  let error = quality(current, targets);
  try {
    for (; iterations < maximumIterations; iterations++) {
      if (error.total < 0.0001 && error.maximum < 0.01) {
        stop = 'converged';
        break;
      }
      const residual = targets.map((target, i) => target - current.areas[i]);
      const direction = newton(current, sites, residual);
      if (!direction) break;
      let accepted = false;
      for (let power = 0; power < 24; power++) {
        const step = 2 ** -power;
        const nextSites = sites.map((site, i) => ({
          ...site,
          weight: site.weight + step * direction[i],
        }));
        const next = diagram(nextSites, tick),
          nextError = quality(next, targets);
        if (
          next.areas.every((area, i) => area >= Math.min(targets[i], current.areas[i]) * 0.1) &&
          nextError.norm < error.norm
        ) {
          sites = nextSites;
          current = next;
          error = nextError;
          accepted = true;
          break;
        }
      }
      if (!accepted) break;
    }
    if (iterations === maximumIterations) stop = 'budget';
  } catch (problem) {
    if (problem instanceof BudgetReached) stop = 'budget';
    else throw problem;
  }
  if (
    Math.abs(current.areas.reduce((sum, area) => sum + area, 0) / AREA - 1) > 1e-8 ||
    current.areas.some((area) => !(area > 0)) ||
    error.total > 0.002 ||
    error.maximum > 0.05
  )
    throw new Error(
      `Voronoi did not reach display precision (total area error ${(error.total * 100).toFixed(2)}%, worst cell ${(error.maximum * 100).toFixed(2)}%). Use Treemap or List.`,
    );
  return {
    cells: items.map((item, i) => ({
      ...item,
      polygon: current.polygons[i].map((point) => ({ x: point.x * 1000, y: point.y * 1000 })),
      anchor: { x: current.anchors[i].x * 1000, y: current.anchors[i].y * 1000 },
      area: current.areas[i] * 1_000_000,
      relativeError: (current.areas[i] - targets[i]) / targets[i],
    })),
    totalError: error.total,
    maximumError: error.maximum,
    groupedEntries: prepared.grouped,
    sourceEntries: rects.length,
    iterations,
    clips,
    stop,
  };
}

export function polygonContains(polygon: Point[], point: Point): boolean {
  return (
    polygon.length >= 3 &&
    polygon.every((end, k) => {
      const start = polygon[(k + polygon.length - 1) % polygon.length],
        dx = end.x - start.x,
        dy = end.y - start.y;
      return dx * (point.y - start.y) - dy * (point.x - start.x) >= -1e-9 * Math.hypot(dx, dy);
    })
  );
}

export function voronoiHit(layout: VoronoiLayout, point: Point): VoronoiCell | null {
  if (point.x < 0 || point.x > 1000 || point.y < 0 || point.y > 700) return null;
  return layout.cells.find((cell) => polygonContains(cell.polygon, point)) ?? null;
}
