<script lang="ts">
  import type { GraphicCapture } from '../lib/graphic-export';
  import StorageMap from './StorageMap.svelte';
  import SpaceAccount from './SpaceAccount.svelte';
  import SystemStorage from './SystemStorage.svelte';
  import RecoveryCopies from './RecoveryCopies.svelte';
  import DeviceHealth from './DeviceHealth.svelte';
  import Applications from './Applications.svelte';
  import StorageCost from './StorageCost.svelte';
  import Libraries from './Libraries.svelte';
  import type { ApplicationCleanupContext } from '../lib/applications';
  import type { Query } from '../lib/types';
  import type { IndexId } from '../lib/index-context';
  import { volumeMeasurement, volumeCaller, volumeObservationLabel } from '../lib/volume-space';
  import {
    bytes,
    date,
    type Details,
    type FileType,
    type FileTypes,
    type Overview,
    type Volume,
    type CloudFilter,
  } from '../lib/types';
  import type { WeightedNode } from '../lib/treemap';
  import { categoryPaint, type ColorMode, type StorageDepth } from '../lib/storage-map';
  import { RANKED_PAINTS } from '../lib/chart-colors';
  import { visibleFileTypes } from '../lib/file-types';
  import { fileSizeLabel } from '../lib/file-size';
  import { selectionOperation, type SelectionOperation, type SelectMany } from '../lib/exploration';
  import type { MapCommandHandler } from '../lib/map-menu';

  let {
    indexId,
    paused,
    volume,
    directory,
    indexSource,
    savedAt,
    scanMethod,
    provisional,
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
    onopenranked,
    onranklist,
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
    appFilters,
    scopeId,
    manageable,
    onreviewcache,
    onapplication,
  }: {
    indexId: IndexId | null;
    paused: boolean;
    volume: Volume | null | undefined;
    directory: Details | null;
    indexSource: Overview['index_source'] | null;
    savedAt: number | null;
    scanMethod: string;
    provisional: boolean;
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
    onopenranked: (id: number, pointer: boolean) => void;
    onranklist: (kind: 'file' | 'directory') => void;
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
    appFilters: Query;
    scopeId: number;
    manageable: boolean;
    onreviewcache: (context: ApplicationCleanupContext) => void;
    onapplication: (id: string) => void;
  } = $props();
  let section = $state<'summary' | 'applications' | 'libraries' | 'cost'>('summary');
  let rankedKind = $state<'file' | 'directory'>('file');
  const ranking = $derived(rankedKind === 'file' ? types.largest_files : types.largest_folders);
  const rankingMaximum = $derived(ranking[0]?.entry.logical ?? 0);
  let manualLibraries = $state<string[]>([]);
  const recorded = $derived(indexSource?.snapshot_id != null);
  let now = $state(Date.now());
  const relativeTime = new Intl.RelativeTimeFormat(undefined, { style: 'short' });
  const measurementDate = new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  });
  const measured = $derived.by(() => {
    if (!indexSource?.observation) return null;
    const measurement = new Date(indexSource.observation.completed_at * 1000);
    return Number.isFinite(measurement.getTime()) ? measurement : null;
  });
  $effect(() => {
    if (section !== 'summary' || measured === null) return;
    now = Date.now();
    const timer = setInterval(() => {
      now = Date.now();
    }, 60_000);
    return () => clearInterval(timer);
  });
  const observationAge = $derived.by(() => {
    if (!measured) return '';
    const minutes = Math.floor((now - measured.getTime()) / 60_000);
    if (minutes < 0) return 'Age unavailable';
    if (minutes === 0) return 'Just measured';
    if (minutes < 60) return relativeTime.format(-minutes, 'minute');
    if (minutes < 1440) return relativeTime.format(-Math.floor(minutes / 60), 'hour');
    return relativeTime.format(-Math.floor(minutes / 1440), 'day');
  });
  const indexStatus = $derived(
    provisional && !recorded
      ? 'Provisional index'
      : directory?.entry.incomplete || indexSource?.observation?.complete === false
        ? 'Partial coverage'
        : indexSource?.observation?.complete
          ? 'Complete within scan scope'
          : 'Coverage unknown',
  );
  const distribution = $derived(visibleFileTypes(types));
  const typeColors = $derived(
    distribution.items.map((_, index) => RANKED_PAINTS[index % RANKED_PAINTS.length].fill),
  );
  const space = $derived(volumeMeasurement(volume));
  const callerSpace = $derived(volumeCaller(volume));
  const usedPercent = $derived(
    space && Number(space.total) > 0 ? (Number(space.used) / Number(space.total)) * 100 : null,
  );
  const typePaint = $derived.by(() => {
    if (types.total_bytes <= 0) return 'var(--paper)';
    let end = 0;
    const stops = distribution.items.flatMap((item, index) => {
      if (item.bytes <= 0) return [];
      const start = end;
      end += (item.bytes / types.total_bytes) * 100;
      return [`${typeColors[index]} ${start}% ${end}%`];
    });
    if (distribution.otherBytes > 0) stops.push(`var(--strong) ${end}% 100%`);
    return stops.length ? `conic-gradient(${stops.join(', ')})` : 'var(--paper)';
  });
