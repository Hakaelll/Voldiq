<script module lang="ts">
  import type { Camera as StoredCamera } from '../lib/disk-city';
  const cameras = new Map<
    string,
    { camera: StoredCamera; fitted: number; width: number; height: number }
  >();
</script>

<script lang="ts">
  import { onMount, untrack } from 'svelte';
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
  import { paintCity, paintFlat, prepareShadows, type MapStyle } from '../lib/map-painter';
  import type { WeightedNode } from '../lib/treemap';
  import { bytes } from '../lib/types';
  import Icon from './Icon.svelte';

  let {
    nodes,
    identity,
    mode = 'treemap',
    colorMode = $bindable<ColorMode>('type'),
    selected,
    search = '',
    extension = '',
    withoutExtension = false,
    sizeMode,
    onmetric,
    onmode,
    onselect,
    onopen,
    onlist,
    onback,
    oninspect,
  }: {
    nodes: WeightedNode[];
    identity: string;
    mode?: 'treemap' | 'city';
    colorMode?: ColorMode;
    selected: number | null;
    search?: string;
    extension?: string;
    withoutExtension?: boolean;
    sizeMode: string;
    onmetric: (value: string) => void;
    onmode: (value: 'treemap' | 'city') => void;
    onselect: (id: number) => void;
    onopen: (id: number) => void;
    onlist: () => void;
    onback: () => void;
    oninspect?: () => void;
  } = $props();
  let canvas: HTMLCanvasElement;
  let container: HTMLDivElement;
  let width = $state(0),
    height = $state(0);
  let heightMode = $state<HeightMode>('modified');
  let relief = $state(1);
  let detail = $state(true);
  let hovered = $state<Rect | null>(null);
  let tooltip = $state({ x: 0, y: 0 });
  let preparing = $state(false),
    failure = $state('');
  let drawn = $state(0),
    drawTime = $state(0);
  const now = Date.now() / 1000;
  const layout = $derived.by(() => {
    const start = performance.now();
    return { ...createStorageLayout(nodes), elapsed: performance.now() - start };
  });
  const maximum = $derived(metricMaximum(layout.rects, heightMode, now));
  const selectedBoundary = $derived(layout.boundaries.find((rect) => rect.id === selected));
  const selectedIds = $derived(
    new Set(
      selectedBoundary
        ? layout.rects
            .filter(
              (rect) =>
                rect.x >= selectedBoundary.x - 1e-6 &&
                rect.y >= selectedBoundary.y - 1e-6 &&
                rect.x + rect.width <= selectedBoundary.x + selectedBoundary.width + 1e-6 &&
                rect.y + rect.height <= selectedBoundary.y + selectedBoundary.height + 1e-6,
            )
            .map((rect) => rect.id)
        : selected === null
          ? []
          : [selected],
    ),
  );
  const colorMaximum = $derived(
    colorMode === 'size'
      ? Math.max(1, ...layout.rects.map((r) => r.value))
      : metricMaximum(layout.rects, 'modified', now),
  );
  const matches = $derived.by(() => {
    const query = search.trim().toLocaleLowerCase();
    const ext = extension.replace(/^\./, '').toLocaleLowerCase();
    if (!query && !ext && !withoutExtension) return null;
    return new Set(
      layout.rects
        .filter((r) => {
          const suffix = r.directory
            ? ''
            : r.name.includes('.')
              ? (r.name.split('.').pop()?.toLocaleLowerCase() ?? '')
              : '';
          return (
            (!query || r.name.toLocaleLowerCase().includes(query)) &&
            (!ext || suffix === ext) &&
            (!withoutExtension || (!r.directory && !suffix))
          );
        })
        .map((r) => r.id),
    );
  });
  let scene: ReturnType<typeof buildCity> | null = null;
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
  let dragging: { x: number; y: number; camera: Camera; moved: boolean } | null = null;
  let style: MapStyle = { text: '#202523', surface: '#f7f7f4', border: '#d5d9d5', dark: false };
  function saveCamera(): void {
    if (currentKey) cameras.set(currentKey, { camera: { ...camera }, fitted, width, height });
    if (cameras.size > 48) cameras.delete(cameras.keys().next().value ?? '');
  }
  function fit(): void {
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
          : mode === 'treemap'
            ? paintFlat(ctx, options)
            : 0;
      drawTime = performance.now() - start;
    } catch (error) {
      failure = error instanceof Error ? error.message : String(error);
    }
  }
  function zoom(factor: number, point = { x: width / 2, y: height / 2 }): void {
    camera = zoomCamera(camera, point, factor, fitted * 0.5, fitted * 12);
    saveCamera();
    schedule();
  }
  function local(event: MouseEvent): Point {
    const bounds = canvas.getBoundingClientRect();
    return { x: event.clientX - bounds.left, y: event.clientY - bounds.top };
  }
  function hit(point: Point): Rect | null {
    if (preparing || failure) return null;
    const p = screenToWorld(point, camera);
    if (mode === 'city') {
      const face = picker?.(p);
      return face ? (layout.rects.find((r) => r.id === face.node.id) ?? null) : null;
    }
    return (
      layout.rects.find(
        (r) => p.x >= r.x && p.y >= r.y && p.x <= r.x + r.width && p.y <= r.y + r.height,
      ) ?? null
    );
  }
  function select(rect: Rect, open = false): void {
    if (rect.id < -1) {
      onopen(-rect.id - 2);
      return;
    }
    if (rect.id === -1) {
      onlist();
      return;
    }
    if (open && rect.directory) onopen(rect.id);
    else onselect(rect.id);
  }
  function keyboard(event: KeyboardEvent): void {
    if (event.target !== canvas) return;
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
      const rect = layout.rects.find((r) => r.id === selected);
      if (rect) select(rect, true);
    } else if (event.key === 'Escape') {
      hovered = null;
      event.stopPropagation();
      onback();
    } else if (event.key.startsWith('Arrow')) {
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
    const rects = layout.rects;
    const heights = rects.map((r) => metricHeight(r, heightMode, now, maximum) * relief);
    const owner = ++generation;
    worker?.terminate();
    worker = null;
    failure = '';
    hovered = null;
    if (mode !== 'city') {
      preparing = false;
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
      const point = { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
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
    resize.observe(container);
    appearance.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme'],
    });
    canvas.addEventListener('wheel', wheel, { passive: false });
    window.addEventListener('resize', resolution);
    theme();
    return () => {
      generation++;
      worker?.terminate();
      cancelAnimationFrame(frame);
      resize.disconnect();
      appearance.disconnect();
      canvas.removeEventListener('wheel', wheel);
      window.removeEventListener('resize', resolution);
    };
  });
