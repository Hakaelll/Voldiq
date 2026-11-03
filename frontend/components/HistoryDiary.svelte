<script lang="ts">
  import { tick } from 'svelte';
  import ActivityCaptureSummary from './ActivityCaptureSummary.svelte';
  import { diaryQueries } from '../lib/queries';
  import { utcDate, type DiaryLocation, type HistoricalComparison } from '../lib/history-activity';
  import {
    bytes,
    type Snapshot,
    type DiaryPage,
    type DiaryEvent,
    type SnapshotChange,
  } from '../lib/types';
  let {
    captures,
    location,
    disabled,
    onlocation,
    oncompare,
  }: {
    captures: Snapshot[];
    location: DiaryLocation;
    disabled: boolean;
    onlocation: (location: DiaryLocation) => void;
    oncompare: (comparison: HistoricalComparison) => Promise<void>;
  } = $props();
  const limit = 5;
  let retry = $state(0),
    region = $state<HTMLElement>();
  type Context = DiaryLocation;
  type Model =
    | { kind: 'loading' }
    | { kind: 'ready'; context: Context; value: DiaryPage }
    | { kind: 'failed'; context: Context; message: string };
  let model = $state.raw<Model>({ kind: 'loading' });
  const current = $derived(
    model.kind !== 'loading' &&
      model.context.snapshot === location.snapshot &&
      model.context.offset === location.offset
      ? model
      : null,
  );
  const diary = $derived(current?.kind === 'ready' && !disabled ? current.value : null);
  $effect(() => {
    const context = { snapshot: location.snapshot, offset: location.offset };
    void retry;
    const retain = region?.contains(document.activeElement) ?? false;
    model = { kind: 'loading' };
    if (disabled) return;
    let alive = true;
    async function focus(restore: boolean): Promise<void> {
      if (!restore) return;
      await tick();
      if (alive && document.activeElement === document.body)
        (
          region?.querySelector<HTMLElement>(
            '[data-activity-reload]:not(:disabled),select:not(:disabled)',
          ) ?? region
        )?.focus();
    }
    void diaryQueries.request({ ...context, limit }).result.then(
      async (value) => {
        if (!alive) return;
        const restore = retain || (region?.contains(document.activeElement) ?? false);
        model =
          value?.anchor === context.snapshot
            ? { kind: 'ready', context, value }
            : { kind: 'failed', context, message: 'Diary query cancelled. Reload to retry.' };
        await focus(restore);
      },
      async (problem: unknown) => {
        if (!alive) return;
        const restore = retain || (region?.contains(document.activeElement) ?? false);
        model = { kind: 'failed', context, message: String(problem) };
        await focus(restore);
      },
    );
    return () => {
      alive = false;
      diaryQueries.cancel();
    };
  });
  function describe(change: SnapshotChange): string {
    switch (change.kind) {
      case 'added':
        return 'Newly recorded';
      case 'removed':
        return 'No longer recorded';
      case 'newly_observed':
        return 'Newly observed · earlier coverage incomplete';
      case 'unobserved':
        return 'Not observed · later coverage incomplete';
      case 'changed':
        return change.delta > 0
          ? 'Recorded size grew'
          : change.delta < 0
            ? 'Recorded size shrank'
            : 'Metadata changed';
    }
  }
  async function inspect(event: DiaryEvent, scope: string | null): Promise<void> {
    const before = event.before;
    if (disabled || !before || !diary?.events.includes(event)) return;
    await oncompare({
      before: before.id,
      after: event.after.id,
      scope,
      returnTo: { snapshot: location.snapshot, offset: location.offset },
    });
  }
</script>

<section
  class="panel history-activity"
  aria-label="Observed history diary"
  tabindex="-1"
  bind:this={region}
