import {
  project,
  worldToScreen,
  createPicker,
  type Camera,
  type Point,
  type Vec3,
} from './disk-city.ts';
import { bytes } from './types.ts';
import { visualPaint, type ColorMode, type Rect } from './storage-map.ts';
import { lensCamera } from './treemap-tools.ts';

type Scene = ReturnType<typeof import('./disk-city').buildCity>;
const pickers = new WeakMap<Scene, ReturnType<typeof createPicker>>();
type Box = { x: number; y: number; width: number; height: number };
export type MapStyle = { text: string; surface: string; border: string; dark: boolean };
export type PaintOptions = {
  rects: Rect[];
  camera: Camera;
  width: number;
  height: number;
  selected: number | null;
  selectedIds?: ReadonlySet<number>;
  hovered: number | null;
  matches: ReadonlySet<number> | null;
  color: ColorMode;
  maximum: number;
  now: number;
  detail: boolean;
  style: MapStyle;
};
const light: Vec3 = { x: -0.65, y: -0.4, z: -1 };

function path(ctx: CanvasRenderingContext2D, points: Point[], begin = true): void {
  if (begin) ctx.beginPath();
  for (let i = 0; i < points.length; i++) {
    if (i === 0) ctx.moveTo(points[i].x, points[i].y);
    else ctx.lineTo(points[i].x, points[i].y);
  }
  ctx.closePath();
}
function tint(hex: string, amount: number): string {
  const value = parseInt(hex.slice(1), 16);
  return `rgb(${Math.round((value >> 16) * amount)},${Math.round(((value >> 8) & 255) * amount)},${Math.round((value & 255) * amount)})`;
}
function bounds(points: Point[]): Box {
  const xs = points.map((p) => p.x),
    ys = points.map((p) => p.y);
  const x = Math.min(...xs),
    y = Math.min(...ys);
  return { x, y, width: Math.max(...xs) - x, height: Math.max(...ys) - y };
}
function intersects(a: Box, b: Box): boolean {
  return a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;
}
function hull(points: Vec3[]): Point[] {
  const list = points.map(project).sort((a, b) => a.x - b.x || a.y - b.y);
  const cross = (a: Point, b: Point, c: Point) =>
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
  const half = (items: Point[]) => {
    const out: Point[] = [];
    for (const p of items) {
      while (out.length >= 2 && cross(out[out.length - 2], out[out.length - 1], p) <= 0) out.pop();
      out.push(p);
    }
    out.pop();
    return out;
  };
  return [...half(list), ...half([...list].reverse())];
}
function corners(rect: Pick<Rect, 'x' | 'y' | 'width' | 'height'>, height: number): Vec3[] {
  const result: Vec3[] = [];
  for (const x of [rect.x, rect.x + rect.width])
    for (const y of [rect.y, rect.y + rect.height])
      for (const z of [0, height]) result.push({ x, y, z });
  return result;
}
function shadow(vertices: Vec3[], axis: 'x' | 'y' | 'z', plane: number): Point[] {
  const max = Math.max(...vertices.map((v) => v[axis]));
  if (max <= plane + 1e-6) return [];
  const clipped = vertices.map((v) => ({ ...v, [axis]: Math.max(plane, v[axis]) }));
  return hull(
    clipped.map((v) => {
      const distance = (plane - v[axis]) / light[axis];
      return {
        x: v.x + distance * light.x,
        y: v.y + distance * light.y,
        z: v.z + distance * light.z,
      };
    }),
  );
}

