<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { historyClipQueries } from '../lib/queries';
  import {
    createReportExport,
    type ReportState,
    type ReportEvent,
    type ReportReceipt,
    type ReportContext,
  } from '../lib/report-export';
  import type { GifProgress, HistoryGifRequest } from '../lib/history-clip';
  import type { Snapshot } from '../lib/types';
  let {
    captures,
    disabled,
    initialDelay,
    initialLoop,
    onshow,
    onbusy,
  }: {
    captures: Snapshot[];
    disabled: boolean;
    initialDelay: 50 | 100 | 200;
    initialLoop: boolean;
    onshow: () => void;
    onbusy: (busy: boolean) => void;
  } = $props();
  let dialog: HTMLDialogElement, trigger: HTMLButtonElement;
  let mounted = false;
  let preparation: AbortController | null = null;
  let ids = $state<number[]>([]),
    paths = $state(false),
    looped = $state(false),
    delay = $state<50 | 100 | 200>(100);
  let pending = $state(false),
    preparing = $state(false),
    stopping = $state(false),
    problem = $state(''),
    cancelled = $state(false);
  let rendered = $state.raw<{ done: number; total: number } | null>(null);
  let model = $state.raw<ReportState<GifProgress>>({ kind: 'idle' });
  const session = createReportExport<Uint8Array, 'gif', GifProgress>(
    {
      save: (_owner, packet) => invoke<ReportReceipt<GifProgress>>('save_history_gif', packet),
      read: (owner) => invoke<ReportEvent<GifProgress> | null>('report_progress', { ...owner }),
      cancel: (owner) => invoke<boolean>('cancel_report', { ...owner }),
    },
    (value) => {
      model = value;
    },
  );
  onMount(() => {
    mounted = true;
  });
  onDestroy(() => {
    mounted = false;
    preparation?.abort();
    historyClipQueries.cancel();
    session.dispose();
    onbusy(false);
  });
  function open(): void {
    if (!mounted || disabled || captures.length < 2 || captures.length > 60) return;
    ids = captures.map((capture) => capture.id);
    paths = false;
    delay = initialDelay;
    looped = initialLoop;
    problem = '';
    cancelled = false;
    model = { kind: 'idle' };
    onshow();
    dialog.showModal();
  }
  function stop(): void {
    stopping = true;
    preparation?.abort();
    historyClipQueries.cancel();
    session.cancel();
  }
  function close(): void {
    if (pending) {
      stop();
      return;
    }
    dialog.close();
    trigger?.focus({ preventScroll: true });
  }
  async function save(): Promise<void> {
    if (!mounted || disabled || pending) return;
    pending = preparing = true;
    stopping = cancelled = false;
    problem = '';
    rendered = null;
    onbusy(true);
    const controller = new AbortController();
    preparation = controller;
    try {
      const clip = await historyClipQueries.request({
        snapshot_ids: [...ids],
        include_paths: paths,
      }).result;
      controller.signal.throwIfAborted();
      if (!clip)
        throw new Error('Capture preparation was cancelled. Retry to read the selected captures.');
      const { prepareHistoryFrames, gifPacket, HISTORY_WIDTH, HISTORY_HEIGHT } =
        await import('../lib/history-export');
      controller.signal.throwIfAborted();
      const css = getComputedStyle(dialog);
      const style = {
        surface: css.getPropertyValue('--canvas').trim(),
        text: css.getPropertyValue('--text').trim(),
        secondary: css.getPropertyValue('--secondary').trim(),
      };
      const images = await prepareHistoryFrames(clip, style, controller.signal, (done, total) => {
        if (mounted) rendered = { done, total };
      });
      controller.signal.throwIfAborted();
      const owner: ReportContext = {
        owner: { kind: 'history', fingerprint: clip.fingerprint },
        requestId: crypto.randomUUID(),
      };
      const request: HistoryGifRequest = {
        clip: {
          snapshot_ids: clip.frames.map((frame) => frame.snapshot),
          include_paths: clip.paths_included,
        },
        fingerprint: clip.fingerprint,
        width: HISTORY_WIDTH,
        height: HISTORY_HEIGHT,
        delay_centiseconds: delay,
        looped,
      };
      const packet = gifPacket(owner.requestId, request, images);
      controller.signal.throwIfAborted();
      preparation = null;
      preparing = false;
      if (mounted) await session.save(owner, packet, 'gif');
    } catch (error) {
      if (mounted) {
        if (controller.signal.aborted) cancelled = true;
        else problem = error instanceof Error ? error.message : String(error);
      }
    } finally {
      preparation = null;
      if (mounted) {
        pending = preparing = false;
        onbusy(false);
      }
    }
  }
  function stage(value: GifProgress): string {
    switch (value.stage) {
      case 'preparing':
        return 'Checking recorded captures';
      case 'encoding':
        return 'Encoding GIF frames';
      case 'revalidating':
        return 'Rechecking source captures';
      case 'finalizing':
        return 'Finalizing and saving';
      default: {
        const exhaustive: never = value.stage;
        return exhaustive;
      }
    }
  }