</script>

<section
  class="storage-map"
  aria-label="Storage visualization"
  data-layout={layout.revision}
  data-ready={!preparing && !failure}
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
      <button onclick={onlist}>List</button>
    </div>
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
    {#if oninspect}<button class="map-inspect" onclick={oninspect}
        ><Icon name="file" size={14} />Inspector</button
      >{/if}
  </div>
  <div class="map storage-viewport" bind:this={container}>
    <canvas
      bind:this={canvas}
      tabindex="0"
      aria-label={`${mode === 'city' ? 'Skyline' : 'Treemap'}. Use the item list for keyboard selection. Arrow keys pan, plus and minus zoom, zero fits the map.`}
      onkeydown={keyboard}
      onpointerdown={(event) => {
        if (event.button !== 0) return;
        canvas.focus({ preventScroll: true });
        const p = local(event);
        dragging = { ...p, camera: { ...camera }, moved: false };
        canvas.setPointerCapture(event.pointerId);
      }}
      onpointermove={(event) => {
        const p = local(event);
        if (dragging) {
          const dx = p.x - dragging.x,
            dy = p.y - dragging.y;
          if (Math.hypot(dx, dy) > 5) dragging.moved = true;
          if (dragging.moved) {
            camera = { ...dragging.camera, x: dragging.camera.x + dx, y: dragging.camera.y + dy };
            hovered = null;
            schedule();
          }
        } else {
          hovered = hit(p);
          tooltip = {
            x: Math.max(8, Math.min(width - 260, p.x + 14)),
            y: Math.max(8, Math.min(height - 100, p.y + 14)),
          };
        }
      }}
      onpointerup={(event) => {
        if (!dragging) return;
        const moved = dragging.moved;
        dragging = null;
        canvas.releasePointerCapture(event.pointerId);
        if (moved) saveCamera();
        else {
          const rect = hit(local(event));
          if (rect) select(rect);
        }
      }}
      onpointercancel={() => {
        dragging = null;
      }}
      onpointerleave={() => {
        hovered = null;
      }}
      ondblclick={(event) => {
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
    {#if preparing}<div class="map-preparing" role="status">Preparing city…</div>{/if}
    {#if failure}<div class="map-empty" role="alert">
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
        style:left={`${tooltip.x}px`}
        style:top={`${tooltip.y}px`}
      >
        <strong>{hovered.name}</strong><span
          >{bytes(hovered.value)} · {((hovered.share ?? 0) * 100).toFixed(1)}%</span
        ><span>{heightDescription(hovered, heightMode, now)}</span>{#if hovered.directory}<span
            >{hovered.modifiedKnown ?? 0} / {hovered.modifiedTotal ?? hovered.files} descendant dates
            known</span
          >{/if}{#if !hovered.allocation_known}<span>Allocation coverage incomplete</span
          >{/if}{#if hovered.aggregated}<span
            >{hovered.represented.toLocaleString()} files represented · Open to inspect</span
          >{/if}
      </div>{/if}
  </div>
  <div class="map-encoding">
    <span
      >Area = {sizeMode === 'allocated'
        ? 'unique allocation, not recoverable space'
        : 'logical bytes'}.</span
    >
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
          : 'age bands: 30 / 180 / 365 days / 10 years'}. Unknown metrics keep a low block.</span
    >
    {#if sizeMode === 'allocated' && layout.rects.some((r) => !r.allocation_known)}<span
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
      >{layout.represented.toLocaleString()} files represented · {layout.rects.length} blocks · {Math.max(
        0,
        layout.rects.length - drawn,
      )} outside viewport{#if matches}
        · {matches.size} highlighted{/if}</span
    >
  </div>
</section>
