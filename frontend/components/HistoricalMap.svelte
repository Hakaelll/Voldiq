<script lang="ts">
  import { layout, type WeightedNode } from '../lib/treemap';
  import { categoryPaint } from '../lib/storage-map';
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
  const rects = $derived.by(() => {
    const nodes: WeightedNode[] = frame.entries.map((entry, index) => ({
      id: index + 1,
      name: entry.name,
      value: Number(entry.logical),
      directory: entry.kind === 'directory',
      incomplete: entry.incomplete,
    }));
    if (frame.remainder)
      nodes.push({
        id: -1,
        name: 'Other indexed contents',
        value: Number(frame.remainder.logical),
        directory: false,
        category: 'remainder',
      });
    return layout(nodes, maximum > 0 ? (width * Number(frame.logical ?? 0)) / maximum : 0, 320).map(
      (rect) => ({
        ...rect,
        paint: categoryPaint(rect),
        source: rect.id > 0 ? frame.entries[rect.id - 1] : null,
      }),
    );
  });
  const absence = $derived(
    frame.presence === 'not_recorded'
      ? 'No entry recorded in this capture.'
      : frame.presence === 'not_observed'
        ? 'Not observed; capture coverage is incomplete.'
        : frame.incomplete
          ? 'No positive-size geometry in the partial records.'
          : 'Zero recorded logical bytes.',
  );
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
