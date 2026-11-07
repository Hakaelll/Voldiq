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
    hideIdle = false,
    concurrency,
    profile,
    onbusy,
    onobserved,
  }: {
    indexId: number;
    path: string;
    disabled: boolean;
    hideIdle?: boolean;
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
  let toolbar = $state<HTMLDivElement>();
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

{#if !hideIdle || active || status.kind === 'failed'}
  <section class="live-control" aria-label="Live analysis" title={path}>
    <div class="live-heading">
      {#if status.kind !== 'idle' || status.reason !== 'off'}<span class="live-phase" role="status"
          >{status.kind === 'starting'
            ? 'Starting…'
            : status.kind === 'watching'
              ? status.report
                ? 'Watching'
                : 'Observing…'
              : status.kind === 'stopping'
                ? 'Stopping…'
                : status.kind === 'failed'
                  ? 'Live stopped with an error'
                  : status.reason === 'hidden'
                    ? 'Live stopped while hidden'
                    : status.reason === 'stopped'
                      ? 'Live stopped'
                      : 'Off'}</span
        >{/if}
      <div class="live-actions" bind:this={toolbar}>
        {#if active}
          <button
            class="quiet-action"
            disabled={status.kind !== 'watching'}
            onclick={() => void session.resynchronize()}>Resynchronize</button
          >
          <button
            class="quiet-action"
            disabled={status.kind === 'stopping'}
            onclick={() => void session.stop()}>Stop Live</button
          >
        {:else}
          <button
            class="quiet-action"
            disabled={disabled || hidden || !isTauri()}
            onclick={() => void session.start(indexId, concurrency, profile)}>Start Live</button
          >
        {/if}
      </div>
    </div>
    {#if status.report}
      <details class="live-report">
        <summary>Live measurements</summary>
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
      </details>
    {/if}
    {#if status.kind === 'failed'}<p class="live-problem" role="alert">{status.message}</p>
    {:else if status.kind === 'stopping' && status.problem}<p class="live-problem" role="alert">
        {status.problem}
      </p>{/if}
  </section>
{/if}
