<script lang="ts">
  import { onDestroy } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import type { IndexId } from '../lib/index-context';
  import type { CatalogState, ViewSpec, Workspaces, RecordRef } from '../lib/workbench';
  let {
    catalog,
    capture,
    indexId,
    folder,
    path,
    disabled,
    canLoad,
    onadopt,
    onreload,
    onload,
    onscan,
    onclose,
  }: {
    catalog: CatalogState;
    capture: ViewSpec | null;
    indexId: IndexId | null;
    folder: number;
    path: string;
    disabled: boolean;
    canLoad: boolean;
    onadopt: (catalog: Workspaces) => void;
    onreload: () => void;
    onload: (reference: RecordRef) => void;
    onscan: (reference: RecordRef, path: string) => void;
    onclose: () => void;
  } = $props();
  let name = $state('');
  let selectedId = $state<number | null>(null);
  let chosenRevision = $state<string | null>(null);
  let scope = $state<ViewSpec['scope']>('current_folder');
  let saving = $state(false),
    problem = $state(''),
    notice = $state('');
  let alive = true;
  const native = isTauri();
  const value = $derived(catalog.kind === 'ready' ? catalog.value : null);
  const selected = $derived(
    chosenRevision === value?.revision
      ? value?.views.find((view) => view.id === selectedId)
      : undefined,
  );
  const unavailable = $derived(disabled || saving || !native || value === null);
  async function change(operation: () => Promise<Workspaces>, message: string): Promise<void> {
    if (unavailable) return;
    saving = true;
    problem = '';
    notice = '';
    try {
      const result = await operation();
      onadopt(result);
      if (alive) {
        selectedId = null;
        chosenRevision = null;
        notice = message;
      }
    } catch (error) {
      if (alive) problem = String(error);
    } finally {
      if (alive) saving = false;
    }
  }
  function save(mode: 'new' | 'replace' | 'rename'): void {
    if (!value || !name.trim()) return;
    const spec = mode === 'rename' ? selected?.spec : capture;
    if (!spec || (mode !== 'new' && !selected)) return;
    const request = {
      id: mode === 'new' ? null : selected?.id,
      revision: value.revision,
      name,
      spec: { ...spec, scope },
    };
    void change(() => invoke<Workspaces>('save_workspace', { request }), 'View saved.');
  }
  function remove(kind: 'view' | 'favorite', id: number): void {
    if (!value) return;
    const reference = { id, revision: value.revision };
    void change(
      () => invoke<Workspaces>('remove_workspace', { kind, reference }),
      'Saved shortcut removed.',
    );
  }
  onDestroy(() => {
    alive = false;
  });
</script>

<section class="saved-workspaces" aria-label="Saved views and favorites" aria-busy={saving}>
  <header class="saved-heading">
    <div>
      <h2>Saved workspaces</h2>
      <span>Views and favorite folders</span>
    </div>
    <div>
      <button onclick={onreload} disabled={saving || !native}>Reload</button><button
        onclick={onclose}>Close</button
      >
    </div>
  </header>
  {#if catalog.kind === 'loading'}<p role="status">Loading saved workspaces…</p>
  {:else if catalog.kind === 'failed'}<p role="alert">{catalog.message}</p>
  {:else}
    <div class="saved-columns">
      <div class="saved-section">
        <h3>Views <span>{catalog.value.views.length}</span></h3>
        <form
          onsubmit={(event) => {
            event.preventDefault();
            save('new');
          }}
        >
          <label for="workspace-name">View name</label>
          <input
            id="workspace-name"
            bind:value={name}
            maxlength="128"
            disabled={unavailable}
            placeholder="e.g. Large video archives"
          />
          <label for="workspace-scope">Apply within</label>
          <select id="workspace-scope" bind:value={scope} disabled={unavailable}>
            <option value="current_folder">Current folder when loaded</option><option
              value="analysis_root">Whole current analysis</option
            >
          </select>
          <div>
            <button type="submit" disabled={unavailable || !capture || !name.trim()}
              >Save as new view</button
            >
          </div>
          <span class="saved-help">Includes all current filters and display settings.</span>
        </form>
        {#if catalog.value.views.length}
          <label for="workspace-view">Saved view</label>
          <select
            id="workspace-view"
            value={selectedId ?? ''}
            disabled={saving}
            onchange={(event) => {
              const view = value?.views.find(
                (view) => String(view.id) === event.currentTarget.value,
              );
              selectedId = view?.id ?? null;
              chosenRevision = value?.revision ?? null;
              if (view) {
                name = view.name;
                scope = view.spec.scope;
              }
            }}
          >
            <option value="">Choose a view</option
            >{#each catalog.value.views as view (view.id)}<option value={view.id}
                >{view.name}</option
              >{/each}
          </select>
          <div class="saved-actions">
            <button
              class="primary"
              disabled={!selected || !canLoad || saving}
              onclick={() => {
                if (selected && value) onload({ id: selected.id, revision: value.revision });
              }}>Load view</button
            >
            <details>
              <summary>Manage selected view</summary>
              <div class="saved-actions">
                <button
                  disabled={unavailable || !selected || !name.trim()}
                  onclick={() => save('rename')}>Rename / change scope</button
                >
                <button
                  disabled={unavailable || !selected || !capture || !name.trim()}
                  onclick={() => save('replace')}>Replace with current view</button
                >
                <button
                  disabled={unavailable || !selected}
                  onclick={() => {
                    if (selected) remove('view', selected.id);
                  }}>Remove saved view</button
                >
              </div>
            </details>
          </div>
        {:else}<p class="saved-help">No saved views.</p>{/if}
      </div>
      <div class="saved-section">
        <h3>Favorite folders <span>{catalog.value.favorites.length}</span></h3>
        <button
          disabled={unavailable || indexId === null || !path}
          onclick={() => {
            if (!value || indexId === null) return;
            const request = { revision: value.revision, indexId, nodeId: folder };
            void change(
              () => invoke<Workspaces>('favorite_folder', request),
              'Current folder saved as a favorite.',
            );
          }}>Favorite current folder</button
        >
        {#if path}<span class="saved-help saved-path" title={path}>{path}</span>{/if}
        {#if catalog.value.favorites.length}<ul class="favorite-list">
            {#each catalog.value.favorites as favorite (favorite.id)}<li>
                <div>
                  <strong>{favorite.name}</strong><span title={favorite.path}>{favorite.path}</span>
                </div>
                <button
                  disabled={unavailable}
                  onclick={() => {
                    if (value) onscan({ id: favorite.id, revision: value.revision }, favorite.path);
                  }}>Scan</button
                >
                <button
                  disabled={unavailable}
                  aria-label={`Remove favorite ${favorite.name}`}
                  onclick={() => remove('favorite', favorite.id)}>Remove</button
                >
              </li>{/each}
          </ul>{:else}<p class="saved-help">No favorite folders.</p>{/if}
      </div>
    </div>
  {/if}
  {#if problem}<p role="alert">
      {problem} <button onclick={onreload} disabled={saving}>Reload</button>
    </p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
</section>
