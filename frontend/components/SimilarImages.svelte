<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { TraySource } from '../lib/cleanup-tray';
  import type { IndexId } from '../lib/index-context';
  import {
    imageComparisonQueries,
    similarImageQueries,
    type ImageComparison,
    type SimilarImagesReport,
    type SimilarPair,
  } from '../lib/queries';
  import { bytes, date, type NodeId } from '../lib/types';

  let {
    indexId,
    scope,
    enabled,
    onselect,
    onadd,
  }: {
    indexId: IndexId;
    scope: NodeId;
    enabled: boolean;
    onselect: (id: NodeId) => void;
    onadd: (source: TraySource, ids: readonly NodeId[], owner: IndexId | null) => boolean;
  } = $props();

  type ReadState<T> =
    | { kind: 'idle' | 'loading' | 'cancelled' }
    | { kind: 'failed'; message: string }
    | { kind: 'ready'; value: T };

  let threshold = $state(8);
  let appliedThreshold = $state(8);
  let search = $state.raw<ReadState<SimilarImagesReport>>({ kind: 'idle' });
  let preview = $state.raw<ReadState<ImageComparison>>({ kind: 'idle' });
  let pair = $state.raw<SimilarPair | null>(null);
  let selected = $state<NodeId[]>([]);
  let zoom = $state(100);
  let owner = 0;
  let cancelRead = () => {};

  function stop(): void {
    owner++;
    cancelRead();
    if (search.kind === 'loading') search = { kind: 'cancelled' };
    if (preview.kind === 'loading') preview = { kind: 'cancelled' };
  }

  async function find(): Promise<void> {
    if (!enabled || search.kind === 'loading') return;
    stop();
    const token = owner;
    appliedThreshold = threshold;
    search = { kind: 'loading' };
    preview = { kind: 'idle' };
    pair = null;
    selected = [];
    const request = similarImageQueries.request({ indexId, scope, maxDistance: appliedThreshold });
    cancelRead = request.cancel;
    try {
      const value = await request.result;
      if (owner === token) search = value ? { kind: 'ready', value } : { kind: 'cancelled' };
    } catch (error) {
      if (owner === token) search = { kind: 'failed', message: String(error) };
    }
  }

  async function compare(next: SimilarPair): Promise<void> {
    if (!enabled) return;
    stop();
    const token = owner;
    pair = next;
    selected = [];
    preview = { kind: 'loading' };
    const request = imageComparisonQueries.request({ indexId, scope, ids: next.ids });
    cancelRead = request.cancel;
    try {
      const value = await request.result;
      if (owner === token) preview = value ? { kind: 'ready', value } : { kind: 'cancelled' };
    } catch (error) {
      if (owner === token) preview = { kind: 'failed', message: String(error) };
    }
  }

  $effect(() => {
    if (!enabled) stop();
  });
  onDestroy(stop);
</script>

