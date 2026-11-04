<script module lang="ts">
  import type { Camera as StoredCamera } from '../lib/disk-city';
  const cameras = new Map<
    string,
    { camera: StoredCamera; fitted: number; width: number; height: number }
  >();
</script>

<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { GraphicCapture, GraphicView, GraphicScene } from '../lib/graphic-export';
  import {
    createStorageLayout,
    metricHeight,
    metricMaximum,
    heightDescription,
    categoryLabel,
    visualPaint,
    type HeightMode,
    type ColorMode,
    type Rect,
    type StorageView,
    type StorageDepth,
    STORAGE_DEPTHS,
    parseStorageDepth,
  } from '../lib/storage-map';
  import {
    buildCity,
    createPicker,
    fitCamera,
    screenToWorld,
    project,
    worldToScreen,
    zoomCamera,
    type Camera,
    type Point,
  } from '../lib/disk-city';
  import {
    paintCity,
    paintFlat,
    paintTreemapLens,
    prepareShadows,
    type MapStyle,
  } from '../lib/map-painter';
  import { appendLassoPoint, selectLasso } from '../lib/treemap-tools';
  import { radialLayout, radialHit, shapeAnchor } from '../lib/radial-map';
  import { paintRadial } from '../lib/radial-painter';
  import { voronoiHit, VORONOI_GROUP_ID, type VoronoiLayout } from '../lib/voronoi';
  import { paintVoronoi } from '../lib/voronoi-painter';
  import { startVoronoiJob } from '../lib/voronoi-job';
  import type { WeightedNode } from '../lib/treemap';
  import { searchHighlightIds } from '../lib/storage-nodes';
  import { bytes, type CloudFilter } from '../lib/types';
  import CloudFilterSelect from './CloudFilterSelect.svelte';
  import Icon from './Icon.svelte';
  import FolderNotePreview from './FolderNotePreview.svelte';
  import MapContextMenu from './MapContextMenu.svelte';
  import {
    createMapMenu,
    mapTarget,
    menuCommand,
    type MapMenuModel,
    type MapCommand,
    type MapCommandHandler,
  } from '../lib/map-menu';
  import { getIndexContext, type IndexId } from '../lib/index-context';
  import {
    projectedSelection,
    selectionOperation,
    type SelectionOperation,
    type SelectMany,
  } from '../lib/exploration';

  let {
    nodes,
    identity,
    mode = 'treemap',
    depth = $bindable<StorageDepth>(6),
    colorMode = $bindable<ColorMode>('type'),
    selected,
    explored,
    scopeAncestors,
    onclearExplore,
    onselectMany,
    oncommand,
    searchState,
    searchIdentity,
    searchTotal,
    sizeMode,
    onmetric,
    onmode,
    onselect,
    onopen,
    onlist,
    ongrid,
    onback,
    oninspect,
    cloudFilter,
    oncloudfilter,
    queryState = 'ready',
    onretry,
    onexport,
    exportDisabled = false,
  }: {
    nodes: WeightedNode[];
    identity: string;
    mode?: StorageView;
    depth?: StorageDepth;
    colorMode?: ColorMode;
    selected: number | null;
    explored: ReadonlySet<number>;
    scopeAncestors: readonly number[];
    onclearExplore: () => void;
    onselectMany: SelectMany;
    oncommand: MapCommandHandler;
    searchState: 'inactive' | 'loading' | 'ready' | 'failed';
    searchIdentity: string;
    searchTotal: number | null;
    sizeMode: string;
    onmetric: (value: string) => void;
    onmode: (value: StorageView) => void;
    onselect: (id: number, operation?: SelectionOperation) => void;
    onopen: (id: number) => void;
    onlist: () => void;
    ongrid?: () => void;
    onback: () => void;
    oninspect?: () => void;
    cloudFilter: CloudFilter | null;
    oncloudfilter: (value: CloudFilter | null) => void;
    queryState?: 'ready' | 'loading' | 'failed';
    onretry?: () => void;
    onexport?: (capture: GraphicCapture) => boolean;
    exportDisabled?: boolean;
  } = $props();
  let canvas: HTMLCanvasElement;
  let container: HTMLDivElement;
  let width = $state(0),
    height = $state(0);
  let heightMode = $state<HeightMode>('modified');
  let relief = $state(1);
  let detail = $state(true);
  let graphicProblem = $state('');
  let alive = false;
  let hovered = $state<Rect | null>(null);
  const noteIndexId = untrack(getIndexContext);
  let tooltipHeight = $state(112);
  let tooltip = $state({ x: 0, y: 0 });
  let preparing = $state(false),
    failure = $state('');
  let drawn = $state(0),
    drawTime = $state(0);
  const now = Date.now() / 1000;
  const layout = $derived.by(() => {
    const start = performance.now();
    return { ...createStorageLayout(nodes, depth), elapsed: performance.now() - start };
  });
  type RadialState =
    | { kind: 'none' }
    | { kind: 'ready'; value: ReturnType<typeof radialLayout> }
    | { kind: 'error'; message: string };
  const radial = $derived.by<RadialState>(() => {
    if (mode !== 'sunburst' && mode !== 'circles') return { kind: 'none' };
    try {
      return { kind: 'ready', value: radialLayout(layout, mode) };
    } catch (error) {
      return {
        kind: 'error',
        message: error instanceof Error ? error.message : String(error),
      };
    }
  });
  const maximum = $derived(metricMaximum(layout.rects, heightMode, now));
  const selectionOutside = $derived(selected !== null && !layout.parents.has(selected));
  const selectedIds = $derived(
    projectedSelection(
      layout.rects.map((rect) => rect.id),
      layout.parents,
      new Set([...explored, ...(selected === null ? [] : [selected])]),
      scopeAncestors,
    ),
  );
  const colorMaximum = $derived(
    colorMode === 'size'
      ? Math.max(1, ...layout.rects.map((r) => r.value))
      : metricMaximum(layout.rects, 'modified', now),
  );
  const matches = $derived(searchState === 'ready' ? searchHighlightIds(layout.rects) : null);
  let scene: ReturnType<typeof buildCity> | null = null;
  let voronoi = $state.raw<VoronoiLayout | null>(null);
  let shadows = new Map<string, Point[][]>();
  let picker: ReturnType<typeof createPicker> | null = null;
  let camera: Camera = { x: 0, y: 0, scale: 1 };
  let fitted = 1;
  let currentKey = '';
  let sceneIdentity = '';
  let geometryMs = $state(0),
    shadowMs = $state(0);
  let projectionMs = $state(0),
    orderingMs = $state(0);
  let frame = 0,
    generation = 0,
    worker: Worker | null = null;
  let tool = $state<'pan' | 'lasso'>('pan');
  let lens = $state(false);
  let lensAnchor: Point | null = null;
  let toolMessage = $state('');
  let toolIssue = $state(false);
  const gestureContext = $derived({
    identity,
    searchIdentity,
    revision: layout.revision,
    mode,
    sizeMode,
    depth,
    width,
    height,
    heightMode,
    relief,
  });
  let contextMenu = $state.raw<{
    model: MapMenuModel;
    anchor: Point;
    context: typeof gestureContext;
  } | null>(null);
  function closeMenu(restoreFocus = false): void {
    if (!contextMenu) return;
    contextMenu = null;
    if (restoreFocus && canvas.isConnected) canvas.focus({ preventScroll: true });
  }
  function dismissMenu(owner: MapMenuModel, restoreFocus: boolean): void {
    if (contextMenu?.model === owner) closeMenu(restoreFocus);
  }
  function openMenu(anchor: Point, rect: Rect | null): void {
    cancelDrag();
    const model = createMapMenu(rect, getIndexContext());
    if (!model) return;
    hovered = null;
    lensAnchor = null;
    contextMenu = { model, anchor, context: gestureContext };
    schedule();
  }
  function keyboardMenu(): void {
    const bounds = canvas.getBoundingClientRect();
    openMenu(
      { x: bounds.left + width / 2, y: bounds.top + height / 2 },
      layout.rects.find((rect) => rect.id === selected) ??
        layout.boundaries.find((rect) => rect.id === selected) ??
        null,
    );
  }
  function chooseMenu(owner: MapMenuModel, command: MapCommand): void {
    const opened = contextMenu;
    if (opened?.model !== owner) return;
    const accepted =
      opened.context === gestureContext ? menuCommand(owner, getIndexContext(), command) : null;
    closeMenu(accepted !== null);
    if (accepted) oncommand(accepted, owner.indexId);
  }
  type Drag = {
    pointerId: number;
    start: Point;
    camera: Camera;
    moved: boolean;
    context: typeof gestureContext;
    indexId: IndexId | null;
    operation: SelectionOperation;
  } & ({ kind: 'pan' } | { kind: 'lasso'; points: readonly Point[] });
  let dragging: Drag | null = null;
  function cancelDrag(): void {
    const previous = dragging;
    dragging = null;
    if (previous && canvas.hasPointerCapture(previous.pointerId))
      canvas.releasePointerCapture(previous.pointerId);
    if (previous) schedule();
  }
  let style: MapStyle = { text: '#202523', surface: '#f7f7f4', border: '#d5d9d5', dark: false };
  function saveCamera(): void {
    if (currentKey) cameras.set(currentKey, { camera: { ...camera }, fitted, width, height });
    if (cameras.size > 48) cameras.delete(cameras.keys().next().value ?? '');
  }
  function fit(): void {
    closeMenu();
    cancelDrag();
    camera = fitCamera(
      mode === 'city' && scene ? scene.bounds : { x: 0, y: 0, width: 1000, height: 700 },
      width,
      height,
    );
    fitted = camera.scale;
    if (mode !== 'city' || (scene && sceneIdentity === identity)) saveCamera();
    schedule();
  }
  function schedule(): void {
    if (!frame)
      frame = requestAnimationFrame(() => {
        frame = 0;
        draw();
      });
  }
  function draw(): void {
    if (!canvas || width <= 0 || height <= 0) return;
    const start = performance.now();
    const ratio = window.devicePixelRatio || 1;
    if (canvas.width !== Math.round(width * ratio)) canvas.width = Math.round(width * ratio);
    if (canvas.height !== Math.round(height * ratio)) canvas.height = Math.round(height * ratio);
    const ctx = canvas.getContext('2d');
    if (!ctx) {
      failure = 'Canvas 2D is unavailable. Use the item list.';
      return;
    }
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.clearRect(0, 0, width, height);
    const options = {
      rects: layout.rects,
      camera,
      width,
      height,
      selected,
      selectedIds,
      hovered: hovered?.id ?? null,
      matches,
      color: colorMode,
      maximum: colorMaximum,
      now,
      detail,
      style,
    };
    try {
      drawn =
        mode === 'city' && scene
          ? paintCity(ctx, scene, shadows, options)
          : mode === 'voronoi' && voronoi
            ? paintVoronoi(ctx, voronoi, options)
            : mode === 'treemap'
              ? paintFlat(ctx, options)
              : radial.kind === 'ready'
                ? paintRadial(ctx, radial.value, options)
                : 0;
      if (mode === 'treemap' && lens && lensAnchor && !dragging)
        paintTreemapLens(ctx, options, lensAnchor);
      if (dragging?.kind === 'lasso') {
        ctx.save();
        ctx.beginPath();
        for (let i = 0; i < dragging.points.length; i++) {
          const point = dragging.points[i];
          if (i === 0) ctx.moveTo(point.x, point.y);
          else ctx.lineTo(point.x, point.y);
        }
        ctx.closePath();
        ctx.globalAlpha = 0.16;
        ctx.fillStyle = style.surface;
        ctx.fill();
        ctx.globalAlpha = 1;
        ctx.strokeStyle = style.text;
        ctx.lineWidth = 3;
        ctx.stroke();
        ctx.setLineDash([5, 3]);
        ctx.strokeStyle = style.surface;
        ctx.lineWidth = 1;
        ctx.stroke();
        ctx.restore();
      }
      drawTime = performance.now() - start;
    } catch (error) {
      failure = error instanceof Error ? error.message : String(error);
    }
  }
  function captureGraphic(): void {
    const indexId = getIndexContext();
    const scope = scopeAncestors[0];
    if (
      !onexport ||
      exportDisabled ||
      preparing ||
      queryState !== 'ready' ||
      searchState === 'loading' ||
      searchState === 'failed' ||
      !alive ||
      indexId === null ||
      scope === undefined
    )
      return;
    closeMenu();
    cancelDrag();
    hovered = null;
    schedule();
    const options = {
      rects: layout.rects,
      camera: { ...camera },
      width,
      height,
      selected,
      selectedIds: new Set(selectedIds),
      hovered: null,
      matches: matches ? new Set(matches) : null,
      color: colorMode,
      maximum: colorMaximum,
      now,
      detail,
      style: { ...style },
    };
    const lensPoint = mode === 'treemap' && lens && lensAnchor ? { ...lensAnchor } : null;
    let snapshot: GraphicScene;
    if (mode === 'city' && scene && sceneIdentity === identity)
      snapshot = { kind: 'city', scene, shadows, options };
    else if (mode === 'voronoi' && voronoi)
      snapshot = { kind: 'voronoi', layout: voronoi, options };
    else if (mode === 'treemap') snapshot = { kind: 'treemap', lens: lensPoint, options };
    else if ((mode === 'sunburst' || mode === 'circles') && radial.kind === 'ready')
      snapshot = { kind: mode, layout: radial.value, options };
    else {
      graphicProblem = 'The map is not ready to export.';
      return;
    }
    const view: GraphicView = {
      representation: snapshot.kind,
      metric: sizeMode === 'allocated' ? 'allocated' : 'logical',
      color: colorMode,
      color_maximum: colorMaximum,
      reference_time: now,
      dark: style.dark,
      depth,
      width,
      height,
      camera: { ...camera },
      detail,
      city_height: mode === 'city' ? { metric: heightMode, relief, maximum } : null,
      lens: !!lensPoint,
    };
    graphicProblem = '';
    try {
      if (
        onexport({ indexId, scope, cloud: cloudFilter, searchIdentity, scene: snapshot, view }) !==
        true
      )
        graphicProblem = 'The map changed. Export again.';
    } catch (problem) {
      graphicProblem = problem instanceof Error ? problem.message : String(problem);
    }
  }
  function zoom(factor: number, point = { x: width / 2, y: height / 2 }): void {
    closeMenu();
    cancelDrag();
    camera = zoomCamera(camera, point, factor, fitted * 0.5, fitted * 12);
    saveCamera();
    schedule();
  }
  function local(event: MouseEvent): Point {
    const bounds = canvas.getBoundingClientRect();
    return { x: event.clientX - bounds.left, y: event.clientY - bounds.top };
  }
  function bounded(point: Point): Point {
    return { x: Math.max(0, Math.min(width, point.x)), y: Math.max(0, Math.min(height, point.y)) };
  }
  function pointerDown(event: PointerEvent): void {
    if (event.button !== 0 || dragging || preparing || failure) return;
    closeMenu();
    canvas.focus({ preventScroll: true });
    const point = bounded(local(event));
    const base = {
      pointerId: event.pointerId,
      start: point,
      camera: { ...camera },
      moved: false,
      context: gestureContext,
      indexId: getIndexContext(),
      operation: selectionOperation(event),
    };
    dragging =
      mode === 'treemap' && tool === 'lasso'
        ? { ...base, kind: 'lasso', points: [point] }
        : { ...base, kind: 'pan' };
    hovered = null;
    toolMessage = '';
    toolIssue = false;
    canvas.setPointerCapture(event.pointerId);
    schedule();
  }
  function pointerMove(event: PointerEvent): void {
    if (contextMenu) return;
    const point = local(event);
    if (dragging) {
      if (event.pointerId !== dragging.pointerId) return;
      const dx = point.x - dragging.start.x,
        dy = point.y - dragging.start.y;
      if (Math.hypot(dx, dy) > 5) dragging.moved = true;
      if (dragging.kind === 'lasso')
        dragging.points = appendLassoPoint(dragging.points, bounded(point));
      else if (dragging.moved)
        camera = { ...dragging.camera, x: dragging.camera.x + dx, y: dragging.camera.y + dy };
      hovered = null;
      schedule();
    } else {
      lensAnchor = point;
      hovered = lens && mode === 'treemap' ? null : hit(point);
      tooltip = {
        x: Math.max(8, Math.min(width - 260, point.x + 14)),
        y: Math.max(8, Math.min(height - 100, point.y + 14)),
      };
      if (lens && mode === 'treemap') schedule();
    }
  }
  function pointerUp(event: PointerEvent): void {
    const active = dragging;
    if (!active || event.pointerId !== active.pointerId) return;
    cancelDrag();
    if (active.context !== gestureContext || active.indexId !== getIndexContext()) return;
    const raw = local(event),
      point = bounded(raw);
    lensAnchor = raw.x >= 0 && raw.y >= 0 && raw.x <= width && raw.y <= height ? point : null;
    const moved = active.moved || Math.hypot(raw.x - active.start.x, raw.y - active.start.y) > 5;
    if (!moved) {
      const rect = hit(point);
      if (rect) select(rect, false, active.operation);
    } else if (active.kind === 'pan') {
      camera = {
        ...active.camera,
        x: active.camera.x + raw.x - active.start.x,
        y: active.camera.y + raw.y - active.start.y,
      };
      saveCamera();
    } else {
      const points = appendLassoPoint(active.points, point, true).map((p) =>
        screenToWorld(p, active.camera),
      );
      const result = selectLasso(layout.rects, points);
      toolIssue = result.kind === 'invalid';
      if (result.kind === 'invalid') toolMessage = result.message;
      else if (!onselectMany(result.ids, active.operation, active.indexId)) {
        toolIssue = true;
        toolMessage = 'Inspection selection unchanged. Check the notice above.';
      } else
        toolMessage = `${result.ids.length} frontier items matched${result.skippedGroups ? ` · ${result.skippedGroups} grouped regions skipped. Open grouped contents in the item list.` : ''}`;
    }
    schedule();
  }
  function hit(point: Point): Rect | null {
    if (preparing || failure) return null;
    const p = screenToWorld(point, camera);
    if (mode === 'voronoi') return voronoi ? (voronoiHit(voronoi, p)?.node ?? null) : null;
    if (mode === 'city') {
      const face = picker?.(p);
      return face ? (layout.rects.find((r) => r.id === face.node.id) ?? null) : null;
    }
    if (radial.kind === 'ready') return radialHit(radial.value, p);
    return (
      layout.rects.find(
        (r) => p.x >= r.x && p.y >= r.y && p.x <= r.x + r.width && p.y <= r.y + r.height,
      ) ?? null
    );
  }
  function select(rect: Rect, open = false, operation: SelectionOperation = 'replace'): void {
    const target = mapTarget(rect.id);
    if (!target) return;
    if (target.kind === 'list') {
      onlist();
      return;
    }
    if (target.kind === 'contents' || (open && rect.directory)) onopen(target.id);
    else onselect(target.id, operation);
  }
  function keyboard(event: KeyboardEvent): void {
    if (event.target !== canvas) return;
    if (event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey)) {
      event.preventDefault();
      event.stopPropagation();
      keyboardMenu();
      return;
    }
    if (
      [
        '+',
        '=',
        '-',
        '0',
        'ArrowLeft',
        'ArrowRight',
        'ArrowUp',
        'ArrowDown',
        'Enter',
        'Escape',
      ].includes(event.key)
    )
      event.preventDefault();
    if (event.key === '+' || event.key === '=') zoom(1.2);
    else if (event.key === '-') zoom(1 / 1.2);
    else if (event.key === '0') fit();
    else if (event.key === 'Enter') {
      cancelDrag();
      const rect =
        layout.rects.find((r) => r.id === selected) ??
        layout.boundaries.find((r) => r.id === selected);
      if (rect) select(rect, true);
    } else if (event.key === 'Escape') {
      hovered = null;
      event.stopPropagation();
      if (dragging) cancelDrag();
      else if (explored.size) onclearExplore();
      else onback();
    } else if (event.key.startsWith('Arrow')) {
      closeMenu();
      cancelDrag();
      camera = {
        ...camera,
        x: camera.x + (event.key === 'ArrowLeft' ? 40 : event.key === 'ArrowRight' ? -40 : 0),
        y: camera.y + (event.key === 'ArrowUp' ? 40 : event.key === 'ArrowDown' ? -40 : 0),
      };
      saveCamera();
      schedule();
    }
  }
  $effect(() => {
    void gestureContext;
    untrack(() => {
      cancelDrag();
      closeMenu();
      lensAnchor = null;
      toolMessage = '';
      toolIssue = false;
    });
  });
  $effect(() => {
    const rects = layout.rects;
    const currentIdentity = identity;
    const heights = rects.map((r) => metricHeight(r, heightMode, now, maximum) * relief);
    const owner = ++generation;
    worker?.terminate();
    worker = null;
    failure = '';
    hovered = null;
    if (mode === 'voronoi') {
      preparing = true;
      voronoi = null;
      const started = performance.now();
      return startVoronoiJob($state.snapshot(rects), layout.total, (reply) => {
        if (owner !== generation || currentIdentity !== identity) return;
        preparing = false;
        geometryMs = performance.now() - started;
        if (reply.kind === 'ready') voronoi = reply.layout;
        else failure = reply.message;
        schedule();
      });
    }
    if (mode !== 'city') {
      preparing = false;
      if (radial.kind === 'error') failure = radial.message;
      untrack(schedule);
      return;
    }
    preparing = true;
    const started = performance.now();
    const accept = (next: ReturnType<typeof buildCity>) => {
      if (owner !== generation) return;
      const first = sceneIdentity !== identity || !scene;
      scene = next;
      picker = createPicker(next.faces);
      sceneIdentity = identity;
      geometryMs = performance.now() - started;
      projectionMs = next.timings.projectionMs;
      orderingMs = next.timings.orderingMs;
      const shadowStarted = performance.now();
      shadows = prepareShadows(next);
      shadowMs = performance.now() - shadowStarted;
      preparing = false;
      if (first && !cameras.has(`${identity}:${mode}`)) fit();
      else schedule();
    };
    const fallback = () => {
      if (owner !== generation) return;
      try {
        const values = new Map(rects.map((r, i) => [r.id, heights[i]]));
        accept(buildCity(rects, (r) => values.get(r.id) ?? 5));
      } catch (error) {
        failure = error instanceof Error ? error.message : String(error);
        preparing = false;
      }
    };
    try {
      worker = new Worker(new URL('../lib/city.worker.ts', import.meta.url), { type: 'module' });
      worker.onmessage = (
        event: MessageEvent<{ scene?: ReturnType<typeof buildCity>; error?: string }>,
      ) => {
        if (owner !== generation) return;
        worker?.terminate();
        worker = null;
        if (event.data.scene) accept(event.data.scene);
        else {
          failure = event.data.error ?? 'City preparation failed.';
          preparing = false;
        }
      };
      worker.onerror = () => {
        if (owner !== generation) return;
        worker?.terminate();
        worker = null;
        fallback();
      };
      worker.postMessage({ rects: $state.snapshot(rects), heights });
    } catch {
      fallback();
    }
    return () => {
      worker?.terminate();
      worker = null;
    };
  });
  $effect(() => {
    const key = `${identity}:${mode}`;
    void width;
    void height;
    untrack(() => {
      if (key !== currentKey) {
        saveCamera();
        currentKey = key;
      }
      const previous = cameras.get(key);
      if (previous && previous.width === width && previous.height === height) {
        camera = { ...previous.camera };
        fitted = previous.fitted;
        schedule();
      } else fit();
    });
  });
  $effect(() => {
    void selected;
    void selectedIds;
    void hovered;
    void matches;
    void colorMode;
    void detail;
    untrack(schedule);
  });
  $effect(() => {
    const id = selected;
    untrack(() => {
      const rect =
        layout.rects.find((rect) => rect.id === id) ??
        layout.boundaries.find((rect) => rect.id === id);
      if (!rect || width <= 0 || height <= 0) return;
      const shape =
        radial.kind === 'ready'
          ? radial.value.shapes.find((shape) => shape.node.id === rect.id)
          : null;
      const polygon =
        mode === 'voronoi' ? voronoi?.cells.find((cell) => cell.members.includes(rect.id)) : null;
      const point = polygon
        ? polygon.anchor
        : shape
          ? shapeAnchor(shape)
          : { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
      const target = worldToScreen(
        mode === 'city'
          ? project({
              ...point,
              z: 'depth' in rect ? metricHeight(rect, heightMode, now, maximum) * relief : 0,
            })
          : point,
        camera,
      );
      if (target.x < 24 || target.x > width - 24 || target.y < 24 || target.y > height - 24) {
        closeMenu();
        cancelDrag();
        camera = {
          ...camera,
          x: camera.x + width / 2 - target.x,
          y: camera.y + height / 2 - target.y,
        };
        saveCamera();
        schedule();
      }
    });
  });
  onMount(() => {
    alive = true;
    const theme = () => {
      const css = getComputedStyle(container);
      style = {
        text: css.getPropertyValue('--text').trim(),
        surface: css.getPropertyValue('--canvas').trim(),
        border: css.getPropertyValue('--border').trim(),
        dark: document.documentElement.dataset.theme === 'dark',
      };
      schedule();
    };
    const resize = new ResizeObserver((entries) => {
      const box = entries[0]?.contentRect;
      if (box) {
        width = box.width;
        height = box.height;
      }
    });
    const appearance = new MutationObserver(theme);
    const wheel = (event: WheelEvent) => {
      event.preventDefault();
      hovered = null;
      zoom(Math.exp(-Math.max(-150, Math.min(150, event.deltaY)) * 0.006), local(event));
    };
    const resolution = () => schedule();
    const blur = () => {
      cancelDrag();
      lensAnchor = null;
      hovered = null;
      schedule();
    };
    resize.observe(container);
    appearance.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme'],
    });
    canvas.addEventListener('wheel', wheel, { passive: false });
    window.addEventListener('resize', resolution);
    window.addEventListener('blur', blur);
    theme();
    return () => {
      alive = false;
      generation++;
      worker?.terminate();
      cancelAnimationFrame(frame);
      resize.disconnect();
      appearance.disconnect();
      canvas.removeEventListener('wheel', wheel);
      window.removeEventListener('resize', resolution);
      window.removeEventListener('blur', blur);
    };
  });
