<script lang="ts">
  import { onDestroy, untrack, tick } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { date } from '../lib/types';

  let { revision, busy }: { revision: number; busy: boolean } = $props();
  type Item = {
    kind: 'permanent' | 'recycling';
    operation: number;
    node: number;
    path: string;
    state: string;
    message: string | null;
    created: number;
  };
  type State =
    | { kind: 'idle' }
    | { kind: 'loading' }
    | { kind: 'ready'; items: Item[]; offset: number; more: boolean }
    | { kind: 'error'; message: string };
  const limit = 50;
  let state = $state<State>({ kind: 'idle' });
  let generation = 0;
  let region: HTMLElement;

  function result(item: Item): string {
    if (item.kind === 'recycling') {
      switch (item.state) {
        case 'recycled':
          return 'Sent to Recycle Bin';
        case 'restored':
          return 'Restored';
        case 'returned':
          return 'Returned to original folder';
        case 'unmoved':
          return 'Not moved';
        case 'cancelled':
          return 'Not attempted';
        case 'prepared':
        case 'stage_ready':
          return 'Pending';
        default:
          return 'Needs review';
      }
    }
    switch (item.state) {
      case 'succeeded':
        return 'Permanently removed';
      case 'failed':
        return 'Failed';
      case 'cancelled':
        return 'Not attempted';
      case 'prepared':
        return 'Pending';
      case 'in_flight':
        return 'Unresolved';
      case 'unknown':
        return 'Needs review';
      default:
        return 'Needs review';
    }
  }
  async function load(offset: number): Promise<void> {
    if (busy) return;
    const token = ++generation;
    const restoreFocus = region?.contains(document.activeElement);
    state = { kind: 'loading' };
    try {
      const items = await invoke<Item[]>('query_cleanup_history', { offset, limit: limit + 1 });
      if (token === generation)
        state = {
          kind: 'ready',
          items: items.slice(0, limit),
          offset,
          more: items.length > limit,
        };
    } catch (problem) {
      if (token === generation) state = { kind: 'error', message: String(problem) };
    }
    if (restoreFocus) {
      await tick();
      if (token === generation) region?.focus();
    }
  }
  $effect(() => {
    revision;
    const paused = busy;
    untrack(() => {
      generation++;
      state = { kind: 'idle' };
      if (!paused && isTauri()) void load(0);
    });
  });
  onDestroy(() => generation++);
</script>

<section
  aria-label="Cleanup activity"
  aria-busy={state.kind === 'loading'}
  tabindex="-1"
  bind:this={region}
>
  <header>
    <h2>Cleanup activity</h2>
    <button onclick={() => load(0)} disabled={busy || state.kind === 'loading' || !isTauri()}
      >Refresh</button
    >
  </header>
  {#if state.kind === 'loading'}
    <p role="status">Loading…</p>
  {:else if state.kind === 'error'}
    <p role="status">{state.message}</p>
  {:else if state.kind === 'ready'}
    {#if state.items.length === 0}
      <p>No activity</p>
    {:else}
      <div class="rows">
        <table>
          <thead
            ><tr
              ><th scope="col">File</th><th scope="col">Result</th><th scope="col">Started</th></tr
            ></thead
          >
          <tbody>
            {#each state.items as item (`${item.kind}:${item.operation}:${item.node}`)}
              <tr>
                <td class="path">{item.path}</td>
                <td>
                  <span>{result(item)}</span>
                  {#if item.message}<small>{item.message}</small>
                  {/if}
                </td>
                <td>{date(item.created)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <footer>
        <small>{state.offset + 1}–{state.offset + state.items.length}</small>
        <button
          disabled={busy || state.offset === 0}
          onclick={() => {
            if (state.kind === 'ready') void load(Math.max(0, state.offset - limit));
          }}>Previous</button
        >
        <button
          disabled={busy || !state.more}
          onclick={() => {
            if (state.kind === 'ready') void load(state.offset + limit);
          }}>Next</button
        >
      </footer>
    {/if}
  {:else if busy}<p role="status">Paused</p>
  {:else}<p>Unavailable</p>{/if}
</section>

<style>
  section {
    margin-top: 24px;
    min-width: 0;
    border-top: 1px solid var(--border);
  }
  header,
  footer {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 16px 0;
  }
  header {
    justify-content: space-between;
  }
  p {
    padding: 0 0 16px;
    overflow-wrap: anywhere;
  }
  .rows {
    max-height: 420px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  table {
    width: 100%;
    table-layout: fixed;
    border-collapse: collapse;
  }
  th,
  td {
    text-align: left;
    padding: 10px 0;
    border-top: 1px solid var(--border);
    vertical-align: top;
    overflow-wrap: anywhere;
  }
  th {
    font-size: 12px;
    color: var(--muted);
    font-weight: 500;
  }
  th:first-child {
    width: 50%;
  }
  th:last-child {
    width: 25%;
  }
  small {
    display: block;
    margin-top: 4px;
  }
  footer small {
    margin: 0 auto 0 0;
  }
</style>
