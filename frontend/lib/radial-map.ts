import type { createStorageLayout, Rect } from './storage-map.ts';
import type { Point } from './disk-city.ts';

export type RadialView = 'sunburst' | 'circles';
type Source = ReturnType<typeof createStorageLayout>;
type ShapeNode = { node: Rect; structural: boolean };
export type RadialShape = ShapeNode &
  (
    | { kind: 'circle'; x: number; y: number; radius: number }
    | {
        kind: 'sector';
        x: number;
        y: number;
        inner: number;
        outer: number;
        start: number;
        end: number;
      }
  );
export interface RadialLayout {
  shapes: RadialShape[];
  dataFraction: number;
}
type Branch = ShapeNode & { children: Branch[] };
type Packed = { radius: number; shapes: RadialShape[]; id: number };
const TAU = Math.PI * 2;
const RADIUS = 330;
const CENTER = { x: 500, y: 350 };
export const RADIAL_BOUNDS = {
  x: CENTER.x - RADIUS,
  y: CENTER.y - RADIUS,
  width: 2 * RADIUS,
  height: 2 * RADIUS,
};
const MAX_SHAPES = 2100;

function hierarchy(source: Source): { roots: Branch[]; depth: number } {
  if (
    !source.rects.length &&
    !source.boundaries.length &&
    !source.parents.size &&
    source.total === 0
  )
    return { roots: [], depth: 0 };
  if (
    !Number.isFinite(source.total) ||
    source.total <= 0 ||
    source.rects.length > 350 ||
    source.rects.length + source.boundaries.length > MAX_SHAPES
  )
    throw new Error('This hierarchy exceeds the drawing limit. Use the item list.');
  const structural = new Set(source.boundaries.map((node) => node.id));
  const nodes = new Map<number, Branch>();
  for (const node of [...source.boundaries, ...source.rects]) {
    if (nodes.has(node.id) || !Number.isFinite(node.value) || node.value <= 0) throw invalid();
    nodes.set(node.id, { node, structural: structural.has(node.id), children: [] });
  }
  if (nodes.size !== source.parents.size) throw invalid();
  const roots: Branch[] = [];
  for (const branch of nodes.values()) {
    if (!source.parents.has(branch.node.id)) throw invalid();
    const parent = source.parents.get(branch.node.id);
    if (parent === null) roots.push(branch);
    else {
      const ancestor = parent === undefined ? undefined : nodes.get(parent);
      if (!ancestor?.structural) throw invalid();
      ancestor.children.push(branch);
    }
  }
  const seen = new Set<number>();
  let depth = 0;
  const visit = (branch: Branch, level: number): void => {
    if (seen.has(branch.node.id) || level > 6 || branch.node.depth !== level) throw invalid();
    seen.add(branch.node.id);
    depth = Math.max(depth, level);
    branch.children.sort((a, b) => b.node.value - a.node.value || a.node.id - b.node.id);
    if (
      branch.structural &&
      (!branch.children.length ||
        !equal(
          branch.children.reduce((sum, child) => sum + child.node.value, 0),
          branch.node.value,
        ))
    )
      throw invalid();
    for (const child of branch.children) visit(child, level + 1);
  };
  roots.sort((a, b) => b.node.value - a.node.value || a.node.id - b.node.id);
  for (const root of roots) visit(root, 1);
  if (
    seen.size !== nodes.size ||
    !equal(
      roots.reduce((sum, branch) => sum + branch.node.value, 0),
      source.total,
    )
  )
    throw invalid();
  return { roots, depth };
}

function equal(a: number, b: number): boolean {
  return Math.abs(a - b) <= Math.max(1e-10, Math.abs(b) * 1e-10);
}
function invalid(): Error {
  return new Error('This hierarchy cannot be drawn consistently. Use the item list.');
}

// Shelves use disjoint bounding boxes. Enclosing circles represent groups, never extra bytes.
function pack(items: Packed[], padding: boolean): Packed {
  if (!items.length) return { radius: 0, shapes: [], id: -1 };
  const sorted = [...items].sort((a, b) => b.radius - a.radius || a.id - b.id);
  const maximum = Math.max(...sorted.map((item) => item.radius));
  const target = Math.max(
    2 * maximum,
    Math.sqrt(8 * sorted.reduce((sum, item) => sum + item.radius ** 2, 0)),
  );
  const rows: { items: { item: Packed; x: number }[]; width: number; height: number }[] = [];
  for (const item of sorted) {
    let row = rows[rows.length - 1];
    if (!row || (row.width > 0 && row.width + 2 * item.radius > target)) {
      row = { items: [], width: 0, height: 2 * item.radius };
      rows.push(row);
    }
    row.items.push({ item, x: row.width + item.radius });
    row.width += 2 * item.radius;
  }
  const width = Math.max(...rows.map((row) => row.width));
  const height = rows.reduce((sum, row) => sum + row.height, 0);
  const shapes: RadialShape[] = [];
  let radius = 0,
    y = 0;
  for (const row of rows) {
    for (const { item, x } of row.items) {
      const dx = x + (width - row.width) / 2 - width / 2;
      const dy = y + row.height / 2 - height / 2;
      radius = Math.max(radius, Math.hypot(dx, dy) + item.radius);
      for (const shape of item.shapes) shapes.push({ ...shape, x: shape.x + dx, y: shape.y + dy });
    }
    y += row.height;
  }
  return { radius: radius + (padding ? maximum * 0.04 : 0), shapes, id: -1 };
}

