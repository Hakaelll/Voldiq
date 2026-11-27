<script lang="ts">
  import { priorityCoverage, priorityPoints, type ReviewPriority } from '../lib/review-priority';

  let { priority }: { priority: ReviewPriority } = $props();
  const coverage = $derived(priorityCoverage(priority));
</script>

<span class="review-priority-meter">
  {#if coverage === null}
    <span class="evidence">No weighted evidence</span>
  {:else}
    <span class="score">
      <strong>{priorityPoints(priority.points)}</strong><span class="scale">/100</span>
    </span>
    <span class="track" aria-hidden="true">
      <span style:inline-size={`${priority.points / 100}%`}></span>
    </span>
    <span class="evidence">{coverage} evidence</span>
  {/if}
</span>

<style>
  .review-priority-meter {
    display: inline-grid;
    grid-template-columns: max-content minmax(36px, 56px);
    align-items: center;
    gap: 2px 8px;
    color: var(--text);
    vertical-align: middle;
    text-align: start;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .score {
    min-width: 65px;
  }
  .scale,
  .evidence {
    color: var(--secondary);
    font-size: 11px;
  }
  .evidence {
    grid-column: 1 / -1;
  }
  .track {
    height: 4px;
    overflow: hidden;
    border-radius: 3px;
    background: var(--border);
  }
  .track > span {
    display: block;
    height: 100%;
    background: var(--chart-violet);
  }
  @media (forced-colors: active) {
    .track {
      forced-color-adjust: none;
      background: GrayText;
    }
    .track > span {
      background: Highlight;
    }
  }
</style>
