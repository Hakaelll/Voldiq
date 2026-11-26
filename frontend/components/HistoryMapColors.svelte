<script lang="ts">
  import { CHART_PAINTS, SIZE_PAINTS } from '../lib/chart-colors';
  import type { HistoricalMapColor } from '../lib/history-geometry';

  let {
    color,
    hasRemainder,
    onchange,
  }: {
    color: HistoricalMapColor;
    hasRemainder: boolean;
    onchange: (color: HistoricalMapColor) => void;
  } = $props();

  const name = $props.id();
  const options = [
    { id: 'size', label: 'Size' },
    { id: 'type', label: 'File type' },
  ] as const;
</script>

<div class="history-map-colors">
  <div class="map-renderer-switch" role="radiogroup" aria-label="Map colors">
    {#each options as option (option.id)}
      <label class="map-renderer-option">
        <input
          type="radio"
          class="sr-only"
          {name}
          value={option.id}
          checked={color === option.id}
          onchange={() => onchange(option.id)}
        />
        <span>{option.label}</span>
      </label>
    {/each}
  </div>
  {#if color === 'size' || hasRemainder}
    <div class="map-category-legend" aria-label="Map color legend">
      {#if color === 'size'}
        <span>
          Smaller {#each SIZE_PAINTS as paint}<i style:background={paint.fill}></i>{/each} Larger
        </span>
      {/if}
      {#if hasRemainder}
        <span><i style:background={CHART_PAINTS.unknown.fill}></i>Other contents</span>
      {/if}
    </div>
  {/if}
</div>
