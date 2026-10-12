<script lang="ts">
  import { onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { bytes } from '../lib/types';

  let { drive }: { drive: string } = $props();
  type Summary = { drive: string; items: number; size: number };
  type LoadState =
    | { kind: 'idle' }
    | { kind: 'loading' }
    | { kind: 'ready'; data: Summary }
    | { kind: 'error'; message: string };
  let state = $state<LoadState>({ kind: 'idle' });
  let generation = 0;
  $effect(() => {
    drive;
    generation++;
    state = { kind: 'idle' };
  });
  async function load(): Promise<void> {
    const token = ++generation;
    state = { kind: 'loading' };
    try {
      const data = await invoke<Summary>('query_recycle_bin', { drive });
      if (token === generation) state = { kind: 'ready', data };
    } catch (problem) {
      if (token === generation) state = { kind: 'error', message: String(problem) };
    }
  }
  onDestroy(() => generation++);
</script>

<section aria-label="Recycle Bin" aria-busy={state.kind === 'loading'}>
  <div class="summary">
    <span>Recycle Bin · {drive}</span>
    <button onclick={load} disabled={!drive || state.kind === 'loading'}>
      {state.kind === 'loading' ? 'Analyzing…' : state.kind === 'ready' ? 'Refresh' : 'Analyze'}
    </button>
  </div>
  {#if state.kind === 'ready'}
    <dl>
      <div>
        <dt>Items</dt>
        <dd>{state.data.items.toLocaleString()}</dd>
      </div>
      <div>
        <dt>Size</dt>
        <dd>{bytes(state.data.size)}</dd>
      </div>
    </dl>
  {:else if state.kind === 'error'}
    <p role="status">{state.message}</p>
  {/if}
</section>

<style>
  section {
    margin-top: 16px;
    min-width: 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
  }
  .summary {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 16px;
  }
  span,
  dt,
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
  dl {
    display: flex;
    flex-wrap: wrap;
    gap: 32px;
    margin: 0;
    padding: 0 16px 16px;
  }
  dd {
    margin: 4px 0 0;
    font-variant-numeric: tabular-nums;
  }
  p {
    margin: 0;
    padding: 0 16px 12px;
    overflow-wrap: anywhere;
  }
</style>