</script>

<button
  type="button"
  bind:this={trigger}
  title="Export 2-60 captures from the loaded root/accounting group"
  onclick={open}
  disabled={disabled || captures.length < 2 || captures.length > 60}>Export GIF</button
>
<dialog
  bind:this={dialog}
  class="history-export"
  aria-labelledby="history-export-title"
  oncancel={(event) => {
    event.preventDefault();
    close();
  }}
  onkeydown={(event) => event.stopPropagation()}
>
  <header>
    <h2 id="history-export-title">Export history</h2>
    <button type="button" onclick={close}>{pending ? 'Request stop' : 'Close'}</button>
  </header>
  <p>
    {ids.length} captures from the loaded root/accounting group. Root-level logical sizes, recorded dates
    and coverage are included. File names are visible.
  </p>
  <form
    onsubmit={(event) => {
      event.preventDefault();
      void save();
    }}
  >
    <fieldset disabled={pending}>
      <legend>GIF animation</legend>
      <label
        >Speed <select bind:value={delay}
          ><option value={200}>0.5 captures / second</option><option value={100}
            >1 capture / second</option
          ><option value={50}>2 captures / second</option></select
        ></label
      >
      <label><input type="checkbox" bind:checked={looped} />Loop</label>
      <label
        ><input type="checkbox" bind:checked={paths} />Include full root and exclusion paths</label
      >
      <p>1000 × 560 pixels · up to 256 colors per frame. Playback timing is illustrative.</p>
    </fieldset>
    <div class="history-export-status" aria-live="polite" aria-atomic="true">
      {#if preparing}
        {#if rendered}<progress
            aria-label="Captures rendered"
            value={rendered.done}
            max={rendered.total}
          ></progress>{/if}
        <p>
          {stopping
            ? 'Stopping…'
            : rendered
              ? `Rendering captures · ${rendered.done} of ${rendered.total}`
              : 'Reading recorded captures…'}
        </p>
      {:else if problem}<p class="history-export-error" role="alert">{problem}</p>
      {:else if cancelled}<p>Export cancelled. No GIF was published.</p>
      {:else if model.kind === 'choosing'}<p>
          {model.cancelling
            ? 'Cancellation requested. Close the Windows save dialog to return.'
            : 'Choose a new GIF file in the Windows save dialog.'}
        </p>
      {:else if model.kind === 'running'}
        <progress
          aria-label="Frames processed in the current stage"
          value={model.progress.completed_frames}
          max={Math.max(1, model.progress.total_frames)}
        ></progress>
        <p>
          {model.cancelling ? 'Stopping…' : stage(model.progress)} · {model.progress
            .completed_frames} of {model.progress.total_frames}
        </p>
      {:else if model.kind === 'failed'}<p class="history-export-error" role="alert">
          {model.message}
        </p>
      {:else if model.kind === 'finished'}
        {#if model.receipt.outcome.kind === 'complete'}<p>GIF saved.</p>
          <p class="history-export-path">{model.receipt.outcome.path}</p>{:else}<p>
            Export cancelled. No GIF was published.
          </p>{/if}
      {/if}
    </div>
    <footer>
      {#if pending}<button
          type="button"
          onclick={stop}
          disabled={stopping ||
            ((model.kind === 'choosing' || model.kind === 'running') && model.cancelling)}
          >Request stop</button
        >{:else}<button type="submit" class="primary" {disabled}>Choose file and save</button>{/if}
    </footer>
  </form>
</dialog>

<style>
  .history-export {
    width: min(470px, calc(100vw - 32px));
    max-height: calc(100dvh - 48px);
    overflow: auto;
    padding: 18px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--canvas);
    color: var(--text);
  }
  .history-export::backdrop {
    background: #0005;
  }
  header,
  footer {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    align-items: center;
    justify-content: space-between;
  }
  h2 {
    margin: 0;
    font-size: 17px;
  }
  p {
    margin: 10px 0;
    font-size: 12px;
    color: var(--secondary);
  }
  fieldset {
    margin: 16px 0 12px;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    min-width: 0;
  }
  legend,
  label {
    font-size: 12px;
  }
  label {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    margin: 8px 0;
  }
  .history-export-status {
    min-height: 26px;
    margin-bottom: 12px;
  }
  .history-export-path,
  .history-export-error {
    overflow-wrap: anywhere;
  }
  .history-export-error {
    color: var(--error);
  }
  progress {
    width: 100%;
  }
</style>