</script>

{#snippet indexedSummary()}
  <section class="indexed-summary">
    <h2>{directory && directory.entry.id !== 0 ? directory.entry.name : 'Indexed scope'}</h2>
    <dl>
      <div class="indexed-total">
        <dt>Indexed logical size</dt>
        <dd>{directory ? bytes(directory.entry.logical) : '—'}</dd>
      </div>
    </dl>
    {#if directory}
      <span class="indexed-path" title={directory.path}>{directory.path}</span>
      <p>
        {types.total_files.toLocaleString()} files · {types.total_folders.toLocaleString()} folders
      </p>
      <details class="index-source">
        <summary>{recorded ? 'Recorded · ' : ''}{indexStatus}</summary>
        <dl>
          <div>
            <dt>Index source</dt>
            <dd>{recorded ? 'Saved snapshot' : 'Current analysis'}</dd>
          </div>
          {#if recorded && savedAt !== null}<div>
              <dt>Snapshot saved</dt>
              <dd>{date(savedAt)}</dd>
            </div>{/if}
          {#if scanMethod && !recorded}<div>
              <dt>Scan method</dt>
              <dd>{scanMethod}</dd>
            </div>{/if}
          {#if indexSource?.observation}
            <div>
              <dt>Voldiq version</dt>
              <dd>{indexSource.observation.engine_version}</dd>
            </div>
            <div>
              <dt>Accounting revision</dt>
              <dd>{indexSource.observation.accounting_version}</dd>
            </div>
          {/if}
          {#if indexSource && indexSource.exclusions > 0}<div>
              <dt>Configured exclusions</dt>
              <dd>{indexSource.exclusions.toLocaleString()}</dd>
            </div>{/if}
        </dl>
      </details>
      <div class="index-measurement">
        {#if measured}<time
            datetime={measured.toISOString()}
            title={date(measured.getTime() / 1000)}
            >Measured {measurementDate.format(measured)}</time
          ><span>{observationAge}</span>
        {:else}<span>Measurement date unavailable</span>{/if}
      </div>
    {/if}
  </section>
{/snippet}

<div class="section-navigation overview-sections" role="group" aria-label="Overview sections">
  <button
    aria-pressed={section === 'summary'}
    onclick={() => {
      section = 'summary';
    }}>Summary</button
  >
  <button
    aria-pressed={section === 'applications'}
    disabled={indexId === null || !directory}
    onclick={() => {
      section = 'applications';
    }}>Applications</button
  >
  <button aria-pressed={section === 'libraries'} onclick={() => (section = 'libraries')}
    >Libraries</button
  >
  <button aria-pressed={section === 'cost'} onclick={() => (section = 'cost')}>Storage cost</button>
</div>
<div hidden={section !== 'cost'}>
  <StorageCost
    folder={!paused && queryState === 'ready' && directory?.entry.id === scopeId ? directory : null}
  />
</div>
{#if section === 'applications'}
  {#if indexId !== null && directory && directory.entry.id === scopeId}
    <Applications
      {manageable}
      {onreviewcache}
      {indexId}
      scope={scopeId}
      scopeName={directory.path}
      filters={appFilters}
      paused={paused || queryState === 'loading' || searchState === 'loading'}
      onopen={onapplication}
    />
  {:else}
    <p role="status">Waiting for the selected folder context.</p>
    <button onclick={onretry} disabled={paused}>Reload folder context</button>
  {/if}
{:else if section === 'libraries'}
  <Libraries
    {indexId}
    scope={scopeId}
    paused={paused || queryState !== 'ready'}
    bind:manual={manualLibraries}
    {onopen}
  />
{:else if section === 'summary'}
  <div class="overview-grid">
    <aside class="overview-summary" aria-label="Storage summary">
      {#if directory && directory.entry.id !== 0}
        {@render indexedSummary()}
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
                style:background={`conic-gradient(from -90deg, var(--chart-blue) 0%, var(--chart-violet) ${usedPercent}%, var(--paper) ${usedPercent}% 100%)`}
              >
                <div>
                  <strong>{usedPercent.toFixed(0)}<small>%</small></strong><span>used</span>
                </div>
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
        {@render indexedSummary()}
      {/if}
    </aside>
    <section class="type-panel">
      <div class="panel-heading">
        <div>
          <h2>File types by size</h2>
          <p class="sr-only">
            {bytes(types.total_bytes)} logical · {types.total_files.toLocaleString()} files
          </p>
        </div>
      </div>
      <div class="type-distribution">
        <div class="type-ring" aria-hidden="true" style:background={typePaint}>
          <div>
            <strong>{bytes(types.total_bytes)}</strong><span>File logical size</span><small
              >{types.total_files.toLocaleString()} files</small
            >
          </div>
        </div>
        <div class="type-legend">
          {#each distribution.items as item, index (item.extension)}<button
              onclick={() => ontype(item)}
            >
              <span class="item-swatch" style:background={typeColors[index]}></span>
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
              <span class="item-swatch" style:background="var(--strong)"></span>
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
      </div>
    </section>
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

    <section class="largest-panel">
      <div class="panel-heading">
        <h2>Largest items</h2>
        <button
          class="quiet-action"
          disabled={!ranking.length}
          onclick={() => onranklist(rankedKind)}>Browse ranking</button
        >
      </div>
      <div class="section-navigation ranking-controls" role="group" aria-label="Ranked item type">
        <button aria-pressed={rankedKind === 'file'} onclick={() => (rankedKind = 'file')}>
          Files
        </button>
        <button aria-pressed={rankedKind === 'directory'} onclick={() => (rankedKind = 'directory')}>
          Folders
        </button>
      </div>
      {#each ranking as { entry, path } (entry.id)}
        <button
          class="ranking-row"
          class:active={selected?.entry.id === entry.id}
          aria-label={`${path}, ${bytes(entry.logical)}. Open in Explorer${explored.has(entry.id) ? ', selected for inspection' : ''}.`}
          class:explored={explored.has(entry.id)}
          style:--item-color={categoryPaint({
            id: entry.id,
            name: entry.name,
            value: entry.logical,
            directory: rankedKind === 'directory',
          }).fill}
          onclick={(event) => {
            const operation = selectionOperation(event);
            if (operation === 'replace') onopenranked(entry.id, event.detail > 0);
            else onselect(entry.id, operation);
          }}
          onkeydown={(event) => {
            if (event.key === ' ' && (event.ctrlKey || event.metaKey)) {
              event.preventDefault();
              onselect(entry.id, 'toggle');
            }
          }}
        >
          <span class="item-swatch"></span>
          <span class="ranking-name" title={path}>{entry.name}</span><strong
            >{bytes(entry.logical)}</strong
          >
          <span class="ranking-track" aria-hidden="true"
            ><span
              style:width={`${rankingMaximum > 0 ? (entry.logical / rankingMaximum) * 100 : 0}%`}
            ></span></span
          >
        </button>
      {:else}
        <p class="ranking-empty">No indexed {rankedKind === 'file' ? 'files' : 'folders'} in this scope.</p>
      {/each}
    </section>
    {#if indexId !== null}
      <details class="overview-diagnostics">
        <summary>Space & drive diagnostics</summary>
        <div>
          <SpaceAccount {indexId} {paused} /><SystemStorage {indexId} {paused} /><DeviceHealth
            {indexId}
            {paused}
          /><RecoveryCopies {indexId} {paused} />
        </div>
      </details>
    {/if}
  </div>
{/if}

<style>
  .overview-sections {
    margin-bottom: 26px;
  }
  .ranking-controls {
    margin-bottom: 6px;
  }
  .ranking-empty {
    color: var(--secondary);
    padding: 12px 0;
  }
  .index-source,
  .index-measurement {
    color: var(--secondary);
    font-size: 12px;
  }
  .index-source {
    margin-top: 10px;
  }
  .index-source summary {
    cursor: pointer;
    width: fit-content;
    max-width: 100%;
  }
  .index-source dl {
    margin: 7px 0 0;
    gap: 5px;
  }
  .index-source dl > div {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    column-gap: 8px;
  }
  .index-measurement {
    display: flex;
    flex-wrap: wrap;
    column-gap: 9px;
    margin-top: 5px;
  }
</style>
