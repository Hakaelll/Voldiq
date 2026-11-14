<script lang="ts">
  import { untrack } from 'svelte';
  import { trendQueries, trendChangeQueries } from '../lib/queries';
  import { historyTrend, type TrendChanges, type TrendReport } from '../lib/history-trend';
  import { utcDate } from '../lib/history-activity';
  import { bytes, difference, type Snapshot } from '../lib/types';

  let {
    captures,
    initialSnapshot,
    disabled,
  }: {
    captures: Snapshot[];
    initialSnapshot: number;
    disabled: boolean;
  } = $props();
  const groups = $derived([
    ...new Map([...captures].reverse().map((item) => [item.root_key, item])).values(),
  ]);
  const initialGroup = untrack(
    () => captures.find((item) => item.id === initialSnapshot)?.root_key,
  );
  let root = $state(initialGroup ?? untrack(() => groups.at(-1)?.root_key ?? 0));
  const anchor = $derived(groups.find((item) => item.root_key === root)?.id ?? 0);
  let days = $state(30);
  let metric = $state<'folder' | 'volume'>('folder');
  let report = $state.raw<TrendReport | null>(null);
  let changes = $state.raw<TrendChanges | null>(null);
  let retry = $state(0),
    changeRetry = $state(0);
  let error = $state(''),
    changeError = $state('');
  let selected = $state(0),
    zoom = $state(0),
    width = $state(680);
  const height = 260;
  const points = $derived(report?.points ?? []);
  const zoomed = $derived(points.slice(zoom));
  const active = $derived(zoomed.find((item) => item.snapshot === selected) ?? zoomed.at(-1));
  const first = $derived(zoomed[0]);
  const last = $derived(zoomed.at(-1));
  const forecast = $derived(report?.forecast ?? null);
  const volume = $derived(report?.volume);
  const volumeLatest = $derived(volume?.points.at(-1));
  let volumeWidth = $state(680);
  let volumeZoom = $state(0);
  const volumePoints = $derived(volume?.points.slice(volumeZoom) ?? []);
  const volumeChart = $derived(
    historyTrend(
      [
        ...volumePoints.map((point) => ({
          captured: point.at,
          logical: Number(point.free),
          capacity: point.capacity,
        })),
        ...(volume?.forecast ? [{ captured: volume.forecast.at, logical: 0, capacity: null }] : []),
      ],
      Math.max(volumeWidth, 260),
      128,
    ),
  );
  const chart = $derived(
    historyTrend(
      [
        ...zoomed.map((point) => ({
          captured: point.measured_at,
          logical: Number(point.logical),
          snapshot: point.snapshot,
        })),
        ...(forecast
          ? [{ captured: forecast.at, logical: Number(forecast.high), snapshot: 0 }]
          : []),
      ],
      Math.max(width, 260),
      height,
    ),
  );
  const measured = $derived(chart?.points.filter((point) => point.snapshot !== 0) ?? []);
  const projected = $derived(chart?.points.find((point) => point.snapshot === 0));
  const currentMarker = $derived(measured.find((point) => point.snapshot === active?.snapshot));
  const deltaScale = $derived(Math.max(1, ...zoomed.map((point) => Math.abs(Number(point.delta)))));
  const route = $derived(
    active?.previous && active.delta !== null
      ? { before: active.previous, after: active.snapshot }
      : null,
  );
  const visibleChanges = $derived(
    changes?.before === route?.before && changes?.after === route?.after ? changes : null,
  );
  const short = (timestamp: number) =>
    new Date(timestamp * 1000).toLocaleDateString(undefined, {
      month: 'short',
      day: 'numeric',
      timeZone: 'UTC',
    });

  $effect(() => {
    void retry;
    report = null;
    error = '';
    zoom = 0;
    volumeZoom = 0;
    selected = 0;
    if (disabled || !anchor) return;
    let current = true;
    void trendQueries.request({ snapshot: anchor, days }).result.then(
      (value) => {
        if (!current) return;
        if (value?.anchor === anchor) report = value;
        else error = 'Trend query cancelled. Try again.';
      },
      (problem: unknown) => {
        if (current) error = String(problem);
      },
    );
    return () => {
      current = false;
      trendQueries.cancel();
    };
  });

  $effect(() => {
    void changeRetry;
    changes = null;
    changeError = '';
    if (disabled || metric !== 'folder' || !route) return;
    let current = true;
    const request = route;
    void trendChangeQueries.request(request).result.then(
      (value) => {
        if (!current) return;
        if (value?.before === request.before && value.after === request.after) changes = value;
        else changeError = 'Folder changes were cancelled. Try again.';
      },
      (problem: unknown) => {
        if (current) changeError = String(problem);
      },
    );
    return () => {
      current = false;
      trendChangeQueries.cancel();
    };
  });
