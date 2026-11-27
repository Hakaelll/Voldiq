<script lang="ts">
  import { CHART_PAINTS, SIZE_PAINTS } from '../lib/chart-colors';
  import { historicalChanges, type ComparisonMapColor } from '../lib/history-geometry';

  let {
    color,
    hasRemainder,
    compare = false,
    onchange,
  }: {
    color: ComparisonMapColor;
    hasRemainder: boolean;
    compare?: boolean;
    onchange: (color: ComparisonMapColor) => void;
  } = $props();

  const name = $props.id();
  const options = [
    { id: 'size', label: 'Size' },
    { id: 'type', label: 'File type' },
    { id: 'change', label: 'Recorded change' },
  ] as const;
</script>

<div class="history-map-colors">
  <div class="map-renderer-switch" role="radiogroup" aria-label="Map colors">
    {#each options.filter((option) => compare || option.id !== 'change') as option (option.id)}
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
  {#if color !== 'type' || hasRemainder}
    <div class="map-category-legend" aria-label="Map color legend">
      {#if color === 'size'}
        <span>
          Smaller {#each SIZE_PAINTS as paint}<i style:background={paint.fill}></i>{/each} Larger
        </span>
      {/if}
      {#if color === 'change'}
        {#each Object.values(historicalChanges) as change (change.label)}
          <span>
            <i style:background={change.paint.fill}></i>
            {hasRemainder && change === historicalChanges.unavailable
              ? 'Change unavailable (includes Other)'
              : change.label}
          </span>
        {/each}
      {/if}
      {#if hasRemainder && color !== 'change'}
        <span><i style:background={CHART_PAINTS.unknown.fill}></i>Other contents</span>
      {/if}
    </div>
  {/if}
</div>
