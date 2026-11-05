<script lang="ts">
  import { invokeIndexed } from '../lib/index-context';
  import { onMount, onDestroy, tick } from 'svelte';
  import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
  import Icon from './Icon.svelte';
  import SnapshotChangeBars from './SnapshotChangeBars.svelte';
  import SnapshotVisualCompare from './SnapshotVisualCompare.svelte';
  import HistoryPlayback from './HistoryPlayback.svelte';
  import SnapshotRetention from './SnapshotRetention.svelte';
  import HistoryCalendar from './HistoryCalendar.svelte';
  import HistoryDiary from './HistoryDiary.svelte';
  import type {
    HistoryWorkspace,
    HistoricalComparison,
    DiaryLocation,
  } from '../lib/history-activity';
  import type { SnapshotRetentionSettings, SnapshotRetentionResult } from '../lib/types';
  import { snapshotChangeQueries } from '../lib/queries';
  import { historyTrend } from '../lib/history-trend';
  import { updateSnapshotPair } from '../lib/snapshot-pair';
  import { bytes, date, difference, type Snapshot, type SnapshotComparison } from '../lib/types';
  let {
    enabled,
    blocked,
    onbusy,
    onload,
    onerror,
  }: {
    enabled: boolean;
    blocked: boolean;
    onbusy: (busy: boolean) => void;
    onload: (snapshot: Snapshot, indexId: number) => Promise<void>;
    onerror: (problem: unknown) => void;
  } = $props();
  const limit = 20;
  let snapshots = $state<Snapshot[]>([]);
  let offset = $state(0),
    storage = $state(0),
    working = $state(false);
  let exporting = $state(false);
  const locked = $derived(working || exporting || blocked);
  let before = $state(0),
    after = $state(0),
    changeOffset = $state(0);
  let comparison = $state<SnapshotComparison | null>(null);
  let workspace = $state<HistoryWorkspace>({ kind: 'captures' });
  const focusedComparison = $derived(workspace.kind === 'comparison');
  const playback = $derived(workspace.kind === 'playback');
  const comparisonContext = $derived(workspace.kind === 'comparison' ? workspace : null);
  let comparisonLoading = $state(false),
    comparisonProblem = $state('');
  let retentionOpen = $state(false),
    retentionResult = $state('');
  let deleting = $state<number | null>(null);
  let loadState = $state<'loading' | 'ready' | 'unavailable'>(isTauri() ? 'loading' : 'ready');
  let region: HTMLDivElement;
  let saveButton: HTMLButtonElement;
  let cancelButton = $state<HTMLButtonElement>();
  let request = 0,
    comparisonRequest = 0;
  let operationId = $state<number | null>(null);
  let canCancel = $state(false),
    cancelPending = $state(false);
  let actionGeneration = 0,
    alive = true;
  function finishOperation(): void {
    actionGeneration += 1;
    operationId = null;
    canCancel = cancelPending = false;
  }
  function operationChannel(): Channel<number> {
    const owner = actionGeneration;
    const channel = new Channel<number>();
    channel.onmessage = (id) => {
      if (!alive || owner !== actionGeneration)
        void invoke('cancel_operation', { id }).catch(onerror);
      else operationId = id;
    };
    return channel;
  }
  async function cancelOperation(): Promise<void> {
    if (operationId === null || cancelPending) return;
    const owner = actionGeneration;
    const id = operationId;
    cancelPending = true;
    try {
      await invoke('cancel_operation', { id });
    } catch (problem) {
      if (alive && owner === actionGeneration) {
        cancelPending = false;
        onerror(problem);
      }
    }
  }

  const labels = {
    added: 'Added',
    removed: 'Removed',
    changed: 'Changed',
    unobserved: 'Not observed',
    newly_observed: 'Newly observed',
  };
  let pair = $derived(snapshots.filter((s) => s.id === before || s.id === after));
  let previous = $derived(snapshots.find((s) => s.id === (before || after)));
  let compatible = $derived(snapshots.filter((s) => s.root_key === previous?.root_key));
  let trend = $derived([...compatible].reverse());
  let chartWidth = $state(600);
  let chart = $derived(historyTrend(trend, chartWidth));
  function selectCapture(snapshot: Snapshot, selected: boolean): void {
    const next = updateSnapshotPair(pair, snapshot, selected);
    if (!next) return;
    resetComparison();
    before = next[0]?.id ?? 0;
    after = next[1]?.id ?? 0;
  }
  async function refresh(next = offset): Promise<void> {
    const token = ++request;
    resetComparison();
    const retainFocus = region?.contains(document.activeElement) ?? false;
    loadState = 'loading';
    try {
      const [items, size] = await Promise.all([
        invoke<Snapshot[]>('list_snapshots', { offset: next, limit }),
        invoke<number>('snapshot_storage'),
      ]);
      if (token !== request) return;
      snapshots = items;
      loadState = 'ready';
      storage = size;
      offset = next;
      comparison = null;
      comparisonRequest += 1;
      workspace = { kind: 'captures' };
      comparisonLoading = false;
      comparisonProblem = '';
      const latest = items[0];
      const older =
        latest && items.find((s) => s.root_key === latest.root_key && s.id !== latest.id);
      before = older?.id ?? latest?.id ?? 0;
      after = latest?.id ?? 0;
    } catch (problem) {
      if (token === request) {
        loadState = 'unavailable';
        onerror(problem);
      }
    } finally {
      if (token === request && retainFocus) {
        await tick();
        if (token === request) region.focus();
      }
    }
  }
  async function mutate<T>(
    action: () => Promise<T>,
    cancellableAction = false,
  ): Promise<T | undefined> {
    if (locked) return;
    const trigger = document.activeElement;
    actionGeneration += 1;
    canCancel = cancellableAction;
    cancelPending = false;
    working = true;
    onbusy(true);
    resetComparison();
    try {
      return await action();
    } catch (problem) {
      onerror(problem);
    } finally {
      finishOperation();
      working = false;
      onbusy(false);
      const completed = actionGeneration;
      await tick();
      if (
        alive &&
        completed === actionGeneration &&
        document.activeElement === document.body &&
        trigger instanceof HTMLElement &&
        trigger.isConnected
      )
        trigger.focus();
    }
  }
  async function save(): Promise<void> {
    await mutate(async () => {
      const saved = await invokeIndexed<number | null>('save_snapshot', {
        operation: operationChannel(),
      });
      finishOperation();
      if (saved !== null && alive) await refresh(0);
    }, true);
  }
  async function configureRetention(
    snapshot: number,
    version: string,
    keepLatest: number | null,
  ): Promise<SnapshotRetentionSettings | undefined> {
    return mutate(() =>
      invoke<SnapshotRetentionSettings>('set_snapshot_retention', {
        snapshot,
        version,
        keepLatest,
      }),
    );
  }
  async function applyRetention(
    snapshot: number,
    fingerprint: string,
  ): Promise<SnapshotRetentionResult | null | undefined> {
    return mutate(async () => {
      const result = await invoke<SnapshotRetentionResult | null>('apply_snapshot_retention', {
        snapshot,
        fingerprint,
        operation: operationChannel(),
      });
      finishOperation();
      if (result !== null && alive) {
        await refresh(0);
        retentionResult = `Removed ${result.removed} saved captures. ${result.remaining_eligible} remain eligible in that history group.`;
        await tick();
        if (alive && document.activeElement === document.body)
          (region.querySelector<HTMLInputElement>('.snapshot-row input:checked') ?? region).focus();
      }
      return result;
    }, true);
  }
  async function open(snapshot: Snapshot): Promise<void> {
    await mutate(async () => {
      const indexId = await invoke<number | null>('load_snapshot', {
        id: snapshot.id,
        operation: operationChannel(),
      });
      finishOperation();
      if (indexId !== null && alive) await onload(snapshot, indexId);
    }, true);
  }
  async function remove(id: number): Promise<void> {
    await mutate(async () => {
      await invoke('delete_snapshot', { id });
      deleting = null;
      await refresh(offset);
    });
    await tick();
    if (deleting === id) cancelButton?.focus();
    else if (!saveButton.disabled) saveButton.focus();
    else region.focus();
  }
  async function confirmDelete(id: number): Promise<void> {
    deleting = id;
    await tick();
    cancelButton?.focus();
  }
  async function cancelDelete(id: number): Promise<void> {
    deleting = null;
    await tick();
    region.querySelector<HTMLButtonElement>(`[data-delete-snapshot="${id}"]`)?.focus();
  }
  async function compare(next = 0, explicit?: HistoricalComparison): Promise<void> {
    const selected = explicit ??
      comparisonContext ?? { before, after, scope: null, returnTo: null };
    if (!selected.before || !selected.after || selected.before === selected.after || locked) return;
    workspace = {
      kind: 'comparison',
      before: selected.before,
      after: selected.after,
      scope: selected.scope,
      returnTo: selected.returnTo,
    };
    comparisonLoading = true;
    comparisonProblem = '';
    const token = ++comparisonRequest;
    try {
      const result = await snapshotChangeQueries.request({
        before: selected.before,
        after: selected.after,
        offset: next,
        limit: 50,
      }).result;
      if (token === comparisonRequest) {
        if (!result) {
          comparisonProblem = 'Comparison cancelled. Compare again to retry.';
          return;
        }
        comparison = result;
        changeOffset = next;
      }
    } catch (problem) {
      if (token === comparisonRequest) comparisonProblem = String(problem);
    } finally {
      if (token === comparisonRequest) comparisonLoading = false;
    }
    if (token === comparisonRequest) await focusWorkspace('comparison');
  }
  function resetComparison(): void {
    comparison = null;
    comparisonRequest += 1;
    snapshotChangeQueries.cancel();
    comparisonLoading = false;
    comparisonProblem = '';
  }
  onMount(() => {
    if (isTauri()) void refresh(0);
  });
  onDestroy(() => {
    alive = false;
    actionGeneration += 1;
    if (operationId !== null) void invoke('cancel_operation', { id: operationId }).catch(onerror);
    request += 1;
    comparisonRequest += 1;
    snapshotChangeQueries.cancel();
  });
  async function showCaptures(): Promise<void> {
    if (comparisonLoading || comparisonContext?.returnTo) resetComparison();
    workspace = { kind: 'captures' };
    await tick();
    if (alive)
      (region.querySelector<HTMLInputElement>('.snapshot-row input:checked') ?? region).focus();
  }
  async function enterActivity(kind: 'playback' | 'calendar' | 'diary'): Promise<void> {
    if (locked || !compatible.length) return;
    resetComparison();
    workspace =
      kind === 'diary'
        ? {
            kind,
            location: {
              snapshot: compatible.find((capture) => capture.id === after)?.id ?? compatible[0].id,
              offset: 0,
            },
          }
        : { kind };
    await focusWorkspace(kind);
  }
  async function focusWorkspace(kind: HistoryWorkspace['kind']): Promise<void> {
    await tick();
    if (alive && workspace.kind === kind && document.activeElement === document.body)
      (
        region.querySelector<HTMLElement>(
          '.history-comparison select:not(:disabled),.history-comparison .snapshot-visual button:not(:disabled),.history-comparison button:not(:disabled)',
        ) ?? region
      ).focus();
  }
  async function inspectDiary(context: HistoricalComparison): Promise<void> {
    resetComparison();
    await compare(0, context);
  }
  function diaryLocation(location: DiaryLocation): void {
    if (workspace.kind === 'diary' && !locked) workspace = { kind: 'diary', location };
  }
  async function returnToDiary(): Promise<void> {
    const target = comparisonContext?.returnTo;
    if (!target || locked) return;
    resetComparison();
    workspace = { kind: 'diary', location: target };
    await focusWorkspace('diary');
  }
  async function cancelComparison(): Promise<void> {
    resetComparison();
    await tick();
    if (alive && document.activeElement === document.body)
      (
        region.querySelector<HTMLButtonElement>(
          '[data-compare-context],[data-compare-snapshots]',
        ) ?? region
      ).focus();
  }
