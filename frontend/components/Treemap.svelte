<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import {
    createTreemapAnimator,
    resolveZoom,
    zoomBox,
    zoomEase,
    type Box,
    type Zoom,
  } from '../lib/treemap-motion';
  import { nestedLayout, mapColor, type Rect, type WeightedNode } from '../lib/treemap';
  import { bytes } from '../lib/types';
  let {
    nodes,
    scope,
    reduceMotion,
    animateNavigation,
    selected,
    onselect,
    onopen,
  }: {
    nodes: WeightedNode[];
    scope: number;
    reduceMotion: boolean;
    animateNavigation: boolean;
    selected: number | null;
    onselect: (id: number) => void;
    onopen: (id: number) => void;
  } = $props();
  let canvas: HTMLCanvasElement;
  let container: HTMLDivElement;
  let width = $state(0),
    height = $state(0);
  let hovered = $state<Rect | null>(null);
  let tooltip = $state({ x: 0, y: 0 });
  let rects = $derived(nestedLayout(nodes, width, height));
  let systemReduced = $state(false);
  let previous: { scope: number; rects: Rect[]; width: number; height: number } | null = null;
  let motion: { zoom: Zoom; before: Rect[]; after: Rect[] } | null = null;
  let progress = 1;
  const animation = createTreemapAnimator(
    {
      now: () => performance.now(),
      request: (callback) => requestAnimationFrame(callback),
      cancel: (id) => cancelAnimationFrame(id),
    },
    (value) => {
      progress = value;
      if (value === 1) motion = null;
      draw();
    },
  );
  function draw(): void {
    if (!canvas || width <= 0 || height <= 0) return;
    const ratio = window.devicePixelRatio || 1;
    const pixelWidth = Math.round(width * ratio);
    const pixelHeight = Math.round(height * ratio);
    if (canvas.width !== pixelWidth) canvas.width = pixelWidth;
    if (canvas.height !== pixelHeight) canvas.height = pixelHeight;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.clearRect(0, 0, width, height);
    const full = { x: 0, y: 0, width, height };
    if (!motion) {
      paint(ctx, rects, 1, full);
      return;
    }
    const amount = zoomEase(progress);
    const box = zoomBox(motion.zoom, width, height, progress);
    if (motion.zoom.direction === 'in') {
      paint(ctx, motion.before, 1 - amount, full);
      paint(ctx, motion.after, amount, box);
    } else {
      paint(ctx, motion.after, amount, full);
      paint(ctx, motion.before, 1 - amount, box);
    }
  }
  function paint(ctx: CanvasRenderingContext2D, items: Rect[], opacity: number, box: Box): void {
    ctx.globalAlpha = opacity;
    for (const item of items) {
      const rect = {
        ...item,
        x: box.x + (item.x * box.width) / width,
        y: box.y + (item.y * box.height) / height,
        width: (item.width * box.width) / width,
        height: (item.height * box.height) / height,
      };
      const paint = rect.paint ?? mapColor(rect.name);
      ctx.fillStyle = paint.fill;
      ctx.fillRect(
        rect.x + 1,
        rect.y + 1,
        Math.max(0, rect.width - 2),
        Math.max(0, rect.height - 2),
      );
      if (rect.width > 3 && rect.height > 3) {
        ctx.strokeStyle = paint.ink;
        ctx.lineWidth = 1;
        ctx.strokeRect(rect.x + 1.5, rect.y + 1.5, rect.width - 3, rect.height - 3);
      }
      if (rect.id === selected || rect.id === hovered?.id) {
        ctx.save();
        ctx.beginPath();
        ctx.rect(rect.x + 1, rect.y + 1, Math.max(0, rect.width - 2), Math.max(0, rect.height - 2));
        ctx.clip();
        ctx.strokeStyle = paint.ink;
        ctx.lineWidth = rect.id === selected ? 4 : 2;
        ctx.strokeRect(
          rect.x + 2,
          rect.y + 2,
          Math.max(0, rect.width - 4),
          Math.max(0, rect.height - 4),
        );
        ctx.restore();
      }
      if (rect.width > 92 && rect.height > 52) {
        ctx.save();
        ctx.beginPath();
        ctx.rect(rect.x + 10, rect.y + 8, rect.width - 20, rect.height - 16);
        ctx.clip();
        ctx.fillStyle = paint.ink;
        ctx.font = rect.children?.length
          ? '600 18px "Segoe UI", sans-serif'
          : '600 13px "Segoe UI", sans-serif';
        ctx.fillText(rect.name, rect.x + 12, rect.y + 24);
        ctx.font = '12px "Segoe UI", sans-serif';
        const size =
          rect.share !== undefined && rect.width >= 180
            ? `${bytes(rect.value)} (${(rect.share * 100).toFixed(1)}%)`
            : bytes(rect.value);
        ctx.fillText(size, rect.x + 12, rect.y + 44);
        ctx.restore();
      }
    }
    ctx.globalAlpha = 1;
  }
  $effect(() => {
    const next = { scope, rects, width, height };
    const reduced = reduceMotion || systemReduced;
    const animate = animateNavigation;
    untrack(() => {
      animation.stop();
      const before = previous;
      previous = next;
      hovered = null;
      if (
        before &&
        !reduced &&
        animate &&
        before.width === width &&
        before.height === height &&
        width > 0 &&
        height > 0
      ) {
        const zoom = resolveZoom(before.scope, scope, before.rects, rects);
        if (zoom) {
          motion = { zoom, before: before.rects, after: rects };
          animation.start();
          return;
        }
      }
      draw();
    });
  });
  $effect(() => {
    void selected;
    void hovered;
    untrack(draw);
  });
  onMount(() => {
    const observer = new ResizeObserver((entries) => {
      const bounds = entries[0]?.contentRect;
      if (bounds) {
        width = bounds.width;
        height = bounds.height;
      }
    });
    const preference = matchMedia('(prefers-reduced-motion: reduce)');
    const preferenceChanged = (): void => {
      systemReduced = preference.matches;
    };
    const interrupt = (): void => {
      animation.stop();
    };
    const visibility = (): void => {
      if (document.hidden) animation.stop();
    };
    preferenceChanged();
    preference.addEventListener('change', preferenceChanged);
    document.addEventListener('pointerdown', interrupt, true);
    document.addEventListener('keydown', interrupt, true);
    document.addEventListener('visibilitychange', visibility);
    observer.observe(container);
    return () => {
      observer.disconnect();
      preference.removeEventListener('change', preferenceChanged);
      document.removeEventListener('pointerdown', interrupt, true);
      document.removeEventListener('keydown', interrupt, true);
      document.removeEventListener('visibilitychange', visibility);
      animation.stop();
    };
  });
  function hit(event: MouseEvent): Rect | null {
    if (animation.running()) return null;
    const bounds = canvas.getBoundingClientRect();
    const x = event.clientX - bounds.left,
      y = event.clientY - bounds.top;
    tooltip = {
      x: Math.min(width - 220, Math.max(8, x + 14)),
      y: Math.min(height - 60, Math.max(8, y + 14)),
    };
    for (let index = rects.length - 1; index >= 0; index -= 1) {
      const rect = rects[index];
      if (x >= rect.x && y >= rect.y && x <= rect.x + rect.width && y <= rect.y + rect.height)
        return rect;
    }
    return null;
  }
</script>

<div class="map" bind:this={container}>
  <canvas
    bind:this={canvas}
    aria-label="Storage treemap. Use the adjacent item list for keyboard navigation."
    onmousemove={(event) => {
      hovered = hit(event);
    }}
    onmouseleave={() => {
      hovered = null;
    }}
    onclick={(event) => {
      const rect = hit(event);
      if (rect) onselect(rect.id);
    }}
    ondblclick={(event) => {
      const rect = hit(event);
      if (rect) onopen(rect.id);
    }}
  ></canvas>
  {#if hovered}<div class="map-tooltip" style:left={`${tooltip.x}px`} style:top={`${tooltip.y}px`}>
      <strong>{hovered.name}</strong><span
        >{bytes(hovered.value)}{hovered.directory ? ' · Double-click to explore' : ''}</span
      >
    </div>{/if}
  {#if nodes.every((n) => n.value === 0)}<div class="map-empty">No files</div>{/if}
</div>
