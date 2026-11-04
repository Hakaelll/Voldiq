<script lang="ts">
  import { historicalRects, historicalAbsence } from '../lib/history-geometry';
  import { bytes, type SnapshotVisualFrame } from '../lib/types';
  let {
    frame,
    maximum,
    width,
    label,
    id,
    front = false,
    reveal = 100,
  }: {
    frame: SnapshotVisualFrame;
    maximum: number;
    width: number;
    label: string;
    id: string;
    front?: boolean;
    reveal?: number;
  } = $props();
  const rects = $derived(historicalRects(frame, maximum, width));
  const absence = $derived(historicalAbsence(frame));
</script>

<svg
  {id}
  class="snapshot-visual-map"
  class:before={front}
  style:clip-path={front ? `inset(0 ${100 - reveal}% 0 0)` : 'none'}
  viewBox={`0 0 ${Math.max(1, width)} 320`}
  role="img"
  aria-label={label}
>
  {#each rects as rect}<g
      ><rect
        x={rect.x}
        y={rect.y}
        width={rect.width}
        height={rect.height}
        fill={rect.paint.fill}
        stroke="var(--canvas)"
        stroke-width="1"
        ><title
          >{rect.name} · {rect.source?.logical ?? frame.remainder?.logical ?? '0'} logical bytes{rect.incomplete
            ? ' · partial'
            : ''}</title
        ></rect
      >
      {#if rect.width >= 80 && rect.height >= 34}<text
          x={rect.x + 6}
          y={rect.y + 15}
          fill={rect.paint.ink}
          >{rect.name.slice(0, Math.max(1, Math.floor((rect.width - 12) / 7)))}</text
        ><text x={rect.x + 6} y={rect.y + 29} fill={rect.paint.ink}>{bytes(rect.value)}</text>{/if}
    </g>{/each}
  {#if !rects.length}<text x="16" y="40" fill="var(--secondary)">{absence}</text>{/if}
</svg>
