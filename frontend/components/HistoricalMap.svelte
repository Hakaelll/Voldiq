<script lang="ts">
  import { historicalRects, historicalAbsence, historicalChange, formatHistoricalDelta, type HistoricalMapColor } from '../lib/history-geometry';
  import { formatStorageBytes } from '../lib/storage-units';
  import type { SnapshotVisualFrame, SnapshotVisualPair } from '../lib/types';
  let {
    frame,
    maximum,
    width,
    height,
    color,
    label,
    id,
    front = false,
    reveal = 100,
    tileDeltas,
  }: {
    frame: SnapshotVisualFrame;
    maximum: number;
    width: number;
    height: number;
    color: HistoricalMapColor;
    label: string;
    id: string;
    front?: boolean;
    reveal?: number;
    tileDeltas?: SnapshotVisualPair['tile_deltas'];
  } = $props();
  const rects = $derived(historicalRects(frame, maximum, width, height, color));
  const absence = $derived(historicalAbsence(frame));
</script>

<svg
  {id}
  class="snapshot-visual-map"
  class:before={front}
  style:clip-path={front ? `inset(0 ${100 - reveal}% 0 0)` : 'none'}
  viewBox={`0 0 ${Math.max(1, width)} ${Math.max(1, height)}`}
  role="img"
  aria-label={label}
>
  {#each rects as rect}
    {@const logical = rect.source?.logical ?? frame.remainder?.logical ?? '0'}
    {@const delta = rect.source ? tileDeltas?.[rect.source.path_id] : undefined}
    {@const change = tileDeltas && rect.source ? historicalChange(delta) : null}
    {@const paint = change?.paint ?? rect.paint}
    <g
      ><rect
        x={rect.x}
        y={rect.y}
        width={rect.width}
        height={rect.height}
        fill={paint.fill}
        stroke="var(--canvas)"
        stroke-width="1"
        ><title
          >{rect.name} · {logical} logical bytes{rect.incomplete
            ? ' · partial'
            : ''}{change ? ` · ${change.label}${delta === undefined ? '' : ` (${formatHistoricalDelta(delta)})`}` : ''}</title
        ></rect
      >
      {#if rect.width >= 80 && rect.height >= 34}<text
          x={rect.x + 6}
          y={rect.y + 15}
          fill={paint.ink}
          >{rect.name.slice(0, Math.max(1, Math.floor((rect.width - 12) / 7)))}</text
        ><text x={rect.x + 6} y={rect.y + 29} fill={paint.ink}>{formatStorageBytes(logical)}</text>{/if}
    </g>{/each}
  {#if !rects.length}<text x="16" y="40" fill="var(--secondary)">{absence}</text>{/if}
</svg>