</script>

<section
  class="storage-map"
  aria-label="Storage visualization"
  aria-busy={queryState === 'loading' || preparing}
  data-layout={layout.revision}
  data-ready={queryState === 'ready' && !preparing && !failure}
  data-draw-ms={drawTime}
  data-blocks={layout.rects.length}
  data-layout-ms={layout.elapsed}
  data-geometry-ms={geometryMs}
  data-shadow-ms={shadowMs}
  data-projection-ms={projectionMs}
  data-ordering-ms={orderingMs}
>
  <div class="storage-map-toolbar">
    <div class="map-switch" role="group" aria-label="Storage representation">
      <button aria-pressed={mode === 'treemap'} onclick={() => onmode('treemap')}>Treemap</button>
      <button aria-pressed={mode === 'city'} onclick={() => onmode('city')}>Skyline</button>
      <button aria-pressed={mode === 'sunburst'} onclick={() => onmode('sunburst')}>Sunburst</button
      >
      <button aria-pressed={mode === 'circles'} onclick={() => onmode('circles')}>Circles</button>
      <button aria-pressed={mode === 'voronoi'} onclick={() => onmode('voronoi')}>Voronoi</button>
      <button onclick={onlist}>List</button>
      {#if ongrid}<button onclick={ongrid}>Grid</button>{/if}
    </div>
    {#if mode === 'treemap'}<div class="map-switch" role="group" aria-label="Treemap tools">
        <button
          aria-pressed={tool === 'pan'}
          onclick={() => {
            cancelDrag();
            tool = 'pan';
          }}>Pan</button
        >
        <button
          aria-pressed={tool === 'lasso'}
          title="Draw a freehand loop to select visible items for inspection"
          onclick={() => {
            cancelDrag();
            tool = 'lasso';
          }}>Lasso</button
        >
        <button
          aria-pressed={lens}
          title="Redraw Treemap geometry at twice the scale around the pointer"
          onclick={() => {
            lens = !lens;
            lensAnchor = lens ? { x: width / 2, y: height / 2 } : null;
            hovered = null;
            schedule();
          }}>Lens 2×</button
        >
      </div>{/if}
    <label
      >Area<select
        aria-label="Map area metric"
        value={sizeMode}
        onchange={(e) => onmetric(e.currentTarget.value)}
        ><option value="logical">Logical size</option><option value="allocated"
          >Unique allocation</option
        ></select
      ></label
    >
    <label
      title="Maximum depth within loaded branches. Small shapes remain grouped for readability."
    >
      Depth<select
        aria-label="Maximum map depth"
        value={depth}
        onchange={(event) => {
          const next = parseStorageDepth(event.currentTarget.value);
          if (next !== null) depth = next;
        }}
      >
        {#each STORAGE_DEPTHS as level}<option value={level}>{level}</option>{/each}
      </select>
    </label>
    <CloudFilterSelect value={cloudFilter} onchange={oncloudfilter} />
    {#if mode === 'city'}<div class="map-metric-group">
        <span>Height</span>
        <div class="map-switch" role="group" aria-label="Height metric">
          <button
            aria-pressed={heightMode === 'modified'}
            title="Age since modification. Last access is unavailable."
            onclick={() => {
              heightMode = 'modified';
            }}>Modified</button
          >
          <button
            aria-pressed={heightMode === 'files'}
            title="Number of indexed descendant files"
            onclick={() => {
              heightMode = 'files';
            }}>Files</button
          >
          <button
            aria-pressed={heightMode === 'depth'}
            title="Depth relative to this folder"
            onclick={() => {
              heightMode = 'depth';
            }}>Depth</button
          >
        </div>
      </div>{/if}
    <div class="map-metric-group">
      <span>Color</span>
      <div class="map-switch" role="group" aria-label="Color metric">
        <button
          aria-pressed={colorMode === 'type'}
          title="Color by file category"
          onclick={() => {
            colorMode = 'type';
          }}>Type</button
        >
        <button
          aria-pressed={colorMode === 'size'}
          title="Color by size"
          onclick={() => {
            colorMode = 'size';
          }}>Size</button
        >
        <button
          aria-pressed={colorMode === 'age'}
          title="Color by age since modification"
          onclick={() => {
            colorMode = 'age';
          }}>Age</button
        >
      </div>
    </div>
    {#if onexport}<button
        disabled={exportDisabled ||
          preparing ||
          queryState !== 'ready' ||
          searchState === 'loading' ||
          searchState === 'failed' ||
          !!failure}
        onclick={() => {
          captureGraphic();
        }}>Export map</button
      >{/if}
    {#if oninspect}<button class="map-inspect" onclick={oninspect}
        ><Icon name="file" size={14} />Inspector</button
      >{/if}
  </div>
  {#if graphicProblem}<div class="map-tool-status" role="alert">{graphicProblem}</div>{/if}
  {#if searchState === 'loading'}<div class="map-tool-status" role="status">
      Updating search matches…
    </div>
  {:else if searchState === 'failed'}<div class="map-tool-status" role="alert">
      Search matches unavailable. {#if onretry}<button onclick={onretry}>Retry matches</button>{/if}
      <button onclick={onlist}>Open item list</button>
    </div>{/if}
  {#if mode === 'treemap' && (tool === 'lasso' || toolMessage)}<div
      class="map-tool-status"
      role={toolIssue ? 'alert' : 'status'}
    >
      {toolMessage || 'Draw a loop. Ctrl toggles; Shift adds. Escape cancels.'}
    </div>{/if}
  <div class="map storage-viewport" bind:this={container}>
    <canvas
      bind:this={canvas}
      tabindex="0"
      class:lasso={mode === 'treemap' && tool === 'lasso'}
      aria-label={`${mode === 'city' ? 'Skyline' : mode === 'sunburst' ? 'Sunburst' : mode === 'circles' ? 'Nested circles' : mode === 'voronoi' ? 'Weighted Voronoi' : 'Treemap'}. Use the item list for keyboard selection. Shift+F10 or the Menu key opens actions for the visible selected item or the map. Arrow keys pan, plus and minus zoom, zero fits the map. Escape cancels an active gesture.`}
      onkeydown={keyboard}
      oncontextmenu={(event) => {
        event.preventDefault();
        event.stopPropagation();
        if (event.button === 0 && event.clientX === 0 && event.clientY === 0) keyboardMenu();
        else openMenu({ x: event.clientX, y: event.clientY }, hit(local(event)));
      }}
      onpointerdown={pointerDown}
      onpointermove={pointerMove}
      onpointerup={pointerUp}
      onpointercancel={(event) => {
        if (dragging?.pointerId === event.pointerId) cancelDrag();
      }}
      onlostpointercapture={(event) => {
        if (dragging?.pointerId === event.pointerId) cancelDrag();
      }}
      onfocus={() => {
        if (lens) {
          lensAnchor = { x: width / 2, y: height / 2 };
          schedule();
        }
      }}
      onblur={() => {
        cancelDrag();
        lensAnchor = null;
        schedule();
      }}
      onpointerleave={() => {
        hovered = null;
        lensAnchor = null;
        schedule();
      }}
      ondblclick={(event) => {
        if (mode === 'treemap' && tool === 'lasso') return;
        if (event.ctrlKey || event.metaKey || event.shiftKey) return;
        const rect = hit(local(event));
        if (rect) select(rect, true);
      }}
    ></canvas>
    <div class="map-camera-tools" role="group" aria-label="Map camera">
      <button title="Zoom out" aria-label="Zoom out" onclick={() => zoom(1 / 1.25)}
        ><Icon name="minus" size={14} /></button
      >
      <button title="Fit to window (0)" onclick={fit}>Fit</button>
      <button title="Zoom in" aria-label="Zoom in" onclick={() => zoom(1.25)}
        ><Icon name="plus" size={14} /></button
      >
      {#if mode === 'city'}<label
          >Relief<input
            type="range"
            min="0.15"
            max="1.6"
            step="0.05"
            bind:value={relief}
            aria-label="Visual relief"
          /></label
        ><label><input type="checkbox" bind:checked={detail} />Detail</label>{/if}
    </div>
    {#if preparing}<div class="map-preparing" role="status">
        Preparing {mode === 'voronoi' ? 'Voronoi' : 'city'}…
      </div>{/if}
    {#if queryState === 'loading'}<div class="map-empty" role="status">
        <p>Updating cloud filter…</p>
      </div>
    {:else if queryState === 'failed'}<div class="map-empty" role="alert">
        <p>Filtered map unavailable.</p>
        {#if onretry}<button onclick={onretry}>Retry map</button>{/if}
      </div>
    {:else if failure}<div class="map-empty" role="alert">
        <p>{failure}</p>
        <div>
          <button onclick={() => onmode('treemap')}>Return to treemap</button><button
            onclick={onlist}>Open item list</button
          >
        </div>
      </div>
    {:else if !layout.rects.length}<div class="map-empty">
        <p>No positive byte weight in this metric.</p>
        <button onclick={onlist}>Inspect empty or unknown items in the list</button>
      </div>{/if}
    {#if hovered && !preparing}<div
        class="map-tooltip"
        bind:clientHeight={tooltipHeight}
        style:max-height={`${Math.max(80, height - 16)}px`}
        style:overflow="hidden"
        style:left={`${tooltip.x}px`}
        style:top={`${Math.max(8, Math.min(tooltip.y, height - tooltipHeight - 8))}px`}
      >
        <strong>{hovered.name}</strong><span
          >{bytes(hovered.value)} · {((hovered.share ?? 0) * 100).toFixed(1)}%</span
        >{#if searchState === 'ready' && hovered.search}<span
            >{hovered.search.own
              ? 'Matches the query'
              : hovered.search.inherited
                ? 'Within a matching folder'
                : hovered.search.descendants > 0
                  ? `${hovered.search.descendants.toLocaleString()} indexed matches in this group`
                  : 'No observed match'}</span
          >{/if}{#if hovered.id === VORONOI_GROUP_ID}<span
            >{voronoi?.groupedEntries} frontier entries grouped. Open the complete item list.</span
          >
        {:else}<span>{heightDescription(hovered, heightMode, now)}</span
          >{#if hovered.directory}<span
              >{hovered.modifiedKnown ?? 0} / {hovered.modifiedTotal ?? hovered.files} descendant dates
              known</span
            >{/if}{/if}
        {#if mode === 'voronoi' && voronoi}{@const cell = voronoi.cells.find(
            (cell) => cell.node.id === hovered?.id,
          )}{#if cell}<span
              >Cell area error {cell.relativeError >= 0 ? '+' : ''}{(
                100 * cell.relativeError
              ).toFixed(2)}%</span
            >{/if}{/if}
        {#if !hovered.allocation_known}<span>Allocation coverage incomplete</span
          >{/if}{#if hovered.aggregated && hovered.id !== VORONOI_GROUP_ID}<span
            >{hovered.represented.toLocaleString()} files represented · Open to inspect</span
          >{/if}
        {#if hovered.directory && hovered.id >= 0}
          {#key hovered.id}<FolderNotePreview nodeId={hovered.id} indexId={noteIndexId} />{/key}
        {/if}
      </div>{/if}
  </div>
  <div class="map-encoding">
    {#if mode === 'treemap' && lens}<span
        >Lens 2× magnifies a local area. The base map keeps its byte scale.</span
      >{/if}
    {#if mode === 'voronoi' && voronoi?.cells.length && voronoi.totalError !== null && voronoi.maximumError !== null}<span
      >
        Approximate weighted Voronoi · <span
          title="Half the sum of absolute differences between cell area share and byte share."
          >area discrepancy {(100 * voronoi.totalError).toFixed(3)}%</span
        >
        · worst cell {(100 * voronoi.maximumError).toFixed(2)}% · {voronoi.groupedEntries} frontier entries
        grouped · {voronoi.stop === 'converged'
          ? 'precision target reached'
          : 'calculation stopped within display tolerance'}.
        {#if voronoi.groupedEntries}<button onclick={onlist}>Inspect complete item list</button
          >{/if}
      </span>{/if}
    <span
      >Up to {depth}
      {depth === 1 ? 'level' : 'levels'} within loaded branches; small groups may remain collapsed.</span
    >
    {#if selectionOutside}<span role="status"
        >The selected item is outside the drawn frontier. Its details remain available.</span
      >{/if}
    {#if mode === 'sunburst'}<span
        >Equal-area depth rings. Colored sectors encode the current frontier; neutral parent bands
        group the hierarchy. Gaps are not free disk space.</span
      >{/if}
    {#if mode === 'circles'}<span
        >Filled circle area encodes the same frontier bytes at every depth. Parent outlines group
        folders. {#if radial.kind === 'ready' && layout.total > 0}{(
            100 *
            (1 - radial.value.dataFraction)
          ).toFixed(1)}% of the enclosing circle is packing space, not free disk space.{:else}Packing
          coverage is unavailable without a positive, drawable total.{/if}</span
      >{/if}
    {#if searchState !== 'inactive'}<span
        >Search highlights matching items and folder groups. Area uses the selected byte metric.</span
      >{/if}
    <span
      >Area = {sizeMode === 'allocated'
        ? 'unique allocation, not recoverable space'
        : 'logical bytes'}.</span
    >
    {#if cloudFilter}<span
        >Filtered file bytes · folders group matching descendants. Details and summary totals remain
        unfiltered.</span
      >{/if}
    {#if mode === 'city'}<span
        >Height = {heightMode === 'modified'
          ? 'modification age, log scale capped at 10 years'
          : heightMode === 'files'
            ? 'descendant files, log scale normalized to this folder'
            : 'depth relative to this folder'}; relief is visual. Volume ≠ bytes.</span
      >{/if}
    <span
      >Color = {colorMode === 'type'
        ? 'file category'
        : colorMode === 'size'
          ? 'size, local log scale'
          : 'age bands: 30 / 180 / 365 days / 10 years'}. Unknown ages use neutral color.{#if mode === 'city'}
        Unknown height metrics keep a low block.{/if}</span
    >
    {#if sizeMode === 'allocated' && nodes.some((r) => !r.allocationKnown)}<span
        >Partial allocation coverage. Unknown bytes are not zero.</span
      >{/if}
  </div>
  <div class="map-category-legend" aria-label="Map color legend">
    {#if colorMode === 'type'}
      {#each [...new Map(layout.rects.map( (r) => [categoryLabel(r), { paint: visualPaint(r, colorMode, colorMaximum, now), label: categoryLabel(r) }] )).values()] as item}<span
          ><i style:background={item.paint.fill}></i>{item.label}</span
        >{/each}
    {:else}
      <span
        >{colorMode === 'size' ? 'Small' : 'Recent'}
        {#each ['#b6d2c6', '#91b9aa', '#679b8e', '#3e7c73', '#23625f'] as fill}<i
            style:background={fill}
          ></i>{/each}
        {colorMode === 'size' ? 'Large · folder-relative' : '10+ years'}
        <i style:background="#6b706b"></i>Unknown
      </span>
    {/if}
    <span
      class="map-render-status"
      title={`Last draw ${drawTime.toFixed(1)} ms. Geometry is cached; idle has no animation loop.`}
      >{layout.represented.toLocaleString()} files represented · {layout.rects.length} frontier items
      {#if mode === 'voronoi'}· {voronoi
          ? `${voronoi.cells.length} cells · ${Math.max(0, voronoi.cells.length - drawn)} outside draw bounds`
          : failure
            ? 'layout unavailable'
            : 'layout pending'}{:else}· {Math.max(0, layout.rects.length - drawn)} outside draw bounds{/if}{#if matches}
        · {searchTotal === null
          ? 'Match count unavailable'
          : `${searchTotal.toLocaleString()} indexed matches`} · {matches.size} highlighted regions{/if}</span
    >
  </div>
  {#if searchState === 'ready' && searchTotal !== null && searchTotal > 0 && matches?.size === 0}<div
      class="map-tool-status"
      role="status"
    >
      Matches are outside the drawn regions. <button onclick={onlist}>Open item list</button>
    </div>{/if}
</section>
{#if contextMenu}{#key contextMenu.model}<MapContextMenu
      model={contextMenu.model}
      anchor={contextMenu.anchor}
      onchoose={chooseMenu}
      ondismiss={dismissMenu}
    />{/key}{/if}
