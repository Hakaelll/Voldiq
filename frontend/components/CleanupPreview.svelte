<script lang="ts">
  import { onMount, onDestroy, tick, untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { Channel } from '@tauri-apps/api/core';
  import {
    bytes,
    date,
    type CleanupPreview,
    type CleanupReport,
    type RecyclingReport,
  } from '../lib/types';
  import { formatStorageBytes } from '../lib/storage-units';
  let {
    plan,
    onclose,
    onerror,
    onbusy,
    onchanged,
  }: {
    plan: CleanupPreview;
    onclose: () => void;
    onerror: (problem: unknown) => void;
    onbusy: (value: boolean) => void;
    onchanged: () => void;
  } = $props();
  type Execution =
    | { kind: 'idle' }
    | { kind: 'confirm' }
    | {
        kind: 'running';
        mode: 'recycle' | 'permanent';
        id: number | null;
        completed: number;
        total: number;
      }
    | { kind: 'finished'; report: CleanupReport }
    | { kind: 'recycled'; report: RecyclingReport }
    | { kind: 'failed'; message: string };
  let execution = $state<Execution>({ kind: 'idle' });
  const receipt = $derived(
    execution.kind === 'finished' || execution.kind === 'recycled'
      ? execution.report.receipt
      : null,
  );
  let confirmation = $state('');
  let recyclingAvailable = $state(false);
  async function execute(mode: 'recycle' | 'permanent'): Promise<void> {
    if (
      mode === 'permanent' &&
      (plan.scheduled || execution.kind !== 'confirm' || confirmation !== 'DELETE')
    )
      return;
    if (mode === 'recycle' && (execution.kind !== 'idle' || !recyclingAvailable)) return;
    execution = { kind: 'running', mode, id: null, completed: 0, total: plan.preview.items.length };
    onbusy(true);
    const progress = new Channel<{ id: number; completed: number; total: number }>();
    progress.onmessage = (update) => {
      if (execution.kind === 'running') execution = { ...execution, ...update };
    };
    try {
      if (mode === 'permanent') {
        const report = await invoke<CleanupReport>('execute_permanent_cleanup', {
          id,
          confirmation,
          progress,
        });
        execution = { kind: 'finished', report };
      } else {
        const report = await invoke<RecyclingReport>('execute_recycling_cleanup', {
          id,
          confirmation: 'RECYCLE',
          progress,
        });
        execution = { kind: 'recycled', report };
      }
    } catch (problem) {
      execution = { kind: 'failed', message: String(problem) };
    } finally {
      const result = receipt;
      const owner = dialog;
      const focus = document.activeElement;
      const restoreFocus =
        focus === document.body ||
        focus === owner ||
        (focus instanceof HTMLElement &&
          owner?.contains(focus) &&
          focus.closest('.execution-actions') !== null);
      onbusy(false);
      onchanged();
      if (result) {
        await tick();
        if (
          dialog === owner &&
          owner?.open &&
          owner.isConnected &&
          receipt === result &&
          restoreFocus &&
          (document.activeElement === focus || document.activeElement === document.body)
        ) {
          previewScroll.scrollTop = 0;
          resultHeading.focus({ preventScroll: true });
        }
      }
    }
  }
  function outcome(item: RecyclingReport['items'][number]): string {
    switch (item.outcome) {
      case 'recycled':
        return 'Sent to Recycle Bin';
      case 'returned':
        return 'Returned to original folder';
      case 'unmoved':
        return 'Not moved';
      case 'not_started':
        return 'Not attempted';
      case 'unknown':
        return 'Needs review';
    }
  }
  function signedBytes(value: string): string {
    const amount = BigInt(value);
    return amount < 0n
      ? `−${formatStorageBytes(-amount)}`
      : `${amount === 0n ? '' : '+'}${formatStorageBytes(amount)}`;
  }
  async function cancel(): Promise<void> {
    if (execution.kind === 'running' && execution.id !== null) {
      try {
        await invoke('cancel_operation', { id: execution.id });
      } catch (problem) {
        onerror(problem);
      }
    }
  }
  let dialog: HTMLDialogElement;
  let resultHeading: HTMLHeadingElement;
  let previewScroll: HTMLDivElement;
  const id = untrack(() => plan.id);
  function close(): void {
    if (execution.kind === 'running') {
      void cancel();
      return;
    }
    dialog.close();
    onclose();
  }
  onMount(() => {
    dialog.showModal();
    let alive = true;
    void invoke<{ recycling: boolean }>('cleanup_capabilities')
      .then((value) => {
        if (alive) recyclingAvailable = value.recycling;
      })
      .catch(onerror);
    return () => {
      alive = false;
    };
  });
  onDestroy(() => {
    void invoke('discard_cleanup', { id }).catch(onerror);
  });
</script>

<dialog
  bind:this={dialog}
  aria-labelledby="cleanup-title"
  oncancel={(event) => {
    event.preventDefault();
    close();
  }}
>
  <div class="preview-header">
    <h2 bind:this={resultHeading} id="cleanup-title" tabindex="-1">
      {receipt ? 'Cleanup result' : 'Selected files'}
    </h2>
    <button onclick={close} disabled={execution.kind === 'running'}>Close</button>
  </div>
  {#if !receipt}
    <div class="preview-totals">
      <span>{plan.preview.items.length} files</span><span
        >{bytes(plan.preview.logical)} logical</span
      ><span>{bytes(plan.preview.reclaimable)} potential</span>
    </div>
  {/if}
  <div bind:this={previewScroll} class="preview-scroll">
    {#if receipt}
      <section class="cleanup-receipt" aria-label="Cleanup space accounting">
        <dl class="receipt-amounts">
          <div>
            <dt>Selected logical bytes</dt>
            <dd title={`${receipt.selected.logical} bytes`}>
              {formatStorageBytes(receipt.selected.logical)}<small
                >{receipt.selected.files} selected {receipt.selected.files === 1
                  ? 'path'
                  : 'paths'}</small
              >
            </dd>
          </div>
          <div>
            <dt>
              {receipt.disposition === 'recycled'
                ? 'Recycle Bin logical bytes'
                : 'Permanently removed logical bytes'}
            </dt>
            <dd title={`${receipt.confirmed.logical} bytes`}>
              {formatStorageBytes(receipt.confirmed.logical)}<small
                >{receipt.confirmed.files}
                {receipt.confirmed.files === 1 ? 'path' : 'paths'}
                {receipt.disposition === 'recycled'
                  ? 'confirmed moved'
                  : 'confirmed removed'}</small
              >
            </dd>
          </div>
          <div>
            <dt>Verified keeper logical bytes</dt>
            {#if receipt.protected.kind === 'verified_keepers'}
              <dd title={`${receipt.protected.logical} bytes`}>
                {formatStorageBytes(receipt.protected.logical)}<small
                  >{receipt.protected.files} existing duplicate {receipt.protected.files === 1
                    ? 'keeper'
                    : 'keepers'} linked to confirmed removals</small
                >
              </dd>
            {:else if receipt.protected.kind === 'unavailable'}
              <dd>
                Unavailable<small>No duplicate removal with verified keepers was confirmed.</small>
              </dd>
            {:else}
              <dd>Not applicable<small>No retained duplicate copies in this plan.</small></dd>
            {/if}
          </div>
          <div>
            <dt>Potentially reclaimable allocation</dt>
            <dd title={`${receipt.potential_allocation} bytes`}>
              {formatStorageBytes(receipt.potential_allocation)}<small
                >Reviewed estimate for single-link files</small
              >
            </dd>
          </div>
        </dl>
        {#if receipt.disposition === 'recycled'}
          <p class="receipt-note">
            Moving files to the Recycle Bin can retain their disk allocation.
          </p>
        {/if}
        {#if receipt.unrecorded.files > 0}
          <p class="receipt-note">
            Confirmed without a recorded result · {formatStorageBytes(receipt.unrecorded.logical)} logical
            across {receipt.unrecorded.files}
            {receipt.unrecorded.files === 1 ? 'file' : 'files'}
          </p>
        {/if}
        <dl class="receipt-volume">
          <dt>Observed free-space change</dt>
          {#if receipt.volume_change.kind === 'observed'}
            {@const volume = receipt.volume_change}
            <dd>
              <strong title={`${volume.delta} bytes`}>{signedBytes(volume.delta)}</strong>
              <small title={`${volume.volume.guid_root} · Volume serial ${volume.volume.serial}`}>
                Whole source volume · {formatStorageBytes(volume.capacity)} capacity ·
                {formatStorageBytes(volume.before_free)} → {formatStorageBytes(volume.after_free)} free
              </small>
              <small
                >Measured from {date(volume.before_at)} to {date(volume.after_at)}. Includes other
                disk activity.</small
              >
            </dd>
          {:else}
            <dd>
              Unavailable<small
                >Two comparable measurements of the source volume are required.</small
              >
            </dd>
          {/if}
        </dl>
      </section>
    {/if}
    {#if plan.origins.some((origin) => origin.id === 'old-browser-http-cache-files')}<p>
        Close the browser. Cached pages may need downloading again.
      </p>{/if}
    <details class="plan-origins">
      <summary>Review sources</summary>
      <ul>
        {#each plan.origins as origin}
          <li>
            <strong>{origin.label}</strong> · {origin.description}
            {#if origin.reference}Rule {origin.reference.id} · Revision {origin.reference
                .revision}{/if}
            <small>{origin.risk === 'review' ? 'Manual review' : origin.risk}</small>
          </li>
        {/each}
      </ul>
    </details>
    {#if plan.preview.retained.length}<div class="retained-files">
        <strong>Keep</strong>{#each plan.preview.retained as path}<div>{path}</div>{/each}
      </div>{/if}
    <table>
      <thead
        ><tr><th>Path</th><th class="number">Logical</th><th class="number">Potential</th></tr
        ></thead
      ><tbody>
        {#each plan.preview.items as item (item.id)}<tr
            ><td
              >{item.path}{#if item.hard_link}<span class="muted"> · Hard link</span>{/if}</td
            ><td class="number">{bytes(item.logical)}</td><td class="number"
              >{bytes(item.reclaimable)}</td
            ></tr
          >{/each}
      </tbody>
    </table>
  </div>
  <div class="execution-actions" aria-live="polite">
    {#if execution.kind === 'idle'}
      <button
        class="primary"
        disabled={!recyclingAvailable}
        title={recyclingAvailable ? undefined : 'Unavailable'}
        onclick={() => execute('recycle')}>Move to Recycle Bin</button
      >
      {#if !plan.scheduled}<button
          onclick={() => {
            execution = { kind: 'confirm' };
          }}>Permanently delete…</button
        >{/if}
    {:else if execution.kind === 'confirm'}
      <span>Permanent · cannot be undone.</span>
      <label
        >Type DELETE<input bind:value={confirmation} autocomplete="off" spellcheck="false" /></label
      >
      <button onclick={() => execute('permanent')} disabled={confirmation !== 'DELETE'}
        >Delete permanently</button
      >
      <button
        onclick={() => {
          execution = { kind: 'idle' };
          confirmation = '';
        }}>Back</button
      >
    {:else if execution.kind === 'running'}
      <span
        >{execution.mode === 'recycle' ? 'Moving' : 'Deleting'} · {execution.completed} / {execution.total}</span
      >
      <button onclick={cancel} disabled={execution.id === null}>Cancel</button>
    {:else if execution.kind === 'finished'}
      <span
        >{execution.report.receipt.confirmed.files} permanently removed · {formatStorageBytes(
          execution.report.receipt.confirmed.logical,
        )} logical · {execution.report.items.filter((item) => !item.removed).length} failed{#if execution.report.cancelled}
          · Cancelled{/if}{#if execution.report.items.length < plan.preview.items.length}
          · {plan.preview.items.length - execution.report.items.length} not attempted{/if}</span
      >
      {#if execution.report.journal_error}<p role="alert">
          Stopped · results not fully recorded. {execution.report.journal_error}
        </p>{/if}
      {#each execution.report.items.filter((item) => item.error || !item.recorded) as item (item.id)}<p
        >
          {item.path} · {item.error ?? 'Removed · result not recorded'}
        </p>{/each}
    {:else if execution.kind === 'recycled'}
      <span
        >{execution.report.items.filter((item) => item.outcome === 'recycled').length} sent to Recycle
        Bin · {execution.report.items.filter(
          (item) => item.outcome === 'unmoved' || item.outcome === 'returned',
        ).length} unchanged
        {#if execution.report.items.some((item) => item.outcome === 'unknown')}
          · {execution.report.items.filter((item) => item.outcome === 'unknown').length} need review{/if}
        {#if execution.report.items.some((item) => item.outcome === 'not_started')}
          · {execution.report.items.filter((item) => item.outcome === 'not_started').length} not attempted{/if}
        {#if execution.report.cancelled}
          · Cancelled{/if}
      </span>
      {#if execution.report.journal_error}<p role="alert">
          Results not fully recorded. {execution.report.journal_error}
        </p>{/if}
      {#if execution.report.helper_error}<p role="alert">{execution.report.helper_error}</p>{/if}
      {#if execution.report.preflight_error}<p role="alert">
          {execution.report.preflight_error}
        </p>{/if}
      {#each execution.report.items.filter((item) => item.error || item.cleanup_error || item.outcome === 'unknown' || (item.operation !== null && !item.recorded)) as item (item.id)}
        <p>
          {item.path} · {outcome(item)}{#if item.error}
            · {item.error}{/if}{#if item.cleanup_error}
            · {item.cleanup_error}{/if}{#if item.operation !== null && !item.recorded}
            · Result not recorded{/if}
        </p>
      {/each}
    {:else if execution.kind === 'failed'}<p role="alert">{execution.message}</p>{/if}
  </div>
</dialog>

<style>
  .execution-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
    padding: 16px 20px;
    border-top: 1px solid var(--border);
  }
  .execution-actions label {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .execution-actions input {
    width: 110px;
  }
  .execution-actions p {
    flex-basis: 100%;
    overflow-wrap: anywhere;
  }
  dialog {
    width: min(820px, calc(100vw - 48px));
    max-height: calc(100vh - 64px);
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--text);
    background: var(--canvas);
  }
  dialog::backdrop {
    background: #0005;
  }
  .preview-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 16px 20px;
  }
  h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
  }
  .preview-totals {
    display: flex;
    flex-wrap: wrap;
    gap: 20px;
    padding: 0 20px 16px;
    font-size: 12px;
    color: var(--secondary);
    font-variant-numeric: tabular-nums;
  }
  .preview-scroll {
    max-height: 420px;
    overflow: auto;
  }
  .cleanup-receipt {
    border-block: 1px solid var(--border);
    padding: 14px 20px;
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .cleanup-receipt dl,
  .cleanup-receipt dd {
    margin: 0;
  }
  .receipt-amounts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 9.5rem), 1fr));
    gap: 10px 16px;
  }
  .cleanup-receipt dt,
  .cleanup-receipt small,
  .receipt-note {
    color: var(--secondary);
  }
  .cleanup-receipt dd {
    padding-top: 3px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .cleanup-receipt small {
    display: block;
    font: inherit;
    font-weight: 400;
  }
  .receipt-note {
    margin: 12px 0 0;
  }
  dl.receipt-volume {
    margin-top: 12px;
    padding-top: 10px;
    border-top: 1px solid var(--border);
  }
  .retained-files {
    padding: 12px 20px;
    border-top: 1px solid var(--border);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .retained-files strong {
    display: block;
    margin-bottom: 8px;
    font-weight: 500;
  }
  table {
    border-collapse: collapse;
    width: 100%;
    font-size: 12px;
  }
  th,
  td {
    padding: 10px 20px;
    border-top: 1px solid var(--border);
    text-align: left;
  }
  th {
    font-weight: 500;
    color: var(--secondary);
  }
  td:first-child {
    overflow-wrap: anywhere;
  }
  .number {
    white-space: nowrap;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .muted {
    color: var(--muted);
  }
</style>
