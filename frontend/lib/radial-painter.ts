import { shapeAnchor, type RadialLayout, type RadialShape } from './radial-map.ts';
import { worldToScreen } from './disk-city.ts';
import { visualPaint } from './storage-map.ts';
import { bytes } from './types.ts';
import type { PaintOptions } from './map-painter.ts';

function trace(ctx: CanvasRenderingContext2D, shape: RadialShape, options: PaintOptions): void {
  const center = worldToScreen(shape, options.camera);
  const scale = options.camera.scale;
  ctx.beginPath();
  if (shape.kind === 'circle') ctx.arc(center.x, center.y, shape.radius * scale, 0, Math.PI * 2);
  else {
    const outer = shape.outer * scale,
      inner = shape.inner * scale;
    ctx.moveTo(center.x + Math.cos(shape.start) * outer, center.y + Math.sin(shape.start) * outer);
    ctx.arc(center.x, center.y, outer, shape.start, shape.end);
    ctx.lineTo(center.x + Math.cos(shape.end) * inner, center.y + Math.sin(shape.end) * inner);
    ctx.arc(center.x, center.y, inner, shape.end, shape.start, true);
  }
  ctx.closePath();
}

export function paintRadial(
  ctx: CanvasRenderingContext2D,
  layout: RadialLayout,
  options: PaintOptions,
): number {
  let drawn = 0;
  for (const shape of layout.shapes) {
    const center = worldToScreen(shape, options.camera);
    const radius = (shape.kind === 'circle' ? shape.radius : shape.outer) * options.camera.scale;
    if (
      center.x + radius < 0 ||
      center.y + radius < 0 ||
      center.x - radius > options.width ||
      center.y - radius > options.height
    )
      continue;
    const node = shape.node;
    const paint = visualPaint(node, options.color, options.maximum, options.now);
    const active =
      node.id === options.selected ||
      node.id === options.hovered ||
      options.selectedIds?.has(node.id);
    ctx.save();
    trace(ctx, shape, options);
    if (shape.structural) {
      if (shape.kind === 'sector') {
        ctx.fillStyle = options.style.surface;
        ctx.fill();
      }
      ctx.globalAlpha = active ? 1 : 0.5;
      ctx.strokeStyle = options.style.text;
      ctx.lineWidth = active ? 3 : 1;
    } else {
      drawn++;
      ctx.globalAlpha = options.matches && !options.matches.has(node.id) ? 0.28 : 1;
      ctx.fillStyle = paint.fill;
      ctx.fill();
      ctx.strokeStyle = active ? paint.ink : options.style.surface;
      ctx.lineWidth = active ? 3 : 1;
    }
    ctx.stroke();
    ctx.restore();
    if (!options.detail && !active) continue;
    const anchor = shapeAnchor(shape);
    const point = worldToScreen(anchor, options.camera);
    const maxWidth =
      shape.kind === 'circle'
        ? radius * 1.5
        : Math.min(
            ((shape.end - shape.start) * (shape.inner + shape.outer)) / 2,
            2 * (shape.outer - shape.inner),
          ) * options.camera.scale;
    const maxHeight =
      shape.kind === 'circle' ? radius * 1.4 : (shape.outer - shape.inner) * options.camera.scale;
    if (shape.structural && shape.kind === 'circle') continue;
    ctx.font = '600 12px "Segoe UI", sans-serif';
    if (maxWidth < ctx.measureText(node.name).width + 10 || maxHeight < 18) continue;
    ctx.save();
    trace(ctx, shape, options);
    ctx.clip();
    ctx.fillStyle = shape.structural ? options.style.text : paint.ink;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(node.name, point.x, point.y - (maxHeight >= 38 ? 7 : 0));
    if (maxHeight >= 38 && !shape.structural) {
      ctx.font = '11px "Segoe UI", sans-serif';
      ctx.fillText(bytes(node.value), point.x, point.y + 9);
    }
    ctx.restore();
  }
  return drawn;
}
