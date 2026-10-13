<script lang="ts">
  import { onDestroy, untrack, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { bytes, type DeveloperPage, type DeveloperItem } from '../lib/types';
  import Icon from './Icon.svelte';
  import DockerStorage from './DockerStorage.svelte';
  let {
    enabled,
    revision,
    onopen,
    onerror,
  }: {
    enabled: boolean;
    revision: number;
    onopen: (id: number) => void;
    onerror: (problem: unknown) => void;
  } = $props();
  const labels = { dependencies: 'Dependencies', build: 'Build artifacts', cache: 'Cache' };
  const limit = 50;
  let page = $state<DeveloperPage | null>(null);
  let selected = $state<DeveloperItem | null>(null);
  const colors = { dependencies: '#075bdd', build: '#087f8c', cache: '#ed701b' };
  let loadState = $state<'loading' | 'ready' | 'unavailable'>('loading');
  let offset = $state(0);
  let category = $state<DeveloperItem['category'] | ''>('');
  let sort = $state<'size' | 'name'>('size');
  let totalLogical = $derived(
    page?.categories?.reduce((sum, group) => sum + group.logical, 0) ?? 0,
  );
  let totalDirectories = $derived(
    page?.categories?.reduce((sum, group) => sum + group.directories, 0) ?? 0,
  );
  let generation = 0;
  let region: HTMLDivElement;
  let detailsRegion: HTMLElement;
  async function load(next: number): Promise<void> {
    const token = ++generation;
    const focusTarget = document.activeElement;
    const restoreFocus =
      region?.contains(document.activeElement) && !detailsRegion?.contains(document.activeElement);
    loadState = 'loading';
    selected = null;
    try {
      const result = await invoke<DeveloperPage>('query_developer_storage', {
        query: { scope: 0, offset: next, limit, category: category || null, sort },
      });
      if (token !== generation) return;
      page = result;
      selected = result.items[0] ?? null;
      offset = next;
      loadState = 'ready';
    } catch (problem) {
      if (token !== generation) return;
      loadState = 'unavailable';
      onerror(problem);
    }
    if (restoreFocus) {
      await tick();
      if (token === generation) {
        if (focusTarget instanceof HTMLElement && focusTarget.isConnected) {
          if (document.activeElement === document.body) focusTarget.focus();
        } else region?.focus();
      }
    }
  }
  $effect(() => {
    revision;
    category;
    sort;
    if (enabled)
      untrack(() => {
        offset = 0;
        void load(0);
      });
    else {
      generation += 1;
      page = null;
      selected = null;
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
  {#if enabled && loadState === 'ready' && page}
    <div class="developer-summary">
      <span>{totalDirectories.toLocaleString()} directories · {bytes(totalLogical)} logical</span
      ><button onclick={() => load(0)}><Icon name="scan" />Refresh</button>
    </div>
    {#if page.categories}<div class="developer-distribution">
        <div class="developer-bar" aria-hidden="true">
          {#each page.categories as category}<span
              style:width={`${totalLogical ? (category.logical / totalLogical) * 100 : 0}%`}
              style:background={colors[category.category]}
            ></span>{/each}
        </div>
        <div class="developer-legend">
          {#each page.categories as category}<div>
              <span class="category-swatch" style:background={colors[category.category]}></span>
              <div>
                <strong>{labels[category.category]}</strong><small
                  >{bytes(category.logical)} · {category.directories.toLocaleString()} directories</small
                >
              </div>
            </div>{/each}
        </div>
      </div>{/if}
  {/if}
  <div class="developer-layout">
    <div class="developer-list">
      <div class="developer-list-heading">
        <h2>Projects and caches</h2>
        <div class="developer-filters">
          <select
            aria-label="Developer storage category"
            bind:value={category}
            disabled={!enabled || loadState === 'loading'}
          >
            <option value="">All categories</option><option value="dependencies"
              >Dependencies</option
            ><option value="build">Build artifacts</option><option value="cache">Cache</option>
          </select>
          <select
            aria-label="Developer storage order"
            bind:value={sort}
            disabled={!enabled || loadState === 'loading'}
          >
            <option value="size">Largest first</option><option value="name">Name</option>
          </select>
        </div>
      </div>
      {#if !enabled}
        <div class="developer-empty">Scan a drive or folder</div>
      {:else if loadState === 'loading'}
        <div class="developer-empty" role="status">Loading…</div>
      {:else if loadState === 'unavailable'}
        <div class="developer-empty"><button onclick={() => load(offset)}>Retry</button></div>
      {:else if page}
        {#if page.items.length === 0}<div class="developer-empty">
            {category ? 'No matching directories' : 'No indexed developer storage'}
          </div>
        {:else}
          <div class="developer-scroll">
            <table>
              <thead
                ><tr
                  ><th>Path</th><th>Category</th><th class="number">Logical size</th><th
                    class="number">Allocated</th
                  ></tr
                ></thead
              >
              <tbody
                >{#each page.items as item (item.id)}<tr class:selected={selected?.id === item.id}>
                    <td class="developer-path"
                      ><button
                        title={item.path}
                        aria-label={`Details for ${item.path}`}
                        onclick={() => {
                          selected = item;
                        }}><Icon name="folder" /><span>{item.path}</span></button
                      >{#if item.incomplete}<span class="partial">Partial</span>{/if}</td
                    >
                    <td
                      ><span class="category-swatch" style:background={colors[item.category]}
                      ></span>{labels[item.category]}</td
                    ><td class="number">{bytes(item.logical)}</td><td class="number"
                      >{item.allocation_known ? bytes(item.allocated) : 'Unknown'}</td
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
    <aside
      class="developer-details"
      aria-label="Developer storage details"
      bind:this={detailsRegion}
    >
      {#if selected}
        <div class="developer-detail-heading">
          <Icon name="folder" size={28} />
          <h2>{selected.path.split(/[\\/]/).at(-1) || selected.path}</h2>
          {#if selected.incomplete}<span class="partial">Partial</span>{/if}
        </div>
        <p class="detail-path">{selected.path}</p>
        <dl>
          <div>
            <dt>Category</dt>
            <dd>
              <span class="category-swatch" style:background={colors[selected.category]}
              ></span>{labels[selected.category]}
            </dd>
          </div>
          <div>
            <dt>Tool</dt>
            <dd>{selected.tool}</dd>
          </div>
          <div>
            <dt>Logical size</dt>
            <dd>{bytes(selected.logical)}</dd>
          </div>
          <div>
            <dt>Allocated</dt>
            <dd>{selected.allocation_known ? bytes(selected.allocated) : 'Unknown'}</dd>
          </div>
          <div>
            <dt>Files</dt>
            <dd>{selected.files.toLocaleString()}</dd>
          </div>
        </dl>
        <button
          class="primary open-developer"
          onclick={() => {
            if (selected) onopen(selected.id);
          }}
          disabled={!enabled}><Icon name="folder" />Open folder</button
        >
      {/if}
      <DockerStorage />
    </aside>
  </div>
</div>

<style>
  .developer-storage {
    min-width: 0;
  }
  .developer-summary,
  .developer-pages {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 0;
  }
  .developer-summary > span,
  .developer-pages > span {
    margin-right: auto;
    color: var(--muted);
    font-size: 12px;
  }
  .developer-summary {
    padding-top: 0;
  }
  .developer-pages {
    border-top: 1px solid var(--border);
  }
  .developer-scroll {
    overflow: auto;
    overscroll-behavior: contain;
  }
  table {
    width: 100%;
    min-width: 620px;
    table-layout: fixed;
    border-collapse: collapse;
    font-size: 13px;
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
  th:nth-child(2) {
    width: 145px;
  }
  th:nth-child(n + 3) {
    width: 105px;
  }
  tr.selected {
    background: var(--selected);
  }
  tr.selected .developer-path button {
    font-weight: 600;
  }
  tbody tr:last-child td {
    border-bottom: 0;
  }
  .developer-path {
    min-width: 0;
  }
  .developer-path button {
    border: 0;
    background: transparent;
    padding: 0;
    width: 100%;
    min-width: 0;
    justify-content: flex-start;
  }
  .developer-path button span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .developer-path button :global(svg) {
    flex-shrink: 0;
  }
  .developer-path .partial {
    display: block;
    margin-top: 3px;
    font-size: 12px;
  }
  .developer-layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    border-top: 1px solid var(--border);
  }
  .developer-list {
    min-width: 0;
    padding: 22px 0;
  }
  .developer-list h2 {
    font-size: 21px;
    margin-bottom: 22px;
  }
  .developer-list-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    flex-wrap: wrap;
    margin-bottom: 20px;
  }
  .developer-list-heading h2 {
    margin: 0;
  }
  .developer-filters {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
    min-width: 0;
  }
  .developer-filters select {
    max-width: 100%;
    font-size: 13px;
  }
  .developer-details {
    min-width: 0;
    border-top: 1px solid var(--border);
    padding: 22px 0;
  }
  .developer-detail-heading {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .developer-detail-heading h2 {
    font-size: 23px;
    overflow-wrap: anywhere;
    min-width: 0;
    flex: 1;
  }
  .developer-detail-heading .partial {
    flex-shrink: 0;
    font-size: 12px;
  }
  .detail-path {
    font-size: 13px;
    overflow-wrap: anywhere;
    margin: 12px 0 20px;
  }
  dl {
    display: grid;
    gap: 15px;
    border-top: 1px solid var(--border);
    padding: 20px 0;
    margin: 0;
  }
  dl > div {
    display: grid;
    grid-template-columns: 100px minmax(0, 1fr);
    gap: 20px;
    font-size: 13px;
  }
  dt {
    color: var(--secondary);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
    font-variant-numeric: tabular-nums;
  }
  .open-developer {
    width: 100%;
  }
  .developer-distribution {
    padding: 12px 0 24px;
  }
  .developer-bar {
    height: 36px;
    display: flex;
    gap: 1px;
    overflow: hidden;
    border-radius: var(--radius);
    background: var(--surface);
  }
  .developer-legend {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 20px;
    padding-top: 18px;
  }
  .developer-legend > div {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    min-width: 0;
  }
  .developer-legend strong {
    font-size: 14px;
  }
  .developer-legend small {
    display: block;
    margin-top: 6px;
    color: var(--secondary);
    font-size: 12px;
  }
  .category-swatch {
    display: inline-block;
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    border-radius: 2px;
    margin-right: 8px;
    vertical-align: middle;
  }
  .developer-legend .category-swatch {
    margin: 2px 0 0;
  }
  @media (min-width: 1200px) {
    .developer-layout {
      grid-template-columns: minmax(0, 1fr) 340px;
    }
    .developer-list {
      padding-right: 20px;
    }
    .developer-details {
      border-top: 0;
      border-left: 1px solid var(--border);
      padding-left: 24px;
    }
  }
  @media (max-width: 840px) {
    .developer-legend {
      grid-template-columns: minmax(0, 1fr);
      gap: 16px;
    }
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
</style>
