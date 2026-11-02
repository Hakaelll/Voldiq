<script lang="ts">
  import { onDestroy, onMount, tick, untrack } from 'svelte';
  import { getIndexContext } from '../lib/index-context';
  import { fileQueries } from '../lib/queries';
  import {
    virtualWindow,
    rowTop,
    scrollToRow,
    keyboardRow,
    wheelScroll,
    gridDimensions,
    type WindowGeometry,
  } from '../lib/virtual-list';
  import { date, type Entry, type Query } from '../lib/types';
  import { fileSizeLabel } from '../lib/file-size';
  import Icon from './Icon.svelte';
  import FolderNotePreview from './FolderNotePreview.svelte';
  import { categoryPaint } from '../lib/storage-map';
  import { selectionOperation, type SelectionOperation } from '../lib/exploration';
  let {
    query,
    revision,
    selected,
    explored,
    onclearExplore,
    onselect,
    onopen,
    onerror,
    onsort,
    checked = new Set<number>(),
    manageable = false,
    oncheck,
    ranked = false,
    paint,
    representation = 'table',
    allocated = false,
  }: {
    query: Query;
    revision: number;
    selected: number | null;
    explored: ReadonlySet<number>;
    onclearExplore: () => void;
    onselect: (entry: Entry, operation?: SelectionOperation) => void;
    onopen: (entry: Entry) => void;
    onerror: (error: unknown) => void;
    onsort: (sort: Exclude<NonNullable<Query['sort']>, 'allocated'>) => void;
    checked?: ReadonlySet<number>;
    manageable?: boolean;
    oncheck?: (id: number, checked: boolean) => void;
    ranked?: boolean;
    paint?: (entry: Entry) => { fill: string; ink: string };
    representation?: 'table' | 'grid';
    allocated?: boolean;
  } = $props();
  const indexId = untrack(getIndexContext);
  const noteTooltipId = $props.id();
  let notePreview = $state<{ entry: Entry; label: string; x: number; y: number } | null>(null);
  function showNote(entry: Entry, target: HTMLButtonElement): void {
    if (entry.kind !== 'directory') {
      notePreview = null;
      return;
    }
    const box = target.getBoundingClientRect();
    notePreview = {
      entry,
      label: ranked
        ? (paths[rows.findIndex((item) => item.id === entry.id)] ?? entry.name)
        : entry.name,
      x: Math.max(8, Math.min(box.left, globalThis.innerWidth - 328)),
      y: Math.max(8, Math.min(box.bottom + 6, globalThis.innerHeight - 188)),
    };
  }
  let scrollTop = $state(0),
    total = $state(0),
    rows = $state<Entry[]>([]),
    offset = $state(0);
  let paths = $state<string[]>([]);
  let largest = $state(0);
  const activeSort = $derived(query.sort === 'allocated' ? 'size' : (query.sort ?? 'size'));
  const grid = $derived(representation === 'grid');
  let viewport: HTMLDivElement;
  let viewportHeight = $state(420);
  let viewportWidth = $state(700);
  const geometry = $derived(grid ? gridDimensions(viewportWidth, viewportHeight) : undefined);
  let previousGeometry: WindowGeometry | undefined;
  let loading = $state(true),
    loadError = $state(''),
    retry = $state(0);
  let pendingFocus = $state<number | null>(null);
  let request = 0;
  let window = $derived(virtualWindow(total, viewportHeight, scrollTop, geometry));
  let desired = $derived(window.offset);
  const pageLimit = $derived(window.limit);
  $effect(() => {
    if (notePreview && !rows.some((entry) => entry.id === notePreview?.entry.id))
      notePreview = null;
  });
  $effect(() => {
    void query;
    void revision;
    pendingFocus = null;
    notePreview = null;
    rows = [];
    paths = [];
    total = 0;
    largest = 0;
    offset = 0;
    if (viewport) viewport.scrollTop = 0;
    scrollTop = 0;
  });
  $effect(() => {
    const next = geometry;
    untrack(() => {
      if (
        !viewport ||
        (next?.columns === previousGeometry?.columns &&
          next?.rowHeight === previousGeometry?.rowHeight)
      )
        return;
      const before = virtualWindow(total, viewportHeight, scrollTop, previousGeometry);
      const after = virtualWindow(total, viewportHeight, scrollTop, next);
      const anchor = Math.floor(before.logicalTop / before.rowHeight) * before.columns;
      viewport.scrollTop = scrollToRow(after, anchor, 'start');
      scrollTop = viewport.scrollTop;
      previousGeometry = next;
      notePreview = null;
    });
  });
  $effect(() => {
    void revision;
    void retry;
    const next = query;
    const start = desired;
    const limit = pageLimit;
    const token = ++request;
    let alive = true;
    let issued: ReturnType<typeof fileQueries.request> | undefined;
    loading = true;
    loadError = '';
    const timer = setTimeout(() => {
      issued = fileQueries.request({ ...next, offset: start, limit, indexId });
      issued.result
        .then((page) => {
          if (alive && token === request && page) {
            rows = page.items;
            paths = page.paths;
            largest = page.largest;
            total = page.total;
            offset = start;
            loading = false;
          } else if (alive && token === request) {
            rows = [];
            paths = [];
            loading = false;
            loadError = 'Item request was cancelled. Retry to load the current analysis.';
          }
        })
        .catch((problem: unknown) => {
          if (alive && token === request) {
            rows = [];
            paths = [];
            loading = false;
            loadError = 'Could not load items.';
            onerror(problem);
          }
        });
    }, 40);
    return () => {
      alive = false;
      clearTimeout(timer);
      issued?.cancel();
    };
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
    const target = keyboardRow(
      event.key,
      pendingFocus ?? index,
      total,
      window.visibleHeight,
      window.columns,
      window.rowHeight,
    );
    if (target === null) return;
    event.preventDefault();
    pendingFocus = target;
    viewport.scrollTop = scrollToRow(window, target);
    scrollTop = viewport.scrollTop;
  }
</script>

<svelte:window
  onresize={() => {
    notePreview = null;
  }}
/>

<div
  class="file-table"
  class:ranked
  class:grid-view={grid}
  role={grid ? 'region' : 'table'}
  aria-label={grid ? 'Indexed file grid' : 'Indexed files'}
  aria-rowcount={grid || loading || loadError ? undefined : total + 1}
  aria-busy={loading}
>
  <div
    class="table-scroll"
    role={grid ? 'list' : 'rowgroup'}
    aria-label={grid ? 'Indexed files' : undefined}
    bind:this={viewport}
    bind:clientHeight={viewportHeight}
    bind:clientWidth={viewportWidth}
    onscroll={() => {
      scrollTop = viewport.scrollTop;
      const focused = document.activeElement;
      if (
        notePreview &&
        focused instanceof HTMLButtonElement &&
        focused.getAttribute('aria-describedby') === noteTooltipId
      )
        showNote(notePreview.entry, focused);
      else notePreview = null;
    }}
  >
    {#if !grid}<div class="table-head" role="row">
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
            title={allocated
              ? 'Sort by known allocation, largest first'
              : 'Sort by logical size, largest first'}
            >{allocated ? 'Unique allocation' : 'Logical size'}{#if activeSort === 'size'}<Icon
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
      </div>{/if}
    <div
      class="virtual-rows"
      role={grid ? 'presentation' : undefined}
      style:height={`${window.height}px`}
    >
      {#each rows as entry, i (entry.id)}
        <div
          class:selected={entry.id === selected}
          class:explored={explored.has(entry.id)}
          class={grid ? 'file-grid-item' : 'table-row'}
          role={grid ? 'listitem' : 'row'}
          aria-rowindex={grid ? undefined : offset + i + 2}
          aria-posinset={grid ? offset + i + 1 : undefined}
          aria-setsize={grid ? total : undefined}
          style:top={`${rowTop(window, offset + i)}px`}
          style:left={grid
            ? `${(100 * ((offset + i) % window.columns)) / window.columns}%`
            : undefined}
          style:width={grid ? `${100 / window.columns}%` : undefined}
          style:height={grid ? `${window.rowHeight}px` : undefined}
        >
          <span role={grid ? undefined : 'cell'} class="file-label">
            {#if manageable}
              {#if entry.kind === 'file' && !entry.incomplete}<input
                  type="checkbox"
                  aria-label={`Add ${entry.name} to cleanup review`}
                  checked={checked.has(entry.id)}
                  onchange={(event) => oncheck?.(entry.id, event.currentTarget.checked)}
                />
              {:else}<span class="selection-spacer"></span>{/if}
            {/if}
            <button
              class="file-name"
              aria-current={entry.id === selected ? 'true' : undefined}
              aria-pressed={explored.has(entry.id)}
              data-index={offset + i}
              title={entry.kind === 'directory'
                ? undefined
                : ranked
                  ? (paths[i] ?? entry.name)
                  : entry.name}
              aria-describedby={notePreview?.entry.id === entry.id ? noteTooltipId : undefined}
              onpointerenter={(event) => showNote(entry, event.currentTarget)}
              onpointerleave={(event) => {
                if (document.activeElement !== event.currentTarget) notePreview = null;
              }}
              onfocus={(event) => showNote(entry, event.currentTarget)}
              onblur={() => {
                notePreview = null;
              }}
              onclick={(event) => onselect(entry, selectionOperation(event))}
              ondblclick={(event) => {
                if (!event.ctrlKey && !event.metaKey && !event.shiftKey) onopen(entry);
              }}
              onkeydown={(event) => {
                if (event.key === 'Escape' && notePreview) {
                  notePreview = null;
                  event.stopPropagation();
                } else if (event.key === 'Escape' && explored.size) {
                  event.stopPropagation();
                  onclearExplore();
                } else if (event.key === ' ' && (event.ctrlKey || event.metaKey)) {
                  event.preventDefault();
                  onselect(entry, 'toggle');
                } else if (event.key === 'Enter') {
                  event.preventDefault();
                  onopen(entry);
                } else navigate(event, offset + i);
              }}
              ><span
                class="item-swatch"
                style:background={paint?.(entry).fill ??
                  categoryPaint({
                    id: entry.id,
                    name: entry.name,
                    value: entry.logical,
                    directory: entry.kind === 'directory',
                  }).fill}
              ></span><Icon name={entry.kind === 'directory' ? 'folder' : 'file'} /><span
                >{entry.name}{#if ranked}<small>{paths[i] ?? ''}</small>{/if}</span
              >{#if entry.incomplete}<Icon name="warning" size={14} />{/if}</button
            ></span
          >
          <span class="number" role={grid ? undefined : 'cell'}>
            {#if grid}<small>{allocated ? 'Unique allocation' : 'Logical size'}</small>{/if}
            {fileSizeLabel(entry, allocated)}
            {#if !allocated}<span class="size-track" aria-hidden="true"
                ><span
                  style:width={`${largest ? Math.min(100, (entry.logical / largest) * 100) : 0}%`}
                ></span></span
              >{/if}
          </span>
          <span role={grid ? undefined : 'cell'}
            >{#if grid}<small>Modified</small>{/if}{date(entry.modified)}</span
          ><span role={grid ? undefined : 'cell'}
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
    {#if loadError}<div class="table-empty" role="alert">
        {loadError} <button onclick={() => retry++}>Retry</button>
      </div>
    {:else if !loading && total === 0}<div class="table-empty">No matching items.</div>{/if}
  </div>
  <div class="table-footer">
    <span
      >{loading
        ? 'Loading items…'
        : loadError
          ? 'Items unavailable'
          : `${total.toLocaleString()} items`}</span
    >
    {#if loadError}<button onclick={() => retry++}>Retry</button>{/if}
    {#if grid}<span>Equal tiles · size is shown as text</span>{/if}
    {#if manageable}<span>Checkboxes select items for cleanup review.</span>{/if}
  </div>
</div>

{#if notePreview}<div
    class="map-tooltip folder-note-tooltip"
    role="tooltip"
    id={noteTooltipId}
    style:left={`${notePreview.x}px`}
    style:top={`${notePreview.y}px`}
  >
    <strong>{notePreview.label}</strong>
    {#key notePreview.entry.id}<FolderNotePreview nodeId={notePreview.entry.id} {indexId} />{/key}
  </div>{/if}

<style>
  .grid-view .virtual-rows {
    min-width: 0;
  }
  .file-grid-item {
    position: absolute;
    display: grid;
    align-content: start;
    gap: 6px;
    padding: 12px;
    overflow: hidden;
    border-right: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
    background: var(--canvas);
  }
  .file-grid-item:hover {
    background: var(--surface);
  }
  .file-grid-item.selected {
    background: var(--selected);
    outline: 2px solid var(--text);
    outline-offset: -2px;
  }
  .explored {
    box-shadow: inset 3px 0 var(--accent);
  }
  .file-grid-item > span {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .file-grid-item > .number {
    text-align: left;
  }
  .file-grid-item > span > small {
    display: block;
    color: var(--secondary);
    font-size: 10px;
  }
  .file-grid-item .file-label {
    align-items: flex-start;
    gap: 6px;
  }
  .file-grid-item .file-name {
    min-height: 36px;
    align-items: flex-start;
  }
  .file-grid-item .file-name > span:last-of-type {
    display: -webkit-box;
    line-clamp: 2;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    white-space: normal;
    overflow-wrap: anywhere;
    line-height: 18px;
  }
  .file-grid-item .file-name > .item-swatch {
    margin-top: 3px;
  }
  .file-grid-item .file-label input {
    margin-top: 3px;
  }
  .table-footer {
    flex-wrap: wrap;
    gap: 6px 12px;
  }
  .folder-note-tooltip {
    position: fixed;
    width: min(320px, calc(100vw - 16px));
    max-height: 180px;
    overflow: hidden;
    z-index: 50;
    pointer-events: none;
  }
  .folder-note-tooltip > strong {
    display: -webkit-box;
    line-clamp: 2;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }
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
