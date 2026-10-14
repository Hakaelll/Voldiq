<script lang="ts">
  import { onDestroy, onMount, tick } from 'svelte';
  import { fileQueries } from '../lib/queries';
  import {
    virtualWindow,
    rowTop,
    scrollToRow,
    keyboardRow,
    wheelScroll,
  } from '../lib/virtual-list';
  import { bytes, date, type Entry, type Query } from '../lib/types';
  import Icon from './Icon.svelte';
  let {
    query,
    revision,
    selected,
    onselect,
    onopen,
    onerror,
    onsort,
    checked = new Set<number>(),
    manageable = false,
    oncheck,
    ranked = false,
  }: {
    query: Query;
    revision: number;
    selected: number | null;
    onselect: (entry: Entry) => void;
    onopen: (entry: Entry) => void;
    onerror: (error: unknown) => void;
    onsort: (sort: NonNullable<Query['sort']>) => void;
    checked?: ReadonlySet<number>;
    manageable?: boolean;
    oncheck?: (id: number, checked: boolean) => void;
    ranked?: boolean;
  } = $props();
  let scrollTop = $state(0),
    total = $state(0),
    rows = $state<Entry[]>([]),
    offset = $state(0);
  let paths = $state<string[]>([]);
  let largest = $state(0);
  const activeSort = $derived(query.sort ?? 'size');
  let viewport: HTMLDivElement;
  let viewportHeight = $state(420);
  let pendingFocus = $state<number | null>(null);
  let request = 0;
  let window = $derived(virtualWindow(total, viewportHeight, scrollTop));
  let desired = $derived(window.offset);
  $effect(() => {
    void revision;
    const next = query;
    const start = desired;
    const token = ++request;
    let alive = true;
    let issued: ReturnType<typeof fileQueries.request> | undefined;
    const timer = setTimeout(() => {
      issued = fileQueries.request({ ...next, offset: start, limit: 80 });
      issued.result
        .then((page) => {
          if (alive && token === request && page) {
            rows = page.items;
            paths = page.paths;
            largest = page.largest;
            total = page.total;
            offset = start;
          }
        })
        .catch((problem: unknown) => {
          if (alive && token === request) onerror(problem);
        });
    }, 40);
    return () => {
      alive = false;
      clearTimeout(timer);
      issued?.cancel();
    };
  });
  $effect(() => {
    void query;
    pendingFocus = null;
    if (viewport) viewport.scrollTop = 0;
    scrollTop = 0;
  });
  $effect(() => {
    const target = pendingFocus;
    const start = offset;
    const count = rows.length;
    if (target === null || target < start || target >= start + count) return;
    const token = request;
    void tick().then(() => {
      if (pendingFocus !== target || token !== request) return;
      const button = viewport.querySelector<HTMLButtonElement>(`button[data-index="${target}"]`);
      if (button) {
        button.focus({ preventScroll: true });
        pendingFocus = null;
      }
    });
  });
  onDestroy(() => {
    request += 1;
    pendingFocus = null;
  });
  onMount(() => {
    function wheel(event: WheelEvent): void {
      pendingFocus = null;
      const next = wheelScroll(window, event);
      if (next === null || next === scrollTop) return;
      event.preventDefault();
      viewport.scrollTop = next;
      scrollTop = viewport.scrollTop;
    }
    function cancelFocus(event: FocusEvent | PointerEvent): void {
      if (pendingFocus === null) return;
      const target = event.target;
      if (
        event.type === 'focusin' &&
        target instanceof HTMLElement &&
        viewport.contains(target) &&
        target.dataset.index === String(pendingFocus)
      )
        return;
      pendingFocus = null;
    }
    document.addEventListener('focusin', cancelFocus);
    document.addEventListener('pointerdown', cancelFocus);
    viewport.addEventListener('wheel', wheel, { passive: false });
    return () => {
      document.removeEventListener('focusin', cancelFocus);
      document.removeEventListener('pointerdown', cancelFocus);
      viewport.removeEventListener('wheel', wheel);
    };
  });
  function navigate(event: KeyboardEvent, index: number): void {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    const target = keyboardRow(event.key, pendingFocus ?? index, total, window.visibleHeight);
    if (target === null) return;
    event.preventDefault();
    pendingFocus = target;
    viewport.scrollTop = scrollToRow(window, target);
    scrollTop = viewport.scrollTop;
  }
