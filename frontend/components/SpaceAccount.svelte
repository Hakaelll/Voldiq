<script lang="ts">
  import { tick } from 'svelte';
  import { spaceAccountQueries } from '../lib/queries';
  import {
    spaceMeasurement,
    spaceCaller,
    type SpaceReport,
    type SpaceResidual,
  } from '../lib/space-account';
  import type { IndexId } from '../lib/index-context';
  import { bytes } from '../lib/types';
  import { utcDate } from '../lib/history-activity';
  let { indexId, paused }: { indexId: IndexId; paused: boolean } = $props();
  type Model =
    | { kind: 'closed' }
    | { kind: 'paused' }
    | { kind: 'loading' }
    | { kind: 'cancelled' }
    | { kind: 'failed'; message: string }
    | { kind: 'ready'; indexId: IndexId; report: SpaceReport };
  let model = $state.raw<Model>({ kind: 'closed' });
  let expanded = $state(false),
    retry = $state(0);
  let controls: HTMLDivElement;
  let reload = $state<HTMLButtonElement>();
  let cancelRead = () => {};
  const report = $derived(
    model.kind === 'ready' && model.indexId === indexId ? model.report : null,
  );
  const measurement = $derived(spaceMeasurement(report));
  const caller = $derived(spaceCaller(report));
  const unavailable: Record<Extract<SpaceResidual, { kind: 'unavailable' }>['reason'], string> = {
    scoped_only: 'Whole-volume data is outside this report.',
    root_unavailable: 'The captured root cannot be matched. Scan again.',
    volume_unavailable: 'Whole-volume counters are unavailable.',
    allocation_unavailable: 'Compatible indexed allocation evidence is unavailable.',
    volume_mismatch: 'Volume identities do not match. Scan again.',
  };
  const signed = (value: string) =>
    `${Number(value) < 0 ? '−' : Number(value) > 0 ? '+' : ''}${bytes(Math.abs(Number(value)))}`;
  $effect(() => {
    void retry;
    if (!expanded) {
      model = { kind: 'closed' };
      return;
    }
    if (paused) {
      model = { kind: 'paused' };
      return;
    }
    const heldFocus = controls?.contains(document.activeElement);
    const context = indexId;
    model = { kind: 'loading' };
    const request = spaceAccountQueries.request({ indexId: context });
    cancelRead = request.cancel;
    let alive = true;
    async function restore(): Promise<void> {
      await tick();
      if (alive && heldFocus && document.activeElement === document.body)
        reload?.focus({ preventScroll: true });
    }
    void request.result.then(
      (value) => {
        if (!alive) return;
        model = value ? { kind: 'ready', indexId: context, report: value } : { kind: 'cancelled' };
        void restore();
      },
      (problem) => {
        if (alive) {
          model = { kind: 'failed', message: String(problem) };
          void restore();
        }
      },
    );
    return () => {
      alive = false;
      request.cancel();
    };
  });
</script>

