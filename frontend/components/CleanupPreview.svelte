<script lang="ts">
  import { onMount, onDestroy, untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { Channel } from '@tauri-apps/api/core';
  import { bytes, type CleanupPreview, type CleanupReport } from '../lib/types';
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
    | { kind: 'running'; id: number | null; completed: number; total: number }
    | { kind: 'finished'; report: CleanupReport }
    | { kind: 'failed'; message: string };
  let execution = $state<Execution>({ kind: 'idle' });
  let confirmation = $state('');
  async function execute(): Promise<void> {
    if (execution.kind !== 'confirm' || confirmation !== 'DELETE') return;
    execution = { kind: 'running', id: null, completed: 0, total: plan.preview.items.length };
    onbusy(true);
    const progress = new Channel<{ id: number; completed: number; total: number }>();
    progress.onmessage = (update) => {
      if (execution.kind === 'running') execution = { kind: 'running', ...update };
    };
    try {
      const report = await invoke<CleanupReport>('execute_permanent_cleanup', {
        id,
        confirmation,
        progress,
      });
      execution = { kind: 'finished', report };
    } catch (problem) {
      execution = { kind: 'failed', message: String(problem) };
    } finally {
      onbusy(false);
      onchanged();
    }
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
    <h2 id="cleanup-title">Selected files</h2>
    <button onclick={close} disabled={execution.kind === 'running'}>Close</button>
  </div>
  <div class="preview-totals">
    <span>{plan.preview.items.length} files</span><span>{bytes(plan.preview.logical)} logical</span
    ><span>{bytes(plan.preview.reclaimable)} potential</span>
  </div>
  <div class="preview-scroll">
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
        onclick={() => {
          execution = { kind: 'confirm' };
        }}>Permanently delete…</button
      >
    {:else if execution.kind === 'confirm'}
      <span>Permanent · cannot be undone.</span>
      <label
        >Type DELETE<input bind:value={confirmation} autocomplete="off" spellcheck="false" /></label
      >
      <button onclick={execute} disabled={confirmation !== 'DELETE'}>Delete permanently</button>
      <button
        onclick={() => {
          execution = { kind: 'idle' };
          confirmation = '';
        }}>Back</button
      >
    {:else if execution.kind === 'running'}
      <span>Deleting · {execution.completed} / {execution.total}</span>
      <button onclick={cancel} disabled={execution.id === null}>Cancel</button>
    {:else if execution.kind === 'finished'}
      <span
        >{execution.report.items.filter((item) => item.removed).length} permanently removed · {execution.report.items.filter(
          (item) => !item.removed,
        ).length} failed{#if execution.report.cancelled}
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
