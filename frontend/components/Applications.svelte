<script lang="ts">
  import { tick, untrack } from 'svelte';
  import type { IndexId } from '../lib/index-context';
  import type { Query } from '../lib/types';
  import { date } from '../lib/types';
  import { applicationQueries } from '../lib/queries';
  import { RANKED_PAINTS } from '../lib/chart-colors';
  import {
    applicationBytes,
    applicationFilters,
    applicationPlot,
    readApplicationPage,
    type ApplicationPage,
    type ApplicationItem,
    type ApplicationQuery,
  } from '../lib/applications';
  let {
    indexId,
    scope,
    scopeName,
    filters,
    paused,
    onopen,
  }: {
    indexId: IndexId;
    scope: number;
    scopeName: string;
    filters: Query;
    paused: boolean;
    onopen: (id: number) => void;
  } = $props();
  type Model =
    | { kind: 'loading' | 'paused' | 'cancelled' }
    | { kind: 'failed'; message: string }
    | { kind: 'ready'; identity: string; page: ApplicationPage };
  let model = $state.raw<Model>({ kind: 'loading' });
  let offset = $state(0),
    sort = $state<ApplicationQuery['sort']>('size'),
    retry = $state(0);
  let active = $state<string | null>(null),
    focusedPoint = $state(0);
  let region: HTMLElement;
  let reload: HTMLButtonElement;
  let cancelRead = () => {};
  const applied = $derived(applicationFilters(filters));
  const context = $derived(JSON.stringify([indexId, scope, applied]));
  const identity = $derived(JSON.stringify([context, offset, sort]));
  const page = $derived(
    !paused && model.kind === 'ready' && model.identity === identity ? model.page : null,
  );
  const plot = $derived(applicationPlot(page?.chart ?? []));
  const selected = $derived(plot.points.find((point) => point.app.id === active)?.app ?? null);
  const paints = (app: ApplicationItem) =>
    RANKED_PAINTS[parseInt(app.id.slice(0, 2), 16) % RANKED_PAINTS.length];
  const percent = (value: string) =>
    page && BigInt(page.total.logical_bytes) > 0n
      ? Number((BigInt(value) * 10000n) / BigInt(page.total.logical_bytes)) / 100
      : 0;
  $effect(() => {
    void context;
    untrack(() => {
      offset = 0;
      active = null;
      focusedPoint = 0;
    });
  });
  $effect(() => {
    void retry;
    const owner = identity;
    if (paused) {
      model = { kind: 'paused' };
      return;
    }
    const input = { indexId, query: { scope, offset, limit: 50, sort, filters: applied } };
    const heldFocus = region?.contains(document.activeElement);
    model = { kind: 'loading' };
    const request = applicationQueries.request(input);
    cancelRead = request.cancel;
    let alive = true;
    void (async () => {
      try {
        const value = await request.result;
        if (!alive) return;
        if (value === null) model = { kind: 'cancelled' };
        else {
          const result = readApplicationPage(value);
          if (result.scope !== input.query.scope)
            throw Error('Application scope changed. Reload to retry.');
          model = { kind: 'ready', identity: owner, page: result };
          active = null;
          focusedPoint = 0;
        }
      } catch (problem) {
        if (alive) model = { kind: 'failed', message: String(problem) };
      } finally {
        await tick();
        if (alive && heldFocus && document.activeElement === document.body)
          reload?.focus({ preventScroll: true });
      }
    })();
    return () => {
      alive = false;
      request.cancel();
    };
  });
  function explore(app: ApplicationItem) {
    if (model.kind === 'ready' && model.identity === identity && !paused) onopen(app.root);
  }
  function movePoint(event: KeyboardEvent, index: number) {
    let next = index;
    if (event.key === 'ArrowRight' || event.key === 'ArrowUp') next++;
    else if (event.key === 'ArrowLeft' || event.key === 'ArrowDown') next--;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = plot.points.length - 1;
    else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      explore(plot.points[index].app);
      return;
    } else return;
    event.preventDefault();
    focusedPoint = Math.max(0, Math.min(plot.points.length - 1, next));
    if (event.currentTarget instanceof SVGCircleElement)
      event.currentTarget.ownerSVGElement
        ?.querySelectorAll<SVGCircleElement>('circle[data-point]')
        [focusedPoint]?.focus();
  }
