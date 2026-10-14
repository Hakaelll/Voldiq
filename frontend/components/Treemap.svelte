<script lang="ts">
  import { onMount } from 'svelte';
  import { nestedLayout, mapColor, type Rect, type WeightedNode } from '../lib/treemap';
  import { bytes } from '../lib/types';
  let {
    nodes,
    selected,
    onselect,
    onopen,
  }: {
    nodes: WeightedNode[];
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
    for (const rect of rects) {
      const paint = mapColor(rect.name);
      ctx.fillStyle = paint.fill;
      ctx.fillRect(
        rect.x + 1,
        rect.y + 1,
        Math.max(0, rect.width - 2),
        Math.max(0, rect.height - 2),
      );
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
        ctx.font = '600 13px "Segoe UI", sans-serif';
        ctx.fillText(rect.name, rect.x + 12, rect.y + 24);
        ctx.font = '12px "Segoe UI", sans-serif';
        ctx.fillText(bytes(rect.value), rect.x + 12, rect.y + 44);
        ctx.restore();
      }
    }
  }
  $effect(() => {
    void rects;
    void selected;
    void hovered;
    draw();
  });
  onMount(() => {
    const observer = new ResizeObserver((entries) => {
      const bounds = entries[0]?.contentRect;
      if (bounds) {
        width = bounds.width;
        height = bounds.height;
      }
    });
    observer.observe(container);
    return () => observer.disconnect();
  });
  function hit(event: MouseEvent): Rect | null {
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
