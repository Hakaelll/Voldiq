<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import {
    createReportExport,
    type ReportState,
    type ReportReceipt,
    type ReportEvent,
  } from '../lib/report-export';
  import {
    mapSvg,
    mapPng,
    pngDimensions,
    pngPacket,
    type GraphicContext,
    type GraphicScene,
    type SvgRequest,
  } from '../lib/graphic-export';
  import type { IndexId } from '../lib/index-context';
  let {
    indexId,
    context,
    scene,
    onclose,
    onbusy,
  }: {
    indexId: IndexId;
    context: GraphicContext;
    scene: GraphicScene;
    onclose: () => void;
    onbusy: (busy: boolean) => void;
  } = $props();
  type Prepared = { kind: 'svg'; request: SvgRequest } | { kind: 'png'; packet: Uint8Array };
  let dialog: HTMLDialogElement;
  let mounted = false;
  let preparation: AbortController | null = null;
  let pending = $state(false),
    preparing = $state(false),
    paths = $state(false);
  let format = $state<'svg' | 'png'>('png'),
    scale = $state<1 | 2>(1);
  let problem = $state(''),
    preparationStopped = $state(false);
  const size = $derived(pngDimensions(context.view, scale));
  let model = $state.raw<ReportState>({ kind: 'idle' });
  const session = createReportExport<Prepared, 'svg' | 'png'>(
    {
      save: (owner, value) =>
        value.kind === 'svg'
          ? invoke<ReportReceipt>('save_svg', { ...owner, request: value.request })
          : invoke<ReportReceipt>('save_png', value.packet),
      read: (owner) => invoke<ReportEvent | null>('report_progress', { ...owner }),
      cancel: (owner) => invoke<boolean>('cancel_report', { ...owner }),
    },
    (value) => {
      model = value;
    },
  );
  onMount(() => {
    mounted = true;
    dialog.showModal();
  });
  onDestroy(() => {
    mounted = false;
    preparation?.abort();
    session.dispose();
    onbusy(false);
  });
  function stop(): void {
    preparation?.abort();
    session.cancel();
  }
  function close(): void {
    if (pending) {
      stop();
      return;
    }
    dialog.close();
    onclose();
  }
  async function save(): Promise<void> {
    if (!mounted || pending || (format === 'png' && !size)) return;
    pending = preparing = true;
    problem = '';
    preparationStopped = false;
    onbusy(true);
    const controller = new AbortController();
    preparation = controller;
    try {
      const owner = { indexId, requestId: crypto.randomUUID() };
      const captured = { ...context, include_paths: paths };
      let value: Prepared;
      if (format === 'png') {
        const image = await mapPng(scene, scale, controller.signal);
        value = { kind: 'png', packet: pngPacket({ ...owner, context: captured, scale }, image) };
      } else {
        const measure = document.createElement('canvas').getContext('2d');
        if (!measure) throw new Error('Canvas text measurement is unavailable.');
        const drawing = await mapSvg(scene, (text, font, baseline) => {
          measure.font = font;
          measure.textBaseline = baseline;
          return measure.measureText(text);
        });
        value = { kind: 'svg', request: { context: captured, drawing } };
      }
      controller.signal.throwIfAborted();
      preparation = null;
      preparing = false;
      if (mounted) await session.save(owner, value, format);
    } catch (error) {
      if (mounted) {
        if (controller.signal.aborted) preparationStopped = true;
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
</script>

<dialog
  bind:this={dialog}
  class="graphic-export"
  aria-labelledby="graphic-title"
  oncancel={(event) => {
    event.preventDefault();
    close();
  }}
  onkeydown={(event) => event.stopPropagation()}
>
  <header>
    <h2 id="graphic-title">Export map</h2>
    <button type="button" onclick={close}>{pending ? 'Request stop' : 'Close'}</button>
  </header>
  <p>
    Captured {context.view.representation === 'city' ? 'Skyline' : context.view.representation} view,
    with source scope and coverage. File names are included.
  </p>
  <form
    onsubmit={(event) => {
      event.preventDefault();
      void save();
    }}
  >
    <fieldset disabled={pending}>
      <legend>Image</legend>
      <label
        >Format <select bind:value={format}
          ><option value="png">PNG image</option><option value="svg">SVG vector</option></select
        ></label
      >
      {#if format === 'png'}
        <label
          >Resolution <select bind:value={scale}
            ><option value={1}>1×</option><option value={2}>2×</option></select
          ></label
        >
        <p>
          {size
            ? `${size.width.toLocaleString()} × ${size.height.toLocaleString()} pixels`
            : 'Choose a lower resolution. Maximum 16.8 megapixels and 8192 pixels per side.'}
        </p>
      {:else}<p>Fonts are not embedded.</p>{/if}
      <label><input type="checkbox" bind:checked={paths} />Include full paths and filter text</label
      >
    </fieldset>
    <div class="graphic-status" aria-live="polite" aria-atomic="true">
      {#if preparing}<p>Preparing image…</p>
      {:else if problem}<p class="graphic-error" role="alert">{problem}</p>
      {:else if preparationStopped}<p>Export cancelled. No image was published.</p>
      {:else if model.kind === 'choosing'}<p>
          {model.cancelling
            ? 'Cancellation requested. Close the Windows save dialog to return.'
            : 'Choose a new file in the Windows save dialog.'}
        </p>
      {:else if model.kind === 'running'}
        <progress
          aria-label="Source records inspected"
          value={model.progress.inspected}
          max={Math.max(1, model.progress.total_entries)}
        ></progress>
        <p>
          {model.cancelling
            ? 'Stopping…'
            : model.progress.inspected >= model.progress.total_entries
              ? 'Encoding image and saving…'
              : 'Preparing source context…'}
          {model.progress.inspected.toLocaleString()} of {model.progress.total_entries.toLocaleString()}
          records inspected
        </p>
      {:else if model.kind === 'failed'}<p class="graphic-error" role="alert">{model.message}</p>
      {:else if model.kind === 'finished'}
        {#if model.receipt.outcome.kind === 'complete'}<p>Image saved.</p>
          <p class="graphic-path">{model.receipt.outcome.path}</p>
        {:else}<p>Export cancelled. No image was published.</p>{/if}
      {/if}
    </div>
    <footer>
      {#if pending}<button
          type="button"
          disabled={(model.kind === 'choosing' || model.kind === 'running') && model.cancelling}
          onclick={stop}>Request stop</button
        >
      {:else}<button class="primary" type="submit" disabled={format === 'png' && !size}
          >Choose file and save</button
        >{/if}
    </footer>
  </form>
</dialog>

<style>
  .graphic-export {
    width: min(470px, calc(100vw - 32px));
    max-height: calc(100dvh - 48px);
    overflow: auto;
    padding: 18px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--canvas);
    color: var(--text);
  }
  .graphic-export::backdrop {
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
    gap: 8px;
    align-items: center;
    margin: 8px 0;
  }
  .graphic-status {
    min-height: 26px;
    margin-bottom: 12px;
  }
  .graphic-path,
  .graphic-error {
    overflow-wrap: anywhere;
  }
  .graphic-error {
    color: var(--error);
  }
  progress {
    width: 100%;
  }
</style>
