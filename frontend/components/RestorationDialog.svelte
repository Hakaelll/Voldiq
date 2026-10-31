<script lang="ts">
  import { onMount, onDestroy, untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { bytes, type RestorationPreview, type RestorationResult } from '../lib/types';
  import Icon from './Icon.svelte';

  let { receipt, onclose, onbusy, onchanged, onerror }: {
    receipt: number;
    onclose: () => void;
    onbusy: (value: boolean) => void;
    onchanged: () => void;
    onerror: (problem: unknown) => void;
  } = $props();
  type State =
    | { kind: 'preparing' }
    | { kind: 'ready'; preview: RestorationPreview }
    | { kind: 'running'; preview: RestorationPreview }
    | { kind: 'finished'; result: RestorationResult }
    | { kind: 'failed'; message: string };
  let state = $state<State>({ kind: 'preparing' });
  let dialog: HTMLDialogElement;
  const receiptId = untrack(() => receipt);
  let reviewId: number | null = null;
  let alive = true;

  async function discard(id: number): Promise<void> {
    try { await invoke('discard_restoration', { id }); }
    catch (problem) { onerror(problem); }
  }
  async function prepare(): Promise<void> {
    onbusy(true);
    try {
      const preview = await invoke<RestorationPreview>('preview_restoration', { receipt: receiptId });
      if (!alive) { await discard(preview.id); return; }
      reviewId = preview.id;
      state = { kind: 'ready', preview };
    } catch (problem) {
      if (alive) state = { kind: 'failed', message: String(problem) };
    } finally { onbusy(false); }
  }
  async function restore(): Promise<void> {
    if (state.kind !== 'ready') return;
    const preview = state.preview;
    state = { kind: 'running', preview };
    reviewId = null;
    onbusy(true);
    try {
      const result = await invoke<RestorationResult>('execute_restoration', { id: preview.id, confirmation: 'RESTORE' });
      if (alive) state = { kind: 'finished', result };
    } catch (problem) {
      if (alive) state = { kind: 'failed', message: String(problem) };
    } finally {
      // Also release a review when execution was refused before native consumption.
      await discard(preview.id);
      onbusy(false);
      onchanged();
    }
  }
  function close(): void {
    if (state.kind === 'running') return;
    dialog.close();
    onclose();
  }
  onMount(() => { dialog.showModal(); void prepare(); });
  onDestroy(() => {
    alive = false;
    if (reviewId !== null) void discard(reviewId);
  });
</script>

<dialog bind:this={dialog} aria-labelledby="restore-title" oncancel={(event) => { event.preventDefault(); close(); }}>
  <header>
    <h2 id="restore-title">{state.kind === 'finished' ? 'Restored' : 'Restore file'}</h2>
    <button class="close" aria-label="Close" onclick={close} disabled={state.kind === 'running'}><Icon name="close" /></button>
  </header>
  <div class="body" aria-live="polite">
    {#if state.kind === 'preparing'}
      <p role="status">Checking file…</p>
    {:else if state.kind === 'ready' || state.kind === 'running'}
      <div class="file">
        <Icon name="file" size={28} />
        <div><strong>{state.preview.path.split(/[\\/]/).pop()}</strong><small>{bytes(state.preview.size)}</small></div>
      </div>
      <small class="label">Original location</small>
      <p class="path">{state.preview.path}</p>
      {#if state.kind === 'running'}<p role="status">Restoring…</p>{/if}
    {:else if state.kind === 'finished'}
      <p class="path">{state.result.path}</p>
      {#if !state.result.recorded}<p role="alert">Restored · history unavailable. {state.result.journal_error}</p>{/if}
    {:else if state.kind === 'failed'}
      <p role="alert">{state.message}</p>
    {/if}
  </div>
  <footer>
    <button onclick={close} disabled={state.kind === 'running'}>{state.kind === 'ready' ? 'Cancel' : 'Close'}</button>
    {#if state.kind === 'ready' || state.kind === 'running'}
      <button class="primary" onclick={restore} disabled={state.kind === 'running'}><Icon name="history" />Restore</button>
    {/if}
  </footer>
</dialog>

<style>
  dialog {
    width: min(480px, calc(100vw - 48px));
    max-height: calc(100dvh - 48px);
    overflow: auto;
    padding: 0;
    color: var(--text);
    background: var(--canvas);
    border: 1px solid var(--strong);
    border-radius: 8px;
  }
  dialog::backdrop { background: rgb(0 0 0 / 25%); }
  header, footer { display: flex; align-items: center; gap: 12px; padding: 20px 24px; }
  header { justify-content: space-between; }
  h2 { margin: 0; font-size: 18px; font-weight: 600; }
  .close { padding: 6px; min-width: 32px; }
  .body { padding: 4px 24px 24px; min-width: 0; }
  .file { display: flex; align-items: center; gap: 14px; margin-bottom: 24px; }
  .file > div { min-width: 0; }
  strong { display: block; overflow-wrap: anywhere; font-size: 15px; font-weight: 600; }
  small { display: block; color: var(--secondary); font-size: 12px; margin-top: 4px; }
  .label { margin-top: 0; margin-bottom: 8px; }
  p { margin: 0; overflow-wrap: anywhere; }
  .path { font-size: 13px; line-height: 1.6; }
  [role='alert'] { margin-top: 12px; color: var(--error); }
  footer { justify-content: flex-end; border-top: 1px solid var(--border); }
</style>
