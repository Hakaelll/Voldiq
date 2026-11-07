<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { IndexId } from '../lib/index-context';
  import { inspectionQueries, type FileInspection, type InspectionRequest } from '../lib/queries';
  import type { NodeId } from '../lib/types';

  let { indexId, id, paused }: { indexId: IndexId; id: NodeId; paused: boolean } = $props();
  let preview = $state<FileInspection | null>(null);
  let source = $state<FileInspection | null>(null);
  let url = $state<string | null>(null);
  let loading = $state<InspectionRequest | null>(null);
  let problem = $state('');
  let generation = 0;
  let cancel = () => {};

  function stop(): void {
    generation++;
    cancel();
    loading = null;
    url = null;
  }

  async function inspect(inspection: InspectionRequest): Promise<void> {
    if (paused || loading) return;
    const owner = ++generation;
    loading = inspection;
    problem = '';
    if (inspection === 'preview') preview = null;
    if (inspection === 'download_source') {
      source = null;
      url = null;
    }
    const request = inspectionQueries.request({ indexId, id, inspection });
    cancel = request.cancel;
    try {
      const result = await request.result;
      if (owner !== generation || result === null) return;
      if (inspection === 'preview') preview = result;
      else if (result.kind === 'download_url') url = result.url;
      else source = result;
    } catch (error) {
      if (owner === generation) problem = String(error);
    } finally {
      if (owner === generation) loading = null;
    }
  }

  $effect(() => {
    if (paused) stop();
  });
  onDestroy(stop);
</script>

<details
  ontoggle={(event) => {
    if (!event.currentTarget.open) stop();
  }}
>
  <summary>Inspect file</summary>
  <div class="inspection-actions">
    <button
      class="quiet-action"
      disabled={paused || loading !== null}
      onclick={() => inspect('preview')}>Read preview</button
    >
    <button
      class="quiet-action"
      disabled={paused || loading !== null}
      onclick={() => inspect('download_source')}>Check download source</button
    >
  </div>
  {#if loading}<p role="status">Reading…</p>{/if}
  {#if problem}<p class="inspection-error" role="alert">{problem}</p>{/if}
  {#if source?.kind === 'download_source'}
    <div class="download-source">
      <p>Recorded domain <strong>{source.domain}</strong></p>
      <button
        class="quiet-action"
        disabled={paused || loading !== null}
        aria-expanded={url !== null}
        onclick={() => {
          if (url !== null) url = null;
          else void inspect('download_url');
        }}>{url === null ? 'Reveal full URL' : 'Hide full URL'}</button
      >
      {#if url !== null}<code>{url}</code>{/if}
    </div>
  {:else if source?.kind === 'download_unavailable'}
    <p role="status">{source.reason}</p>
  {/if}
  {#if preview?.kind === 'metadata'}
    <p role="status">{preview.reason}</p>
  {:else if preview?.kind === 'text'}
    <div class="text-preview">
      <p>{preview.truncated ? 'First 64 KiB · Plain text' : 'Plain text preview'}</p>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard users need to scroll the preview.) -->
      <div class="preview-scroll" tabindex="0" role="region" aria-label="File text preview">
        <pre>{preview.text || '(Empty file)'}</pre>
      </div>
    </div>
  {:else if preview?.kind === 'image'}
    <figure>
      <img src={`data:image/png;base64,${preview.png}`} alt="Selected file preview" />
      <figcaption>{preview.width} × {preview.height} pixels</figcaption>
    </figure>
  {/if}
</details>

<style>
  details {
    border-block: 1px solid var(--border);
    padding-block: 10px;
    min-width: 0;
  }
  summary {
    width: fit-content;
    font-weight: 600;
    cursor: pointer;
  }
  .inspection-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 8px;
  }
  .inspection-actions button {
    padding-inline: 6px;
  }
  .download-source,
  .text-preview,
  figure {
    margin: 10px 0 0;
  }
  p,
  figcaption {
    font-size: 13px;
  }
  .inspection-error {
    color: var(--error);
    overflow-wrap: anywhere;
  }
  code,
  pre {
    display: block;
    background: var(--surface);
    border: 1px solid var(--border);
    padding: 8px;
    user-select: text;
    overflow-wrap: anywhere;
  }
  .preview-scroll {
    margin: 4px 0 0;
    max-height: 260px;
    overflow: auto;
  }
  pre {
    margin: 0;
    white-space: pre-wrap;
  }
  .preview-scroll:focus-visible {
    outline: 2px solid var(--accent);
  }
  img {
    display: block;
    max-width: 100%;
    max-height: 300px;
    object-fit: contain;
  }
  figcaption {
    margin-top: 4px;
    color: var(--secondary);
  }
</style>
