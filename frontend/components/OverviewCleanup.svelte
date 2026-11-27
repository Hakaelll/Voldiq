<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { IndexId } from '../lib/index-context';
  import { overviewCleanupQueries, type OverviewCleanupReport } from '../lib/queries';
  import { formatStorageBytes } from '../lib/storage-units';
  import { date } from '../lib/types';

  let {
    indexId,
    paused,
    onreview,
  }: { indexId: IndexId; paused: boolean; onreview: (owner: IndexId) => void } = $props();

  type Model =
    | { kind: 'idle' | 'loading' | 'cancelled' }
    | { kind: 'failed'; message: string }
    | { kind: 'ready'; report: OverviewCleanupReport };

  let model = $state.raw<Model>({ kind: 'idle' });
  let active: ReturnType<typeof overviewCleanupQueries.request> | null = null;

  function cancelRead() {
    if (!active) return;
    active.cancel();
    active = null;
    model = { kind: 'cancelled' };
  }

  async function query() {
    if (active) {
      cancelRead();
      return;
    }
    if (paused) return;
    const owner = indexId;
    const request = overviewCleanupQueries.request({ indexId: owner, scope: 0 });
    active = request;
    model = { kind: 'loading' };
    try {
      const report = await request.result;
      if (active === request && owner === indexId)
        model = report && !paused ? { kind: 'ready', report } : { kind: 'cancelled' };
    } catch (problem) {
      if (active === request && owner === indexId)
        model = paused ? { kind: 'cancelled' } : { kind: 'failed', message: String(problem) };
    } finally {
      if (active === request) active = null;
    }
  }

  $effect(() => {
    if (paused) cancelRead();
  });
  onDestroy(() => {
    active?.cancel();
    active = null;
  });
</script>

<section class="overview-cleanup" aria-labelledby="overview-cleanup-title">
  <div class="summary-heading">
    <h2 id="overview-cleanup-title">Cleanup candidates</h2>
    <button class="quiet-action" onclick={query} disabled={paused}>
      {model.kind === 'loading' ? 'Cancel' : model.kind === 'idle' ? 'Check' : 'Check again'}
    </button>
  </div>
  <p>Old files · Scan root</p>
  <div role="status">
    {#if model.kind === 'loading'}
      <p>Checking the current analysis…</p>
    {:else if model.kind === 'cancelled'}
      <p>Check cancelled.</p>
    {:else if model.kind === 'failed'}
      <p class="check-error">{model.message}</p>
    {:else if model.kind === 'ready'}
      {@const report = model.report}
      {#if report.kind === 'observed'}
        {#if report.total > 0 || report.complete}
          <p class="candidate-summary" title={`${report.logical} logical bytes`}>
            <strong>{report.total.toLocaleString()} {report.total === 1 ? 'candidate' : 'candidates'}</strong>
            · {formatStorageBytes(report.logical)} logical
          </p>
        {:else}
          <p>No matches within verified coverage.</p>
        {/if}
        <p>{report.risk === 'review' ? 'Manual review' : report.risk}{report.complete ? '' : ' · Partial coverage'}</p>
        <p class="assessment" title={report.root}>Checked {date(report.assessed_at)}</p>
      {:else}
        <p>Candidate evidence unavailable for this analysis.</p>
      {/if}
    {/if}
  </div>
  {#if model.kind === 'ready' && model.report.kind === 'observed'}
    <button class="quiet-action review-action" onclick={() => onreview(indexId)} disabled={paused}>
      Review old files
    </button>
  {/if}
</section>

<style>
  .candidate-summary {
    color: var(--text);
  }
  .check-error {
    overflow-wrap: anywhere;
  }
  .assessment {
    font-variant-numeric: tabular-nums;
  }
  .review-action {
    margin-top: 6px;
  }
</style>
