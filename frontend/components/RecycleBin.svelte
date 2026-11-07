<script lang="ts">
  import { onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { bytes } from '../lib/types';
  import Icon from './Icon.svelte';

  let { drive }: { drive: string } = $props();
  type Summary = { drive: string; items: number; size: number };
  type LoadState =
    | { kind: 'idle' }
    | { kind: 'loading' }
    | { kind: 'ready'; data: Summary }
    | { kind: 'error'; message: string };
  let loadState = $state<LoadState>({ kind: 'idle' });
  let generation = 0;
  let opening = $state(false);
  let launchError = $state('');
  $effect(() => {
    drive;
    generation++;
    loadState = { kind: 'idle' };
  });
  async function load(): Promise<void> {
    const token = ++generation;
    loadState = { kind: 'loading' };
    try {
      const data = await invoke<Summary>('query_recycle_bin', { drive });
      if (token === generation) loadState = { kind: 'ready', data };
    } catch (problem) {
      if (token === generation) loadState = { kind: 'error', message: String(problem) };
    }
  }
  async function openInWindows(): Promise<void> {
    if (opening) return;
    opening = true;
    launchError = '';
    try {
      await invoke('open_windows_recycle_bin');
    } catch (problem) {
      launchError = String(problem);
    } finally {
      opening = false;
    }
  }
  onDestroy(() => generation++);
</script>

<section aria-label="Recycle Bin" aria-busy={loadState.kind === 'loading'}>
  <div class="summary">
    <div class="identity">
      <Icon name="trash" size={22} /><strong>Recycle Bin</strong>
      {#if loadState.kind === 'ready'}<span
          >{loadState.data.items.toLocaleString()} items · {bytes(loadState.data.size)}</span
        >{/if}
    </div>
    <div class="actions">
      <button
        class="quiet-action"
        onclick={openInWindows}
        disabled={opening}
        aria-label="Open Windows Recycle Bin"
      >
        <Icon name="external" />Open in Windows
      </button>
      <button class="primary" onclick={load} disabled={!drive || loadState.kind === 'loading'}>
        {loadState.kind === 'loading'
          ? 'Analyzing…'
          : loadState.kind === 'ready'
            ? 'Refresh'
            : 'Analyze'}
      </button>
    </div>
  </div>
  {#if loadState.kind === 'error'}
    <p role="status">{loadState.message}</p>
  {/if}
  {#if launchError}<p role="alert">{launchError}</p>{/if}
</section>

<style>
  section {
    margin-top: 16px;
    min-width: 0;
    border-block: 1px solid var(--border);
  }
  .summary {
    flex-wrap: wrap;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 14px 0;
  }
  .identity {
    display: flex;
    align-items: center;
    gap: 16px;
    min-width: 0;
    flex-wrap: wrap;
  }
  .actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  span,
  p {
    color: var(--muted);
    font-size: 12px;
  }
  span {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  button {
    flex-shrink: 0;
  }
  p {
    margin: 0;
    padding: 0 16px 12px;
    overflow-wrap: anywhere;
  }
</style>
