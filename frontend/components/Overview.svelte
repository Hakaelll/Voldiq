<script lang="ts">
  import StorageMap from './StorageMap.svelte';
  import {
    bytes,
    type Details,
    type FileType,
    type FileTypes,
    type Page,
    type Volume,
    type CloudFilter,
  } from '../lib/types';
  import type { WeightedNode } from '../lib/treemap';
  import { categoryPaint, type ColorMode, type StorageDepth } from '../lib/storage-map';
  import { visibleFileTypes } from '../lib/file-types';
  import { selectionOperation, type SelectionOperation, type SelectMany } from '../lib/exploration';
  import type { MapCommandHandler } from '../lib/map-menu';

  let {
    volume,
    directory,
    children,
    nodes,
    types,
    selected,
    explored,
    onclearExplore,
    onselectMany,
    oncommand,
    sizeMode,
    reduceMotion,
    animateNavigation,
    onmetric,
    onselect,
    onopen,
    onlist,
    ongrid,
    ontype,
    mapMode,
    depth = $bindable<StorageDepth>(6),
    colorMode = $bindable<ColorMode>('type'),
    mapIdentity,
    search,
    extension,
    withoutExtension,
    onmode,
    onback,
    cloudFilter,
    queryState,
    oncloudfilter,
    onretry,
  }: {
    volume: Volume | null | undefined;
    directory: Details | null;
    children: Page;
    nodes: WeightedNode[];
    types: FileTypes;
    selected: Details | null;
    explored: ReadonlySet<number>;
    onclearExplore: () => void;
    onselectMany: SelectMany;
    oncommand: MapCommandHandler;
    sizeMode: string;
    reduceMotion: boolean;
    animateNavigation: boolean;
    onmetric: (metric: string) => void;
    onselect: (id: number, operation?: SelectionOperation) => void;
    onopen: (id: number, pointer?: boolean) => void;
    onlist: () => void;
    ongrid?: () => void;
    ontype: (item: FileType) => void;
    mapMode: import('../lib/storage-map').StorageView;
    depth?: StorageDepth;
    colorMode?: ColorMode;
    mapIdentity: string;
    search: string;
    extension: string;
    withoutExtension: boolean;
    onmode: (mode: import('../lib/storage-map').StorageView) => void;
    onback: () => void;
    cloudFilter: CloudFilter | null;
    queryState: 'ready' | 'loading' | 'failed';
    oncloudfilter: (value: CloudFilter | null) => void;
    onretry: () => void;
  } = $props();
  const distribution = $derived(visibleFileTypes(types));
  const usedPercent = $derived(
    volume?.total ? ((volume.total - volume.free) / volume.total) * 100 : 0,
  );
</script>

