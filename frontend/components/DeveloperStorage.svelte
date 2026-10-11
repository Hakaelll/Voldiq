<script lang="ts">
  import { onDestroy, untrack, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { bytes, type DeveloperPage } from '../lib/types';
  import Icon from './Icon.svelte';
  let {
    enabled,
    revision,
    onselect,
    onerror,
  }: {
    enabled: boolean;
    revision: number;
    onselect: (id: number) => Promise<void>;
    onerror: (problem: unknown) => void;
  } = $props();
  const labels = { dependencies: 'Dependencies', build: 'Build artifacts', cache: 'Cache' };
  const limit = 50;
  let page = $state<DeveloperPage | null>(null);
  let loadState = $state<'loading' | 'ready' | 'unavailable'>('loading');
  let offset = $state(0);
  let generation = 0;
  let region: HTMLDivElement;
  async function load(next: number): Promise<void> {
    const token = ++generation;
    const restoreFocus = region?.contains(document.activeElement);
    loadState = 'loading';
    try {
      const result = await invoke<DeveloperPage>('query_developer_storage', {
        scope: 0,
        offset: next,
        limit,
      });
      if (token !== generation) return;
      page = result;
      offset = next;
      loadState = 'ready';
    } catch (problem) {
      if (token !== generation) return;
      loadState = 'unavailable';
      onerror(problem);
    }
    if (restoreFocus) {
      await tick();
      if (token === generation) region?.focus();
    }
  }
  $effect(() => {
    revision;
    if (enabled)
      untrack(() => {
        void load(0);
      });
    else {
      generation += 1;
      page = null;
    }
  });
  onDestroy(() => {
    generation += 1;
  });
</script>

<div
  class="developer-storage"
  bind:this={region}
  role="region"
  aria-label="Developer storage"
  tabindex="-1"
  aria-busy={enabled && loadState === 'loading'}
>
  {#if !enabled}
    <div class="developer-empty">Scan a drive or folder</div>
  {:else if loadState === 'loading'}
    <div class="developer-empty" role="status">Loading…</div>
  {:else if loadState === 'unavailable'}
    <div class="developer-empty"><button onclick={() => load(offset)}>Retry</button></div>
  {:else if page}
    <div class="developer-summary">
      <span>{page.total.toLocaleString()} directories · {bytes(page.logical)} logical</span><button
        onclick={() => load(0)}><Icon name="scan" />Refresh</button
      >
    </div>
    {#if page.items.length === 0}<div class="developer-empty">No indexed developer storage</div>
    {:else}
      <div class="developer-scroll">
        <table>
          <thead
            ><tr
              ><th>Location</th><th>Tool</th><th>Category</th><th class="number">Logical</th><th
                class="number">Allocated</th
              ><th><span class="sr-only">Details</span></th></tr
            ></thead
          >
          <tbody
            >{#each page.items as item (item.id)}<tr>
                <td class="developer-path" title={item.path}
                  >{item.path}{#if item.incomplete}<span class="partial"> · Partial</span>{/if}</td
                >
                <td>{item.tool}</td><td>{labels[item.category]}</td><td class="number"
                  >{bytes(item.logical)}</td
                ><td class="number">{item.allocation_known ? bytes(item.allocated) : 'Unknown'}</td>
                <td
                  ><button aria-label={`Details for ${item.path}`} onclick={() => onselect(item.id)}
                    ><Icon name="external" /></button
                  ></td
                >
              </tr>{/each}</tbody
          >
        </table>
      </div>
      <div class="developer-pages">
        <span>{offset + 1}–{offset + page.items.length} of {page.total}</span><button
          disabled={offset === 0}
          onclick={() => load(Math.max(0, offset - limit))}>Previous</button
        ><button disabled={offset + limit >= page.total} onclick={() => load(offset + limit)}
          >Next</button
        >
      </div>
    {/if}
  {/if}
</div>

<style>
  .developer-storage {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
    overflow: hidden;
  }
  .developer-summary,
  .developer-pages {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
  }
  .developer-summary > span,
  .developer-pages > span {
    margin-right: auto;
    color: var(--muted);
    font-size: 12px;
  }
  .developer-summary {
    border-bottom: 1px solid var(--border);
  }
  .developer-pages {
    border-top: 1px solid var(--border);
  }
  .developer-scroll {
    overflow: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  th,
  td {
    text-align: left;
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
  }
  th {
    font-weight: 500;
    color: var(--muted);
  }
  tbody tr:last-child td {
    border-bottom: 0;
  }
  .developer-path {
    max-width: 420px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .number {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .partial {
    color: var(--muted);
  }
  .developer-empty {
    min-height: 180px;
    display: grid;
    place-items: center;
    color: var(--muted);
    font-size: 13px;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