// Cache physical shadow polygons with geometry, independently of zoom, colors and selection.
export function prepareShadows(scene: Scene): Map<string, Point[][]> {
  const result = new Map<string, Point[][]>();
  const blocks = new Map<
    number,
    { node: Scene['faces'][number]['node']; vertices: Vec3[]; box: Box }
  >();
  for (const face of scene.faces) {
    if (face.kind !== 'roof' || blocks.has(face.node.id)) continue;
    const node = face.node;
    const vertices = corners(node, face.original[0].z);
    const footprint = shadow(vertices, 'z', 0);
    blocks.set(node.id, { node, vertices, box: bounds([...vertices.map(project), ...footprint]) });
    result.set(`ground:${node.id}`, [footprint]);
  }
  const grid = new Map<string, typeof blocks extends Map<number, infer T> ? T[] : never>();
  const keys = (box: Box): string[] => {
    const out: string[] = [];
    for (let x = Math.floor(box.x / 140); x <= Math.floor((box.x + box.width) / 140); x++)
      for (let y = Math.floor(box.y / 140); y <= Math.floor((box.y + box.height) / 140); y++)
        out.push(`${x},${y}`);
    return out;
  };
  for (const block of blocks.values())
    for (const key of keys(block.box)) {
      const bucket = grid.get(key) ?? [];
      bucket.push(block);
      grid.set(key, bucket);
    }
  for (const face of scene.faces) {
    const key = `${face.node.id}:${face.kind}`;
    if (result.has(key)) continue;
    const box = bounds(face.original.map(project));
    const candidates = new Set(keys(box).flatMap((key) => grid.get(key) ?? []));
    const axis = face.kind === 'roof' ? 'z' : face.kind === 'east' ? 'x' : 'y';
    result.set(
      key,
      [...candidates]
        .filter((block) => block.node.id !== face.node.id && intersects(box, block.box))
        .map((block) => shadow(block.vertices, axis, face.original[0][axis]))
        .filter((points) => points.length >= 3),
    );
  }
  return result;
}

function label(
  ctx: CanvasRenderingContext2D,
  rect: Rect,
  box: Box,
  options: PaintOptions,
  occupied: Box[],
): void {
  const { camera, width, height, selected, hovered } = options;
  const active = rect.id === selected || rect.id === hovered;
  const text = `${rect.name}${rect.id < 0 ? ' · group' : ''}`;
  ctx.font = '600 12px "Segoe UI", sans-serif';
  const textWidth = ctx.measureText(text).width;
  if (!active && (box.width < textWidth + 18 || box.height < 38)) return;
  const x = Math.max(
    6,
    Math.min(
      width - Math.min(textWidth + 20, 240) - 6,
      box.x + box.width / 2 - Math.min(textWidth + 20, 240) / 2,
    ),
  );
  const y = Math.max(6, Math.min(height - 44, box.y + box.height / 2 - 19));
  const area = { x, y, width: Math.min(textWidth + 20, 240), height: 38 };
  if (!active && occupied.some((other) => intersects(area, other))) return;
  occupied.push(area);
  ctx.fillStyle = options.style.surface;
  ctx.fillRect(x, y, area.width, area.height);
  ctx.strokeStyle = active ? options.style.text : options.style.border;
  ctx.lineWidth = active ? 2 : 1;
  ctx.strokeRect(x + 0.5, y + 0.5, area.width - 1, area.height - 1);
  ctx.save();
  ctx.beginPath();
  ctx.rect(x + 8, y, area.width - 16, 38);
  ctx.clip();
  ctx.fillStyle = options.style.text;
  ctx.fillText(text, x + 8, y + 15);
  ctx.font = '11px "Segoe UI", sans-serif';
  ctx.fillText(bytes(rect.value), x + 8, y + 30);
  ctx.restore();
  void camera;
}

