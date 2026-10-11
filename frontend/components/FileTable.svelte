<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { bytes, date, type Entry, type Page, type Query } from '../lib/types';
  import Icon from './Icon.svelte';
  let {
    query,
    revision,
    selected,
    onselect,
    onopen,
    onerror,
    checked = new Set<number>(),
    manageable = false,
    oncheck,
  }: {
    query: Query;
    revision: number;
    selected: number | null;
    onselect: (entry: Entry) => void;
    onopen: (entry: Entry) => void;
    onerror: (error: unknown) => void;
    checked?: ReadonlySet<number>;
    manageable?: boolean;
    oncheck?: (id: number, checked: boolean) => void;
  } = $props();
  const rowHeight = 36;
  let scrollTop = $state(0),
    total = $state(0),
    rows = $state<Entry[]>([]),
    offset = $state(0);
  let viewport: HTMLDivElement;
  let request = 0;
  let desired = $derived(Math.max(0, Math.floor(scrollTop / rowHeight) - 8));
  $effect(() => {
    void revision;
    const next = query;
    const start = desired;
    const token = ++request;
    const timer = setTimeout(() => {
      invoke<Page>('query_files', { query: { ...next, offset: start, limit: 80 } })
        .then((page) => {
          if (token === request) {
            rows = page.items;
            total = page.total;
            offset = start;
          }
        })
        .catch((problem: unknown) => {
          if (token === request) onerror(problem);
        });
    }, 40);
    return () => clearTimeout(timer);
  });
  $effect(() => {
    void query;
    if (viewport) viewport.scrollTop = 0;
    scrollTop = 0;
  });
</script>

<div class="file-table" role="table" aria-label="Indexed files" aria-rowcount={total + 1}>
  <div class="table-head" role="row">
    <span role="columnheader">Name</span><span role="columnheader">Logical size</span><span
      role="columnheader">Modified</span
    ><span role="columnheader">Type</span>
  </div>
  <div
    class="table-scroll"
    bind:this={viewport}
    onscroll={() => {
      scrollTop = viewport.scrollTop;
    }}
  >
    <div class="virtual-rows" style:height={`${total * rowHeight}px`}>
      {#each rows as entry, i (entry.id)}
        <div
          class:selected={entry.id === selected}
          class="table-row"
          role="row"
          aria-rowindex={offset + i + 2}
          style:top={`${(offset + i) * rowHeight}px`}
        >
          <span role="cell" class="file-label">
            {#if manageable}
              {#if entry.kind === 'file' && !entry.incomplete}<input
                  type="checkbox"
                  aria-label={`Select ${entry.name}`}
                  checked={checked.has(entry.id)}
                  onchange={(event) => oncheck?.(entry.id, event.currentTarget.checked)}
                />
              {:else}<span class="selection-spacer"></span>{/if}
            {/if}
            <button
              class="file-name"
              title={entry.name}
              onclick={() => onselect(entry)}
              ondblclick={() => onopen(entry)}
              onkeydown={(event) => {
                if (event.key === 'Enter') onopen(entry);
              }}
              ><Icon name={entry.kind === 'directory' ? 'folder' : 'file'} /><span
                >{entry.name}</span
              >{#if entry.incomplete}<Icon name="warning" size={14} />{/if}</button
            ></span
          >
          <span class="number" role="cell">{bytes(entry.logical)}</span><span role="cell"
            >{date(entry.modified)}</span
          ><span role="cell"
            >{entry.kind === 'directory'
              ? 'Folder'
              : entry.kind === 'link'
                ? 'Reparse point'
                : entry.hard_link
                  ? 'File · hard link'
                  : 'File'}</span
          >
        </div>
      {/each}
    </div>
    {#if total === 0}<div class="table-empty">No matching items.</div>{/if}
  </div>
  <div class="table-footer">
    {total.toLocaleString()} items
  </div>
</div>

<style>
  .file-label {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .file-label input[type='checkbox'] {
    width: 14px;
    height: 14px;
    min-height: 0;
    flex-shrink: 0;
    padding: 0;
    accent-color: var(--accent);
  }
  .file-label .selection-spacer {
    width: 14px;
    flex-shrink: 0;
  }
  .file-label .file-name {
    min-width: 0;
  }
</style>