<div class="overview-grid">
  <aside class="overview-summary" aria-label="Storage summary">
    {#if directory && directory.entry.id !== 0}
      <section class="scope-summary">
        <h2>{directory.entry.name}</h2>
        <dl>
          <div>
            <dt>Logical size</dt>
            <dd>{bytes(directory.entry.logical)}</dd>
          </div>
          <div>
            <dt>Indexed files</dt>
            <dd>{directory.entry.files.toLocaleString()}</dd>
          </div>
        </dl>
      </section>
    {/if}
    {#if volume?.accessible}
      <section class="volume-summary">
        <h2>{volume.name || volume.path}</h2>
        <dl>
          <div>
            <dt>Used on volume</dt>
            <dd>{bytes(volume.total - volume.free)}</dd>
          </div>
          <div>
            <dt>Free space</dt>
            <dd>{bytes(volume.free)}</dd>
          </div>
          <div>
            <dt>Total capacity</dt>
            <dd>{bytes(volume.total)}</dd>
          </div>
        </dl>
        <div class="volume-meter">
          <progress
            aria-label="Volume space used"
            value={volume.total - volume.free}
            max={volume.total || 1}
          ></progress>
          <span>{usedPercent.toFixed(1)}%</span>
        </div>
      </section>
    {/if}
    {#if !directory || directory.entry.id === 0}
      <section class="indexed-summary">
        <dl>
          <div>
            <dt>Indexed logical size</dt>
            <dd>{directory ? bytes(directory.entry.logical) : '—'}</dd>
          </div>
        </dl>
        {#if directory}<p>{directory.entry.files.toLocaleString()} files</p>{/if}
      </section>
    {/if}
    <section class="largest-panel">
      <div class="panel-heading">
        <h2>Largest items</h2>
        <button onclick={onlist}>See all</button>
      </div>
      {#each children.items.slice(0, 10) as entry, position}
        <button
          class="ranking-row"
          class:active={selected?.entry.id === entry.id}
          aria-pressed={explored.has(entry.id)}
          class:explored={explored.has(entry.id)}
          style:--item-color={categoryPaint({
            id: entry.id,
            name: entry.name,
            value: entry.logical,
            directory: entry.kind === 'directory',
          }).fill}
          onclick={(event) => onselect(entry.id, selectionOperation(event))}
          ondblclick={(event) => {
            if (!event.ctrlKey && !event.metaKey && !event.shiftKey) onopen(entry.id, true);
          }}
          onkeydown={(event) => {
            if (event.key === ' ' && (event.ctrlKey || event.metaKey)) {
              event.preventDefault();
              onselect(entry.id, 'toggle');
            }
          }}
        >
          <span class="item-swatch"></span>
          <span class="ranking-name" title={entry.name}>{entry.name}</span><strong
            >{bytes(entry.logical)}</strong
          >
          <span class="ranking-track" aria-hidden="true"
            ><span
              style:width={`${directory && directory.entry.logical > 0 ? Math.min(100, (entry.logical / directory.entry.logical) * 100) : 0}%`}
            ></span></span
          >
        </button>
      {/each}
    </section>
  </aside>
  <div class="overview-main">
    <section class="map-panel" aria-busy={!directory}>
      {#if directory}<StorageMap
          {nodes}
          {cloudFilter}
          {queryState}
          {oncloudfilter}
          {onretry}
          identity={mapIdentity}
          mode={mapMode}
          bind:depth
          bind:colorMode
          {search}
          {extension}
          {withoutExtension}
          {sizeMode}
          {onmetric}
          {onmode}
          {onback}
          {onlist}
          {ongrid}
          selected={selected?.entry.id ?? null}
          {explored}
          {onclearExplore}
          {onselectMany}
          {oncommand}
          scopeAncestors={[directory.entry.id, ...directory.ancestors.map((entry) => entry.id)]}
          {onselect}
          onopen={(id) => onopen(id, true)}
        />
      {:else}<div class="overview-loading" role="status">Loading…</div>{/if}
      {#if directory}<div class="map-caption">
          <span title={selected?.path || directory.path}>{selected?.path || directory.path}</span>
          <div class="map-selection-stats">
            <span>{selected ? 'Full item' : 'Full folder'}</span>
            <span
              >{bytes(
                sizeMode === 'allocated'
                  ? (selected?.entry.allocated ?? directory.entry.allocated)
                  : (selected?.entry.logical ?? directory.entry.logical),
              )}</span
            >
            <span
              >{(selected?.entry.kind === 'file'
                ? 1
                : (selected?.entry.files ?? directory.entry.files)
              ).toLocaleString()} files</span
            >
          </div>
        </div>{/if}
    </section>
    <section class="type-panel">
      <div class="panel-heading"><h2>File types</h2></div>
      <div class="type-bar" role="group" aria-label="Filter by file type">
        {#each distribution.items as item}{#if item.bytes > 0}<button
              onclick={() => ontype(item)}
              style:flex-grow={item.bytes}
              style:background={categoryPaint({
                id: 0,
                name: `file.${item.extension ?? ''}`,
                value: item.bytes,
                directory: false,
              }).fill}
              style:color={categoryPaint({
                id: 0,
                name: `file.${item.extension ?? ''}`,
                value: item.bytes,
                directory: false,
              }).ink}
              aria-label={`${item.extension === null ? 'No extension' : `.${item.extension}`}: ${bytes(item.bytes)}, ${((item.bytes / types.total_bytes) * 100).toFixed(1)}%. Show files`}
              title={`${item.extension === null ? 'No extension' : `.${item.extension}`}: ${bytes(item.bytes)}`}
              >{#if item.bytes / types.total_bytes >= 0.12}<span
                  >{item.extension === null ? 'No extension' : `.${item.extension}`}</span
                >{/if}</button
            >{/if}{/each}
        {#if distribution.otherBytes > 0}<span
            class="type-remainder"
            style:flex-grow={distribution.otherBytes}
            title={`Other: ${bytes(distribution.otherBytes)}`}
            >{#if distribution.otherBytes / types.total_bytes >= 0.12}<span>Other</span>{/if}</span
          >{/if}
      </div>
      <div class="type-legend">
        {#each distribution.items as item}<button onclick={() => ontype(item)}>
            <span
              class="item-swatch"
              style:background={categoryPaint({
                id: 0,
                name: `file.${item.extension ?? ''}`,
                value: item.bytes,
                directory: false,
              }).fill}
            ></span>
            <strong title={item.extension === null ? 'No extension' : `.${item.extension}`}
              >{item.extension === null ? 'No extension' : `.${item.extension}`}</strong
            >
            <span>{bytes(item.bytes)}</span><small>{item.count.toLocaleString()} files</small>
          </button>{/each}
        {#if distribution.otherFiles > 0}<div class="type-other">
            <span class="item-swatch" style:background="var(--paper)"></span>
            <strong>Other</strong><span>{bytes(distribution.otherBytes)}</span><small
              >{distribution.otherFiles.toLocaleString()} files</small
            >
          </div>{/if}
      </div>
    </section>
  </div>
</div>