export function radialLayout(source: Source, mode: RadialView): RadialLayout {
  const tree = hierarchy(source);
  if (!tree.roots.length) return { shapes: [], dataFraction: 0 };
  let shapes: RadialShape[];
  if (mode === 'circles') {
    const build = (branch: Branch): Packed => {
      if (!branch.structural) {
        const radius = Math.sqrt(branch.node.value / source.total);
        if (!(radius > 0))
          throw new Error('The size range exceeds radial precision. Use the item list.');
        return {
          radius,
          id: branch.node.id,
          shapes: [{ kind: 'circle', node: branch.node, structural: false, x: 0, y: 0, radius }],
        };
      }
      const packed = pack(branch.children.map(build), true);
      return {
        ...packed,
        id: branch.node.id,
        shapes: [
          {
            kind: 'circle',
            node: branch.node,
            structural: true,
            x: 0,
            y: 0,
            radius: packed.radius,
          },
          ...packed.shapes,
        ],
      };
    };
    const root = pack(tree.roots.map(build), false);
    const scale = RADIUS / root.radius;
    shapes = root.shapes.map((shape) => {
      if (shape.kind !== 'circle') throw invalid();
      return {
        ...shape,
        x: CENTER.x + shape.x * scale,
        y: CENTER.y + shape.y * scale,
        radius: shape.radius * scale,
      };
    });
  } else {
    shapes = [];
    const hole = RADIUS * 0.16;
    const band = (RADIUS ** 2 - hole ** 2) / tree.depth;
    const visit = (branches: Branch[], start: number, end: number, total: number): void => {
      let consumed = 0;
      branches.forEach((branch, index) => {
        const cursor = start + (end - start) * (consumed / total);
        consumed += branch.node.value;
        const next =
          index === branches.length - 1 ? end : start + (end - start) * (consumed / total);
        if (!(next > cursor))
          throw new Error('The size range exceeds radial precision. Use the item list.');
        shapes.push({
          kind: 'sector',
          ...CENTER,
          node: branch.node,
          structural: branch.structural,
          inner: Math.sqrt(hole ** 2 + (branch.node.depth - 1) * band),
          outer: branch.structural ? Math.sqrt(hole ** 2 + branch.node.depth * band) : RADIUS,
          start: cursor,
          end: next,
        });
        if (branch.children.length) visit(branch.children, cursor, next, branch.node.value);
      });
    };
    visit(tree.roots, -Math.PI / 2, Math.PI * 1.5, source.total);
  }
  shapes.sort((a, b) => a.node.depth - b.node.depth || a.node.id - b.node.id);
  const dataFraction =
    shapes.reduce((sum, shape) => sum + (shape.structural ? 0 : shapeArea(shape)), 0) /
    (Math.PI * RADIUS ** 2);
  return { shapes, dataFraction: Math.max(0, Math.min(1, dataFraction)) };
}

export function shapeArea(shape: RadialShape): number {
  return shape.kind === 'circle'
    ? Math.PI * shape.radius ** 2
    : ((shape.outer ** 2 - shape.inner ** 2) * (shape.end - shape.start)) / 2;
}

export function shapeAnchor(shape: RadialShape): Point {
  if (shape.kind === 'circle') return { x: shape.x, y: shape.y };
  const angle = (shape.start + shape.end) / 2;
  const radius = (shape.inner + shape.outer) / 2;
  return { x: shape.x + Math.cos(angle) * radius, y: shape.y + Math.sin(angle) * radius };
}

export function radialHit(layout: RadialLayout, point: Point): Rect | null {
  for (let index = layout.shapes.length - 1; index >= 0; index--) {
    const shape = layout.shapes[index];
    const distance = Math.hypot(point.x - shape.x, point.y - shape.y);
    if (shape.kind === 'circle') {
      if (distance <= shape.radius) return shape.node;
    } else if (distance >= shape.inner && distance < shape.outer) {
      const offset =
        (((Math.atan2(point.y - shape.y, point.x - shape.x) - shape.start) % TAU) + TAU) % TAU;
      if (offset < shape.end - shape.start) return shape.node;
    }
  }
  return null;
}