export function paintFlat(ctx: CanvasRenderingContext2D, options: PaintOptions): number {
  const visible: { rect: Rect; box: Box }[] = [];
  for (const rect of options.rects) {
    const p = worldToScreen(rect, options.camera);
    const box = {
      ...p,
      width: rect.width * options.camera.scale,
      height: rect.height * options.camera.scale,
    };
    if (!intersects(box, { x: 0, y: 0, width: options.width, height: options.height })) continue;
    visible.push({ rect, box });
    const paint = visualPaint(rect, options.color, options.maximum, options.now);
    ctx.globalAlpha = options.matches && !options.matches.has(rect.id) ? 0.28 : 1;
    ctx.fillStyle = paint.fill;
    ctx.fillRect(box.x, box.y, box.width, box.height);
    ctx.strokeStyle = options.style.surface;
    ctx.lineWidth = 1;
    ctx.strokeRect(box.x, box.y, box.width, box.height);
    if (
      rect.id === options.selected ||
      options.selectedIds?.has(rect.id) ||
      rect.id === options.hovered
    ) {
      ctx.strokeStyle = paint.ink;
      ctx.lineWidth = rect.id === options.selected ? 4 : 2;
      ctx.strokeRect(box.x + 2, box.y + 2, Math.max(0, box.width - 4), Math.max(0, box.height - 4));
    }
  }
  ctx.globalAlpha = 1;
  const occupied: Box[] = [];
  for (const { rect, box } of visible.sort(
    (a, b) =>
      Number(b.rect.id === options.selected || b.rect.id === options.hovered) -
      Number(a.rect.id === options.selected || a.rect.id === options.hovered),
  ))
    label(ctx, rect, box, options, occupied);
  return visible.length;
}

export function paintTreemapLens(
  ctx: CanvasRenderingContext2D,
  options: PaintOptions,
  anchor: Point,
): void {
  const radius = Math.min(88, options.width / 2, options.height / 2);
  if (radius < 16) return;
  ctx.save();
  try {
    ctx.beginPath();
    ctx.arc(anchor.x, anchor.y, radius, 0, Math.PI * 2);
    ctx.clip();
    ctx.translate(anchor.x - radius, anchor.y - radius);
    ctx.fillStyle = options.style.surface;
    ctx.fillRect(0, 0, radius * 2, radius * 2);
    paintFlat(ctx, {
      ...options,
      width: radius * 2,
      height: radius * 2,
      camera: lensCamera(options.camera, anchor, { x: radius, y: radius }),
    });
  } finally {
    ctx.restore();
  }
  ctx.save();
  ctx.beginPath();
  ctx.arc(anchor.x, anchor.y, radius, 0, Math.PI * 2);
  ctx.strokeStyle = options.style.text;
  ctx.lineWidth = 3;
  ctx.stroke();
  ctx.strokeStyle = options.style.surface;
  ctx.lineWidth = 1;
  ctx.stroke();
  ctx.beginPath();
  ctx.moveTo(anchor.x - 4, anchor.y);
  ctx.lineTo(anchor.x + 4, anchor.y);
  ctx.moveTo(anchor.x, anchor.y - 4);
  ctx.lineTo(anchor.x, anchor.y + 4);
  ctx.strokeStyle = options.style.text;
  ctx.stroke();
  ctx.restore();
}

