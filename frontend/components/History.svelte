<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import Icon from './Icon.svelte';
  import { historyTrend } from '../lib/history-trend';
  import { updateSnapshotPair } from '../lib/snapshot-pair';
  import { bytes, date, difference, type Snapshot, type SnapshotComparison } from '../lib/types';
  let {
    enabled,
    onbusy,
    onload,
    onerror,
  }: {
    enabled: boolean;
    onbusy: (busy: boolean) => void;
    onload: (snapshot: Snapshot) => Promise<void>;
    onerror: (problem: unknown) => void;
  } = $props();
  const limit = 20;
  let snapshots = $state<Snapshot[]>([]);
  let offset = $state(0),
    storage = $state(0),
    working = $state(false);
  let before = $state(0),
    after = $state(0),
    changeOffset = $state(0);
  let comparison = $state<SnapshotComparison | null>(null);
  let deleting = $state<number | null>(null);
  let loadState = $state<'loading' | 'ready' | 'unavailable'>(isTauri() ? 'loading' : 'ready');
  let region: HTMLDivElement;
  let saveButton: HTMLButtonElement;
  let cancelButton = $state<HTMLButtonElement>();
  let request = 0,
    comparisonRequest = 0;
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
  async function mutate(action: () => Promise<void>): Promise<void> {
    if (working) return;
    working = true;
    onbusy(true);
    comparisonRequest += 1;
    try {
      await action();
    } catch (problem) {
      onerror(problem);
    } finally {
      working = false;
      onbusy(false);
    }
  }
  async function save(): Promise<void> {
    await mutate(async () => {
      await invoke('save_snapshot');
      await refresh(0);
    });
  }
  async function open(snapshot: Snapshot): Promise<void> {
    await mutate(async () => {
      await invoke('load_snapshot', { id: snapshot.id });
      await onload(snapshot);
    });
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
  async function compare(next = 0): Promise<void> {
    if (!before || !after || before === after || working) return;
    const token = ++comparisonRequest;
    try {
      const result = await invoke<SnapshotComparison>('compare_snapshots', {
        before,
        after,
        offset: next,
        limit: 50,
      });
      if (token === comparisonRequest) {
        comparison = result;
        changeOffset = next;
      }
    } catch (problem) {
      if (token === comparisonRequest) onerror(problem);
    }
  }
  function resetComparison(): void {
    comparison = null;
    comparisonRequest += 1;
  }
  onMount(() => {
    if (isTauri()) void refresh(0);
  });
  onDestroy(() => {
    request += 1;
    comparisonRequest += 1;
  });
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
      bind:this={saveButton}
      class="primary"
      onclick={save}
      disabled={!enabled || working || !isTauri()}><Icon name="history" />Save snapshot</button
    >
    <span
      >{#if loadState === 'ready'}{bytes(storage)} stored{/if}</span
    >
    <button
      onclick={() =>
        mutate(async () => {
          await invoke('compact_snapshots');
          await refresh();
        })}
      disabled={working || !isTauri() || !storage || loadState !== 'ready'}>Compact</button
    >
  </div>
  {#if loadState !== 'ready'}
    <section class="panel history-empty" aria-live="polite">
      <h2>{loadState === 'loading' ? 'Loading…' : 'History unavailable'}</h2>
      {#if loadState === 'unavailable'}<button onclick={() => refresh()} disabled={working}
          >Retry</button
        >{/if}
    </section>
  {:else if snapshots.length === 0 && offset === 0}
    <section class="panel history-empty">
      <Icon name="history" size={28} />
      <h2>No snapshots yet</h2>
    </section>
  {:else}
    <div class="history-layout">
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
                disabled={working ||
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
              <button onclick={() => open(snapshot)} disabled={working}>Open</button>
              {#if deleting === snapshot.id}
                <button
                  bind:this={cancelButton}
                  onclick={() => cancelDelete(snapshot.id)}
                  disabled={working}>Cancel</button
                ><button class="danger" onclick={() => remove(snapshot.id)} disabled={working}
                  >Delete snapshot</button
                >
              {:else}
                <button
                  title="Delete snapshot"
                  data-delete-snapshot={snapshot.id}
                  aria-label={`Delete snapshot from ${date(snapshot.captured)}`}
                  onclick={() => confirmDelete(snapshot.id)}
                  disabled={working}><Icon name="trash" /></button
                >
              {/if}
            </article>
          {/each}
          {#if offset || snapshots.length === limit}<div class="duplicate-pagination">
              <button
                onclick={() => refresh(Math.max(0, offset - limit))}
                disabled={working || offset === 0}>Previous</button
              ><span
                >{snapshots.length
                  ? `${offset + 1}–${offset + snapshots.length}`
                  : 'End of history'}</span
              ><button
                onclick={() => refresh(offset + limit)}
                disabled={working || snapshots.length < limit}>Next</button
              >
            </div>{/if}
        </section>
        {#if chart}<section class="panel history-trend" aria-label="Recorded logical size">
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
                <polyline
                  points={chart.points.map((point) => `${point.x},${point.y}`).join(' ')}
                  fill="none"
                  stroke="var(--accent)"
                  stroke-width="2"
                  vector-effect="non-scaling-stroke"
                />
                {#each chart.points as point, i}
                  <circle cx={point.x} cy={point.y} r="3" fill="var(--accent)"
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
          </section>{/if}
      </div>
      <div class="history-comparison">
        <h2>Compare snapshots</h2>
        <div class="history-compare">
          <label
            >Before<select
              bind:value={before}
              onchange={() => {
                resetComparison();
                after =
                  snapshots.find((s) => s.root_key === previous?.root_key && s.id !== before)?.id ??
                  before;
              }}
              disabled={working}
            >
              <option value={0}>Select snapshot</option>
              {#each snapshots as s}<option value={s.id}
                  >{s.root} · {date(s.captured)} · {bytes(s.logical)}</option
                >{/each}
            </select></label
          >
          <label
            >After<select bind:value={after} onchange={resetComparison} disabled={working}>
              <option value={0}>Select snapshot</option>
              {#each compatible as s}<option value={s.id}
                  >{date(s.captured)} · {bytes(s.logical)}</option
                >{/each}
            </select></label
          >
          <button
            onclick={() => compare()}
            disabled={working || !before || !after || before === after}>Compare</button
          >
        </div>
        {#if comparison}<section class="panel change-list">
            {#if comparison.file_changes}
              <div class="history-change-summary">
                <div>
                  <span>Added</span><strong>{bytes(comparison.file_changes.added.logical)}</strong
                  ><small>{comparison.file_changes.added.files.toLocaleString()} files</small>
                </div>
                <div>
                  <span>Changed</span><strong
                    >{difference(comparison.file_changes.changed.delta)}</strong
                  ><small>{comparison.file_changes.changed.files.toLocaleString()} files</small>
                </div>
                <div>
                  <span>Removed</span><strong
                    >{bytes(comparison.file_changes.removed.logical)}</strong
                  ><small>{comparison.file_changes.removed.files.toLocaleString()} files</small>
                </div>
              </div>
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
                >{comparison.total.toLocaleString()} changes{#if !comparison.reliable}
                  · Partial coverage{/if}</span
              >
            </div>
            {#if !comparison.changes.length}<p class="history-no-changes">No changes</p>{/if}
            {#if comparison.changes.length}<div class="history-change-columns">
                <span>Path</span><span>Change</span><span>Before</span><span>After</span>
              </div>{/if}
            {#each comparison.changes as change}
              <div class="change-row">
                <Icon name={change.directory ? 'folder' : 'file'} /><span title={change.path}
                  >{change.path}</span
                ><small>{labels[change.kind]}</small>
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
                  disabled={working || changeOffset === 0}>Previous</button
                ><span
                  >{changeOffset + 1}–{changeOffset + comparison.changes.length} of {comparison.total}</span
                ><button
                  onclick={() => compare(changeOffset + 50)}
                  disabled={working || changeOffset + 50 >= comparison.total}>Next</button
                >
              </div>{/if}
          </section>{/if}
      </div>
    </div>
  {/if}
</div>
