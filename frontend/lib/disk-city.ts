import type { Rect } from './treemap.ts';

export type Point = { x: number; y: number };
export type Vec3 = Point & { z: number };
export type Bounds = Point & { width: number; height: number };
export type Camera = Point & { scale: number };
export type Face = {
  node: Rect;
  kind: 'roof' | 'east' | 'south';
  vertices: Vec3[];
  original: Vec3[];
};
type Axis = 'x' | 'y' | 'z';
type Polygon = { face: Face; min: Vec3; max: Vec3 };
type Plane = { axis: Axis; value: number };
const horizontal = Math.sqrt(3) / 2;
const axes: Axis[] = ['x', 'y', 'z'];

export function project(point: Vec3): Point {
  return { x: horizontal * (point.x - point.y), y: (point.x + point.y) / 2 - point.z };
}

function polygon(face: Face): Polygon {
  const min = { x: Infinity, y: Infinity, z: Infinity };
  const max = { x: -Infinity, y: -Infinity, z: -Infinity };
  for (const vertex of face.vertices) {
    for (const axis of axes) {
      min[axis] = Math.min(min[axis], vertex[axis]);
      max[axis] = Math.max(max[axis], vertex[axis]);
    }
  }
  return { face, min, max };
}

function faceAxis(face: Face): Axis {
  return face.kind === 'roof' ? 'z' : face.kind === 'east' ? 'x' : 'y';
}

// Every candidate is a polygon's support plane, so each partition removes
// coplanar polygons. Bounding intervals classify whole polygons before clipping.
function partitionPlane(polygons: Polygon[]): Plane {
  let best: Plane = { axis: 'z', value: polygons[0].min.z };
  let bestScore = Infinity;
  for (const axis of axes) {
    const values = polygons
      .filter(({ face }) => faceAxis(face) === axis)
      .map(({ min }) => min[axis])
      .sort((a, b) => a - b);
    if (!values.length) continue;
    for (const fraction of [0.25, 0.5, 0.75]) {
      const value = values[Math.floor((values.length - 1) * fraction)];
      let far = 0;
      let near = 0;
      let crossing = 0;
      for (const item of polygons) {
        if (item.max[axis] < value) far++;
        else if (item.min[axis] > value) near++;
        else if (item.min[axis] < value && item.max[axis] > value) crossing++;
        else if (item.min[axis] < value) far++;
        else if (item.max[axis] > value) near++;
      }
      const score = crossing * 4 + Math.abs(far - near);
      if (score < bestScore) {
        bestScore = score;
        best = { axis, value };
      }
    }
  }
  return best;
}

function split(item: Polygon, { axis, value }: Plane): [Polygon, Polygon] {
  const far: Vec3[] = [];
  const near: Vec3[] = [];
  const vertices = item.face.vertices;
  for (let index = 0; index < vertices.length; index++) {
    const a = vertices[index];
    const b = vertices[(index + 1) % vertices.length];
    const distance = a[axis] - value;
    const nextDistance = b[axis] - value;
    if (distance <= 0) far.push(a);
    if (distance >= 0) near.push(a);
    if ((distance < 0 && nextDistance > 0) || (distance > 0 && nextDistance < 0)) {
      const fraction = distance / (distance - nextDistance);
      const intersection = {
        x: a.x + (b.x - a.x) * fraction,
        y: a.y + (b.y - a.y) * fraction,
        z: a.z + (b.z - a.z) * fraction,
        [axis]: value,
      };
      far.push(intersection);
      near.push(intersection);
    }
  }
  return [polygon({ ...item.face, vertices: far }), polygon({ ...item.face, vertices: near })];
}

export function buildCity(
  rects: Rect[],
  height: (node: Rect) => number,
): {
  faces: Face[];
  bounds: Bounds;
  fragments: number;
  timings: { projectionMs: number; orderingMs: number };
} {
  const started = performance.now();
  const input: Polygon[] = [];
  for (const node of rects) {
    const { x, y, width, height: depth } = node;
    const z = height(node);
    if (![x, y, width, depth, z].every(Number.isFinite) || width <= 0 || depth <= 0 || z < 0)
      throw new RangeError('Invalid disk city prism');
    const right = x + width;
    const bottom = y + depth;
    const add = (kind: Face['kind'], original: Vec3[]): void => {
      input.push(polygon({ node, kind, vertices: original, original }));
    };
    add('roof', [
      { x, y, z },
      { x: right, y, z },
      { x: right, y: bottom, z },
      { x, y: bottom, z },
    ]);
    if (z > 0) {
      add('east', [
        { x: right, y, z: 0 },
        { x: right, y: bottom, z: 0 },
        { x: right, y: bottom, z },
        { x: right, y, z },
      ]);
      add('south', [
        { x, y: bottom, z: 0 },
        { x: right, y: bottom, z: 0 },
        { x: right, y: bottom, z },
        { x, y: bottom, z },
      ]);
    }
  }
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const { face } of input) {
    for (const vertex of face.vertices) {
      const point = project(vertex);
      minX = Math.min(minX, point.x);
      minY = Math.min(minY, point.y);
      maxX = Math.max(maxX, point.x);
      maxY = Math.max(maxY, point.y);
    }
  }
  const projected = performance.now();
  const faces: Face[] = [];
  let fragments = input.length;
  const limit = Math.min(150_000, Math.max(12_000, input.length * 32));
  type Work = { kind: 'partition'; polygons: Polygon[] } | { kind: 'emit'; faces: Face[] };
  const pending: Work[] = [{ kind: 'partition', polygons: input }];
  while (pending.length) {
    const work = pending.pop();
    if (!work) break;
    if (work.kind === 'emit') {
      for (const face of work.faces) faces.push(face);
      continue;
    }
    if (!work.polygons.length) continue;
    const plane = partitionPlane(work.polygons);
    const far: Polygon[] = [];
    const near: Polygon[] = [];
    const coplanar: Face[] = [];
    for (const item of work.polygons) {
      const low = item.min[plane.axis];
      const high = item.max[plane.axis];
      if (low === plane.value && high === plane.value) coplanar.push(item.face);
      else if (high <= plane.value) far.push(item);
      else if (low >= plane.value) near.push(item);
      else {
        if (++fragments > limit)
          throw new RangeError('Disk city geometry is too complex; use the flat map');
        const [back, front] = split(item, plane);
        far.push(back);
        near.push(front);
      }
    }
    // The eye is in (+x,+y,+z). A ray crosses every axis plane from
    // its negative half-space to its positive half-space, at most once.
    pending.push(
      { kind: 'partition', polygons: near },
      { kind: 'emit', faces: coplanar },
      { kind: 'partition', polygons: far },
    );
  }
  return {
    faces,
    bounds: input.length
      ? { x: minX, y: minY, width: maxX - minX, height: maxY - minY }
      : { x: 0, y: 0, width: 0, height: 0 },
    fragments,
    timings: { projectionMs: projected - started, orderingMs: performance.now() - projected },
  };
}

