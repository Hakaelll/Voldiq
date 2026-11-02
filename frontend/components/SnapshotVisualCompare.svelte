<script lang="ts">
  import Icon from './Icon.svelte';
  import { tick } from 'svelte';
  import { snapshotVisualQueries } from '../lib/queries';
  import HistoricalMap from './HistoricalMap.svelte';
  import { bytes, date, type SnapshotVisualPair, type SnapshotVisualFrame } from '../lib/types';

  let {
    before,
    after,
    disabled = false,
  }: { before: number; after: number; disabled?: boolean } = $props();
  let scope = $state<string | null>(null);
  let folderCursor = $state<string | null>(null),
    reverse = $state(false),
    retry = $state(0);
  let divider = $state(50),
    width = $state(800);
  let plot = $state<HTMLDivElement>();
  let region = $state<HTMLElement>();
  let dragging: number | null = null;
  type Context = {
    before: number;
    after: number;
    scope: string | null;
    folderCursor: string | null;
    reverse: boolean;
  };
  type Model =
    | { kind: 'loading' }
    | { kind: 'ready'; context: Context; pair: SnapshotVisualPair }
    | { kind: 'failed'; context: Context; message: string };
  let model = $state.raw<Model>({ kind: 'loading' });
  const current = $derived(
    model.kind !== 'loading' &&
      model.context.before === before &&
      model.context.after === after &&
      model.context.scope === scope &&
      model.context.folderCursor === folderCursor &&
      model.context.reverse === reverse
      ? model
      : null,
  );
  const pair = $derived(current?.kind === 'ready' && !disabled ? current.pair : null);
  const maximum = $derived(
    Math.max(Number(pair?.before.logical ?? 0), Number(pair?.after.logical ?? 0)),
  );
  const frames = $derived(pair ? [pair.before, pair.after] : []);
  const listed = $derived.by(() => {
    const entries = new Map<
      string,
      {
        path_id: string;
        name: string;
        directory: boolean;
        before: string | null;
        after: string | null;
      }
    >();
    for (const [i, frame] of frames.entries())
      for (const entry of frame.entries) {
        const row = entries.get(entry.path_id) ?? {
          path_id: entry.path_id,
          name: entry.name,
          directory: false,
          before: null,
          after: null,
        };
        row.directory ||= entry.kind === 'directory';
        if (i === 0) row.before = entry.logical;
        else row.after = entry.logical;
        entries.set(entry.path_id, row);
      }
    return [...entries.values()].sort((a, b) => a.name.localeCompare(b.name));
  });
  $effect(() => {
    void retry;
    const context: Context = { before, after, scope, folderCursor, reverse };
    const retainFocus = region?.contains(document.activeElement) ?? false;
    model = { kind: 'loading' };
    dragging = null;
    if (disabled) return;
    let alive = true;
    async function restoreFocus(): Promise<void> {
      if (!retainFocus) return;
      await tick();
      if (alive && document.activeElement === document.body) {
        (
          region?.querySelector<HTMLElement>('select:not(:disabled),button:not(:disabled)') ??
          region
        )?.focus();
      }
    }
    void snapshotVisualQueries.request(context).result.then(
      async (value) => {
        if (alive)
          model = value
            ? { kind: 'ready', context, pair: value }
            : { kind: 'failed', context, message: 'Comparison query cancelled. Reload to retry.' };
        await restoreFocus();
      },
      async (problem: unknown) => {
        if (alive) model = { kind: 'failed', context, message: String(problem) };
        await restoreFocus();
      },
    );
    return () => {
      alive = false;
      snapshotVisualQueries.cancel();
    };
  });
  function navigate(id: string | null): void {
    if (!pair || disabled) return;
    scope = id;
    folderCursor = null;
    reverse = false;
  }
  function resetRoot(): void {
    if (disabled) return;
    scope = folderCursor = null;
    reverse = false;
    retry++;
  }
  function page(cursor: string, backwards: boolean): void {
    if (!pair || disabled) return;
    folderCursor = cursor;
    reverse = backwards;
  }
  function measured(frame: SnapshotVisualFrame): string {
    return frame.observation
      ? `Measured ${date(frame.observation.completed_at)}`
      : 'Measurement date unavailable';
  }
  function setDivider(value: number): void {
    divider = Math.max(0, Math.min(100, Math.round(value)));
  }
  function drag(event: PointerEvent): void {
    if (dragging !== event.pointerId || !pair || !plot) return;
    const box = plot.getBoundingClientRect();
    if (box.width > 0) setDivider(((event.clientX - box.left) / box.width) * 100);
  }
  function startDrag(event: PointerEvent): void {
    if (event.button !== 0 || !pair) return;
    const handle = event.currentTarget;
    if (!(handle instanceof HTMLElement)) return;
    event.preventDefault();
    dragging = event.pointerId;
    handle.focus({ preventScroll: true });
    handle.setPointerCapture(event.pointerId);
    drag(event);
  }
  function dividerKey(event: KeyboardEvent): void {
    const key = event.key;
    if (
      ![
        'ArrowLeft',
        'ArrowRight',
        'ArrowUp',
        'ArrowDown',
        'Home',
        'End',
        'PageUp',
        'PageDown',
      ].includes(key)
    )
      return;
    event.preventDefault();
    if (key === 'Home') setDivider(0);
    else if (key === 'End') setDivider(100);
    else
      setDivider(
        divider +
          (key === 'ArrowRight' || key === 'ArrowUp' || key === 'PageUp' ? 1 : -1) *
            (key === 'PageUp' || key === 'PageDown' ? 10 : 1),
      );
  }