</script>

<div
  class="history-region"
  role="region"
  aria-label="Snapshot history"
  tabindex="-1"
  bind:this={region}
>
  <div class="history-toolbar">
    <button
      onclick={() => refresh(offset)}
      disabled={locked || !isTauri() || loadState === 'loading'}>Reload captures</button
    >
    <button
      bind:this={saveButton}
      class="primary"
      onclick={save}
      disabled={!enabled || locked || !isTauri()}><Icon name="history" />Save snapshot</button
    >
    {#if canCancel}<button
        onclick={cancelOperation}
        disabled={operationId === null || cancelPending}
        >{cancelPending ? 'Cancelling…' : 'Cancel'}</button
      >{/if}
    <span
      >{#if loadState === 'ready'}{bytes(storage)} stored{/if}</span
    >
    <button
      onclick={() =>
        mutate(async () => {
          await invoke('compact_snapshots');
          await refresh();
        })}
      disabled={locked || !isTauri() || !storage || loadState !== 'ready'}>Compact</button
    >
  </div>
  {#if retentionResult}<p role="status">{retentionResult}</p>{/if}
  {#if loadState !== 'ready'}
    <section class="panel history-empty" aria-live="polite">
      <h2>{loadState === 'loading' ? 'Loading…' : 'History unavailable'}</h2>
      {#if loadState === 'unavailable'}<button onclick={() => refresh()} disabled={locked}
          >Retry</button
        >{/if}
    </section>
  {:else if snapshots.length === 0 && offset === 0}
    <section class="panel history-empty">
      <Icon name="history" size={28} />
      <h2>No snapshots yet</h2>
    </section>
  {:else}
    <div class="history-modes" role="group" aria-label="History view">
      <button
        aria-pressed={workspace.kind === 'captures' || focusedComparison}
        onclick={showCaptures}
        disabled={locked}>Compare</button
      >
      <button
        aria-pressed={workspace.kind === 'calendar'}
        onclick={() => enterActivity('calendar')}
        disabled={locked || !compatible.length}>Calendar</button
      >
      <button
        aria-pressed={workspace.kind === 'diary'}
        onclick={() => enterActivity('diary')}
        disabled={locked || !compatible.length}>Diary</button
      >
      <button
        aria-pressed={playback}
        onclick={() => enterActivity('playback')}
        disabled={locked || !compatible.length}>Playback</button
      >
    </div>
    {#if compatible.length}<details
        class="panel snapshot-retention-disclosure"
        bind:open={retentionOpen}
      >
        <summary>Snapshot retention</summary>
        {#if retentionOpen}{#key compatible[0].id}<SnapshotRetention
              snapshot={compatible[0].id}
              disabled={locked}
              onconfigure={configureRetention}
              onapply={applyRetention}
            />{/key}{/if}
      </details>{/if}
    <div class="history-layout" class:comparing={workspace.kind !== 'captures'}>
      <div class="history-captures">
        <h2>Snapshots</h2>
        <section class="panel snapshot-list" aria-label="Saved snapshots">
          {#each snapshots as snapshot (snapshot.id)}
            <article
              class="snapshot-row"
              class:selected={snapshot.id === before || snapshot.id === after}
            >
              <input
                class="snapshot-select"
                type="checkbox"
                aria-label={`Compare snapshot ${date(snapshot.captured)} · ${snapshot.root}`}
                checked={snapshot.id === before || snapshot.id === after}
                disabled={locked ||
                  (!pair.some((s) => s.id === snapshot.id) &&
                    (pair.length === 2 ||
                      (pair.length === 1 && pair[0].root_key !== snapshot.root_key)))}
                onchange={(event) => {
                  selectCapture(snapshot, event.currentTarget.checked);
                  event.currentTarget.checked = snapshot.id === before || snapshot.id === after;
                }}
              />
              <div class="snapshot-name">
                <strong title={date(snapshot.captured)}>{date(snapshot.captured)}</strong><small
                  title={snapshot.root}>{snapshot.root}</small
                >
              </div>
              {#if !snapshot.complete}<span class="snapshot-status">Partial</span>{/if}
              <span class="number">{bytes(snapshot.logical)}</span>
              <span class="snapshot-entries" title="Indexed entries"
                >{snapshot.entries.toLocaleString()}</span
              >
              <button onclick={() => open(snapshot)} disabled={locked}>Open</button>
              {#if deleting === snapshot.id}
                <button
                  bind:this={cancelButton}
                  onclick={() => cancelDelete(snapshot.id)}
                  disabled={locked}>Cancel</button
                ><button class="danger" onclick={() => remove(snapshot.id)} disabled={locked}
                  >Delete snapshot</button
                >
              {:else}
                <button
                  title="Delete snapshot"
                  data-delete-snapshot={snapshot.id}
                  aria-label={`Delete snapshot from ${date(snapshot.captured)}`}
                  onclick={() => confirmDelete(snapshot.id)}
                  disabled={locked}><Icon name="trash" /></button
                >
              {/if}
            </article>
          {/each}
          {#if offset || snapshots.length === limit}<div class="duplicate-pagination">
              <button
                onclick={() => refresh(Math.max(0, offset - limit))}
                disabled={locked || offset === 0}>Previous</button
              ><span
                >{snapshots.length
                  ? `${offset + 1}–${offset + snapshots.length}`
                  : 'End of history'}</span
              ><button
                onclick={() => refresh(offset + limit)}
                disabled={locked || snapshots.length < limit}>Next</button
              >
            </div>{/if}
        </section>
        {#if chart}
          {@const plotPoints = chart.points.map((point) => `${point.x},${point.y}`).join(' ')}
          <section class="panel history-trend" aria-label="Recorded logical size">
            <div class="panel-heading">
              <h2>Logical size</h2>
              <span>{bytes(trend[0]?.logical ?? 0)} → {bytes(trend.at(-1)?.logical ?? 0)}</span>
            </div>
            <div class="history-plot" bind:clientWidth={chartWidth}>
              <svg
                viewBox={`0 0 ${chartWidth} 244`}
                role="img"
                aria-label={`Logical size across ${trend.length} saved snapshots`}
              >
                {#each chart.ticks as tick}
                  <line
                    x1="60"
                    x2={chartWidth - 16}
                    y1={tick.y}
                    y2={tick.y}
                    stroke="var(--border)"
                  />
                  <text x="52" y={tick.y + 4} text-anchor="end" class="trend-axis"
                    >{bytes(tick.value)}</text
                  >
                {/each}
                <polygon
                  points={`${chart.points[0].x},180 ${plotPoints} ${chart.points[chart.points.length - 1].x},180`}
                  fill="var(--chart-blue)"
                  opacity="0.12"
                />
                <polyline
                  points={plotPoints}
                  fill="none"
                  stroke="var(--chart-blue)"
                  stroke-width="2.5"
                  vector-effect="non-scaling-stroke"
                />
                {#each chart.points as point, i}
                  <circle
                    cx={point.x}
                    cy={point.y}
                    r={i === chart.points.length - 1 ? 4.5 : 3.5}
                    fill={i === chart.points.length - 1 ? 'var(--chart-violet)' : 'var(--panel)'}
                    stroke={i === chart.points.length - 1
                      ? 'var(--chart-violet)'
                      : 'var(--chart-blue)'}
                    stroke-width="2"
                    ><title>{date(point.captured)} · {bytes(point.logical)}</title></circle
                  >
                  {#if point.labelled}
                    <text
                      x={point.x}
                      y={point.y - 10}
                      text-anchor={i === 0
                        ? 'start'
                        : i === chart.points.length - 1
                          ? 'end'
                          : 'middle'}
                      class="trend-value">{bytes(point.logical)}</text
                    >
                    <text
                      x={point.x}
                      y="207"
                      text-anchor={i === 0
                        ? 'start'
                        : i === chart.points.length - 1
                          ? 'end'
                          : 'middle'}
                      class="trend-axis"
                    >
                      <tspan x={point.x}
                        >{new Date(point.captured * 1000).toLocaleDateString('en', {
                          day: '2-digit',
                          month: 'short',
                        })}</tspan
                      >
                      <tspan x={point.x} dy="17"
                        >{new Date(point.captured * 1000).getFullYear()}</tspan
                      >
                    </text>
                  {/if}
                {/each}
              </svg>
            </div>
            <div class="history-chart-legend">
              <span><i style:background="var(--chart-blue)"></i>Recorded logical size</span>
              <span><i style:background="var(--chart-violet)"></i>Latest capture</span>
            </div>
          </section>{/if}
      </div>
      <div class="history-comparison">
        <div class="history-compare-heading">
          <h2>
            {workspace.kind === 'calendar'
              ? 'Modification calendar'
              : workspace.kind === 'diary'
                ? 'Observed diary'
                : playback
                  ? 'History playback'
                  : 'Compare snapshots'}
          </h2>
          {#if comparisonContext?.returnTo}<button onclick={returnToDiary} disabled={locked}
              >Back to diary</button
            >{/if}
          {#if workspace.kind !== 'captures'}<button onclick={showCaptures}>Back to captures</button
            >{/if}
        </div>
        {#if workspace.kind === 'calendar'}
          {#key compatible.map((capture) => capture.id).join(':')}<HistoryCalendar
              captures={compatible}
              initialSnapshot={after}
              disabled={locked}
              onopen={open}
            />{/key}
        {:else if workspace.kind === 'diary'}
          {#key compatible.map((capture) => capture.id).join(':')}<HistoryDiary
              captures={compatible}
              location={workspace.location}
              disabled={locked}
              onlocation={diaryLocation}
              oncompare={inspectDiary}
            />{/key}
        {:else if playback}
          {#key compatible.map((snapshot) => snapshot.id).join(':')}
            <HistoryPlayback
              captures={compatible}
              disabled={locked}
              onopen={open}
              onbusy={(value) => {
                exporting = value;
                onbusy(value);
              }}
            />
          {/key}
        {:else}
          {#if comparisonContext?.returnTo}<p class="activity-help">
              Diary context · capture #{comparisonContext.before} → #{comparisonContext.after}
              <button
                data-compare-context
                onclick={() => compare(0)}
                disabled={locked || comparisonLoading}>Reload comparison</button
              >
            </p>{:else}
            <div class="history-compare">
              <label
                >Before<select
                  bind:value={before}
                  onchange={() => {
                    resetComparison();
                    after =
                      snapshots.find((s) => s.root_key === previous?.root_key && s.id !== before)
                        ?.id ?? before;
                  }}
                  disabled={locked}
                >
                  <option value={0}>Select snapshot</option>
                  {#each snapshots as s}<option value={s.id}
                      >{s.root} · {date(s.captured)} · {bytes(s.logical)}</option
                    >{/each}
                </select></label
              >
              <label
                >After<select bind:value={after} onchange={resetComparison} disabled={locked}>
                  <option value={0}>Select snapshot</option>
                  {#each compatible as s}<option value={s.id}
                      >{date(s.captured)} · {bytes(s.logical)}</option
                    >{/each}
                </select></label
              >
              <button
                data-compare-snapshots
                onclick={() => compare(0, { before, after, scope: null, returnTo: null })}
                disabled={locked || comparisonLoading || !before || !after || before === after}
                >{comparisonLoading ? 'Comparing…' : 'Compare'}</button
              >
            </div>
          {/if}
          {#if comparisonLoading}<p role="status">
              Reading recorded changes… <button onclick={cancelComparison}>Cancel comparison</button
              >
            </p>{/if}
          {#if comparisonProblem}<p role="alert">{comparisonProblem}</p>{/if}
          {#if comparison && focusedComparison}{#key `${comparison.before}:${comparison.after}:${comparisonContext?.scope ?? 'root'}`}<SnapshotVisualCompare
                before={comparison.before}
                after={comparison.after}
                initialScope={comparisonContext?.scope ?? null}
                disabled={locked}
              />{/key}{/if}
          {#if comparison}<section class="panel change-list">
              {#if comparison.accounting === 'unknown'}<p class="history-coverage-summary">
                  Legacy accounting revision unavailable; comparability is unverified.
                </p>{/if}
              {#if comparison.file_changes}
                <SnapshotChangeBars changes={comparison.file_changes} />
                {#if comparison.file_changes.newly_observed.files || comparison.file_changes.unobserved.files}<div
                    class="history-coverage-summary"
                  >
                    {#if comparison.file_changes.newly_observed.files}<span
                        >Newly observed · {comparison.file_changes.newly_observed.files.toLocaleString()}
                        files · {bytes(comparison.file_changes.newly_observed.logical)}</span
                      >{/if}
                    {#if comparison.file_changes.unobserved.files}<span
                        >Not observed · {comparison.file_changes.unobserved.files.toLocaleString()} files
                        · {bytes(comparison.file_changes.unobserved.logical)}</span
                      >{/if}
                  </div>{/if}
              {/if}
              <div class="panel-heading">
                <h2>{difference(comparison.delta)}</h2>
                <span
                  >{comparison.total.toLocaleString()} changes{#if !comparison.coverage_complete}
                    · Partial coverage{/if}</span
                >
              </div>
              {#if !comparison.changes.length}<p class="history-no-changes">No changes</p>{/if}
              {#if comparison.changes.length}<div class="history-change-columns">
                  <span>Path</span><span>Change</span><span>Before</span><span>After</span>
                </div>{/if}
              {#each comparison.changes as change}
                <div
                  class="change-row"
                  class:growth={change.kind === 'changed' && change.after > change.before}
                  class:reduction={change.kind === 'changed' && change.after < change.before}
                >
                  <Icon name={change.directory ? 'folder' : 'file'} /><span title={change.path}
                    >{change.path}</span
                  ><small
                    >{change.kind === 'changed' && change.after !== change.before
                      ? change.after > change.before
                        ? 'Grew'
                        : 'Shrank'
                      : labels[change.kind]}</small
                  >
                  <span class="number"
                    >{change.kind === 'added'
                      ? '—'
                      : change.kind === 'newly_observed'
                        ? 'Unavailable'
                        : bytes(change.before)}</span
                  >
                  <span class="number"
                    >{change.kind === 'removed'
                      ? '—'
                      : change.kind === 'unobserved'
                        ? 'Unavailable'
                        : bytes(change.after)}</span
                  >
                </div>
              {/each}
              {#if comparison.total > 50}<div class="duplicate-pagination">
                  <button
                    onclick={() => compare(Math.max(0, changeOffset - 50))}
                    disabled={locked || changeOffset === 0}>Previous</button
                  ><span
                    >{changeOffset + 1}–{changeOffset + comparison.changes.length} of {comparison.total}</span
                  ><button
                    onclick={() => compare(changeOffset + 50)}
                    disabled={locked || changeOffset + 50 >= comparison.total}>Next</button
                  >
                </div>{/if}
            </section>{/if}
        {/if}
      </div>
    </div>
  {/if}
</div>
