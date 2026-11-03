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

export interface SvgView {
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
export interface SvgRequest {
  scope: number;
  cloud: CloudFilter | null;
  highlight: ReportSelection['filter'];
  view: SvgView;
  include_paths: boolean;
  drawing: string;
}
export interface SvgCapture {
  indexId: IndexId;
  scope: number;
  cloud: CloudFilter | null;
  searchIdentity: string;
  drawing: string;
  view: SvgView;
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
  return surface.finish();
}
