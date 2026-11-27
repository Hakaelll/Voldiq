<script lang="ts">
  import {
    priorityPoints,
    reviewEvidence,
    reviewFactors,
    type ReviewPriority,
  } from '../lib/review-priority';
  import { date } from '../lib/types';

  let { id, priority, assessedAt }: { id?: string; priority: ReviewPriority; assessedAt: number } =
    $props();
</script>

<div {id} class="priority-details">
  <div class="priority-summary">
    <strong>Evidence coverage {priority.coverage}%</strong>
    <span>Assessed {date(assessedAt)}</span>
  </div>
  <dl>
    {#each priority.signals as signal, at}
      <div>
        <dt>{reviewFactors[at][1]}</dt>
        <dd>{reviewEvidence[signal.evidence]}</dd>
        <dd class="number">Weight {signal.weight} · {priorityPoints(signal.points)} pts</dd>
      </div>
    {/each}
  </dl>
  <p>
    A matching local file provides no backup guarantee. Review the files before confirming any
    action.
  </p>
</div>

<style>
  .priority-details {
    max-width: 760px;
    margin-inline: 30px 0;
    padding-block: 6px;
  }
  .priority-summary {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 18px;
  }
  span,
  dd,
  p {
    color: var(--secondary);
  }
  dl {
    margin: 12px 0;
  }
  dl div {
    display: grid;
    grid-template-columns: minmax(110px, 1fr) minmax(0, 2fr) max-content;
    gap: 12px;
    padding-block: 6px;
    border-bottom: 1px solid var(--border);
  }
  dd {
    margin: 0;
  }
  @media (max-width: 700px) {
    dl div {
      display: flex;
      flex-wrap: wrap;
    }
  }
</style>