</script>

<section
  class="panel snapshot-visual"
  aria-label="Visual snapshot comparison"
  tabindex="-1"
  bind:this={region}
  aria-busy={!pair && current?.kind !== 'failed'}
>
  <div class="panel-heading">
    <h2>Recorded logical sizes</h2>
    <button {disabled} onclick={() => retry++}>Reload</button>
  </div>
  {#if current?.kind === 'failed'}<p role="alert">{current.message}</p>
    <button {disabled} onclick={resetRoot}>Reset to root</button>
  {:else if !pair}<p role="status">
      {disabled
        ? 'Comparison paused during the current operation.'
        : 'Loading historical folder summaries…'}
    </p>
  {:else}
    <div class="snapshot-visual-nav">
      <button onclick={() => navigate(null)} disabled={pair.parent_id === null}>Root</button>
      <button onclick={() => navigate(pair.parent_id)} disabled={pair.parent_id === null}
        ><Icon name="chevron-up" size={14} />Up</button
      >
      <strong title={pair.path}>{pair.path}</strong>
    </div>
    <div class="snapshot-visual-folders">
      <label
        >Open folder in both captures<select
          value=""
          onchange={(event) => {
            if (event.currentTarget.value) navigate(event.currentTarget.value);
          }}
          disabled={!pair.folders.items.length}
        >
          <option value="">Select an indexed folder…</option>
          {#each pair.folders.items as folder}<option value={folder.path_id}>{folder.name}</option
            >{/each}
        </select></label
      >
      <button
        disabled={!pair.folders.previous}
        onclick={() => {
          if (pair?.folders.previous) page(pair.folders.previous, true);
        }}>Previous folders</button
      >
      <button
        disabled={!pair.folders.next}
        onclick={() => {
          if (pair?.folders.next) page(pair.folders.next, false);
        }}>More folders</button
      >
      <span>{pair.folders.items.length} folders on this page</span>
    </div>
    {#if pair.accounting === 'unknown'}<p class="snapshot-visual-warning">
        Accounting revision is unavailable in a legacy capture; size comparability is unverified.
      </p>{/if}
    {#if pair.before.incomplete || pair.after.incomplete}<p class="snapshot-visual-warning">
        Coverage is incomplete. Recorded sizes are lower bounds; missing entries may be unobserved.
      </p>{/if}
    <div class="snapshot-visual-captions">
      {#each frames as frame, i}<div>
          <strong
            >{i === 0 ? 'Before' : 'After'} · {frame.logical === null
              ? 'Unavailable'
              : `${frame.incomplete ? 'At least ' : ''}${bytes(Number(frame.logical))}`}</strong
          ><span>Saved {date(frame.captured)}</span><small>{measured(frame)}</small>
        </div>{/each}
    </div>
    <div class="snapshot-visual-plot" bind:this={plot} bind:clientWidth={width}>
      {#each frames as frame, i}
        <HistoricalMap
          {frame}
          {maximum}
          {width}
          id={i === 0 ? 'snapshot-map-before' : 'snapshot-map-after'}
          front={i === 0}
          reveal={divider}
          label={`${i === 0 ? 'Before' : 'After'} recorded logical sizes in ${pair.path}`}
        />
      {/each}
      <div class="snapshot-visual-line" style:left={`${divider}%`} aria-hidden="true"></div>
      <div
        class="snapshot-visual-handle"
        style:left={`${divider}%`}
        role="slider"
        tabindex="0"
        aria-label="Comparison divider, reveal before capture"
        aria-valuemin="0"
        aria-valuemax="100"
        aria-valuenow={divider}
        aria-valuetext={`${divider}% before, ${100 - divider}% after`}
        aria-controls="snapshot-map-before snapshot-map-after"
        onkeydown={dividerKey}
        onpointerdown={startDrag}
        onpointermove={drag}
        onpointerup={() => (dragging = null)}
        onlostpointercapture={() => (dragging = null)}
      >
        <span aria-hidden="true">↔</span>
      </div>
    </div>
    <div class="snapshot-visual-controls">
      <button onclick={() => setDivider(100)}>Show before</button><span
        >Drag the divider · arrows adjust · Home / End reveal a capture</span
      ><button onclick={() => setDivider(0)}>Show after</button>
    </div>
    <p class="snapshot-visual-basis">
      Logical path bytes use the same area scale in both captures. Blank plot area is not free disk
      space. Up to 160 entries per side; Other retains the recorded remainder.
    </p>
    <p class="snapshot-visual-delta">
      Recorded-size difference: <strong
        >{pair.delta === null
          ? 'Unavailable for a missing scope'
          : `${BigInt(pair.delta) > 0n ? '+' : ''}${BigInt(pair.delta).toLocaleString()} bytes`}</strong
      >{#if pair.relative_percent !== null}<span>
          · {pair.relative_percent > 0 ? '+' : ''}{pair.relative_percent.toLocaleString(undefined, {
            maximumFractionDigits: 1,
          })}%</span
        >{:else if pair.delta !== null}<span>
          · Relative change unavailable for a zero baseline</span
        >{/if}
    </p>
    <details class="snapshot-visual-list">
      <summary>Map entries · {listed.length} ranked names</summary>
      <p>
        Not listed can mean an entry was omitted by the map limit. It does not establish absence.
        Open a full snapshot for all records.
      </p>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (The named scroll region needs keyboard scrolling.) -->
      <div
        class="snapshot-visual-table"
        role="region"
        tabindex="0"
        aria-label="Ranked historical map entries"
      >
        <table>
          <thead><tr><th>Name</th><th>Before</th><th>After</th></tr></thead><tbody
            >{#each listed as row}<tr
                ><td
                  >{#if row.directory}<button onclick={() => navigate(row.path_id)}
                      >{row.name}</button
                    >{:else}{row.name}{/if}</td
                ><td>{row.before === null ? 'Not listed' : bytes(Number(row.before))}</td><td
                  >{row.after === null ? 'Not listed' : bytes(Number(row.after))}</td
                ></tr
              >{/each}</tbody
          >
        </table>
      </div>
    </details>
  {/if}
</section>
