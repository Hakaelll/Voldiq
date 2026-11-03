import type { PaintSurface } from './paint-surface.ts';
import { worldToScreen } from './disk-city.ts';
import { visualPaint } from './storage-map.ts';
import { bytes } from './types.ts';
import { polygonContains, type VoronoiLayout } from './voronoi.ts';
import type { PaintOptions } from './map-painter.ts';

export function paintVoronoi(
  ctx: PaintSurface,
  layout: VoronoiLayout,
  options: PaintOptions,
): number {
  let drawn = 0;
  for (const cell of layout.cells) {
    const points = cell.polygon.map((point) => worldToScreen(point, options.camera));
    if (
      points.every((point) => point.x < 0) ||
      points.every((point) => point.y < 0) ||
      points.every((point) => point.x > options.width) ||
      points.every((point) => point.y > options.height)
    )
      continue;
    const node = cell.node,
      paint = visualPaint(node, options.color, options.maximum, options.now);
    const active =
      node.id === options.hovered ||
      cell.members.some((id) => id === options.selected || options.selectedIds?.has(id));
    const match = !options.matches || cell.members.some((id) => options.matches?.has(id));
    ctx.save();
    ctx.beginPath();
    points.forEach((point, i) => (i ? ctx.lineTo(point.x, point.y) : ctx.moveTo(point.x, point.y)));
    ctx.closePath();
    ctx.globalAlpha = match ? 1 : 0.28;
    ctx.fillStyle = paint.fill;
    ctx.fill();
    ctx.strokeStyle = active ? paint.ink : options.style.surface;
    ctx.lineWidth = active ? 3 : 1;
    ctx.stroke();
    drawn++;
    ctx.clip();
    if (options.detail || active) {
      const anchor = worldToScreen(cell.anchor, options.camera);
      ctx.font = '600 12px "Segoe UI", sans-serif';
      const value = bytes(node.value),
        width = Math.max(ctx.measureText(node.name).width, ctx.measureText(value).width) + 10;
      const fits = (height: number) =>
        [-1, 1].every((x) =>
          [-1, 1].every((y) =>
            polygonContains(points, {
              x: anchor.x + (x * width) / 2,
              y: anchor.y + (y * height) / 2,
            }),
          ),
        );
      const twoLines = fits(38);
      if (twoLines || fits(18)) {
        ctx.fillStyle = paint.ink;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText(node.name, anchor.x, anchor.y - (twoLines ? 7 : 0));
        if (twoLines) {
          ctx.font = '11px "Segoe UI", sans-serif';
          ctx.fillText(value, anchor.x, anchor.y + 9);
        }
      }
    }
    ctx.restore();
  }
  return drawn;
}