</script>

<section class="applications" aria-labelledby="applications-heading" bind:this={region}>
  <header class="application-heading">
    <div>
      <h2 id="applications-heading">Applications by indexed size</h2>
      <p>{scopeName} · logical file-path totals</p>
    </div>
    <div class="application-controls">
      <label
        >Sort <select bind:value={sort}
          ><option value="size">Size</option><option value="files">File paths</option><option
            value="name">Name</option
          ></select
        ></label
      >
      <button
        bind:this={reload}
        disabled={paused}
        onclick={() => {
          retry++;
        }}>Reload</button
      >
      {#if model.kind === 'loading'}<button onclick={() => cancelRead()}>Cancel read</button>{/if}
    </div>
  </header>
  <p class="application-caption">
    Registered install locations matched to this index. Filters match file records, not registration
    names; data, caches and logs are not classified by this view.
  </p>
  {#if model.kind === 'loading'}<p class="application-state" role="status">
      Reading registration metadata and indexed sizes…
    </p>
  {:else if model.kind === 'paused'}<p class="application-state">
      Available after the current operation.
    </p>
  {:else if model.kind === 'cancelled'}<p class="application-state" role="status">
      Request cancelled. Reload to retry.
    </p>
  {:else if model.kind === 'failed'}<p class="application-state application-error" role="alert">
      {model.message}
    </p>
  {:else if page}
    <dl class="application-totals">
      {#each [{ label: 'One application claim', count: page.exclusive }, { label: 'Overlapping claims', count: page.overlapping }, { label: 'Unattributed', count: page.unattributed }, { label: 'Matching total', count: page.total }] as group}
        <div>
          <dt>{group.label}</dt>
          <dd title={`${group.count.logical_bytes} logical bytes`}>
            {applicationBytes(group.count.logical_bytes)}
          </dd>
          <dd class="application-path-count">{group.count.files.toLocaleString()} file paths</dd>
        </div>
      {/each}
    </dl>
    {#if page.partial_index}<p class="application-notice">
        Partial or provisional index. Totals describe observed records, not the whole disk.
      </p>{/if}
    <section class="application-chart" aria-labelledby="application-chart-heading">
      <div class="chart-heading">
        <h3 id="application-chart-heading">Exclusive attribution · size and file count</h3>
        <span>{plot.points.length} points · logarithmic axes</span>
      </div>
      {#if plot.points.length}
        <svg
          viewBox="0 0 900 280"
          aria-labelledby="application-chart-heading application-chart-help"
        >
          <desc id="application-chart-help"
            >Each point is one available application claim. Use the arrow keys between points, Enter
            to explore its indexed location. The list below provides the same measurements.
            Positions use log of one plus the measured value.</desc
          >
          {#each [0, 1, 2, 3, 4] as line}
            <line x1="70" x2="880" y1={30 + line * 51.25} y2={30 + line * 51.25} />
          {/each}
          <text x="70" y="18">Logical bytes</text><text x="62" y="33" text-anchor="end"
            >{applicationBytes(plot.maxBytes)}</text
          >
          <text x="62" y="238" text-anchor="end">0 B</text><text x="70" y="255">0</text>
          <text x="880" y="255" text-anchor="end">{plot.maxFiles.toLocaleString()}</text><text
            x="475"
            y="275"
            text-anchor="middle">Indexed file paths</text
          >
          {#each plot.points as point, index (point.app.id)}
            <circle
              data-point
              cx={point.x}
              cy={point.y}
              r={active === point.app.id ? 7 : 5}
              fill={paints(point.app).fill}
              role="button"
              tabindex={focusedPoint === index ? 0 : -1}
              aria-label={`${point.app.name}: ${point.app.exclusive.files} file paths, ${point.app.exclusive.logical_bytes} logical bytes. Explore indexed location.`}
              onpointerenter={() => {
                active = point.app.id;
              }}
              onfocus={() => {
                active = point.app.id;
                focusedPoint = index;
              }}
              onkeydown={(event) => movePoint(event, index)}
              onclick={() => explore(point.app)}
            >
              <title
                >{point.app.name} · {applicationBytes(point.app.exclusive.logical_bytes)} · {point.app.exclusive.files.toLocaleString()}
                paths</title
              >
            </circle>
          {/each}
        </svg>
        <div class="application-tooltip" aria-live="polite">
          {#if selected}<strong>{selected.name}</strong><span
              >{selected.exclusive.files.toLocaleString()} paths · {applicationBytes(
                selected.exclusive.logical_bytes,
              )}</span
            >{:else}<span>Point to or focus an application to see its measurements.</span>{/if}
        </div>
      {:else}<p class="application-state">No exclusive application measurements to plot.</p>{/if}
      {#if page.chart_omitted || plot.omittedZero}<p class="application-caption">
          Largest 200 available matches at most. {page.chart_omitted} outside the chart selection; {plot.omittedZero}
          with no positive exclusive bytes. Overlapping claims remain in the list.
        </p>{/if}
    </section>
    <div class="application-list-heading">
      <h3>Registered location claims</h3>
      <span role="status">{page.application_count.toLocaleString()} with matching file paths</span>
    </div>
    {#if !page.application_count}<p class="application-state">
        No matching indexed file paths could be linked to the available registrations. This is not
        an installed-app inventory.
      </p>{/if}
    <ul class="application-list">
      {#each page.applications as app (app.id)}
        <li>
          <span
            class="application-key"
            aria-hidden="true"
            style:background={paints(app).fill}
            style:color={paints(app).ink}>{Array.from(app.name.trim())[0]}</span
          >
          <div class="application-name">
            <strong>{app.name}</strong><span
              >{app.publisher ?? 'Publisher unavailable'}{app.version
                ? ` · ${app.version}`
                : ''}</span
            >
            {#if app.overlapping.files}<small
                >{applicationBytes(app.overlapping.logical_bytes)} in overlapping claims · not additive
                across applications</small
              >{/if}
          </div>
          <div class="application-value">
            <strong title={`${app.exclusive.logical_bytes} logical bytes`}
              >{applicationBytes(app.exclusive.logical_bytes)}</strong
            ><span>{app.exclusive.files.toLocaleString()} exclusive paths</span>
            <span class="application-track" aria-hidden="true"
              ><span
                style:width={`${percent(app.exclusive.logical_bytes)}%`}
                style:background={paints(app).fill}
              ></span></span
            >
          </div>
          <button
            onclick={() => explore(app)}
            disabled={paused}
            title="Explore the indexed portion of this registered location">Explore</button
          >
        </li>
      {/each}
    </ul>
    <div class="application-pagination">
      <button
        disabled={offset === 0}
        onclick={() => {
          offset = Math.max(0, offset - 50);
        }}>Previous</button
      ><span
        >{page.applications.length
          ? `${offset + 1}–${offset + page.applications.length} of ${page.application_count}`
          : page.application_count
            ? 'No matches on this page'
            : '0 matches'}</span
      ><button
        disabled={offset + 50 >= page.application_count}
        onclick={() => {
          offset += 50;
        }}>Next</button
      >
    </div>
    {#if offset > 0 && !page.applications.length}<button
        onclick={() => {
          offset = 0;
        }}>Return to first page</button
      >{/if}
    <details class="application-evidence">
      <summary>Evidence and coverage</summary>
      <p>
        Registration claims are contextual evidence, not proof of ownership, recoverability or
        permission to clean. Shared claims are counted once in the matching total. These are logical
        path bytes; per-application physical allocation is unavailable.
      </p>
      <p>
        Index measurement: {date(page.indexed_at)}. Registration read: {date(
          page.registrations_started_at,
        )}–{date(page.registrations_observed_at)}. Registration reads are not an atomic inventory.
      </p>
      <p>
        {page.excluded_roots} recorded exclusion roots · {page.unbound_records} registration records with
        an outside or unavailable location · {page.duplicate_records} duplicate registration records merged.
      </p>
      {#each page.coverage as source}<p>
          {source.source.replaceAll('_', ' ')} · {source.complete
            ? 'Enumeration complete'
            : 'Incomplete'} · {source.missing_location} missing install locations · {source.rejected}
          unsupported/unavailable records{source.problem ? ` · ${source.problem}` : ''}
        </p>{/each}
      {#if !page.coverage.length}<p>
          Source coverage is unavailable for these supplied claims.
        </p>{/if}
    </details>
  {/if}
</section>

<style>
  .applications {
    min-width: 0;
  }
  .application-heading,
  .application-controls,
  .chart-heading,
  .application-list-heading,
  .application-pagination {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }
  h2 {
    font-size: 18px;
    letter-spacing: -0.02em;
  }
  h3 {
    font-size: 12px;
    font-weight: 600;
  }
  .application-heading p,
  .application-caption,
  .application-controls label,
  .chart-heading > span,
  .application-list-heading > span,
  .application-pagination > span {
    color: var(--secondary);
    font-size: 12px;
  }
  .application-heading p {
    margin-top: 5px;
    overflow-wrap: anywhere;
  }
  .application-caption {
    margin: 12px 0;
    line-height: 1.5;
  }
  .application-controls {
    justify-content: flex-start;
  }
  .application-controls label {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .application-totals {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    border-block: 1px solid var(--border);
    margin: 18px 0;
    padding: 16px 0;
    gap: 18px;
  }
  .application-totals dt {
    color: var(--secondary);
    font-size: 12px;
  }
  .application-totals dd {
    margin: 6px 0;
    font-size: 23px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .application-totals .application-path-count {
    color: var(--muted);
    font-size: 11px;
    font-weight: 400;
  }
  .application-state {
    color: var(--secondary);
    padding: 24px 0;
    font-size: 13px;
  }
  .application-error {
    color: var(--error);
    overflow-wrap: anywhere;
  }
  .application-notice {
    font-size: 12px;
    padding: 10px 12px;
    border-left: 3px solid var(--strong);
    background: var(--surface);
  }
  .application-chart {
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 16px;
    margin-block: 18px;
    background: var(--surface);
  }
  svg {
    display: block;
    width: 100%;
    min-height: 190px;
    margin-top: 14px;
    overflow: visible;
  }
  svg line {
    stroke: var(--border);
    stroke-dasharray: 2 5;
  }
  svg text {
    fill: var(--secondary);
    font:
      11px 'Segoe UI',
      sans-serif;
  }
  circle {
    cursor: pointer;
    stroke: var(--secondary);
    stroke-width: 1.25px;
  }
  circle:focus-visible {
    outline: none;
    stroke: var(--text);
    stroke-width: 3px;
  }
  .application-tooltip {
    min-height: 24px;
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    font-size: 12px;
  }
  .application-tooltip span {
    color: var(--secondary);
  }
  .application-list-heading {
    margin: 22px 0 12px;
  }
  .application-list {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .application-list li {
    display: grid;
    grid-template-columns: 36px minmax(0, 1fr) minmax(140px, 180px) auto;
    align-items: center;
    gap: 12px;
    padding: 14px 0;
    border-bottom: 1px solid var(--border);
  }
  .application-key {
    width: 34px;
    height: 34px;
    display: grid;
    place-items: center;
    font-size: 16px;
    border-radius: 8px;
    font-weight: 600;
  }
  .application-name,
  .application-value {
    display: flex;
    flex-direction: column;
    gap: 5px;
    min-width: 0;
  }
  .application-name strong {
    font-size: 13px;
    overflow-wrap: anywhere;
  }
  .application-name > span,
  .application-name small,
  .application-value > span {
    font-size: 11px;
    color: var(--secondary);
    overflow-wrap: anywhere;
  }
  .application-value {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .application-value strong {
    font-size: 14px;
  }
  .application-track {
    height: 4px;
    background: var(--paper);
    border-radius: 2px;
    overflow: hidden;
  }
  .application-track > span {
    height: 100%;
    display: block;
  }
  .application-pagination {
    margin-top: 16px;
    justify-content: flex-end;
  }
  .application-evidence {
    margin-top: 22px;
    color: var(--secondary);
    font-size: 12px;
  }
  .application-evidence summary {
    cursor: pointer;
    padding-block: 8px;
  }
  .application-evidence p {
    margin-top: 8px;
    line-height: 1.5;
    overflow-wrap: anywhere;
  }
  @media (max-width: 1000px) {
    .application-totals {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .application-list li {
      grid-template-columns: 34px minmax(0, 1fr) auto;
    }
    .application-value {
      grid-column: 2;
      text-align: left;
    }
    .application-list li > button {
      grid-column: 3;
      grid-row: 1 / span 2;
    }
  }
</style>
