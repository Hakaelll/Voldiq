<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
  import {
    createLiveSession,
    type LiveEvent,
    type LiveObservation,
    type LiveState,
  } from '../lib/live-session';
  import { bytes } from '../lib/types';
  import { utcDate } from '../lib/history-activity';
  import type { ScanProfile } from '../lib/scan-profile';
  let {
    indexId,
    path,
    disabled,
    concurrency,
    profile,
    onbusy,
    onobserved,
  }: {
    indexId: number;
    path: string;
    disabled: boolean;
    concurrency: number | 'auto';
    profile: ScanProfile;
    onbusy: (value: boolean) => void;
    onobserved: (event: LiveObservation) => Promise<void>;
  } = $props();
  let status = $state.raw<LiveState>({ kind: 'idle', reason: 'off', report: null });
  let active = $derived(
    status.kind === 'starting' || status.kind === 'watching' || status.kind === 'stopping',
  );
  let hidden = $state(false);
  let toolbar: HTMLDivElement;
  let alive = true;
  let restoreControls = false;
  const session = createLiveSession(
    {
      start: (indexId, concurrency, profile, receive) => {
        const progress = new Channel<LiveEvent>();
        progress.onmessage = receive;
        return invoke<number>('start_live', { indexId, concurrency, profile, progress });
      },
      stop: (id) => invoke('stop_live', { id }),
      acknowledge: (id, indexId) => invoke('acknowledge_live', { id, indexId }),
      resynchronize: (id) => invoke('resynchronize_live', { id }),
    },
    (next) => {
      const heldFocus = toolbar?.contains(document.activeElement);
      if (heldFocus) restoreControls = true;
      status = next;
      onbusy(next.kind === 'starting' || next.kind === 'watching' || next.kind === 'stopping');
      if (restoreControls)
        void tick().then(() => {
          if (!alive || status !== next) return;
          if (document.activeElement === document.body) {
            const candidate = toolbar?.querySelector<HTMLButtonElement>('button:not(:disabled)');
            candidate?.focus({ preventScroll: true });
            if (candidate) restoreControls = false;
          } else restoreControls = false;
        });
    },
    (event) => onobserved(event),
  );
  onMount(() => {
    const visibility = () => {
      hidden = document.visibilityState !== 'visible';
      if (hidden) void session.stop('hidden');
    };
    visibility();
    document.addEventListener('visibilitychange', visibility);
    return () => {
      alive = false;
      document.removeEventListener('visibilitychange', visibility);
      session.dispose();
      onbusy(false);
    };
  });
  const delta = $derived(status.report?.net_change ?? null);
</script>

<section class="live-control" aria-label="Live analysis">
  <div class="live-heading">
    <strong>Live</strong>
    <span class="live-root" title={path}>{path}</span>
    <span class="live-phase" role="status"
      >{status.kind === 'starting'
        ? 'Starting…'
        : status.kind === 'watching'
          ? status.report
            ? 'Watching'
            : 'Observing…'
          : status.kind === 'stopping'
            ? 'Stopping…'
            : status.kind === 'failed'
              ? 'Stopped with an error'
              : status.reason === 'hidden'
                ? 'Stopped while hidden'
                : status.reason === 'stopped'
                  ? 'Stopped'
                  : 'Off'}</span
    >
    <div class="live-actions" bind:this={toolbar}>
      {#if active}
        <button disabled={status.kind !== 'watching'} onclick={() => void session.resynchronize()}
          >Resynchronize</button
        >
        <button disabled={status.kind === 'stopping'} onclick={() => void session.stop()}
          >Stop Live</button
        >
      {:else}
        <button
          disabled={disabled || hidden || !isTauri()}
          onclick={() => void session.start(indexId, concurrency, profile)}>Start Live</button
        >
      {/if}
    </div>
  </div>
  {#if status.report}
    <div class="live-measurement">
      <span title={`${status.report.after.logical} logical bytes`}
        >{status.report.after.complete ? '' : 'Unverified coverage · '}{bytes(
          Number(status.report.after.logical),
        )} logical</span
      >
      <span
        title={delta === null
          ? 'A compatible complete baseline is unavailable'
          : `${delta} logical bytes`}
      >
        Observed net change · {delta === null
          ? 'Unavailable'
          : `${Number(delta) > 0 ? '+' : Number(delta) < 0 ? '−' : ''}${bytes(Math.abs(Number(delta)))}`}
      </span>
      <span>Measured {utcDate(status.report.after.measured_at)}</span>
      <span>Since {utcDate(status.report.before.measured_at)}</span>
      {#if status.report.reason === 'lost_events'}<span>Resynchronized after lost events</span>
      {:else if status.report.reason === 'requested'}<span>Resynchronized</span>{/if}
    </div>
  {/if}
  {#if status.kind === 'failed'}<p class="live-problem" role="alert">{status.message}</p>
  {:else if status.kind === 'stopping' && status.problem}<p class="live-problem" role="alert">
      {status.problem}
    </p>{/if}
</section>