>
  <div class="activity-toolbar">
    <label
      >Through capture<select
        value={location.snapshot}
        onchange={(event) => onlocation({ snapshot: Number(event.currentTarget.value), offset: 0 })}
        {disabled}
        >{#each captures as capture}<option value={capture.id}
            >#{capture.id} · {utcDate(capture.captured)}</option
          >{/each}</select
      ></label
    >
    <button data-activity-reload onclick={() => retry++} {disabled}>Reload diary</button>
    <button onclick={() => diaryQueries.cancel()} disabled={disabled || current !== null}
      >Cancel query</button
    >
    <button
      onclick={() => onlocation({ ...location, offset: Math.max(0, location.offset - limit) })}
      disabled={disabled || location.offset === 0}>Newer observations</button
    >
    <button
      onclick={() => onlocation({ ...location, offset: location.offset + limit })}
      disabled={disabled || !diary?.has_more || location.offset >= 10000}>Older observations</button
    >
  </div>
  <p class="activity-help">
    Adjacent retained captures in one history group. Intermediate changes, causes and actors are not
    established. Capture choices cover this history page; the diary can page further back.
  </p>
  {#if disabled}<p role="status">Diary paused during the current operation.</p>
  {:else if current?.kind === 'failed'}<p role="alert">{current.message}</p>
  {:else if !diary}<p role="status">Reading retained observations…</p>
  {:else}
    <h3 class="activity-root">{diary.root}</h3>
    {#if !diary.events.length}<p>No observations on this page.</p>{/if}
    {#each diary.events as event (event.after.id)}
      <article class="diary-event">
        <div class="diary-event-heading">
          <h3>{utcDate(event.after.captured)}</h3>
          {#if event.before}<span>#{event.before.id} → #{event.after.id}</span>{:else}<span
              >First retained observation</span
            >{/if}
        </div>
        <ActivityCaptureSummary capture={event.after} />
        {#if event.before}<p class="activity-help">
            Earlier capture #{event.before.id} · saved {utcDate(event.before.captured)} · {event
              .before.complete
              ? 'recorded complete coverage'
              : 'partial coverage'}{event.before.accounting === 'unknown'
              ? ' · legacy accounting unverified'
              : event.before.accounting === 'unsupported'
                ? ' · unsupported accounting'
                : ''}
          </p>{/if}
        {#if event.detail.kind === 'baseline'}<p>
            Baseline for this retained history. Earlier observations may have been retired.
          </p>
        {:else if event.detail.kind === 'incompatible'}<p class="activity-warning">
            Accounting revisions are incompatible. A size comparison is unavailable.
          </p>
        {:else}
          <p class="diary-difference">
            Recorded root difference <strong title={`${event.detail.delta} logical bytes`}
              >{BigInt(event.detail.delta) > 0n ? '+' : ''}{bytes(
                Number(event.detail.delta) < 0
                  ? -Number(event.detail.delta)
                  : Number(event.detail.delta),
              )}{BigInt(event.detail.delta) < 0n
                ? ' smaller'
                : BigInt(event.detail.delta) > 0n
                  ? ' larger'
                  : ''}</strong
            >
          </p>
          {#if !event.detail.comparison.coverage_complete}<p class="activity-warning">
              Partial coverage. Missing entries do not confirm deletion.
            </p>{/if}
          {#if event.detail.comparison.accounting === 'unknown'}<p class="activity-help">
              Legacy accounting is unavailable; comparability is unverified.
            </p>{/if}
          <p class="activity-help">
            {event.detail.comparison.total.toLocaleString()} recorded changes · up to five ranked paths
            below. Folder totals overlap their descendants.
          </p>
          {#if event.before}<button
              onclick={() => inspect(event, null)}
              aria-label={`Compare captures ${event.before.id} and ${event.after.id}`}
              {disabled}>Compare these captures</button
            >{/if}
          {#if !event.detail.comparison.changes.length}<p>No recorded changes.</p>{/if}
          <ul class="diary-changes">
            {#each event.detail.comparison.changes as change}<li>
                <div>
                  <strong>{describe(change)}</strong><span>{change.path}</span><small
                    >{change.kind === 'added' || change.kind === 'newly_observed'
                      ? change.kind === 'added'
                        ? 'Not recorded'
                        : 'Unavailable'
                      : bytes(change.before)} → {change.kind === 'removed' ||
                    change.kind === 'unobserved'
                      ? change.kind === 'removed'
                        ? 'Not recorded'
                        : 'Unavailable'
                      : bytes(change.after)}</small
                  >
                </div>
                {#if event.before}<button
                    onclick={() => inspect(event, change.path_id)}
                    aria-label={`Inspect recorded context for ${change.path}`}
                    {disabled}>Inspect context</button
                  >{/if}
              </li>{/each}
          </ul>
        {/if}
      </article>
    {/each}
    {#if diary.has_more && location.offset >= 10000}<p class="activity-help">
        The 10,000-observation paging bound is reached. Choose an older anchor capture to continue.
      </p>{/if}
  {/if}
</section>