</script>

<div
  class="file-table"
  class:ranked
  role="table"
  aria-label="Indexed files"
  aria-rowcount={total + 1}
>
  <div
    class="table-scroll"
    role="rowgroup"
    bind:this={viewport}
    bind:clientHeight={viewportHeight}
    onscroll={() => {
      scrollTop = viewport.scrollTop;
    }}
  >
    <div class="table-head" role="row">
      <span role="columnheader" aria-sort={activeSort === 'name' ? 'ascending' : 'none'}>
        <button
          class:active={activeSort === 'name'}
          onclick={() => onsort('name')}
          title="Sort by name"
          >Name{#if activeSort === 'name'}<Icon name="chevron-up" size={14} />{/if}</button
        >
      </span>
      <span role="columnheader" aria-sort={activeSort === 'size' ? 'descending' : 'none'}>
        <button
          class:active={activeSort === 'size'}
          onclick={() => onsort('size')}
          title="Sort by logical size, largest first"
          >Logical size{#if activeSort === 'size'}<Icon
              name="chevron-down"
              size={14}
            />{/if}</button
        >
      </span>
      <span role="columnheader" aria-sort={activeSort === 'modified' ? 'descending' : 'none'}>
        <button
          class:active={activeSort === 'modified'}
          onclick={() => onsort('modified')}
          title="Sort by modification date, newest first"
          >Modified{#if activeSort === 'modified'}<Icon
              name="chevron-down"
              size={14}
            />{/if}</button
        >
      </span>
      <span role="columnheader">Type</span>
    </div>
    <div class="virtual-rows" style:height={`${window.height}px`}>
      {#each rows as entry, i (entry.id)}
        <div
          class:selected={entry.id === selected}
          class="table-row"
          role="row"
          aria-rowindex={offset + i + 2}
          style:top={`${rowTop(window, offset + i)}px`}
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
              data-index={offset + i}
              title={ranked ? (paths[i] ?? entry.name) : entry.name}
              onclick={() => onselect(entry)}
              ondblclick={() => onopen(entry)}
              onkeydown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  onopen(entry);
                } else navigate(event, offset + i);
              }}
              ><Icon name={entry.kind === 'directory' ? 'folder' : 'file'} /><span
                >{entry.name}{#if ranked}<small>{paths[i] ?? ''}</small>{/if}</span
              >{#if entry.incomplete}<Icon name="warning" size={14} />{/if}</button
            ></span
          >
          <span class="number" role="cell"
            >{bytes(entry.logical)}<span class="size-track" aria-hidden="true"
              ><span
                style:width={`${largest ? Math.min(100, (entry.logical / largest) * 100) : 0}%`}
              ></span></span
            ></span
          >
          <span role="cell">{date(entry.modified)}</span><span role="cell"
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
  .table-head button {
    width: 100%;
    min-height: 32px;
    justify-content: flex-start;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--secondary);
    font-size: inherit;
    font-weight: 400;
    gap: 6px;
  }
  .table-head > span:nth-child(2) button {
    justify-content: flex-end;
  }
  .table-head button.active {
    color: var(--text);
    font-weight: 600;
  }
  .table-head button:hover {
    color: var(--accent);
  }
  .ranked .file-name > span {
    display: block;
    line-height: 15px;
    white-space: nowrap;
  }
  .ranked .file-name small {
    display: block;
    font-size: 11px;
    font-weight: 400;
    color: var(--secondary);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .size-track {
    display: block;
    height: 3px;
    background: var(--paper);
    margin-top: 5px;
  }
  .size-track > span {
    display: block;
    height: 100%;
    background: var(--accent-fill);
  }
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
