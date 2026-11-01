<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import type { FolderMetadata } from '../lib/folder-metadata';
  let { path }: { path: string } = $props();
  type LoadState = { kind: 'loading' } | { kind: 'failed' } | { kind: 'ready'; value: FolderMetadata };
  let model = $state<LoadState>({ kind: 'loading' });
  let draft = $state(''),
    busy = $state(false),
    error = $state(''),
    notice = $state('');
  const native = isTauri();
  let alive = true;
  let size = $derived(new TextEncoder().encode(draft).length);

  async function load(): Promise<void> {
    if (!native) {
      model = { kind: 'failed' };
      error = 'Notes require the Windows application.';
      return;
    }
    model = { kind: 'loading' };
    error = '';
    try {
      const value = await invoke<FolderMetadata>('get_folder_metadata', { path });
      if (alive) {
        model = { kind: 'ready', value };
        draft = value.folder.note;
      }
    } catch (problem) {
      if (alive) {
        model = { kind: 'failed' };
        error = String(problem);
      }
    }
  }
  async function save(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (busy || size > 8192) return;
    busy = true;
    error = '';
    notice = '';
    try {
      const value = await invoke<FolderMetadata>('save_folder_note', {
        request: { path, note: draft },
      });
      if (alive) {
        model = { kind: 'ready', value };
        notice = 'Note saved.';
      }
    } catch (problem) {
      if (alive) error = String(problem);
    } finally {
      if (alive) busy = false;
    }
  }
  async function protect(): Promise<void> {
    if (model.kind !== 'ready' || busy) return;
    const folder = model.value.folder;
    busy = true;
    error = '';
    notice = '';
    try {
      if (folder.protected && folder.id !== null) {
        const removed = await invoke<boolean>('remove_folder_protection', {
          id: folder.id,
          version: folder.version,
        });
        if (alive && removed) {
          const value = await invoke<FolderMetadata>('get_folder_metadata', { path });
          if (alive) {
            model = { kind: 'ready', value };
            notice = 'Folder protection removed.';
          }
        }
      } else {
        const value = await invoke<FolderMetadata>('protect_folder', { path });
        if (alive) {
          model = { kind: 'ready', value };
          notice = 'Folder and contents protected from cleanup.';
        }
      }
    } catch (problem) {
      if (alive) error = String(problem);
    } finally {
      if (alive) busy = false;
    }
  }
  onMount(() => {
    void load();
  });
  onDestroy(() => {
    alive = false;
  });
</script>

<section
  class="folder-notes"
  aria-label="Folder notes and protection"
  aria-busy={busy || model.kind === 'loading'}
>
  {#if model.kind === 'loading'}<p class="muted">Loading folder notes…</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if model.kind === 'failed' && native}<button onclick={load}>Retry</button>{/if}
  {#if model.kind === 'ready'}
    <div class="protection-control">
      <span
        >{model.value.folder.protected
          ? 'Protected from cleanup'
          : model.value.protected_by_parent
            ? 'Protected by parent or Voldiq'
            : 'Folder protection'}</span
      >
      <button onclick={protect} disabled={busy || model.value.protected_by_parent}
        >{model.value.folder.protected ? 'Remove protection…' : 'Protect folder'}</button
      >
    </div>
    <form onsubmit={save}>
      <label for="folder-note">Folder note</label>
      <textarea
        id="folder-note"
        bind:value={draft}
        rows="4"
        disabled={busy}
        maxlength="8192"
        aria-describedby="folder-note-size"
        placeholder="Add a note about this folder"></textarea>
      <div class="note-footer">
        <small id="folder-note-size" class:error={size > 8192}
          >{size.toLocaleString()} / 8,192 bytes</small
        ><button type="submit" disabled={busy || size > 8192 || draft === model.value.folder.note}
          >Save note</button
        >
      </div>
    </form>
  {/if}
  {#if notice}<p role="status">{notice}</p>{/if}
</section>

<style>
  .folder-notes {
    border-top: 1px solid var(--border);
    margin-top: 16px;
    padding-top: 14px;
    font-size: 12px;
  }
  .protection-control,
  .note-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 8px;
  }
  .protection-control {
    margin-bottom: 14px;
  }
  form {
    display: grid;
    gap: 8px;
  }
  label {
    font-weight: 600;
  }
  textarea {
    width: 100%;
    min-height: 80px;
    resize: vertical;
  }
  small,
  .muted {
    color: var(--muted);
  }
  .error {
    color: var(--error);
    overflow-wrap: anywhere;
  }
</style>