<section class="type-panel space-account" aria-labelledby="space-account-heading">
  <div class="panel-heading">
    <h2 id="space-account-heading">Space account</h2>
    <div class="space-controls" bind:this={controls}>
      <button
        aria-expanded={expanded}
        aria-controls="space-account-body"
        onclick={() => {
          expanded = !expanded;
        }}>{expanded ? 'Close' : 'Explain space'}</button
      >
      {#if expanded}<button
          bind:this={reload}
          disabled={model.kind === 'loading' || paused}
          onclick={() => {
            retry += 1;
          }}>Reload</button
        >{/if}
      {#if model.kind === 'loading'}<button onclick={() => cancelRead()}>Cancel</button>{/if}
    </div>
  </div>
  {#if expanded}<div id="space-account-body" class="space-body">
      {#if model.kind === 'loading'}<p role="status">Reading space measurements…</p>
      {:else if model.kind === 'paused'}<p>Available after the current operation.</p>
      {:else if model.kind === 'cancelled'}<p role="status">Read cancelled. Reload to retry.</p>
      {:else if model.kind === 'failed'}<p class="space-error" role="alert">{model.message}</p>
      {:else if report}
        <p class="space-root">Analyzed root · {report.indexed.root}</p>
        <dl class="space-totals">
          <div>
            <dt>Known indexed allocation</dt>
            <dd
              title={report.indexed.allocation.kind === 'measured'
                ? `${report.indexed.allocation.known} bytes`
                : undefined}
            >
              {report.indexed.allocation.kind === 'measured'
                ? bytes(Number(report.indexed.allocation.known))
                : 'Unavailable'}
            </dd>
          </div>
          <div>
            <dt>Observed volume used</dt>
            <dd title={measurement ? `${measurement.used} bytes` : undefined}>
              {measurement ? bytes(Number(measurement.used)) : 'Unavailable'}
            </dd>
          </div>
          <div>
            <dt>Remaining difference</dt>
            <dd
              title={report.residual.kind === 'compared'
                ? `${report.residual.bytes} bytes`
                : undefined}
            >
              {report.residual.kind === 'compared' ? signed(report.residual.bytes) : 'Unavailable'}
            </dd>
          </div>
        </dl>
        {#if report.residual.kind === 'unavailable'}<p>{unavailable[report.residual.reason]}</p>
        {:else}<p>
            Volume used minus known indexed file allocation. {report.residual.against_complete_scope
              ? 'Recorded scope complete.'
              : 'Unmeasured allocation remains.'} Measurements can come from different times.
          </p>{/if}
        <div class="space-facts">
          <span
            >{report.indexed.observation
              ? `Files measured ${utcDate(report.indexed.observation.completed_at)}`
              : 'File measurement date unavailable'}</span
          >
          <span
            >{measurement?.observed_at !== null && measurement?.observed_at !== undefined
              ? `Volume measured ${utcDate(measurement.observed_at)}`
              : 'Volume measurement date unavailable'}</span
          >
          <span>Coverage · {report.indexed.coverage}</span>
          <span>Allocation accounting · {report.indexed.accounting}</span>
          <span
            >{report.indexed.starts_at_volume_root
              ? 'Index starts at volume root'
              : 'Folder scope within a larger volume'} · {report.indexed.exclusions} exclusions · {report
              .indexed.errors} access issues</span
          >
          <span title={`${report.indexed.logical_paths} logical path bytes`}
            >Logical path total · {bytes(Number(report.indexed.logical_paths))}</span
          >
        </div>
        <details>
          <summary>Context and possible contributors</summary>
          <dl class="space-context">
            <div>
              <dt>Indexed paths reporting multiple links</dt>
              <dd>{report.indexed.hardlink_paths.toLocaleString()}</dd>
            </div>
            <div>
              <dt>Cloud file placeholder paths</dt>
              <dd title={`${report.indexed.cloud_logical_paths} logical bytes`}>
                {report.indexed.cloud_file_paths.toLocaleString()} · {bytes(
                  Number(report.indexed.cloud_logical_paths),
                )} logical
              </dd>
            </div>
            <div>
              <dt>File paths with recall flags</dt>
              <dd>{report.indexed.recall_required_file_paths.toLocaleString()}</dd>
            </div>
            <div>
              <dt>Other reparse records not followed</dt>
              <dd>{report.indexed.opaque_reparse_records.toLocaleString()}</dd>
            </div>
            {#if report.indexed.allocation.kind === 'measured'}<div>
                <dt>File records without known allocation</dt>
                <dd>{report.indexed.allocation.unknown_file_records.toLocaleString()}</dd>
              </div>{/if}
            {#if caller}<div>
                <dt>Available to you</dt>
                <dd title={`${caller.available} bytes`}>{bytes(Number(caller.available))}</dd>
              </div>{/if}
            {#if caller && !measurement}<div>
                <dt>Availability measured</dt>
                <dd>{utcDate(caller.observed_at)}</dd>
              </div>{/if}
            {#if measurement}
              <div>
                <dt>Volume free</dt>
                <dd title={`${measurement.free} bytes`}>{bytes(Number(measurement.free))}</dd>
              </div>
              <div>
                <dt>Reserved space reported by Windows</dt>
                <dd title={`${measurement.reserved} bytes`}>
                  {bytes(Number(measurement.reserved))}
                </dd>
              </div>
              <div>
                <dt>Storage reserve, included in reserved space</dt>
                <dd title={`${measurement.storage_reserve} bytes`}>
                  {bytes(Number(measurement.storage_reserve))}
                </dd>
              </div>
              <div>
                <dt>Pool space unavailable</dt>
                <dd title={`${measurement.pool_unavailable} bytes`}>
                  {bytes(Number(measurement.pool_unavailable))}
                </dd>
              </div>
            {/if}
          </dl>
          <p>
            {#if report.indexed.accounting === 'known'}Indexed accounting counts hardlink allocation
              once.{/if} Logical path totals include file names and placeholder metadata; resident content
            is not established by these totals.
          </p>
          <p>
            Possible contributors include files outside this scope, excluded or inaccessible areas,
            filesystem metadata, shared extents, sparse or compressed data, system copies and files
            retained by processes. These causes are not measured here. Recycle Bin and recovery
            copies may already be included in indexed allocation. Context values are not additional
            deductions or promised reclaimable space.
          </p>
        </details>
      {/if}
    </div>{/if}
</section>

<style>
  .space-controls,
  .space-facts {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 14px;
  }
  .space-controls button {
    min-height: 28px;
    font-size: 12px;
  }
  .space-root {
    overflow-wrap: anywhere;
  }
  .space-body {
    font-size: 12px;
    color: var(--secondary);
  }
  .space-body p {
    line-height: 1.5;
  }
  .space-totals {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 12px;
  }
  .space-totals div {
    min-width: 0;
  }
  .space-totals dd {
    margin: 6px 0 0;
    color: var(--text);
    font-size: 20px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .space-facts {
    margin: 12px 0;
  }
  details {
    border-top: 1px solid var(--border);
    padding-top: 10px;
  }
  summary {
    cursor: pointer;
    color: var(--text);
  }
  .space-context div {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    padding: 5px 0;
  }
  .space-context dd {
    margin: 0;
    text-align: right;
    overflow-wrap: anywhere;
  }
  .space-error {
    color: var(--error);
  }
  @media (max-width: 1000px) {
    .space-totals {
      grid-template-columns: 1fr;
    }
  }
</style>