<section class="similar-images" aria-label="Similar images">
  <div class="filter-bar similar-toolbar">
    <label>
      Maximum distance <output>{threshold} / 64</output>
      <input
        type="range"
        min="0"
        max="16"
        step="1"
        bind:value={threshold}
        aria-describedby="visual-distance-help"
      />
    </label>
    <button class="primary" disabled={!enabled || search.kind === 'loading'} onclick={find}>
      Find similar images
    </button>
    {#if search.kind === 'loading' || preview.kind === 'loading'}
      <button onclick={stop}>Cancel</button>
    {/if}
  </div>
  <p id="visual-distance-help" class="similar-note">
    Compare local PNG and JPEG files in the current folder and its subfolders. A lower distance
    means fewer differences between 64 brightness comparisons. A distance of 0 can still describe
    different images.
  </p>

  {#if search.kind === 'loading'}
    <p role="status">Analyzing images…</p>
  {:else if search.kind === 'cancelled'}
    <p role="status">Image analysis cancelled.</p>
  {:else if search.kind === 'failed'}
    <p role="alert">{search.message}</p>
  {:else if search.kind === 'ready'}
    {@const report = search.value}
    <div class="similar-report">
      <p role="status">
        <strong>{report.matching_pairs.toLocaleString()} visual pairs</strong>
        · {report.compared_images.toLocaleString()} images compared · threshold {appliedThreshold} / 64
        {#if report.matching_pairs > report.pairs.length}
          · showing closest {report.pairs.length}{/if}
      </p>
      <small title={report.scope}>{report.scope}</small>
      {#if report.incomplete_scope || report.limit || report.unverified_branches}
        <p class="similar-note">
          Partial coverage.
          {#if report.incomplete_scope}The scan has unobserved entries.{/if}
          {#if report.unverified_branches}{report.unverified_branches} unverified branches.{/if}
          {#if report.limit === 'files'}The 512-file limit was reached.{/if}
          {#if report.limit === 'bytes'}The 256 MiB input limit was reached.{/if}
        </p>
      {/if}
      {#if report.unavailable_images || report.protected_images || report.repeated_images}
        <p class="similar-note">
          {report.unavailable_images} unavailable or outside decoding limits · {report.protected_images}
          protected · {report.repeated_images} repeated identities or bytes skipped
        </p>
      {/if}
    </div>
    {#if report.pairs.length > 0}
      <div class="similar-layout">
        <section aria-label="Matching image pairs" class="similar-pairs">
          <h2>Pairs</h2>
          <ul>
            {#each report.pairs as match (`${match.ids[0]}/${match.ids[1]}`)}
              <li>
                <button
                  aria-pressed={pair === match}
                  disabled={!enabled}
                  onclick={() => compare(match)}
                >
                  <span><strong>{match.names[0]}</strong><small>{match.names[1]}</small></span>
                  <span class="number" title="Visual distance">{match.distance} / 64</span>
                </button>
              </li>
            {/each}
          </ul>
        </section>
        <section aria-label="Image comparison" class="similar-comparison">
          <div class="compare-heading">
            <h2>Compare images</h2>
            {#if preview.kind === 'ready'}
              <label
                >Zoom
                <select bind:value={zoom}>
                  <option value={100}>Fit</option>
                  <option value={150}>150%</option>
                  <option value={200}>200%</option>
                </select>
              </label>
            {/if}
          </div>
          {#if preview.kind === 'loading'}
            <p class="table-empty" role="status">Loading previews…</p>
          {:else if preview.kind === 'failed'}
            <p role="alert">{preview.message}</p>
          {:else if preview.kind === 'cancelled'}
            <p class="table-empty" role="status">Preview cancelled. Choose a pair to retry.</p>
          {:else if preview.kind === 'ready'}
            <p class="similar-note">
              Visual distance {preview.value.distance} / 64 · Select files manually for review.
            </p>
            <div class="compare-images">
              {#each preview.value.images as image (image.id)}
                <figure>
                  <!-- svelte-ignore a11y_no_noninteractive_tabindex (The zoomed preview scrolls.) -->
                  <div
                    class="photo-viewport"
                    tabindex="0"
                    role="region"
                    aria-label={`Preview of ${image.name}`}
                  >
                    <img
                      src={`data:image/png;base64,${image.png}`}
                      alt={image.name}
                      style:width={`${zoom}%`}
                      style:height={`${zoom}%`}
                    />
                  </div>
                  <figcaption>
                    <strong title={image.name}>{image.name}</strong>
                    <small title={image.path}>{image.path}</small>
                    <span>{image.width} × {image.height} px · {bytes(image.logical)}</span>
                    <span>Modified {date(image.modified)}</span>
                    <div class="compare-actions">
                      <label>
                        <input
                          type="checkbox"
                          checked={selected.includes(image.id)}
                          disabled={!enabled}
                          aria-label={`Select ${image.name} for review`}
                          onchange={(event) => {
                            selected = event.currentTarget.checked
                              ? [...selected, image.id]
                              : selected.filter((id) => id !== image.id);
                          }}
                        />
                        Select for review
                      </label>
                      <button class="quiet-action" onclick={() => onselect(image.id)}
                        >Inspect file</button
                      >
                    </div>
                  </figcaption>
                </figure>
              {/each}
            </div>
            <div class="selection-bar">
              <span>{selected.length} selected</span>
              <button
                class="primary"
                disabled={!enabled || !selected.length}
                onclick={() => {
                  if (onadd({ kind: 'manual' }, selected, indexId)) selected = [];
                }}>Add to review tray</button
              >
            </div>
          {:else}
            <p class="table-empty">Choose a pair to compare its images.</p>
          {/if}
        </section>
      </div>
    {:else}
      <p class="table-empty">
        {report.compared_images === 0
          ? 'No supported local images could be compared in this folder.'
          : 'No pairs matched in the images compared.'}
      </p>
    {/if}
  {/if}
</section>

<style>
  .similar-images {
    padding-block: 10px;
  }
  .similar-toolbar {
    flex-wrap: wrap;
  }
  .similar-toolbar label {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .similar-toolbar input {
    width: 112px;
    min-width: 0;
    accent-color: var(--accent);
  }
  .similar-toolbar output {
    font-variant-numeric: tabular-nums;
  }
  .similar-note,
  small {
    color: var(--muted);
  }
  .similar-note {
    margin: 8px 0 12px;
  }
  .similar-report {
    padding-block: 12px;
    border-top: 1px solid var(--border);
  }
  .similar-report p {
    margin: 0 0 5px;
  }
  .similar-report small {
    overflow-wrap: anywhere;
  }
  .similar-layout {
    display: grid;
    grid-template-columns: minmax(150px, 23%) minmax(0, 1fr);
    gap: 20px;
  }
  .similar-pairs ul {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: min(440px, 60vh);
    overflow: auto;
  }
  .similar-pairs button {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    width: 100%;
    text-align: left;
    padding: 8px;
    border: 0;
    border-bottom: 1px solid var(--border);
    background: transparent;
  }
  .similar-pairs button:hover,
  .similar-pairs button[aria-pressed='true'] {
    background: var(--selected);
  }
  .similar-pairs button > span:first-child {
    min-width: 0;
  }
  .similar-pairs strong,
  .similar-pairs small {
    display: block;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .similar-pairs .number {
    white-space: nowrap;
  }
  .compare-heading,
  .compare-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
  }
  .compare-heading select {
    margin-inline-start: 4px;
  }
  .compare-images {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
  }
  figure {
    margin: 0;
    min-width: 0;
  }
  .photo-viewport {
    height: clamp(150px, 37vh, 420px);
    background: var(--paper);
    overflow: auto;
  }
  .photo-viewport img {
    display: block;
    object-fit: contain;
  }
  figcaption {
    display: grid;
    gap: 3px;
    padding-block: 8px;
    overflow-wrap: anywhere;
  }
  .compare-actions label {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .compare-actions input {
    accent-color: var(--accent);
  }
  @media (max-width: 1050px) {
    .similar-layout {
      grid-template-columns: minmax(0, 1fr);
      gap: 12px;
    }
    .similar-pairs ul {
      max-height: 140px;
    }
  }
</style>