export function pointInPolygon(point: Point, vertices: Point[]): boolean {
  let inside = false;
  for (let index = 0, previous = vertices.length - 1; index < vertices.length; previous = index++) {
    const a = vertices[previous];
    const b = vertices[index];
    const cross = (point.x - a.x) * (b.y - a.y) - (point.y - a.y) * (b.x - a.x);
    if (
      Math.abs(cross) <= 1e-9 &&
      point.x >= Math.min(a.x, b.x) - 1e-9 &&
      point.x <= Math.max(a.x, b.x) + 1e-9 &&
      point.y >= Math.min(a.y, b.y) - 1e-9 &&
      point.y <= Math.max(a.y, b.y) + 1e-9
    )
      return true;
    if (
      a.y > point.y !== b.y > point.y &&
      point.x < ((b.x - a.x) * (point.y - a.y)) / (b.y - a.y) + a.x
    )
      inside = !inside;
  }
  return inside;
}

export function createPicker(faces: Face[]): (point: Point) => Face | null {
  if (!faces.length) return () => null;
  const projected = faces.map((face) => face.vertices.map(project));
  const boxes = projected.map((vertices) => ({
    minX: Math.min(...vertices.map((point) => point.x)),
    maxX: Math.max(...vertices.map((point) => point.x)),
    minY: Math.min(...vertices.map((point) => point.y)),
    maxY: Math.max(...vertices.map((point) => point.y)),
  }));
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const box of boxes) {
    minX = Math.min(minX, box.minX);
    minY = Math.min(minY, box.minY);
    maxX = Math.max(maxX, box.maxX);
    maxY = Math.max(maxY, box.maxY);
  }
  const side = Math.max(1, Math.min(128, Math.ceil(Math.sqrt(faces.length))));
  const width = maxX - minX || 1;
  const height = maxY - minY || 1;
  const cellX = (x: number): number =>
    Math.max(0, Math.min(side - 1, Math.floor(((x - minX) / width) * side)));
  const cellY = (y: number): number =>
    Math.max(0, Math.min(side - 1, Math.floor(((y - minY) / height) * side)));
  const bins = new Map<number, number[]>();
  for (let index = 0; index < boxes.length; index++) {
    const box = boxes[index];
    for (let y = cellY(box.minY); y <= cellY(box.maxY); y++) {
      for (let x = cellX(box.minX); x <= cellX(box.maxX); x++) {
        const key = y * side + x;
        const bin = bins.get(key);
        if (bin) bin.push(index);
        else bins.set(key, [index]);
      }
    }
  }
  return (point) => {
    if (point.x < minX || point.x > maxX || point.y < minY || point.y > maxY) return null;
    const bin = bins.get(cellY(point.y) * side + cellX(point.x));
    if (!bin) return null;
    for (let cursor = bin.length - 1; cursor >= 0; cursor--) {
      const index = bin[cursor];
      const box = boxes[index];
      if (
        point.x >= box.minX &&
        point.x <= box.maxX &&
        point.y >= box.minY &&
        point.y <= box.maxY &&
        pointInPolygon(point, projected[index])
      )
        return faces[index];
    }
    return null;
  };
}

export function pickFace(faces: Face[], point: Point): Face | null {
  for (let index = faces.length - 1; index >= 0; index--) {
    if (pointInPolygon(point, faces[index].vertices.map(project))) return faces[index];
  }
  return null;
}

export function fitCamera(bounds: Bounds, width: number, height: number): Camera {
  const scale = Math.min(
    Math.max(1, width - 48) / Math.max(1, bounds.width),
    Math.max(1, height - 48) / Math.max(1, bounds.height),
  );
  return {
    x: width / 2 - (bounds.x + bounds.width / 2) * scale,
    y: height / 2 - (bounds.y + bounds.height / 2) * scale,
    scale,
  };
}

export function worldToScreen(point: Point, camera: Camera): Point {
  return { x: point.x * camera.scale + camera.x, y: point.y * camera.scale + camera.y };
}

export function screenToWorld(point: Point, camera: Camera): Point {
  return { x: (point.x - camera.x) / camera.scale, y: (point.y - camera.y) / camera.scale };
}

export function zoomCamera(
  camera: Camera,
  cursor: Point,
  factor: number,
  min: number,
  max: number,
): Camera {
  const scale = Math.max(min, Math.min(max, camera.scale * factor));
  const world = screenToWorld(cursor, camera);
  return { x: cursor.x - world.x * scale, y: cursor.y - world.y * scale, scale };
}
