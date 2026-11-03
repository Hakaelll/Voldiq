<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import {
    createReportExport,
    type ReportState,
    type ReportReceipt,
    type ReportEvent,
  } from '../lib/report-export';
  import type { SvgRequest } from '../lib/graphic-export';
  import type { IndexId } from '../lib/index-context';
  let {
    indexId,
    request,
    onclose,
    onbusy,
  }: {
    indexId: IndexId;
    request: SvgRequest;
    onclose: () => void;
    onbusy: (busy: boolean) => void;
  } = $props();
  let dialog: HTMLDialogElement;
  let mounted = false;
  let pending = $state(false),
    paths = $state(false);
  let model = $state.raw<ReportState>({ kind: 'idle' });
  const session = createReportExport<SvgRequest, 'svg'>(
    {
      save: (context, request) => invoke<ReportReceipt>('save_svg', { ...context, request }),
      read: (context) => invoke<ReportEvent | null>('report_progress', { ...context }),
      cancel: (context) => invoke<boolean>('cancel_report', { ...context }),
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
    session.dispose();
    onbusy(false);
  });
  function close(): void {
    if (pending) {
      session.cancel();
      return;
    }
    dialog.close();
    onclose();
  }
  async function save(): Promise<void> {
    if (!mounted || pending) return;
    pending = true;
    onbusy(true);
    try {
      await session.save(
        { indexId, requestId: crypto.randomUUID() },
        { ...request, include_paths: paths },
        'svg',
      );
    } finally {
      if (mounted) {
        pending = false;
        onbusy(false);
      }
    }
  }
</script>

<dialog
  bind:this={dialog}
  class="svg-export"
  aria-labelledby="svg-title"
  oncancel={(event) => {
    event.preventDefault();
    close();
  }}
  onkeydown={(event) => event.stopPropagation()}
>
  <header>
    <h2 id="svg-title">Export SVG</h2>
    <button type="button" onclick={close}>{pending ? 'Request stop' : 'Close'}</button>
  </header>
  <p>
    Captured {request.view.representation === 'city' ? 'Skyline' : request.view.representation} view,
    with source scope and coverage. File names are included; fonts are not embedded.
  </p>
  <form
    onsubmit={(event) => {
      event.preventDefault();
      void save();
    }}
  >
    <fieldset disabled={pending}>
      <legend>Shared context</legend><label
        ><input type="checkbox" bind:checked={paths} />Include full paths and filter text</label
      >
    </fieldset>
    <div class="svg-status" aria-live="polite" aria-atomic="true">
      {#if model.kind === 'choosing'}<p>
          {model.cancelling
            ? 'Cancellation requested. Close the Windows save dialog to return.'
            : 'Choose a new SVG file in the Windows save dialog.'}
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
              ? 'Validating SVG and saving…'
              : 'Preparing source context…'}
          {model.progress.inspected.toLocaleString()} of {model.progress.total_entries.toLocaleString()}
          records inspected
        </p>
      {:else if model.kind === 'failed'}<p class="svg-error" role="alert">{model.message}</p>
      {:else if model.kind === 'finished'}
        {#if model.receipt.outcome.kind === 'complete'}<p>SVG saved.</p>
          <p class="svg-path">{model.receipt.outcome.path}</p>
        {:else}<p>Export cancelled. No SVG was published.</p>{/if}
      {/if}
    </div>
    <footer>
      {#if pending}<button
          type="button"
          disabled={(model.kind === 'choosing' || model.kind === 'running') && model.cancelling}
          onclick={() => session.cancel()}>Request stop</button
        >{:else}<button class="primary" type="submit">Choose file and save</button>{/if}
    </footer>
  </form>
</dialog>

<style>
  .svg-export {
    width: min(470px, calc(100vw - 32px));
    max-height: calc(100dvh - 48px);
    overflow: auto;
    padding: 18px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--canvas);
    color: var(--text);
  }
  .svg-export::backdrop {
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
  }
  .svg-status {
    min-height: 26px;
    margin-bottom: 12px;
  }
  .svg-path,
  .svg-error {
    overflow-wrap: anywhere;
  }
  .svg-error {
    color: var(--error);
  }
  progress {
    width: 100%;
  }
</style>
