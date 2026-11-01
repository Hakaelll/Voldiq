<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import type { FolderNote, ProtectedPage } from '../lib/folder-metadata';
  const native = isTauri();
  const limit = 25;
  let page = $state<ProtectedPage>({ items: [], next_offset: null });
  let offset = $state(0),
    loading = $state(true),
    busy = $state(false),
    error = $state(''),
    notice = $state('');
  let alive = true;
  async function refresh(start = offset): Promise<void> {
    if (!native) {
      loading = false;
      return;
    }
    loading = true;
    error = '';
    try {
      const result = await invoke<ProtectedPage>('list_protected_folders', {
        offset: start,
        limit,
      });
      if (alive) {
        page = result;
        offset = start;
      }
    } catch (problem) {
      if (alive) error = String(problem);
    } finally {
      if (alive) loading = false;
    }
  }
  async function add(): Promise<void> {
    busy = true;
    error = '';
    notice = '';
    try {
      const path = await invoke<string | null>('choose_folder');
      if (!alive || !path) return;
      await invoke('protect_folder', { path });
      if (alive) {
        notice = 'Folder and contents protected from cleanup.';
        await refresh(0);
      }
    } catch (problem) {
      if (alive) error = String(problem);
    } finally {
      if (alive) busy = false;
    }
  }
  async function remove(folder: FolderNote): Promise<void> {
    if (folder.id === null) return;
    busy = true;
    error = '';
    notice = '';
    try {
      const removed = await invoke<boolean>('remove_folder_protection', {
        id: folder.id,
        version: folder.version,
      });
      if (alive && removed) {
        notice = 'Folder protection removed. Notes kept.';
        await refresh();
      }
    } catch (problem) {
      if (alive) {
        error = String(problem);
      }
    } finally {
      if (alive) busy = false;
    }
  }
  onMount(() => {
    void refresh(0);
  });
  onDestroy(() => {
    alive = false;
  });
</script>

<section
  class="protected-folders"
  aria-labelledby="protected-folders-title"
  aria-busy={loading || busy}
>
  <div class="protection-heading">
    <h2 id="protected-folders-title">Protected folders</h2>
    <div class="protection-actions">
      <button onclick={() => refresh()} disabled={!native || loading || busy}>Refresh</button
      ><button onclick={add} disabled={!native || loading || busy}>Protect a folder…</button>
    </div>
  </div>
  <p class="muted">
    Exclude folders and their contents from cleanup. Protection follows a renamed folder and also
    guards its recorded path. Voldiq’s data is always protected.
  </p>
  {#if !native}<p class="notice">Protection management requires the Windows application.</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
  {#if loading}<p class="muted">Loading protected folders…</p>
  {:else if native && page.items.length === 0 && !error}<p class="muted">
      No protected folders on this page.
    </p>
  {:else}
    <ul>
      {#each page.items as folder (folder.id)}<li>
          <span class="protected-path">{folder.path}</span><button
            onclick={() => remove(folder)}
            disabled={busy}
            aria-label={`Remove protection for ${folder.path}`}>Remove protection…</button
          >
        </li>{/each}
    </ul>
  {/if}
  {#if offset > 0 || page.next_offset !== null}<nav aria-label="Protected folder pages">
      <button
        disabled={loading || busy || offset === 0}
        onclick={() => refresh(Math.max(0, offset - limit))}>Previous</button
      ><button
        disabled={loading || busy || page.next_offset === null}
        onclick={() => {
          if (page.next_offset !== null) void refresh(page.next_offset);
        }}>Next</button
      >
    </nav>{/if}
</section>

<style>
  .protection-heading,
  .protection-actions,
  li,
  nav {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 10px;
  }
  .protection-actions {
    justify-content: flex-end;
  }
  .muted {
    color: var(--muted);
    font-size: 12px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    border-bottom: 1px solid var(--border);
    padding: 10px 0;
  }
  .protected-path {
    flex: 1 1 250px;
    overflow-wrap: anywhere;
    font-size: 12px;
  }
  .error {
    color: var(--error);
    overflow-wrap: anywhere;
  }
  nav {
    justify-content: flex-end;
    margin-top: 12px;
  }
</style>
