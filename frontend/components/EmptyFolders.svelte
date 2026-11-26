<script lang="ts">
  import { isTauri } from '@tauri-apps/api/core';
  import { onDestroy, tick, untrack } from 'svelte';
  import type { IndexId } from '../lib/index-context';
  import { emptyFolderQueries, type EmptyFolderPage } from '../lib/queries';
  import { date, type NodeId } from '../lib/types';
  import Icon from './Icon.svelte';

  let {
    indexId,
    enabled,
    revision,
    onopen,
  }: {
    indexId: IndexId | null;
    enabled: boolean;
    revision: number;
    onopen: (id: NodeId) => void;
  } = $props();

  type Cursor = NodeId | null;
  type State =
    | { kind: 'idle' | 'loading' }
    | { kind: 'error'; message: string }
    | { kind: 'ready'; page: EmptyFolderPage; after: Cursor; previous: Cursor[] };
  let state = $state<State>({ kind: 'idle' });
  let region: HTMLElement;
  let cancel = () => {};
  let generation = 0;

  async function load(after: Cursor, previous: Cursor[]): Promise<void> {
    if (!enabled || indexId === null || !isTauri()) return;
    const owner = indexId;
    const turn = ++generation;
    const hadFocus = region?.contains(document.activeElement);
    state = { kind: 'loading' };
    cancel();
    const request = emptyFolderQueries.request({ indexId: owner, request: { after, limit: 12 } });
    cancel = request.cancel;
    try {
      const page = await request.result;
      if (turn !== generation || owner !== indexId) return;
      state = page === null ? { kind: 'idle' } : { kind: 'ready', page, after, previous };
    } catch (problem) {
      if (turn === generation && owner === indexId)
        state = { kind: 'error', message: String(problem) };
    }
    if (hadFocus) {
      await tick();
      if (turn === generation && document.activeElement === document.body) region?.focus();
    }
  }

  function previousPage(): void {
    if (state.kind !== 'ready' || state.previous.length === 0) return;
    void load(state.previous.at(-1) ?? null, state.previous.slice(0, -1));
  }

  function nextPage(): void {
    if (
      state.kind !== 'ready' ||
      state.page.availability !== 'observed' ||
      state.page.next_after === null
    )
      return;
    void load(state.page.next_after, [...state.previous, state.after]);
  }

  $effect(() => {
    revision;
    indexId;
    enabled;
    untrack(() => {
      generation += 1;
      cancel();
      state = { kind: 'idle' };
      void load(null, []);
    });
  });

  onDestroy(() => {
    generation += 1;
    cancel();
  });
</script>

<section
  class="empty-folders"
  aria-label="Empty folders"
  aria-busy={state.kind === 'loading'}
  tabindex="-1"
  bind:this={region}
>
  <header>
    <div role="status">
      {#if state.kind === 'ready' && state.page.availability === 'observed'}
        <strong>{state.page.items.length} checked empty on this page</strong>
        <span>Checked {date(state.page.checked_at)}</span>
      {:else if state.kind === 'loading'}
        <strong>Checking folders on disk…</strong>
      {:else if state.kind === 'error'}
        <strong>Folder check unavailable</strong>
        <span>{state.message}</span>
      {:else if state.kind === 'ready'}
        <strong>Folder check unavailable for this analysis</strong>
      {:else}
        <strong>Complete a live scan to review empty folders.</strong>
      {/if}
    </div>
    <button
      class="quiet-action"
      onclick={() => void load(null, [])}
      disabled={!enabled || state.kind === 'loading'}>Refresh</button
    >
  </header>

  {#if state.kind === 'ready' && state.page.availability === 'observed'}
    <p>Space savings have not been measured.</p>
    {#if !state.page.scan_complete || state.page.unverified_folders > 0}
      <p class="coverage">
        {#if !state.page.scan_complete}The scan has partial coverage.{/if}
        {#if state.page.unverified_folders > 0}{state.page.unverified_folders} folders on this page could
          not be checked.{/if}
      </p>
    {/if}
    {#if state.page.items.length > 0}
      <ul aria-label="Folders verified empty">
        {#each state.page.items as folder (folder.node_id)}
          <li>
            <button
              class="folder-row"
              onclick={() => onopen(folder.node_id)}
              disabled={!enabled}
              aria-label={`Open ${folder.relative_path} in Explorer`}
            >
              <Icon name="folder" />
              <span>{folder.relative_path}</span>
              <small>Open in Explorer</small>
            </button>
          </li>
        {/each}
      </ul>
    {:else if state.page.next_after !== null}
      <p>No confirmed empty folders on this page. More folders are available to check.</p>
    {:else}
      <p>No empty folders confirmed on this page.</p>
    {/if}
    {#if state.previous.length || state.page.next_after !== null}
      <footer aria-label="Empty folder pagination">
        <button class="quiet-action" onclick={previousPage} disabled={state.previous.length === 0}
          >Previous</button
        >
        <button class="quiet-action" onclick={nextPage} disabled={state.page.next_after === null}
          >Next</button
        >
      </footer>
    {/if}
  {/if}
</section>

<style>
  .empty-folders {
    padding: 12px 0;
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    padding-bottom: 12px;
  }
  header div {
    display: grid;
    gap: 3px;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  header span,
  p,
  small {
    color: var(--muted);
  }
  header span,
  small {
    font-size: 0.82rem;
  }
  p {
    margin: 6px 0 12px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    border-top: 1px solid var(--border);
  }
  .folder-row {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    width: 100%;
    align-items: center;
    gap: 10px;
    border: 0;
    background: transparent;
    padding: 10px 4px;
    text-align: start;
  }
  .folder-row span {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .folder-row:hover:not(:disabled) small {
    text-decoration: underline;
  }
  footer {
    display: flex;
    justify-content: end;
    gap: 8px;
    padding-top: 12px;
  }
  @media (max-width: 620px) {
    .folder-row {
      grid-template-columns: auto minmax(0, 1fr);
    }
    small {
      grid-column: 2;
    }
  }
</style>
