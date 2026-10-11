<script lang="ts">
  import { onMount, onDestroy, untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { bytes, type CleanupPreview } from '../lib/types';
  let {
    plan,
    onclose,
    onerror,
  }: { plan: CleanupPreview; onclose: () => void; onerror: (problem: unknown) => void } = $props();
  let dialog: HTMLDialogElement;
  const id = untrack(() => plan.id);
  function close(): void {
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
    <button onclick={close}>Close</button>
  </div>
  <div class="preview-totals">
    <span>{plan.preview.items.length} files</span><span>{bytes(plan.preview.logical)} logical</span
    ><span>{bytes(plan.preview.reclaimable)} potential</span>
  </div>
  <div class="preview-scroll">
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
</dialog>

<style>
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
