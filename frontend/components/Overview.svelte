<script lang="ts">
  import type { GraphicCapture } from '../lib/graphic-export';
  import StorageMap from './StorageMap.svelte';
  import SpaceAccount from './SpaceAccount.svelte';
  import RecoveryCopies from './RecoveryCopies.svelte';
  import type { IndexId } from '../lib/index-context';
  import { volumeMeasurement, volumeCaller, volumeObservationLabel } from '../lib/volume-space';
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
  import { fileSizeLabel } from '../lib/file-size';
  import { selectionOperation, type SelectionOperation, type SelectMany } from '../lib/exploration';
  import type { MapCommandHandler } from '../lib/map-menu';

  let {
    indexId,
    paused,
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
    searchState,
    searchIdentity,
    searchTotal,
    onmode,
    onback,
    cloudFilter,
    queryState,
    oncloudfilter,
    onretry,
    onexport,
    exportDisabled = false,
  }: {
    indexId: IndexId | null;
    paused: boolean;
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
    searchState: 'inactive' | 'loading' | 'ready' | 'failed';
    searchIdentity: string;
    searchTotal: number | null;
    onmode: (mode: import('../lib/storage-map').StorageView) => void;
    onback: () => void;
    cloudFilter: CloudFilter | null;
    queryState: 'ready' | 'loading' | 'failed';
    oncloudfilter: (value: CloudFilter | null) => void;
    onretry: () => void;
    onexport?: (capture: GraphicCapture) => boolean;
    exportDisabled?: boolean;
  } = $props();
  const distribution = $derived(visibleFileTypes(types));
  const space = $derived(volumeMeasurement(volume));
  const callerSpace = $derived(volumeCaller(volume));
  const usedPercent = $derived(
    space && Number(space.total) > 0 ? (Number(space.used) / Number(space.total)) * 100 : null,
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
        <div class="summary-heading">
          <h2>Selected drive</h2>
          <span title={volume.path}>{volume.name || volume.path}</span>
        </div>
        <div class="overview-volume-body">
          {#if usedPercent !== null}<div
              class="overview-volume-ring"
              aria-hidden="true"
              style:background={`conic-gradient(var(--accent-fill) ${usedPercent}%, var(--paper) 0)`}
            >
              <div><strong>{usedPercent.toFixed(0)}<small>%</small></strong><span>used</span></div>
            </div>{/if}
          <dl>
            <div>
              <dt>Used on volume</dt>
              <dd title={space ? `${space.used} bytes` : undefined}>
                {space ? bytes(Number(space.used)) : 'Unavailable'}
              </dd>
            </div>
            <div>
              <dt>Free space</dt>
              <dd title={space ? `${space.free} bytes` : undefined}>
                {space ? bytes(Number(space.free)) : 'Unavailable'}
              </dd>
            </div>
            <div>
              <dt>Total capacity</dt>
              <dd title={space ? `${space.total} bytes` : undefined}>
                {space ? bytes(Number(space.total)) : 'Unavailable'}
              </dd>
            </div>
          </dl>
        </div>
        <div class="volume-meter sr-only">
          {#if usedPercent !== null && space}
            <progress
              aria-label="Volume space used"
              value={Number(space.used)}
              max={Number(space.total)}
            ></progress>
            <span>{usedPercent.toFixed(1)}%</span>
          {:else}<span>Usage unavailable</span>{/if}
        </div>
        {#if callerSpace}<p title={`${callerSpace.available} bytes available to the caller`}>
            Available to you · {bytes(Number(callerSpace.available))}
          </p>{/if}
        <p>{volumeObservationLabel(volume)}</p>
      </section>
    {/if}
    {#if !directory || directory.entry.id === 0}
      <section class="indexed-summary">
        <h2>Indexed scope</h2>
        <dl>
          <div class="indexed-total">
            <dt>Indexed logical size</dt>
            <dd>{directory ? bytes(directory.entry.logical) : '—'}</dd>
          </div>
        </dl>
        {#if directory}<span class="indexed-path" title={directory.path}>{directory.path}</span
          >{/if}
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
          {onexport}
          {exportDisabled}
          {nodes}
          {cloudFilter}
          {queryState}
          {oncloudfilter}
          {onretry}
          identity={mapIdentity}
          mode={mapMode}
          bind:depth
          bind:colorMode
          {searchState}
          {searchIdentity}
          {searchTotal}
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
              >{fileSizeLabel(selected?.entry ?? directory.entry, sizeMode === 'allocated')}</span
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
    {#if indexId !== null}
      <SpaceAccount {indexId} {paused} />
      <RecoveryCopies {indexId} {paused} />
    {/if}
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
            <span>{bytes(item.bytes)}</span><small>{item.count.toLocaleString()} files</small><em
              >{types.total_bytes > 0
                ? `${((item.bytes / types.total_bytes) * 100).toFixed(1)}%`
                : '—'}</em
            >
          </button>{/each}
        {#if distribution.otherFiles > 0}<div class="type-other">
            <span class="item-swatch" style:background="var(--paper)"></span>
            <strong>Other</strong><span>{bytes(distribution.otherBytes)}</span><small
              >{distribution.otherFiles.toLocaleString()} files</small
            >
            <em
              >{types.total_bytes > 0
                ? `${((distribution.otherBytes / types.total_bytes) * 100).toFixed(1)}%`
                : '—'}</em
            >
          </div>{/if}
      </div>
    </section>
  </div>
</div>