export function paintCity(
  ctx: CanvasRenderingContext2D,
  scene: Scene,
  shadows: Map<string, Point[][]>,
  options: PaintOptions,
): number {
  const screen = (point: Point) => worldToScreen(point, options.camera);
  const floor = [
    project({ x: 0, y: 0, z: 0 }),
    project({ x: 1000, y: 0, z: 0 }),
    project({ x: 1000, y: 700, z: 0 }),
    project({ x: 0, y: 700, z: 0 }),
  ].map(screen);
  path(ctx, floor);
  ctx.fillStyle = options.style.surface;
  ctx.fill();
  ctx.save();
  path(ctx, floor);
  ctx.clip();
  if (options.detail) {
    ctx.beginPath();
    for (const [key, shapes] of shadows) {
      if (!key.startsWith('ground:')) continue;
      for (const shape of shapes) {
        path(ctx, shape.map(screen), false);
      }
    }
    ctx.fillStyle = options.style.dark ? '#00000024' : '#26332b12';
    ctx.fill();
  }
  ctx.restore();
  const visible = new Set<number>();
  const nodeById = new Map(options.rects.map((rect) => [rect.id, rect]));
  const roofBoxes = new Map<number, Box>();
  let windowBudget = 1800;
  for (const face of scene.faces) {
    const points = face.vertices.map((v) => screen(project(v)));
    const box = bounds(points);
    if (!intersects(box, { x: 0, y: 0, width: options.width, height: options.height })) continue;
    const rect = nodeById.get(face.node.id);
    if (!rect) continue;
    visible.add(rect.id);
    const paint = visualPaint(rect, options.color, options.maximum, options.now);
    const active =
      rect.id === options.selected ||
      options.selectedIds?.has(rect.id) ||
      rect.id === options.hovered;
    ctx.save();
    path(ctx, points);
    ctx.clip();
    ctx.globalAlpha = options.matches && !options.matches.has(rect.id) ? 0.32 : 1;
    ctx.fillStyle = tint(paint.fill, face.kind === 'roof' ? 1 : face.kind === 'east' ? 0.82 : 0.64);
    ctx.fillRect(box.x - 1, box.y - 1, box.width + 2, box.height + 2);
    if (options.detail) {
      ctx.beginPath();
      for (const shape of shadows.get(`${rect.id}:${face.kind}`) ?? []) {
        path(ctx, shape.map(screen), false);
      }
      ctx.fillStyle = '#14251d1c';
      ctx.fill();
      if (face.kind !== 'roof' && box.width > 25 && box.height > 25) {
        const full = face.original;
        const base = full.map((v) => screen(project(v)));
        ctx.strokeStyle = '#ffffff20';
        ctx.lineWidth = 0.7;
        const floors = Math.min(
          12,
          Math.floor((Math.abs(full[0].z - full[2].z) * options.camera.scale) / 9),
        );
        for (let step = 1; step < floors; step++) {
          const z = (Math.max(...full.map((v) => v.z)) * step) / floors;
          const corners = full.filter((v) => v.z === 0);
          if (corners.length === 2) {
            const a = screen(project({ ...corners[0], z })),
              b = screen(project({ ...corners[1], z }));
            ctx.beginPath();
            ctx.moveTo(a.x, a.y);
            ctx.lineTo(b.x, b.y);
            ctx.stroke();
          }
        }
        const top = Math.max(...full.map((v) => v.z));
        const bottom = full.filter((v) => v.z === 0);
        if (bottom.length === 2) {
          const panels = Math.min(
            15,
            Math.floor(Math.hypot(base[0].x - base[1].x, base[0].y - base[1].y) / 12),
          );
          if (box.width > 48 && box.height > 36 && floors >= 2 && panels >= 2) {
            const pane = (u: number, z: number): Vec3 => ({
              x: bottom[0].x + (bottom[1].x - bottom[0].x) * u,
              y: bottom[0].y + (bottom[1].y - bottom[0].y) * u,
              z,
            });
            for (let row = 0; row < floors && windowBudget > 0; row++) {
              for (let column = 0; column < panels && windowBudget > 0; column++) {
                const seed = Math.imul(rect.id + 1, 2654435761) ^ (row * 53 + column * 97);
                if ((seed & 7) === 0) continue;
                const u0 = (column + 0.2) / panels,
                  u1 = (column + 0.76) / panels;
                const z0 = (top * (row + 0.2)) / floors,
                  z1 = (top * (row + 0.72)) / floors;
                path(
                  ctx,
                  [pane(u0, z0), pane(u1, z0), pane(u1, z1), pane(u0, z1)].map((v) =>
                    screen(project(v)),
                  ),
                );
                ctx.fillStyle = (seed & 3) === 0 ? '#ffffff21' : '#162a2525';
                ctx.fill();
                windowBudget--;
              }
            }
          }
          for (let step = 1; step < panels; step++) {
            const p = {
              x: bottom[0].x + ((bottom[1].x - bottom[0].x) * step) / panels,
              y: bottom[0].y + ((bottom[1].y - bottom[0].y) * step) / panels,
              z: 0,
            };
            const a = screen(project(p)),
              b = screen(project({ ...p, z: top }));
            ctx.beginPath();
            ctx.moveTo(a.x, a.y);
            ctx.lineTo(b.x, b.y);
            ctx.stroke();
          }
        }
      }
      if (face.kind === 'roof' && box.width > 70 && box.height > 35 && !active) {
        const roof = face.original[0].z;
        const p = { x: rect.x + rect.width * 0.18, y: rect.y + rect.height * 0.18, z: roof };
        const w = Math.min(20, rect.width * 0.18),
          h = Math.min(16, rect.height * 0.18);
        if ((rect.id >>> 0) % 3 !== 0) {
          path(
            ctx,
            [p, { ...p, x: p.x + w }, { ...p, x: p.x + w, y: p.y + h }, { ...p, y: p.y + h }].map(
              (v) => screen(project(v)),
            ),
          );
          ctx.fillStyle = '#14251d38';
          ctx.fill();
          ctx.strokeStyle = '#ffffff50';
          ctx.lineWidth = 1;
          ctx.stroke();
          const rise = 3;
          const top = [
            p,
            { ...p, x: p.x + w },
            { ...p, x: p.x + w, y: p.y + h },
            { ...p, y: p.y + h },
          ].map((v) => ({ ...v, z: v.z + rise }));
          for (const [a, b] of [
            [1, 2],
            [2, 3],
          ]) {
            path(
              ctx,
              [{ ...top[a], z: roof }, { ...top[b], z: roof }, top[b], top[a]].map((v) =>
                screen(project(v)),
              ),
            );
            ctx.fillStyle = tint(paint.fill, 0.65);
            ctx.fill();
          }
          path(
            ctx,
            top.map((v) => screen(project(v))),
          );
          ctx.fillStyle = tint(paint.fill, 0.91);
          ctx.fill();
          ctx.strokeStyle = '#ffffff4d';
          ctx.stroke();
        }
      }
      if (face.kind === 'roof' && rect.aggregated > 1 && box.width > 30 && box.height > 18) {
        ctx.strokeStyle = '#14251d20';
        ctx.lineWidth = 0.7;
        for (let offset = 0; offset < Math.min(box.width + box.height, 1200); offset += 10) {
          ctx.beginPath();
          ctx.moveTo(box.x + offset, box.y);
          ctx.lineTo(box.x + offset - box.height, box.y + box.height);
          ctx.stroke();
        }
      }
    }
    path(
      ctx,
      face.original.map((v) => screen(project(v))),
    );
    ctx.strokeStyle = active ? paint.ink : face.kind === 'roof' ? '#ffffff75' : '#14251d38';
    ctx.lineWidth = active ? (rect.id === options.selected ? 3.5 : 2) : 1;
    ctx.stroke();
    ctx.restore();
    if (face.kind === 'roof')
      roofBoxes.set(rect.id, bounds(face.original.map((v) => screen(project(v)))));
  }
  const occupied: Box[] = [];
  let pick = pickers.get(scene);
  if (!pick) {
    pick = createPicker(scene.faces);
    pickers.set(scene, pick);
  }
  for (const rect of [...options.rects].sort(
    (a, b) =>
      Number(b.id === options.selected || b.id === options.hovered) -
      Number(a.id === options.selected || a.id === options.hovered),
  )) {
    const box = roofBoxes.get(rect.id);
    if (box) {
      const roof = scene.faces.find((face) => face.kind === 'roof' && face.node.id === rect.id);
      if (!roof) continue;
      const center = project({
        x: rect.x + rect.width / 2,
        y: rect.y + rect.height / 2,
        z: roof.original[0].z,
      });
      const front = pick(center);
      if (front?.node.id === rect.id && front.kind === 'roof')
        label(ctx, rect, box, options, occupied);
    }
  }
  return visible.size;
}
