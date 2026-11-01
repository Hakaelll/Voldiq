<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { getIndexContext, type IndexId } from '../lib/index-context';
  import type { FolderRead } from '../lib/folder-metadata';
  import { date } from '../lib/types';
  let { nodeId, indexId }: { nodeId: number; indexId: IndexId | null } = $props();
  type Preview =
    { kind: 'loading' } | { kind: 'unavailable' } | { kind: 'ready'; value: FolderRead };
  let preview = $state<Preview>({ kind: 'loading' });
  let alive = true;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let snippet = $derived.by(() => {
    if (preview.kind !== 'ready' || preview.value.note.kind !== 'plain_text') return '';
    const characters = [...preview.value.note.text.replace(/\s+/gu, ' ').trim()];
    return characters.slice(0, 180).join('') + (characters.length > 180 ? '…' : '');
  });
  onMount(() => {
    const owner = indexId;
    const folder = nodeId;
    if (!isTauri() || owner === null || owner !== getIndexContext()) {
      preview = { kind: 'unavailable' };
      return;
    }
    timer = setTimeout(() => {
      void invoke<FolderRead>('get_indexed_folder_metadata', { indexId: owner, nodeId: folder })
        .then((value) => {
          if (alive)
            preview =
              owner === getIndexContext() ? { kind: 'ready', value } : { kind: 'unavailable' };
        })
        .catch(() => {
          if (alive) preview = { kind: 'unavailable' };
        });
    }, 140);
  });
  onDestroy(() => {
    alive = false;
    clearTimeout(timer);
  });
</script>

<div class="folder-note-preview" aria-live="polite">
  {#if preview.kind === 'loading'}<p class="muted">Loading folder note…</p>
  {:else if preview.kind === 'unavailable'}<p class="muted">Folder note unavailable</p>
  {:else}
    {#if preview.value.protected_from_cleanup}<p>Protected from cleanup</p>{/if}
    {#if preview.value.note.kind === 'plain_text'}
      <p class="note-text">
        {snippet || (preview.value.note.text ? 'Blank folder note' : 'No folder note')}
      </p>
      {#if preview.value.note.updated !== null}<p class="muted">
          Current note · {date(preview.value.note.updated)}
        </p>{/if}
    {:else}<p class="muted">Folder note unavailable</p>{/if}
  {/if}
</div>

<style>
  .folder-note-preview {
    border-top: 1px solid var(--border);
    margin-top: 6px;
    padding-top: 6px;
    font-size: 12px;
  }
  p {
    margin: 0;
    line-height: 1.4;
    overflow-wrap: anywhere;
  }
  p + p {
    margin-top: 4px;
  }
  .note-text {
    color: var(--text);
  }
  .muted {
    color: var(--muted);
    font-size: 11px;
  }
</style>