</script>

<div class="activity-toolbar">
  <label
    >Folder<select bind:value={root} {disabled}>
      {#each groups as item (item.root_key)}
        <option value={item.root_key}>{item.root} · scope {item.root_key}</option>
      {/each}
    </select></label
  >
  <label
    >Interval<select bind:value={days} {disabled}>
      <option value={7}>7 days</option>
      <option value={30}>30 days</option>
      <option value={90}>90 days</option>
      <option value={365}>1 year</option>
    </select></label
  >
  <div class="section-navigation" role="group" aria-label="Trend measurement">
    <button type="button" aria-pressed={metric === 'folder'} onclick={() => (metric = 'folder')}
      >Folder size</button
    >
    <button type="button" aria-pressed={metric === 'volume'} onclick={() => (metric = 'volume')}
      >Volume free space</button
    >
  </div>
</div>

{#snippet volumeTrend()}
  <section class="panel history-extended-trend" aria-labelledby="volume-trend-heading">
    <div class="panel-heading">
      <h3 id="volume-trend-heading">Volume free space</h3>
      {#if volumeLatest}
        <span title={`${volumeLatest.free} bytes`}>{bytes(Number(volumeLatest.free))} free</span>
      {/if}
    </div>
    {#if volumeLatest && volume}
      <p class="activity-help">
        {volume.points.length} measurements · last measured {utcDate(volumeLatest.at)}
      </p>
      {#if volumeChart}
        <div class="history-plot" bind:clientWidth={volumeWidth}>
          <svg
            viewBox={`0 0 ${Math.max(volumeWidth, 260)} 160`}
            role="img"
            aria-label="Observed volume free space and conditional fill projection"
          >
            {#each volumeChart.ticks as mark (mark.value)}
              <line
                x1="60"
                x2={Math.max(volumeWidth, 260) - 16}
                y1={mark.y}
                y2={mark.y}
                stroke="var(--border)"
              />
              <text x="52" y={mark.y + 4} text-anchor="end" class="trend-axis"
                >{bytes(mark.value)}</text
              >
            {/each}
            {#each volumeChart.points as point, i (point.captured)}
              {@const previous = volumeChart.points[i - 1]}
              {#if previous && (point.capacity === null || point.capacity === previous.capacity)}
                <line
                  x1={previous.x}
                  x2={point.x}
                  y1={previous.y}
                  y2={point.y}
                  stroke={point.capacity === null ? 'var(--chart-violet)' : 'var(--chart-blue)'}
                  stroke-dasharray={point.capacity === null ? '5 5' : undefined}
                  stroke-width="2"
                />
              {/if}
              <circle
                cx={point.x}
                cy={point.y}
                r="3"
                fill={point.capacity === null ? 'var(--chart-violet)' : 'var(--chart-blue)'}
              >
                <title>{utcDate(point.captured)} · {bytes(point.logical)}</title>
              </circle>
              {#if i === 0 || i === volumeChart.points.length - 1}
                <text x={point.x} y="150" class="trend-axis" text-anchor={i === 0 ? 'start' : 'end'}
                  >{short(point.captured)}</text
                >
              {/if}
            {/each}
          </svg>
        </div>
      {/if}
      {#if volume.points.length > 2 && volumePoints[0]}
        <label class="trend-zoom">
          Zoom start
          <input
            type="range"
            min="0"
            max={volume.points.length - 2}
            bind:value={volumeZoom}
            aria-valuetext={utcDate(volumePoints[0].at)}
          />
          <span>{short(volumePoints[0].at)} UTC</span>
        </label>
      {/if}
      {#if volume.forecast}
        <p><strong>Estimated to fill around {short(volume.forecast.at)} UTC</strong></p>
        <p class="activity-help">
          {short(volume.forecast.earliest_at)} to {short(volume.forecast.latest_at)} UTC ·
          {volume.forecast.confidence === 'higher' ? 'Higher' : 'Moderate'} confidence from
          {volume.forecast.samples} measured days.
        </p>
      {:else if volumeLatest.free === '0'}
        <p>No free space at the last measurement.</p>
      {:else}
        <p class="activity-help">
          A fill date needs recent measurements with sustained, steady loss of free space.
        </p>
      {/if}
    {:else}
      <p class="activity-help">
        No volume measurement is available for the latest capture in this scope.
      </p>
    {/if}
  </section>
{/snippet}

{#if disabled}
  <p role="status">Trends will resume when the active operation finishes.</p>
{:else if error}
  <p role="alert">{error} <button onclick={() => retry++}>Retry trends</button></p>
{:else if !report}
  <p role="status">Loading measured trends…</p>
{:else if metric === 'volume'}
  {@render volumeTrend()}
{:else if !last || !first}
  <p>No captures in this scope have a known measurement date.</p>
{:else}
  <section class="panel history-trend history-extended-trend" aria-label="Logical size over time">
    <div class="panel-heading">
      <h3>Logical size over time</h3>
      <span>{bytes(Number(last.logical))} · {short(last.measured_at)} UTC</span>
    </div>
    {#if chart && measured.length > 1}
      <div class="history-plot" bind:clientWidth={width}>
        <svg
          viewBox={`0 0 ${Math.max(width, 260)} ${height + 34}`}
          role="img"
          aria-label="Recorded logical folder size and compatible forecast"
        >
          {#each chart.ticks as mark (mark.value)}
            <line
              x1="60"
              x2={Math.max(width, 260) - 16}
              y1={mark.y}
              y2={mark.y}
              stroke="var(--border)"
            />
            <text x="52" y={mark.y + 4} text-anchor="end" class="trend-axis"
              >{bytes(mark.value)}</text
            >
          {/each}
          {#each measured as point, i (point.snapshot)}
            {@const prev = measured[i - 1]}
            {@const capture = zoomed[i]}
            {#if prev && capture.previous === prev.snapshot}
              <path
                d={`M${prev.x},${height} L${prev.x},${prev.y} L${point.x},${point.y} L${point.x},${height} Z`}
                fill="var(--chart-blue)"
                opacity=".12"
              />
              <line
                x1={prev.x}
                x2={point.x}
                y1={prev.y}
                y2={point.y}
                stroke="var(--chart-blue)"
                stroke-width="2"
              />
            {/if}
            <circle
              cx={point.x}
              cy={point.y}
              r="3"
              fill={capture.complete && capture.supported ? 'var(--chart-blue)' : 'var(--paper)'}
              stroke="var(--chart-blue)"
            >
              <title>{utcDate(capture.measured_at)} · {capture.logical} bytes</title>
            </circle>
            {#if point.labelled}
              <text
                x={point.x}
                y={height + 22}
                class="trend-axis"
                text-anchor={i === 0 ? 'start' : 'middle'}>{short(point.captured)}</text
              >
            {/if}
          {/each}
          {#if projected && forecast}
            {@const end = measured.at(-1)}
            {@const valueY = chart.y(Number(forecast.logical))}
            {#if end}<line
                x1={end.x}
                x2={projected.x}
                y1={end.y}
                y2={valueY}
                stroke="var(--chart-violet)"
                stroke-width="2"
                stroke-dasharray="5 5"
              />{/if}
            <text x={projected.x} y={height + 22} class="trend-axis" text-anchor="end"
              >{short(projected.captured)}</text
            >
          {/if}
          {#if currentMarker}
            <circle
              cx={currentMarker.x}
              cy={currentMarker.y}
              r="6"
              fill="var(--panel)"
              stroke="var(--chart-violet)"
              stroke-width="2"
            />
          {/if}
        </svg>
      </div>
    {:else}
      <p>One measurement in view.</p>
    {/if}
    {#if points.length > 2}
      <label class="trend-zoom">
        Zoom start
        <input
          type="range"
          min="0"
          max={points.length - 2}
          bind:value={zoom}
          aria-valuetext={utcDate(first.measured_at)}
        />
        <span>{short(first.measured_at)} UTC</span>
      </label>
    {/if}
    {#if forecast}
      <p class="activity-help">
        Dashed line · projected {bytes(Number(forecast.logical))} on {short(forecast.at)} UTC.
        {forecast.confidence === 'higher' ? 'Higher' : 'Moderate'} confidence based on {forecast.samples}
        measured days. Observed-rate range {bytes(Number(forecast.low))} to {bytes(
          Number(forecast.high),
        )}.
      </p>
    {:else}
      <p class="activity-help">
        A projection requires recent, sustained growth across comparable measurement days.
      </p>
    {/if}
    {#if report.limited || report.missing_dates}
      <p class="activity-help">
        {report.limited
          ? 'Showing the 240 newest measurements in this interval. '
          : ''}{report.missing_dates ? 'Captures without a measurement date are omitted.' : ''}
      </p>
    {/if}
  </section>

  {#if active}
    <section
      class="panel history-extended-trend trend-changes"
      aria-labelledby="trend-change-title"
    >
      <div class="panel-heading"><h3 id="trend-change-title">Changes between scans</h3></div>
      {#if measured.length > 1 && zoomed.some((point) => point.delta !== null)}
        <svg
          viewBox={`0 0 ${Math.max(width, 260)} 88`}
          role="img"
          aria-label="Logical size change per comparable scan"
        >
          <text x="52" y="48" text-anchor="end" class="trend-axis">0 B</text>
          <line x1="60" x2={Math.max(width, 260) - 16} y1="44" y2="44" stroke="var(--border)" />
          {#each measured as point, i (point.snapshot)}
            {@const sample = zoomed[i]}
            {@const delta = Number(sample.delta)}
            {#if sample.delta !== null}
              <line
                x1={point.x}
                x2={point.x}
                y1="44"
                y2={44 - (delta / deltaScale) * 35}
                stroke={delta < 0 ? 'var(--chart-reduction)' : 'var(--chart-growth)'}
                stroke-width="3"
              >
                <title>{utcDate(sample.measured_at)} · {sample.delta} bytes</title>
              </line>
            {/if}
          {/each}
        </svg>
      {/if}
      <div class="activity-toolbar">
        <label
          >Measurement<select
            value={active.snapshot}
            onchange={(event) => (selected = Number(event.currentTarget.value))}
          >
            {#each [...zoomed].reverse() as point (point.snapshot)}
              <option value={point.snapshot}
                >{utcDate(point.measured_at)} · {bytes(Number(point.logical))}</option
              >
            {/each}
          </select></label
        >
        {#if active.delta !== null}
          <strong
            class:trend-positive={Number(active.delta) > 0}
            class:trend-negative={Number(active.delta) < 0}
            title={`${active.delta} bytes`}
          >
            {difference(Number(active.delta))} since the previous scan
          </strong>
        {:else}
          <span>No comparable predecessor in this interval.</span>
        {/if}
      </div>
      {#if !active.complete || !active.supported}
        <p class="activity-help">
          {!active.complete ? 'Partial coverage. ' : ''}{!active.supported
            ? 'Accounting revision is unsupported.'
            : ''}
        </p>
      {/if}
      {#if route}
        {#if changeError}
          <p role="alert">
            {changeError} <button onclick={() => changeRetry++}>Retry changes</button>
          </p>
        {:else if visibleChanges}
          <div class="trend-folder-columns">
            {#each [{ label: 'Grew', changes: visibleChanges.grew }, { label: 'Shrank', changes: visibleChanges.shrank }] as group (group.label)}
              <div>
                <h4>{group.label}</h4>
                {#if group.changes.length}
                  {#each group.changes as item (item.name)}
                    <div class="trend-folder-row">
                      <span title={item.name}>{item.name}</span>
                      <strong
                        class:trend-positive={Number(item.delta) > 0}
                        class:trend-negative={Number(item.delta) < 0}
                        title={`${item.delta} bytes`}>{difference(Number(item.delta))}</strong
                      >
                    </div>
                  {/each}
                {:else}<p class="activity-help">No matching folder changes.</p>{/if}
              </div>
            {/each}
          </div>
          <p class="activity-help">Largest changes in folders directly below the scan root.</p>
        {:else}<p role="status">Loading folder changes…</p>{/if}
      {/if}
    </section>
  {/if}
{/if}
