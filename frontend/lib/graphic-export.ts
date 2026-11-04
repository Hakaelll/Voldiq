import type { IndexId } from './index-context.ts';
import type { ReportSelection } from './report-export.ts';
import type { CloudFilter } from './types.ts';
import type { StorageView, StorageDepth, ColorMode, HeightMode } from './storage-map.ts';
import type { Camera, Point, buildCity } from './disk-city.ts';
import type { PaintOptions } from './map-painter.ts';
import { paintFlat, paintCity, paintTreemapLens } from './map-painter.ts';
import { paintRadial } from './radial-painter.ts';
import { paintVoronoi } from './voronoi-painter.ts';
import type { RadialLayout } from './radial-map.ts';
import type { VoronoiLayout } from './voronoi.ts';
import type { TextMeasure } from './svg-surface.ts';
import type { PaintSurface } from './paint-surface.ts';

export interface GraphicView {
  representation: StorageView;
  metric: 'logical' | 'allocated';
  color: ColorMode;
  color_maximum: number;
  reference_time: number;
  dark: boolean;
  depth: StorageDepth;
  width: number;
  height: number;
  camera: Camera;
  detail: boolean;
  city_height: { metric: HeightMode; relief: number; maximum: number } | null;
  lens: boolean;
}
export interface GraphicContext {
  scope: number;
  cloud: CloudFilter | null;
  highlight: ReportSelection['filter'];
  view: GraphicView;
  include_paths: boolean;
}
export interface SvgRequest {
  context: GraphicContext;
  drawing: string;
}
export interface GraphicCapture {
  indexId: IndexId;
  scope: number;
  cloud: CloudFilter | null;
  searchIdentity: string;
  scene: GraphicScene;
  view: GraphicView;
}
export type GraphicScene = { options: PaintOptions } & (
  | { kind: 'treemap'; lens: Point | null }
  | { kind: 'city'; scene: ReturnType<typeof buildCity>; shadows: Map<string, Point[][]> }
  | { kind: 'sunburst' | 'circles'; layout: RadialLayout }
  | { kind: 'voronoi'; layout: VoronoiLayout }
);

/** The caller captures immutable scene references and obtains measurements from its actual Canvas. */
export async function mapSvg(scene: GraphicScene, measure: TextMeasure): Promise<string> {
  const { SvgSurface } = await import('./svg-surface.ts');
  const surface = new SvgSurface(scene.options.width, scene.options.height, measure);
  paintScene(surface, scene);
  return surface.finish();
}

export function paintScene(surface: PaintSurface, scene: GraphicScene): void {
  surface.fillStyle = scene.options.style.surface;
  surface.fillRect(0, 0, scene.options.width, scene.options.height);
  switch (scene.kind) {
    case 'treemap':
      paintFlat(surface, scene.options);
      if (scene.lens) paintTreemapLens(surface, scene.options, scene.lens);
      break;
    case 'city':
      paintCity(surface, scene.scene, scene.shadows, scene.options);
      break;
    case 'sunburst':
    case 'circles':
      paintRadial(surface, scene.layout, scene.options);
      break;
    case 'voronoi':
      paintVoronoi(surface, scene.layout, scene.options);
      break;
    default: {
      const exhaustive: never = scene;
      return exhaustive;
    }
  }
}

export function pngDimensions(
  view: Pick<GraphicView, 'width' | 'height'>,
  scale: 1 | 2,
): { width: number; height: number } | null {
  const width = Math.ceil(view.width * scale),
    height = Math.ceil(view.height * scale);
  return Number.isFinite(width) &&
    Number.isFinite(height) &&
    width > 0 &&
    height > 0 &&
    width <= 8192 &&
    height <= 8192 &&
    width * height <= 16_777_216
    ? { width, height }
    : null;
}

export async function mapPng(
  scene: GraphicScene,
  scale: 1 | 2,
  signal: AbortSignal,
): Promise<Uint8Array> {
  const size = pngDimensions(scene.options, scale);
  if (!size)
    throw new Error(
      'Choose a lower resolution. PNG is limited to 16.8 megapixels and 8192 pixels per side.',
    );
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
  signal.throwIfAborted();
  const canvas = document.createElement('canvas');
  try {
    canvas.width = size.width;
    canvas.height = size.height;
    const surface = canvas.getContext('2d', { colorSpace: 'srgb' });
    if (!surface) throw new Error('Canvas image export is unavailable.');
    surface.setTransform(scale, 0, 0, scale, 0, 0);
    paintScene(surface, scene);
    signal.throwIfAborted();
    const blob = await new Promise<Blob>((resolve, reject) =>
      canvas.toBlob(
        (value) => (value ? resolve(value) : reject(new Error('PNG encoding failed.'))),
        'image/png',
      ),
    );
    signal.throwIfAborted();
    if (blob.size > 16 * 1024 * 1024)
      throw new Error('PNG exceeds 16 MiB. Choose a lower resolution.');
    const bytes = new Uint8Array(await blob.arrayBuffer());
    signal.throwIfAborted();
    return bytes;
  } finally {
    canvas.width = canvas.height = 0;
  }
}

export function pngPacket(
  metadata: { indexId: IndexId; requestId: string; context: GraphicContext; scale: 1 | 2 },
  image: Uint8Array,
): Uint8Array {
  const header = new TextEncoder().encode(JSON.stringify(metadata));
  if (header.length > 64 * 1024 || image.length > 16 * 1024 * 1024)
    throw new Error('PNG packet exceeds its export limit.');
  const packet = new Uint8Array(4 + header.length + image.length);
  new DataView(packet.buffer).setUint32(0, header.length, true);
  packet.set(header, 4);
  packet.set(image, 4 + header.length);
  return packet;
}
