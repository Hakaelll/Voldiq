<script lang="ts">
  import Treemap from './Treemap.svelte';
  import {
    bytes,
    type Details,
    type FileType,
    type FileTypes,
    type Page,
    type Volume,
  } from '../lib/types';
  import { color, type WeightedNode } from '../lib/treemap';
  import { visibleFileTypes } from '../lib/file-types';

  let {
    volume,
    directory,
    children,
    nodes,
    types,
    selected,
    sizeMode,
    onmetric,
    onselect,
    onopen,
    onlist,
    ontype,
  }: {
    volume: Volume | null | undefined;
    directory: Details | null;
    children: Page;
    nodes: WeightedNode[];
    types: FileTypes;
    selected: Details | null;
    sizeMode: string;
    onmetric: (metric: string) => void;
    onselect: (id: number) => void;
    onopen: (id: number) => void;
    onlist: () => void;
    ontype: (item: FileType) => void;
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
          onclick={() => onselect(entry.id)}
          ondblclick={() => onopen(entry.id)}
        >
          <span
            class="item-swatch"
            style:background={nodes[position]?.paint?.fill ?? color(entry.name)}
          ></span>
          <span title={entry.name}>{entry.name}</span><strong>{bytes(entry.logical)}</strong>
        </button>
      {/each}
    </section>
  </aside>
  <div class="overview-main">
    <section class="map-panel" aria-busy={!directory}>
      <div class="panel-heading">
        <h2>Storage map</h2>
        <div class="map-controls">
          <select
            aria-label="Treemap size metric"
            value={sizeMode}
            onchange={(event) => onmetric(event.currentTarget.value)}
          >
            <option value="logical">Logical size</option><option value="allocated"
              >Unique allocation</option
            >
          </select>
          <span class="map-mode" aria-current="true">Map</span><button onclick={onlist}>List</button
          >
        </div>
      </div>
      {#if directory}<Treemap {nodes} selected={selected?.entry.id ?? null} {onselect} {onopen} />
      {:else}<div class="overview-loading" role="status">Loading…</div>{/if}
      {#if directory}<div class="map-caption">
          <span title={selected?.path || directory.path}>{selected?.path || directory.path}</span>
          <span
            >{bytes(
              sizeMode === 'allocated'
                ? (selected?.entry.allocated ?? directory.entry.allocated)
                : (selected?.entry.logical ?? directory.entry.logical),
            )}</span
          >
        </div>{/if}
    </section>
    <section class="type-panel">
      <div class="panel-heading"><h2>File types</h2></div>
      <div class="type-bar">
        {#each distribution.items as item}<span
            style:flex-grow={item.bytes}
            style:background={color(item.extension ?? '<no-extension>')}
            title={`${item.extension === null ? 'No extension' : `.${item.extension}`}: ${bytes(item.bytes)}`}
          ></span>{/each}
        {#if distribution.otherBytes > 0}<span
            style:flex-grow={distribution.otherBytes}
            style:background="var(--muted)"
            title={`Other: ${bytes(distribution.otherBytes)}`}
          ></span>{/if}
      </div>
      <div class="type-legend">
        {#each distribution.items as item}<button onclick={() => ontype(item)}>
            <span class="item-swatch" style:background={color(item.extension ?? '<no-extension>')}
            ></span>
            <strong title={item.extension === null ? 'No extension' : `.${item.extension}`}
              >{item.extension === null ? 'No extension' : `.${item.extension}`}</strong
            >
            <span>{bytes(item.bytes)}</span><small>{item.count.toLocaleString()} files</small>
          </button>{/each}
        {#if distribution.otherFiles > 0}<div class="type-other">
            <span class="item-swatch" style:background="var(--muted)"></span>
            <strong>Other</strong><span>{bytes(distribution.otherBytes)}</span><small
              >{distribution.otherFiles.toLocaleString()} files</small
            >
          </div>{/if}
      </div>
    </section>
  </div>
</div>
