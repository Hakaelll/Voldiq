<script lang="ts">
  import { isTauri } from '@tauri-apps/api/core';
  import { utcDate } from '../lib/history-activity';
  import type { DashboardTrend } from '../lib/history-trend';
  import type { IndexId } from '../lib/index-context';
  import { dashboardTrendQueries } from '../lib/queries';
  import { formatStorageBytes } from '../lib/storage-units';
  import type { Snapshot } from '../lib/types';

  let { indexId, paused, onopen }: {
    indexId: IndexId;
    paused: boolean;
    onopen: (capture: Snapshot, index: IndexId) => void;
  } = $props();
  type Model =
    | { kind: 'loading' }
    | { kind: 'error' }
    | { kind: 'ready'; owner: IndexId; value: DashboardTrend };
  let model = $state.raw<Model>({ kind: 'loading' });
  let retry = $state(0);
  const native = isTauri();
  const report = $derived(model.kind === 'ready' && model.owner === indexId ? model.value : null);

  function signedBytes(raw: string): string {
    const bytes = BigInt(raw);
    if (bytes < 0n) return `−${formatStorageBytes(-bytes)}`;
    return `${bytes > 0n ? '+' : ''}${formatStorageBytes(bytes)}`;
  }

  $effect(() => {
    const owner = indexId;
    void retry;
    model = { kind: 'loading' };
    if (paused || !native) return;
    const request = dashboardTrendQueries.request({ indexId: owner });
    let current = true;
    void request.result.then(
      (value) => {
        if (current && owner === indexId)
          model = value ? { kind: 'ready', owner, value } : { kind: 'error' };
      },
      () => {
        if (current && owner === indexId) model = { kind: 'error' };
      },
    );
    return () => {
      current = false;
      request.cancel();
    };
  });
</script>

{#if native}
  <section class="overview-trend" aria-labelledby="overview-trend-title">
    <div class="summary-heading">
      <h2 id="overview-trend-title">Recent folder growth</h2>
      {#if !paused && report?.kind === 'recorded'}
        {@const capture = report.summary.capture}
        <button class="quiet-action" onclick={() => onopen(capture, indexId)}
          >Open trends</button
        >
      {/if}
    </div>
    {#if paused}
      <p>Available after this operation.</p>
    {:else if report?.kind === 'unavailable'}
      <p>No retained capture for this analysis scope.</p>
    {:else if report?.kind === 'recorded'}
      {@const { latest, forecast } = report.summary}
      {#if latest && latest.delta !== null}
        <p>
          <strong title={`${latest.delta} bytes`}>{signedBytes(latest.delta)}</strong>
          since the previous comparable measurement
        </p>
        <p>Measured {utcDate(latest.measured_at)}</p>
      {:else}
        <p>Comparable measurements unavailable.</p>
      {/if}
      {#if forecast}
        <p title={`${forecast.logical} logical bytes`}>
          Projected {formatStorageBytes(forecast.logical)} · {utcDate(forecast.at)}
        </p>
      {/if}
    {:else if model.kind === 'error'}
      <p role="status">History unavailable. <button onclick={() => retry++}>Retry</button></p>
    {:else}
      <p role="status">Loading measured growth…</p>
    {/if}
  </section>
{/if}

<style>
  .overview-trend {
    border-top: 1px solid var(--border);
    padding-top: 1rem;
  }
  p {
    margin: 0.375rem 0 0;
    color: var(--secondary);
    font-size: 0.8125rem;
    line-height: 1.5;
    overflow-wrap: anywhere;
  }
  strong {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
</style>
