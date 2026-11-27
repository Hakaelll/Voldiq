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
    applicationPointInDirection,
    readApplicationPage,
    type ApplicationPage,
    type ApplicationItem,
    type ApplicationQuery,
    type ApplicationCategory,
    type ApplicationCleanupContext,
    canReviewApplicationCache,
  } from '../lib/applications';
  let {
    indexId,
    scope,
    scopeName,
    filters,
    paused,
    onopen,
    manageable,
    onreviewcache,
  }: {
    indexId: IndexId;
    scope: number;
    scopeName: string;
    filters: Query;
    paused: boolean;
    onopen: (id: string) => void;
    manageable: boolean;
    onreviewcache: (context: ApplicationCleanupContext) => void;
  } = $props();
  type Model =
    | { kind: 'loading' | 'paused' | 'cancelled' }
    | { kind: 'failed'; message: string }
    | { kind: 'ready'; identity: string; page: ApplicationPage };
  let model = $state.raw<Model>({ kind: 'loading' });
  let offset = $state(0),
    sort = $state<ApplicationQuery['sort']>('size'),
    retry = $state(0);
  let choice = $state.raw<{ context: string; id: string } | null>(null);
  let hoveredPoint = $state<string | null>(null),
    focusedPoint = $state<string | null>(null),
    rovingPoint = $state<string | null>(null);
  let region: HTMLElement;
  let reload: HTMLButtonElement;
  let plotWidth = $state(0);
  let selectionHeading = $state<HTMLHeadingElement>();
  let pageFocus: { owner: string; direction: 'previous' | 'next' } | null = null;
  let cancelRead = () => {};
  const applied = $derived(applicationFilters(filters));
  const context = $derived(JSON.stringify([indexId, scope, applied]));
  const identity = $derived(JSON.stringify([context, offset, sort]));
  const page = $derived(
    !paused && model.kind === 'ready' && model.identity === identity ? model.page : null,
  );
  const canManageCache = $derived(manageable && page !== null && page.attribution !== 'captured');
  const plot = $derived(applicationPlot(page?.chart ?? [], plotWidth));
  const claims = $derived(
    new Map([...(page?.chart ?? []), ...(page?.applications ?? [])].map((app) => [app.id, app])),
  );
  const selectedId = $derived(choice?.context === context ? choice.id : null);
  const selected = $derived(selectedId ? (claims.get(selectedId) ?? null) : null);
  const marker = $derived(plot.points.find((point) => point.app.id === selectedId));
  const rowSelected = $derived(page?.applications.some((app) => app.id === selectedId));
  const preview = $derived(
    plot.points.find((point) => point.app.id === (focusedPoint ?? hoveredPoint))?.app,
  );
  const tabPoint = $derived(
    plot.points.find((point) => point.app.id === rovingPoint)?.app.id ??
      marker?.app.id ??
      plot.points[0]?.app.id,
  );
  const atSelectedLocation = $derived(
    marker ? plot.points.filter((point) => point.x === marker.x && point.y === marker.y).length : 0,
  );
  const paints = (app: ApplicationItem) =>
    RANKED_PAINTS[parseInt(app.id.slice(0, 2), 16) % RANKED_PAINTS.length];
  const fileTick = new Intl.NumberFormat(undefined, {
    notation: 'compact',
    maximumFractionDigits: 1,
  });
  const largeTick = new Intl.NumberFormat(undefined, {
    notation: 'scientific',
    maximumFractionDigits: 1,
  });
  const percent = (value: string) =>
    page && BigInt(page.total.logical_bytes) > 0n
      ? Number((BigInt(value) * 10000n) / BigInt(page.total.logical_bytes)) / 100
      : 0;
  const categories: ApplicationCategory[] = ['installation', 'data', 'cache', 'logs', 'unknown'];
  const categoryStyle = {
    installation: { label: 'Installation', paint: RANKED_PAINTS[0] },
    data: { label: 'Profile data', paint: RANKED_PAINTS[4] },
    cache: { label: 'HTTP cache', paint: RANKED_PAINTS[2] },
    logs: { label: 'Logs', paint: RANKED_PAINTS[5] },
    unknown: { label: 'Unknown / ambiguous', paint: RANKED_PAINTS[7] },
  } satisfies Record<ApplicationCategory, { label: string; paint: { fill: string; ink: string } }>;
  $effect(() => {
    void context;
    untrack(() => {
      offset = 0;
      choice = null;
      hoveredPoint = focusedPoint = rovingPoint = null;
    });
  });
  $effect(() => {
    void retry;
    const owner = identity;
    const restoreDirection = pageFocus?.owner === owner ? pageFocus.direction : null;
    pageFocus = null;
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
          hoveredPoint = focusedPoint = null;
        }
      } catch (problem) {
        if (alive) model = { kind: 'failed', message: String(problem) };
      } finally {
        await tick();
        if (!alive || (!heldFocus && !restoreDirection) || document.activeElement !== document.body)
          return;
        const pagination = [
          ...region.querySelectorAll<HTMLButtonElement>(
            '.application-pagination button:not(:disabled)',
          ),
        ];
        const target = pagination.find((button) => button.dataset.pageStep === restoreDirection);
        (restoreDirection ? (target ?? pagination[0] ?? reload) : reload)?.focus({
          preventScroll: true,
        });
      }
    })();
    return () => {
      alive = false;
      request.cancel();
    };
  });
  async function choose(id: string, trigger?: HTMLElement): Promise<void> {
    if (!page || !claims.has(id)) return;
    const owner = context;
    choice = { context: owner, id };
    if (plot.points.some((point) => point.app.id === id)) rovingPoint = id;
    if (!trigger) return;
    await tick();
    if (selected?.id === id && context === owner && document.activeElement === trigger)
      selectionHeading?.focus();
  }
  function explore(id: string): void {
    if (page && claims.has(id)) onopen(id);
  }
  function reviewCache(id: string): void {
    const app = claims.get(id);
    if (canManageCache && app && canReviewApplicationCache(app))
      onreviewcache({ scope, application_id: id });
  }
  function paginate(nextOffset: number, direction: 'previous' | 'next', button: HTMLButtonElement) {
    offset = nextOffset;
    if (document.activeElement === button) pageFocus = { owner: identity, direction };
  }
  function movePoint(event: KeyboardEvent, index: number) {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      const point = plot.points[index];
      if (point) void choose(point.app.id);
      return;
    }
    let target: number;
    if (event.key === 'Home') target = 0;
    else if (event.key === 'End') target = plot.points.length - 1;
    else if (
      event.key === 'ArrowLeft' ||
      event.key === 'ArrowRight' ||
      event.key === 'ArrowUp' ||
      event.key === 'ArrowDown'
    ) {
      target = applicationPointInDirection(plot.points, index, event.key);
    } else return;
    event.preventDefault();
    if (event.currentTarget instanceof SVGCircleElement)
      event.currentTarget.ownerSVGElement
        ?.querySelectorAll<SVGCircleElement>('circle[data-point]')
        [target]?.focus();
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
        >Sort <select
          bind:value={sort}
          onchange={() => {
            offset = 0;
          }}
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
  {#if model.kind === 'loading'}<p class="application-state" role="status">
      Reading application evidence and indexed sizes…
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
    <div class="application-workspace">
      <section class="application-chart" aria-labelledby="application-chart-heading">
        <div class="chart-heading">
          <h3 id="application-chart-heading">Indexed size and file count</h3>
          <span>{plot.points.length.toLocaleString()} plotted · logarithmic axes</span>
        </div>
        {#if plot.points.length}
          <div
            class="application-plot"
            role="region"
            aria-label="Application chart"
            bind:clientWidth={plotWidth}
          >
            <svg
              viewBox={`0 0 ${plot.chart.width} ${plot.chart.height}`}
              height={plot.chart.height}
              aria-labelledby="application-chart-heading application-chart-help"
            >
              <desc id="application-chart-help"
                >Each point shows one application's exclusive indexed file paths and logical bytes.
                Use the arrow keys to move between points and Enter or Space to select an
                application. View files opens the selected application's matching paths.</desc
              >
              {#each plot.yTicks as tick (tick.value)}
                {@const label = applicationBytes(tick.value)}
                <line
                  x1={plot.chart.left}
                  x2={plot.chart.right}
                  y1={tick.position}
                  y2={tick.position}
                />
                <text x={plot.chart.left - 10} y={tick.position + 4} text-anchor="end"
                  ><title>{tick.value} logical bytes</title>{label.length <= 12
                    ? label
                    : `${largeTick.format(BigInt(tick.value))} B`}</text
                >
              {/each}
              {#each plot.xTicks as tick (tick.value)}
                <line
                  x1={tick.position}
                  x2={tick.position}
                  y1={plot.chart.top}
                  y2={plot.chart.bottom}
                />
                <text
                  x={tick.position}
                  y={plot.chart.bottom + 23}
                  text-anchor={tick.value === 0
                    ? 'start'
                    : tick.value === plot.maxFiles
                      ? 'end'
                      : 'middle'}>{fileTick.format(tick.value)}</text
                >
              {/each}
              <text x={plot.chart.left} y="18">Exclusive logical bytes</text>
              <text x={plot.chart.width / 2} y={plot.chart.height - 6} text-anchor="middle"
                >Exclusive indexed file paths</text
              >
              {#each plot.points as point, index (point.app.id)}
                <circle
                  data-point
                  cx={point.x}
                  cy={point.y}
                  r={preview?.id === point.app.id ? 7 : 5}
                  fill={paints(point.app).fill}
                  role="button"
                  tabindex={tabPoint === point.app.id ? 0 : -1}
                  aria-current={selectedId === point.app.id ? 'true' : undefined}
                  aria-controls="application-selection"
                  aria-label={`Select ${point.app.name}, ${point.app.exclusive.files.toLocaleString()} exclusive file paths, ${applicationBytes(point.app.exclusive.logical_bytes)} logical.`}
                  onpointerenter={() => {
                    hoveredPoint = point.app.id;
                  }}
                  onpointerleave={() => {
                    if (hoveredPoint === point.app.id) hoveredPoint = null;
                  }}
                  onfocus={(event) => {
                    focusedPoint = rovingPoint = point.app.id;
                    event.currentTarget.scrollIntoView({ block: 'nearest', inline: 'nearest' });
                  }}
                  onblur={() => {
                    if (focusedPoint === point.app.id) focusedPoint = null;
                  }}
                  onkeydown={(event) => movePoint(event, index)}
                  onclick={() => void choose(point.app.id)}
                >
                  <title
                    >{point.app.name} · {applicationBytes(point.app.exclusive.logical_bytes)} · {point.app.exclusive.files.toLocaleString()}
                    paths</title
                  >
                </circle>
              {/each}
              {#if marker}
                <circle
                  class="selected-point"
                  aria-hidden="true"
                  cx={marker.x}
                  cy={marker.y}
                  r="10"
                />
              {/if}
            </svg>
          </div>
          <div class="application-tooltip" aria-hidden="true">
            {#if preview}<strong>{preview.name}</strong><span
                >{preview.exclusive.files.toLocaleString()} paths · {applicationBytes(
                  preview.exclusive.logical_bytes,
                )}</span
              >{/if}
          </div>
        {:else}<p class="application-state">No exclusive application measurements to plot.</p>{/if}
        <p class="application-caption">
          The chart is limited to the 200 largest claims by exclusive size.{#if page.chart_omitted}
            {page.chart_omitted.toLocaleString()} more in the paged list.{/if}{#if plot.omittedZero}
            {plot.omittedZero.toLocaleString()} in this selection have no positive exclusive measurement.{/if}
        </p>
      </section>
      <section class="application-inspector" aria-labelledby="application-selection">
        {#if selected}
          {@const selectedApplication = selected.id}
          <h3 id="application-selection" bind:this={selectionHeading} tabindex="-1">
            {selected.name}
          </h3>
          <p
            class="application-selection-value"
            title={`${selected.exclusive.logical_bytes} logical bytes`}
          >
            {applicationBytes(selected.exclusive.logical_bytes)} · {selected.exclusive.files.toLocaleString()}
            exclusive paths
          </p>
          {#if selected.overlapping.files}
            <p>{applicationBytes(selected.overlapping.logical_bytes)} in overlapping claims</p>
          {/if}
          {#if !marker}
            <p>
              {selected.exclusive.files === 0 || selected.exclusive.logical_bytes === '0'
                ? 'No positive exclusive measurement to plot.'
                : 'Outside the 200 claims used for the chart.'}
            </p>
          {:else if atSelectedLocation > 1}
            <p>
              {atSelectedLocation.toLocaleString()} claims share this position. Use arrow keys to inspect
              each point.
            </p>
          {/if}
          {#if !rowSelected}<p>Outside the current list page.</p>{/if}
          <dl class="application-breakdown">
            {#each categories as category}
              {@const measure = selected.breakdown.find((part) => part.category === category)}
              <div>
                <dt>
                  <span aria-hidden="true" style:background={categoryStyle[category].paint.fill}
                  ></span>
                  {categoryStyle[category].label}
                </dt>
                <dd>
                  {#if measure}
                    <strong title={`${measure.exclusive.logical_bytes} logical bytes`}
                      >{applicationBytes(measure.exclusive.logical_bytes)}</strong
                    >
                    <small
                      >{measure.exclusive.files.toLocaleString()} exclusive paths{#if measure.overlapping.files}
                        · {applicationBytes(measure.overlapping.logical_bytes)} shared{/if}</small
                    >
                  {:else}
                    <span>Not classified</span>
                  {/if}
                </dd>
              </div>
            {/each}
          </dl>
          <p>
            Location evidence · {selected.confidence === 'supplied'
              ? 'supplied metadata'
              : selected.confidence === 'ambiguous'
                ? 'conflicting locations'
                : 'contextual'} · {page.attribution === 'captured'
              ? 'saved with snapshot'
              : page.attribution === 'indexed'
                ? 'recorded with analysis'
                : 'current registrations'}
          </p>
          <div class="application-selection-actions">
            <button class="quiet-action" onclick={() => explore(selectedApplication)}
              >View files</button
            >
            {#if canReviewApplicationCache(selected)}
              <button
                class="quiet-action"
                disabled={!canManageCache}
                onclick={() => reviewCache(selectedApplication)}
                >Review supported cache files</button
              >
            {/if}
          </div>
          {#if canManageCache && canReviewApplicationCache(selected)}
            <p>Cache review keeps this folder scope and uses its own file criteria.</p>
          {/if}
        {:else}
          <h3 id="application-selection">Application details</h3>
          <p>
            {selectedId
              ? 'The selected claim is unavailable on this page or chart.'
              : 'Select a point or an application name to see its location breakdown.'}
          </p>
        {/if}
      </section>
    </div>
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
        <li class:selected-row={selectedId === app.id}>
          <span
            class="application-key"
            aria-hidden="true"
            style:background={paints(app).fill}
            style:color={paints(app).ink}>{Array.from(app.name.trim())[0]}</span
          >
          <div class="application-name">
            <button
              class="application-choose"
              aria-label={`Inspect ${app.name}`}
              aria-current={selectedId === app.id ? 'true' : undefined}
              aria-controls="application-selection"
              onclick={(event) => void choose(app.id, event.currentTarget)}>{app.name}</button
            ><span
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
              >{#each app.breakdown as part (part.category)}<span
                  style:width={`${percent(part.exclusive.logical_bytes)}%`}
                  style:background={categoryStyle[part.category].paint.fill}
                ></span>{/each}</span
            >
          </div>
          <button
            class="quiet-action"
            onclick={() => explore(app.id)}
            disabled={paused}
            aria-label={`View files for ${app.name}`}
            title="Show this application's matching files in the current folder">View files</button
          >
        </li>
      {/each}
    </ul>
    <div class="application-pagination">
      <button
        data-page-step="previous"
        disabled={offset === 0}
        onclick={(event) => paginate(Math.max(0, offset - 50), 'previous', event.currentTarget)}
        >Previous</button
      ><span
        >{page.applications.length
          ? `${offset + 1}–${offset + page.applications.length} of ${page.application_count}`
          : page.application_count
            ? 'No matches on this page'
            : '0 matches'}</span
      ><button
        data-page-step="next"
        disabled={offset + 50 >= page.application_count}
        onclick={(event) => paginate(offset + 50, 'next', event.currentTarget)}>Next</button
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
        Categories describe indexed locations, not verified file contents. Profile data includes
        unclassified profile content. The supported default Chrome and Edge layouts include HTTP
        cache. Chrome's documented debug log is a separate category. Missing categories are
        unavailable; Unknown marks conflicting location claims. Bars show the exclusive share of all
        matching bytes and share category colors with the breakdown.
      </p>
      <p>
        Index measurement: {date(page.indexed_at)}. {page.attribution === 'captured'
          ? 'Registration evidence saved with this snapshot'
          : page.attribution === 'indexed'
            ? 'Registration evidence recorded with this analysis'
            : 'Current registration read'}: {date(page.registrations_started_at)}–{date(
          page.registrations_observed_at,
        )}. Registration reads are not an atomic inventory.
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
    font-size: 16px;
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
    font-size: 12px;
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
    padding: 12px 0;
    border-bottom: 1px solid var(--border);
    background: transparent;
  }
  .application-workspace {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 16px;
    margin-block: 20px;
  }
  .application-chart {
    min-width: 0;
  }
  svg {
    display: block;
    width: 100%;
    margin-top: 14px;
  }
  svg line {
    stroke: var(--border);
    stroke-dasharray: 2 5;
  }
  svg text {
    fill: var(--secondary);
    font:
      14px 'Segoe UI',
      sans-serif;
  }
  .application-plot {
    padding-bottom: 8px;
  }
  circle[data-point] {
    cursor: pointer;
    stroke: var(--secondary);
    stroke-width: 1.25px;
  }
  circle[data-point]:focus-visible {
    outline: none;
    stroke: var(--text);
    stroke-width: 3px;
  }
  .selected-point {
    pointer-events: none;
    fill: none;
    stroke: var(--text);
    stroke-width: 2px;
    vector-effect: non-scaling-stroke;
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
  .application-inspector {
    min-width: 0;
    padding-top: 12px;
    border-top: 1px solid var(--border);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .application-inspector p {
    margin: 6px 0;
    color: var(--secondary);
    line-height: 1.4;
  }
  .application-inspector .application-selection-value {
    color: var(--text);
    font-size: 14px;
    font-variant-numeric: tabular-nums;
  }
  .application-selection-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 3px 12px;
    margin-block: 8px 4px;
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
    gap: 8px 12px;
    padding: 10px 8px;
    border-bottom: 1px solid var(--border);
  }
  .application-list li.selected-row {
    background: var(--selected);
    box-shadow: inset 2px 0 var(--accent);
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
  .application-choose {
    align-self: start;
    min-height: 26px;
    max-width: 100%;
    padding: 0;
    border: 0;
    background: transparent;
    text-align: left;
    font-size: 14px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .application-choose:hover {
    text-decoration: underline;
  }
  .application-name > span,
  .application-name small,
  .application-value > span {
    font-size: 12px;
    color: var(--secondary);
    overflow-wrap: anywhere;
  }
  .application-value {
    text-align: right;
    font-size: 14px;
    font-variant-numeric: tabular-nums;
  }
  .application-value strong {
    font-size: 14px;
  }
  .application-track {
    display: flex;
    height: 4px;
    background: var(--paper);
    border-radius: 2px;
    overflow: hidden;
  }
  .application-track > span {
    height: 100%;
    display: block;
  }
  .application-breakdown {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(140px, 100%), 1fr));
    gap: 9px 14px;
    margin: 10px 0;
    padding-block: 8px;
    border-block: 1px solid var(--border);
    color: var(--secondary);
    min-width: 0;
    font-size: 12px;
  }
  .application-breakdown dt {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .application-breakdown dt > span {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    flex-shrink: 0;
  }
  .application-breakdown dd {
    margin: 6px 0 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    overflow-wrap: anywhere;
    font-variant-numeric: tabular-nums;
  }
  .application-breakdown strong {
    color: var(--text);
    font-size: 13px;
  }
  .application-breakdown small {
    font-size: 12px;
    line-height: 1.4;
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
  @media (min-width: 1450px) {
    .application-workspace {
      grid-template-columns: minmax(0, 1fr) minmax(220px, 260px);
      gap: 22px;
    }
    .application-inspector {
      border-top: 0;
      border-inline-start: 1px solid var(--border);
      padding: 0 0 0 17px;
    }
    .application-inspector .application-breakdown {
      grid-template-columns: minmax(0, 1fr);
      gap: 6px;
    }
    .application-inspector .application-breakdown > div {
      display: flex;
      align-items: baseline;
      justify-content: space-between;
      gap: 4px 8px;
    }
    .application-inspector .application-breakdown dd {
      margin: 0;
      text-align: end;
    }
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
