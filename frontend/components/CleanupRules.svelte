<script lang="ts">
  import { invokeIndexed, getIndexContext } from '../lib/index-context';
  import { applicationCacheQueries } from '../lib/queries';
  import { readApplicationCachePage, type ApplicationCleanupContext } from '../lib/applications';
  import { onDestroy, untrack, tick } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { bytes, date, type CleanupRulePage, type CleanupPreview } from '../lib/types';
  import Icon from './Icon.svelte';
  import { updateCleanupSelection } from '../lib/cleanup-selection';
  let {
    revision,
    manageable,
    busy,
    scanning,
    onscan,
    onbusy,
    onpreview,
    onerror,
    application = null,
    onclearapplication,
  }: {
    revision: number;
    manageable: boolean;
    busy: boolean;
    scanning: boolean;
    onscan: (path: string) => Promise<void>;
    onbusy: (value: boolean) => void;
    onpreview: (plan: CleanupPreview) => void;
    onerror: (problem: unknown) => void;
    application?: ApplicationCleanupContext | null;
    onclearapplication: () => void;
  } = $props();
  const limit = 50;
  let rule = $state<'user_temp' | 'old_files' | 'project_temp' | 'browser_cache'>('user_temp');
  const activeRule = $derived(application ? 'browser_cache' : rule);
  let applicationName = $state<string | null>(null);
  let cancelRead = () => {};
  let page = $state<CleanupRulePage | null>(null),
    offset = $state(0);
  let loadState = $state<'loading' | 'ready' | 'unavailable'>('loading');
  let selected = $state(new Map<number, CleanupRulePage['items'][number]>()),
    reviewing = $state(false);
  const rules = [
    { id: 'user_temp', name: 'User temporary files', icon: 'folder' },
    { id: 'old_files', name: 'Old files', icon: 'history' },
    { id: 'project_temp', name: 'Project temporary files', icon: 'code' },
    { id: 'browser_cache', name: 'Browser HTTP cache', icon: 'browser' },
  ] as const;
  let selection = $derived([...selected.values()]);
  let selectionLogical = $derived(selection.reduce((sum, item) => sum + item.logical, 0));
  let pageSelected = $derived(page?.items.filter((item) => selected.has(item.id)).length ?? 0);
  let allPageSelected = $derived(
    Boolean(page?.items.length) && pageSelected === page?.items.length,
  );
  function filename(path: string): string {
    return path.slice(Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/')) + 1);
  }
  let generation = 0,
    alive = true;
  let region: HTMLDivElement;
  let selectionRegion: HTMLElement;
  async function load(next: number): Promise<void> {
    const token = ++generation;
    cancelRead();
    const focusTarget = document.activeElement;
    const restoreFocus = region?.contains(focusTarget) && !selectionRegion?.contains(focusTarget);
    loadState = 'loading';
    page = null;
    try {
      let result: CleanupRulePage;
      if (application) {
        const indexId = getIndexContext();
        if (indexId === null) throw Error('Scan a live folder before reviewing application cache.');
        const context = application;
        const request = applicationCacheQueries.request({
          indexId,
          request: { context, offset: next, limit },
        });
        cancelRead = request.cancel;
        const value = await request.result;
        if (token !== generation) return;
        if (value === null || indexId !== getIndexContext())
          throw Error('Application cache review cancelled.');
        const response = readApplicationCachePage(value);
        if (response.application.id !== context.application_id)
          throw Error('Application evidence changed.');
        applicationName = response.application.name;
        result = response.candidates;
      } else {
        applicationName = null;
        result = await invokeIndexed<CleanupRulePage>('query_cleanup_rule', {
          rule: activeRule,
          offset: next,
          limit,
        });
      }
      if (token !== generation) return;
      page = result;
      offset = next;
      loadState = 'ready';
    } catch (problem) {
      if (token !== generation) return;
      loadState = 'unavailable';
      onerror(problem);
    }
    if (restoreFocus) {
      await tick();
      if (token === generation) {
        if (focusTarget instanceof HTMLElement && focusTarget.isConnected) {
          if (document.activeElement === document.body) focusTarget.focus();
        } else region?.focus();
      }
    }
  }
  function checkItems(items: CleanupRulePage['items'], value: boolean): void {
    const next = updateCleanupSelection(selected, items, value);
    if (next) selected = next;
    else onerror('Select up to 500 files.');
  }
  async function review(): Promise<void> {
    if (!manageable || busy || reviewing || selected.size === 0) return;
    reviewing = true;
    onbusy(true);
    const owner = generation;
    try {
      const plan = await invokeIndexed<CleanupPreview>('preview_cleanup_rule', {
        rule: activeRule,
        application,
        selection: [...selected.keys()],
      });
      if (alive && owner === generation) onpreview(plan);
      else await invoke('discard_cleanup', { id: plan.id });
    } catch (problem) {
      if (alive) onerror(problem);
    } finally {
      reviewing = false;
      onbusy(false);
    }
  }
  $effect(() => {
    revision;
    rule;
    application;
    untrack(() => {
      selected = new Map();
      offset = 0;
      if (isTauri()) void load(0);
      else loadState = 'unavailable';
    });
  });
  onDestroy(() => {
    alive = false;
    generation += 1;
    cancelRead();
  });
</script>

<div
  class="cleanup-rules"
  role="region"
  aria-label="Cleanup rules"
  tabindex="-1"
  bind:this={region}
>
  <div class="rule-heading">
    {#if application}<div class="application-review-heading">
        <strong>{applicationName ?? 'Application cache review'}</strong>
        <small>Browser HTTP cache · explicit file selection</small>
      </div>
      <button onclick={onclearapplication} disabled={busy || reviewing}>All cleanup rules</button>
    {:else}
      <div class="rule-choices" role="group" aria-label="Cleanup rule">
        {#each rules as choice}<button
            class:active={rule === choice.id}
            aria-pressed={rule === choice.id}
            disabled={busy || reviewing || loadState === 'loading'}
            onclick={() => {
              rule = choice.id;
            }}><Icon name={choice.icon} size={17} />{choice.name}</button
          >{/each}
      </div>
    {/if}
    {#if page && loadState === 'ready'}<button
        class="primary"
        onclick={() => {
          if (page) void onscan(page.root);
        }}
        disabled={busy || !page.root}
        ><Icon name="scan" />{activeRule === 'user_temp' ? 'Scan user temp' : 'Rescan'}</button
      >{/if}
  </div>
  {#if application}<p class="application-review-note">
      Only supported external Blockfile payloads unchanged for seven days are listed. Close the
      browser before review. Folder scope is retained; Explorer file filters are not applied.
      Nothing is selected automatically.
    </p>{/if}
  {#if page && loadState === 'ready'}<div class="rule-summary">
      <span
        >{activeRule === 'old_files'
          ? '90+ days · Manual selection'
          : activeRule === 'browser_cache'
            ? '7+ days · Close browser before review'
            : '7+ days · .tmp / .temp'}</span
      >
      {#if page.indexed}<span
          >{page.total.toLocaleString()} files · {bytes(page.logical)} logical · {bytes(
            page.reclaimable,
          )} potential{#if scanning || !page.complete}
            · Partial{/if}</span
        >{/if}
    </div>{/if}
  <div class="rule-layout">
    <div class="rule-list">
      {#if loadState === 'loading'}<div class="rule-empty" role="status">Loading</div>
      {:else if loadState === 'unavailable'}<div class="rule-empty">
          Unavailable<button onclick={() => load(offset)} disabled={busy}>Retry</button>
        </div>
      {:else if page}
        {#if page.unverified_branches > 0}<p class="notice" role="status">
            {page.unverified_branches.toLocaleString()} folder branches omitted because their identity
            is unverified. Rescan to include them in review.
          </p>{/if}
        {#if !page.indexed}<div class="rule-empty">Not indexed</div>
        {:else if page.items.length === 0}<div class="rule-empty">
            {page.unverified_branches > 0 ? 'No verified matching files' : 'No matching files'}
          </div>
        {:else}<div class="rule-scroll">
            <table>
              <thead
                ><tr
                  ><th
                    ><input
                      type="checkbox"
                      aria-label="Select files on this page"
                      checked={allPageSelected}
                      indeterminate={pageSelected > 0 && !allPageSelected}
                      disabled={!manageable || busy || loadState !== 'ready'}
                      onchange={(event) => {
                        if (page) checkItems(page.items, event.currentTarget.checked);
                        event.currentTarget.checked = allPageSelected;
                        event.currentTarget.indeterminate = pageSelected > 0 && !allPageSelected;
                      }}
                    /></th
                  ><th>Name</th><th class="number">Logical size</th><th>Modified</th><th
                    class="number">Potential</th
                  ></tr
                ></thead
              >
              <tbody
                >{#each page.items as item (item.id)}<tr class:selected={selected.has(item.id)}>
                    <td
                      ><input
                        type="checkbox"
                        aria-label={`Select ${item.path}`}
                        checked={selected.has(item.id)}
                        disabled={!manageable || busy}
                        onchange={(event) => {
                          checkItems([item], event.currentTarget.checked);
                          event.currentTarget.checked = selected.has(item.id);
                        }}
                      /></td
                    >
                    <td
                      ><div class="file-name">
                        <Icon name="file" />
                        <div>
                          <strong>{filename(item.path)}</strong><small>{item.path}</small
                          >{#if item.hard_link}<small>Hard link</small>{/if}
                        </div>
                      </div></td
                    >
                    <td class="number">{bytes(item.logical)}</td><td class="modified"
                      >{date(item.modified)}</td
                    ><td class="number">{bytes(item.reclaimable)}</td>
                  </tr>{/each}</tbody
              >
            </table>
          </div>{/if}
      {/if}
      {#if page?.indexed && page.total > limit}<div class="rule-actions">
          <span>{offset + 1}–{offset + page.items.length} of {page.total}</span>
          <button
            onclick={() => load(Math.max(0, offset - limit))}
            disabled={offset === 0 || busy || loadState !== 'ready'}>Previous</button
          >
          <button
            onclick={() => load(offset + limit)}
            disabled={offset + limit >= page.total || busy || loadState !== 'ready'}>Next</button
          >
        </div>{/if}
    </div>
    <aside aria-label="Cleanup selection" bind:this={selectionRegion}>
      <header>
        <h2>Selection</h2>
        <span>{selected.size.toLocaleString()} files</span>
      </header>
      {#if selection.length}<ul>
          {#each selection as item (item.id)}<li>
              <div class="file-name">
                <Icon name="file" />
                <div><strong>{filename(item.path)}</strong><small>{item.path}</small></div>
              </div>
              <span>{bytes(item.logical)}</span>
              <span class="selection-size-track" aria-hidden="true"
                ><span
                  style:width={`${selectionLogical > 0 ? (item.logical / selectionLogical) * 100 : 0}%`}
                ></span></span
              >
            </li>{/each}
        </ul>
      {:else}<div class="selection-empty">No files selected</div>{/if}
      <div class="selection-total">
        <span>Logical size</span><strong>{bytes(selectionLogical)}</strong>
      </div>
      <button
        class="primary review"
        onclick={review}
        disabled={!manageable || busy || reviewing || loadState !== 'ready' || selected.size === 0}
        >Review selection</button
      >
      <button
        class="clear"
        onclick={() => {
          selected = new Map();
        }}
        disabled={busy || selected.size === 0}>Clear</button
      >
    </aside>
  </div>
</div>

<style>
  .application-review-heading {
    display: grid;
    gap: 4px;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .application-review-heading small,
  .application-review-note {
    color: var(--secondary);
    font-size: 12px;
  }
  .application-review-note {
    border-left: 3px solid var(--accent);
    padding-left: 12px;
    line-height: 1.6;
  }
  .cleanup-rules {
    min-width: 0;
    margin-top: 24px;
  }
  .rule-heading,
  .rule-summary,
  .rule-actions {
    display: flex;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
  }
  .rule-heading {
    justify-content: space-between;
    padding: 4px 0;
  }
  .rule-choices {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .rule-choices button {
    border: 0;
    border-radius: var(--radius);
    padding: 10px 12px;
    background: transparent;
    color: var(--secondary);
  }
  .rule-choices button:hover:not(:disabled) {
    background: var(--paper);
  }
  .rule-choices button.active {
    color: var(--accent);
    background: var(--selected);
    font-weight: 600;
  }
  .rule-summary {
    padding: 14px 0 20px;
    color: var(--secondary);
    font-size: 12px;
  }
  .rule-layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 24px;
  }
  .rule-list,
  aside {
    min-width: 0;
  }
  .rule-scroll {
    overflow: auto;
    max-height: 560px;
    overscroll-behavior: contain;
  }
  table {
    width: 100%;
    min-width: 740px;
    border-collapse: collapse;
    font-size: 13px;
  }
  th,
  td {
    border-top: 1px solid var(--border);
    padding: 12px 10px;
    text-align: left;
  }
  th {
    font-weight: 500;
    color: var(--secondary);
    font-size: 12px;
  }
  th:first-child {
    width: 38px;
  }
  tr.selected {
    background: var(--selected);
  }
  input[type='checkbox'] {
    width: 16px;
    height: 16px;
    accent-color: var(--accent);
  }
  .file-name {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    min-width: 0;
  }
  .file-name > div {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  strong {
    font-weight: 600;
  }
  small {
    display: block;
    margin-top: 4px;
    color: var(--secondary);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .number {
    text-align: right;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .modified {
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .rule-empty {
    min-height: 200px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--secondary);
  }
  .rule-actions {
    margin-top: 16px;
    font-size: 12px;
    color: var(--secondary);
  }
  .rule-actions > span {
    margin-right: auto;
  }
  aside {
    align-self: start;
    background: var(--surface);
    padding: 20px;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
  }
  h2 {
    font-size: 17px;
    margin: 0;
  }
  header > span {
    color: var(--secondary);
    font-size: 12px;
  }
  ul {
    list-style: none;
    margin: 16px 0;
    padding: 0;
    max-height: 360px;
    overflow: auto;
    overscroll-behavior: contain;
  }
  li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: start;
    gap: 8px 12px;
    padding: 12px 0;
  }
  li > span {
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .selection-size-track {
    grid-column: 1 / -1;
    height: 4px;
    background: var(--paper);
    overflow: hidden;
  }
  .selection-size-track > span {
    display: block;
    height: 100%;
    background: var(--accent-fill);
  }
  .selection-empty {
    padding: 40px 0;
    color: var(--secondary);
  }
  .selection-total {
    display: flex;
    justify-content: space-between;
    padding: 20px 0;
    gap: 12px;
  }
  .selection-total strong {
    font-size: 20px;
    font-variant-numeric: tabular-nums;
  }
  .review,
  .clear {
    width: 100%;
    justify-content: center;
  }
  .clear {
    margin-top: 8px;
    border-color: transparent;
    color: var(--accent);
  }
  @media (min-width: 1200px) {
    .rule-layout {
      grid-template-columns: minmax(0, 1fr) 340px;
    }
  }
</style>
