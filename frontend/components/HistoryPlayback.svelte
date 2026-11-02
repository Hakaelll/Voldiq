<script lang="ts">
  import { onMount } from 'svelte';
  import HistoricalMap from './HistoricalMap.svelte';
  import { snapshotCaptureQueries } from '../lib/queries';
  import { bytes, date, type Snapshot, type SnapshotVisualFrame } from '../lib/types';

  let {
    captures,
    disabled = false,
    onopen,
  }: {
    captures: Snapshot[];
    disabled?: boolean;
    onopen: (snapshot: Snapshot) => Promise<void>;
  } = $props();
  const ordered = $derived([...captures].sort((a, b) => a.captured - b.captured || a.id - b.id));
  let position = $state(0),
    playing = $state(false),
    speed = $state(1),
    loop = $state(false),
    retry = $state(0),
    width = $state(800);
  const selected = $derived(ordered[position] ?? ordered[0]);
  type Model =
    | { kind: 'loading' }
    | { kind: 'ready'; snapshot: number; frame: SnapshotVisualFrame }
    | { kind: 'failed'; snapshot: number; message: string };
  let model = $state.raw<Model>({ kind: 'loading' });
  const current = $derived(
    model.kind !== 'loading' && model.snapshot === selected?.id ? model : null,
  );
  const frame = $derived(current?.kind === 'ready' && !disabled ? current.frame : null);
  const maximum = $derived(
    Math.max(
      0,
      ...ordered
        .filter(
          (capture) =>
            capture.observation?.accounting_version === selected?.observation?.accounting_version,
        )
        .map((capture) => capture.logical),
    ),
  );

  $effect(() => {
    const snapshot = selected?.id;
    void retry;
    model = { kind: 'loading' };
    if (!snapshot || disabled) return;
    let alive = true;
    void snapshotCaptureQueries.request({ snapshot }).result.then(
      (value) => {
        if (!alive) return;
        if (value?.snapshot === snapshot) model = { kind: 'ready', snapshot, frame: value };
        else {
          model = {
            kind: 'failed',
            snapshot,
            message: 'Capture query cancelled. Reload to retry.',
          };
          playing = false;
        }
      },
      (problem: unknown) => {
        if (alive) {
          model = { kind: 'failed', snapshot, message: String(problem) };
          playing = false;
        }
      },
    );
    return () => {
      alive = false;
      snapshotCaptureQueries.cancel();
    };
  });
  $effect(() => {
    if (disabled) playing = false;
  });
  $effect(() => {
    if (!playing || !frame || disabled || ordered.length < 2) return;
    const next = position + 1;
    const repeat = loop;
    const timer = setTimeout(() => {
      if (next < ordered.length) position = next;
      else if (repeat) position = 0;
      else playing = false;
    }, 1000 / speed);
    return () => clearTimeout(timer);
  });
  onMount(() => {
    const pause = () => {
      if (document.hidden) playing = false;
    };
    document.addEventListener('visibilitychange', pause);
    return () => document.removeEventListener('visibilitychange', pause);
  });
  function step(value: number): void {
    playing = false;
    position = Math.max(0, Math.min(ordered.length - 1, Math.round(value)));
  }
  function toggle(): void {
    if (disabled || ordered.length < 2) return;
    if (!playing && position === ordered.length - 1) position = 0;
    playing = !playing;
  }
  async function open(): Promise<void> {
    playing = false;
    if (selected && !disabled) await onopen(selected);
  }
</script>

<section class="panel snapshot-playback" aria-label="Recorded snapshot playback">
  <div class="snapshot-playback-toolbar">
    <button class="primary" onclick={toggle} disabled={disabled || ordered.length < 2}
      >{playing ? 'Pause' : 'Play'}</button
    >
    <button onclick={() => step(position - 1)} disabled={disabled || position === 0}
      >Previous</button
    >
    <button onclick={() => step(position + 1)} disabled={disabled || position >= ordered.length - 1}
      >Next</button
    >
    <label
      >Speed<select bind:value={speed} {disabled}
        ><option value={0.5}>0.5 captures / second</option><option value={1}
          >1 capture / second</option
        ><option value={2}>2 captures / second</option></select
      ></label
    >
    <label class="snapshot-playback-loop"
      ><input type="checkbox" bind:checked={loop} {disabled} />Loop</label
    >
  </div>
  <label class="snapshot-playback-timeline"
    >Capture {ordered.length ? position + 1 : 0} of {ordered.length}
    <input
      type="range"
      min="0"
      max={Math.max(0, ordered.length - 1)}
      step="1"
      value={position}
      oninput={(event) => step(event.currentTarget.valueAsNumber)}
      disabled={disabled || ordered.length < 2}
      aria-valuetext={selected
        ? `Capture ${position + 1} of ${ordered.length}, saved ${date(selected.captured)}`
        : 'No captures'}
    />
  </label>
  <p class="snapshot-visual-basis">
    Root-level maps for compatible captures on this history page. Playback waits for each real
    frame; no sizes or dates are interpolated. Return to captures to load another page or folder
    group.
  </p>
  {#if selected}<div class="snapshot-playback-caption">
      <strong>{selected.root}</strong><span>Saved {date(selected.captured)}</span><button
        onclick={open}
        {disabled}>Open full snapshot</button
      >
    </div>{/if}
  {#if disabled}<p role="status">Playback paused during the current operation.</p>
  {:else if current?.kind === 'failed'}<p role="alert">
      {current.message} <button onclick={() => retry++}>Reload capture</button>
    </p>
  {:else if !frame}<p role="status">Reading recorded capture…</p>
  {:else}
    <div class="snapshot-playback-values">
      <strong>{frame.incomplete ? 'At least ' : ''}{bytes(Number(frame.logical))}</strong><span
        >{frame.observation
          ? `Measured ${date(frame.observation.completed_at)} · accounting revision ${frame.observation.accounting_version}`
          : 'Measurement date and accounting revision unavailable'}</span
      >
    </div>
    {#if frame.incomplete}<p class="snapshot-visual-warning">
        Partial coverage; recorded sizes are lower bounds.
      </p>{/if}
    <div class="snapshot-visual-plot" bind:clientWidth={width}>
      <HistoricalMap
        {frame}
        {maximum}
        {width}
        id="snapshot-playback-map"
        label={`Recorded logical sizes in ${selected?.root ?? 'the captured root'}`}
      />
    </div>
    <p class="snapshot-visual-basis">
      Scale: largest recorded root in the loaded accounting group. Legacy comparability is
      unverified. Blank plot area is not free space. Up to 160 entries; Other retains the recorded
      remainder.
    </p>
    <details class="snapshot-visual-list">
      <summary>Map entries · {frame.entries.length} ranked names</summary>
      <ul>
        {#each frame.entries as entry}<li>
            {entry.name} · {entry.incomplete ? 'At least ' : ''}{bytes(Number(entry.logical))}
          </li>{/each}{#if frame.remainder}<li>
            Other indexed contents · {bytes(Number(frame.remainder.logical))}
          </li>{/if}
      </ul>
    </details>
  {/if}
</section>
