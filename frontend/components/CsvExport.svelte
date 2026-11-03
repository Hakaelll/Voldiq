<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import Icon from './Icon.svelte';
  import {
    createCsvExport,
    csvRequest,
    type CsvState,
    type CsvReceipt,
    type CsvEvent,
  } from '../lib/csv-export';
  import type { IndexId } from '../lib/index-context';
  import type { Query } from '../lib/types';
  let {
    indexId,
    scope,
    root,
    folder,
    query,
    disabled,
    onbusy,
  }: {
    indexId: IndexId;
    scope: number;
    root: string;
    folder: string;
    query: Query;
    disabled: boolean;
    onbusy: (busy: boolean) => void;
  } = $props();
  let dialog: HTMLDialogElement;
  let trigger: HTMLButtonElement;
  let closeButton: HTMLButtonElement;
  let mounted = false;
  let model = $state.raw<CsvState>({ kind: 'idle' });
  let opened = $state(false),
    pending = $state(false);
  let items = $state<'files' | 'folders'>('files');
  let scopeChoice = $state<'folder' | 'root'>('folder');
  let filtered = $state(true),
    paths = $state(false);
  const result = $derived(model.kind === 'finished' ? model.receipt.outcome : null);
  const exportSession = createCsvExport(
    {
      save: (context, request) => invoke<CsvReceipt>('save_csv_report', { ...context, request }),
      read: (context) => invoke<CsvEvent | null>('csv_report_progress', { ...context }),
      cancel: (context) => invoke<boolean>('cancel_csv_report', { ...context }),
    },
    (value) => {
      model = value;
    },
  );
  onMount(() => {
    mounted = true;
  });
  onDestroy(() => {
    mounted = false;
    exportSession.dispose();
    onbusy(false);
  });
  function open(): void {
    if (disabled || !mounted) return;
    model = { kind: 'idle' };
    paths = false;
    opened = true;
    dialog.showModal();
  }
  function close(): void {
    if (pending) {
      exportSession.cancel();
      return;
    }
    dialog.close();
    opened = false;
    trigger?.focus({ preventScroll: true });
  }
  async function save(): Promise<void> {
    if (pending || disabled || !mounted) return;
    const request = csvRequest(query, scopeChoice === 'folder' ? scope : 0, items, filtered, paths);
    pending = true;
    onbusy(true);
    try {
      await exportSession.save({ indexId, requestId: crypto.randomUUID() }, request);
    } finally {
      if (mounted) {
        pending = false;
        onbusy(false);
        await tick();
        if (mounted && opened) closeButton?.focus({ preventScroll: true });
      }
    }
  }
</script>

<button bind:this={trigger} onclick={open} {disabled} aria-haspopup="dialog"
  ><Icon name="external" />Export</button
>
<dialog
  bind:this={dialog}
  class="csv-export"
  aria-labelledby="csv-title"
  aria-describedby="csv-summary"
  oncancel={(event) => {
    event.preventDefault();
    close();
  }}
  onkeydown={(event) => event.stopPropagation()}
>
  <header>
    <h2 id="csv-title">Export CSV</h2>
    <button
      bind:this={closeButton}
      onclick={close}
      aria-label={pending ? 'Request export cancellation' : 'Close export'}
      >{pending ? 'Cancel' : 'Close'}</button
    >
  </header>
  <p id="csv-summary">A report of the loaded analysis. Existing files are preserved.</p>
  <form
    onsubmit={(event) => {
      event.preventDefault();
      void save();
    }}
  >
    <fieldset disabled={pending || disabled}>
      <legend>Report content</legend>
      <label
        >Records<select bind:value={items}
          ><option value="files">Files</option><option value="folders">Folders</option></select
        ></label
      >
      <label
        >Scope<select bind:value={scopeChoice}
          ><option value="folder">Current folder and descendants</option><option value="root"
            >Entire analysis</option
          ></select
        ></label
      >
      <p class="scope-path">{scopeChoice === 'folder' ? folder : root}</p>
      <label class="check"
        ><input type="checkbox" bind:checked={filtered} />Apply current filters</label
      >
      <label class="check"
        ><input type="checkbox" bind:checked={paths} />Include full paths and filter text</label
      >
    </fieldset>
    <details>
      <summary>Report details</summary>
      <p>
        Every matching record, in analysis order. Sizes are bytes; dates are UTC. Scope, filters and
        coverage are included. Text cells carry a safety prefix; import large numbers as text in
        spreadsheets.
      </p>
    </details>
    <div class="csv-status" aria-live="polite" aria-atomic="true">
      {#if model.kind === 'choosing'}<p>
          {model.cancelling
            ? 'Cancellation requested. Close the Windows save dialog to return.'
            : 'Choose a new file in the Windows save dialog.'}
        </p>
      {:else if model.kind === 'running'}
        <progress
          aria-label="Indexed records inspected"
          value={model.progress.inspected}
          max={Math.max(1, model.progress.total_entries)}
        ></progress>
        <p>
          {model.cancelling ? 'Stopping…' : 'Exporting…'}
          {model.progress.rows.toLocaleString()} rows · {model.progress.inspected.toLocaleString()} of
          {model.progress.total_entries.toLocaleString()} records inspected
        </p>
      {:else if model.kind === 'failed'}<p class="csv-error" role="alert">{model.message}</p>
      {:else if result?.kind === 'complete'}<p>
          Saved {result.progress.rows.toLocaleString()} rows.
        </p>
        <p class="scope-path">{result.path}</p>
      {:else if result?.kind === 'cancelled'}<p>Export cancelled. No report was published.</p>
      {:else if result?.kind === 'dialog_cancelled'}<p>
          Save cancelled. No report was published.
        </p>{/if}
    </div>
    <footer>
      {#if pending}<button
          type="button"
          disabled={(model.kind === 'running' || model.kind === 'choosing') && model.cancelling}
          onclick={() => exportSession.cancel()}>Request stop</button
        >{:else}<button class="primary" type="submit" {disabled}>Choose file and save</button>{/if}
    </footer>
  </form>
</dialog>

<style>
  .csv-export {
    width: min(470px, calc(100vw - 32px));
    max-height: calc(100dvh - 48px);
    overflow: auto;
    padding: 18px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--canvas);
    color: var(--text);
  }
  .csv-export::backdrop {
    background: #0005;
  }
  header,
  footer {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  h2 {
    margin: 0;
    font-size: 17px;
  }
  p {
    margin: 10px 0;
    font-size: 12px;
    color: var(--secondary);
  }
  fieldset {
    margin: 16px 0 12px;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    min-width: 0;
  }
  legend {
    padding: 0 4px;
    font-size: 12px;
  }
  label {
    display: grid;
    gap: 6px;
    font-size: 12px;
    margin-bottom: 12px;
  }
  select {
    width: 100%;
    min-width: 0;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  .check input {
    margin: 0;
  }
  .scope-path {
    overflow-wrap: anywhere;
  }
  summary {
    cursor: pointer;
    font-size: 12px;
    color: var(--secondary);
  }
  .csv-status {
    min-height: 28px;
    margin-top: 14px;
  }
  .csv-error {
    color: var(--error);
    overflow-wrap: anywhere;
  }
  progress {
    width: 100%;
    height: 6px;
    accent-color: var(--accent);
  }
  footer {
    justify-content: flex-end;
    margin-top: 16px;
  }
</style>
