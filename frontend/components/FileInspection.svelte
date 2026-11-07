<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { IndexId } from '../lib/index-context';
  import { inspectionQueries, type FileInspection, type InspectionRequest } from '../lib/queries';
  import { bytes, type NodeId } from '../lib/types';

  let { indexId, id, paused }: { indexId: IndexId; id: NodeId; paused: boolean } = $props();
  let preview = $state<FileInspection | null>(null);
  let source = $state<FileInspection | null>(null);
  let url = $state<string | null>(null);
  let loading = $state<InspectionRequest | null>(null);
  let problem = $state('');
  let archiveOffset = $state(0);
  const archivePage = 24;
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
    if (inspection === 'preview') {
      preview = null;
      archiveOffset = 0;
    }
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
  {:else if preview?.kind === 'archive'}
    {@const total = preview.items.length}
    <section class="archive-preview" aria-label="Archive entries">
      <p>
        <strong>{preview.format.toUpperCase()}</strong> · {total} virtual {total === 1
          ? 'entry'
          : 'entries'}
      </p>
      {#if preview.limit || preview.omitted}
        <p role="status">
          {#if preview.limit}
            Partial list. {preview.limit === 'bytes'
              ? 'Read limit reached.'
              : 'Entry limit reached.'}
          {/if}
          {#if preview.omitted}
            {preview.omitted} unsafe, unsupported or deeply nested records skipped.
          {/if}
        </p>
      {/if}
      {#if total}
        <ul>
          {#each preview.items.slice(archiveOffset, archiveOffset + archivePage) as item}
            <li>
              <span>{item.path}</span>
              {#if item.kind === 'file' && item.logical_bytes !== null}
                <small title={`${item.logical_bytes} logical bytes declared by the archive`}>
                  {bytes(Number(item.logical_bytes))}
                </small>
              {:else}
                <small>{item.kind === 'folder' ? 'Folder' : 'Link'}</small>
              {/if}
            </li>
          {/each}
        </ul>
      {:else}
        <p>No displayable entries.</p>
      {/if}
      {#if total > archivePage}
        <div class="archive-pages" role="group" aria-label="Archive pages">
          <button
            class="quiet-action"
            disabled={archiveOffset === 0}
            onclick={() => (archiveOffset -= archivePage)}>Previous</button
          >
          <span role="status"
            >{archiveOffset + 1}–{Math.min(archiveOffset + archivePage, total)}</span
          >
          <button
            class="quiet-action"
            disabled={archiveOffset + archivePage >= total}
            onclick={() => (archiveOffset += archivePage)}>Next</button
          >
        </div>
      {/if}
    </section>
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
  .archive-preview,
  figure {
    margin: 10px 0 0;
  }
  p,
  figcaption {
    font-size: 13px;
  }
  .archive-preview ul {
    list-style: none;
    margin: 6px 0;
    padding: 0;
  }
  .archive-preview li,
  .archive-pages {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    font-size: 13px;
  }
  .archive-preview li {
    padding-block: 5px;
    border-top: 1px solid var(--border);
  }
  .archive-preview li span {
    overflow-wrap: anywhere;
  }
  .archive-preview li small {
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
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
