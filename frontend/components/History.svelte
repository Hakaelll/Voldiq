<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import Icon from './Icon.svelte';
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
  let previous = $derived(snapshots.find((s) => s.id === before));
  let compatible = $derived(snapshots.filter((s) => s.root_key === previous?.root_key));
  let trend = $derived([...compatible].reverse());
  let points = $derived.by(() => {
    if (trend.length < 2) return '';
    const low = Math.min(...trend.map((s) => s.logical));
    const high = Math.max(...trend.map((s) => s.logical));
    return trend
      .map(
        (s, i) =>
          `${12 + (i * 576) / (trend.length - 1)},${88 - ((s.logical - low) * 68) / Math.max(1, high - low)}`,
      )
      .join(' ');
  });
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
    <section class="panel snapshot-list" aria-label="Saved snapshots">
      {#each snapshots as snapshot (snapshot.id)}
        <article class="snapshot-row">
          <Icon name="disk" />
          <div class="snapshot-name">
            <strong title={snapshot.root}>{snapshot.root}</strong><small
              >{date(snapshot.captured)}{#if !snapshot.complete}
                · Partial{/if}</small
            >
          </div>
          <span class="number">{bytes(snapshot.logical)}</span>
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
          {#each snapshots as s}<option value={s.id}
              >{s.root} · {date(s.captured)} · {bytes(s.logical)}</option
            >{/each}
        </select></label
      >
      <label
        >After<select bind:value={after} onchange={resetComparison} disabled={working}>
          {#each compatible as s}<option value={s.id}
              >{date(s.captured)} · {bytes(s.logical)}</option
            >{/each}
        </select></label
      >
      <button onclick={() => compare()} disabled={working || !before || !after || before === after}
        >Compare</button
      >
    </div>
    {#if trend.length > 1}<section class="panel history-trend" aria-label="Recorded logical size">
        <div class="panel-heading">
          <h2>Recorded size</h2>
          <span>{bytes(trend[0]?.logical ?? 0)} → {bytes(trend.at(-1)?.logical ?? 0)}</span>
        </div>
        <svg
          viewBox="0 0 600 104"
          role="img"
          aria-label={`Logical size across ${trend.length} saved snapshots`}
          ><polyline
            {points}
            fill="none"
            stroke="var(--accent)"
            stroke-width="2"
            vector-effect="non-scaling-stroke"
          /></svg
        >
      </section>{/if}
    {#if comparison}<section class="panel change-list">
        <div class="panel-heading">
          <h2>{difference(comparison.delta)}</h2>
          <span
            >{comparison.total.toLocaleString()} changes{#if !comparison.reliable}
              · Partial coverage{/if}</span
          >
        </div>
        {#if !comparison.changes.length}<p class="history-no-changes">No changes</p>{/if}
        {#each comparison.changes as change}
          <div class="change-row">
            <Icon name={change.directory ? 'folder' : 'file'} /><span title={change.path}
              >{change.path}</span
            ><small>{labels[change.kind]}</small><strong class="number"
              >{difference(change.delta)}</strong
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
  {/if}
</div>
