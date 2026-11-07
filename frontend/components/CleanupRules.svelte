<script lang="ts">
  import { invokeIndexed, getIndexContext, type IndexId } from '../lib/index-context';
  import { applicationCacheQueries } from '../lib/queries';
  import { readApplicationCachePage, type ApplicationCleanupContext } from '../lib/applications';
  import { onDestroy, untrack, tick } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';
  import { bytes, date, type CleanupRulePage } from '../lib/types';
  import Icon from './Icon.svelte';
  import type { TraySource } from '../lib/cleanup-tray';
  import { updateCleanupSelection } from '../lib/cleanup-selection';
  let {
    revision,
    manageable,
    busy,
    scanning,
    onscan,
    onadd,
    onerror,
    application = null,
    onclearapplication,
  }: {
    revision: number;
    manageable: boolean;
    busy: boolean;
    scanning: boolean;
    onscan: (path: string) => Promise<void>;
    onadd: (source: TraySource, ids: readonly number[], owner: IndexId | null) => boolean;
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
  let selected = $state(new Map<number, CleanupRulePage['items'][number]>());
  let pageOwner = $state<IndexId | null>(null);
  const rules = [
    { id: 'user_temp', name: 'User temporary files', icon: 'folder' },
    { id: 'old_files', name: 'Old files', icon: 'history' },
    { id: 'project_temp', name: 'Project temporary files', icon: 'code' },
    { id: 'browser_cache', name: 'Browser HTTP cache', icon: 'browser' },
  ] as const;
  let pageSelected = $derived(page?.items.filter((item) => selected.has(item.id)).length ?? 0);
  let allPageSelected = $derived(
    Boolean(page?.items.length) && pageSelected === page?.items.length,
  );
  function filename(path: string): string {
    return path.slice(Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/')) + 1);
  }
  let generation = 0;
  let region: HTMLDivElement;
  async function load(next: number): Promise<void> {
    const token = ++generation;
    const owner = getIndexContext();
    cancelRead();
    const focusTarget = document.activeElement;
    const restoreFocus = region?.contains(focusTarget);
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
      if (token !== generation || owner !== getIndexContext()) return;
      page = result;
      pageOwner = owner;
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
  function checkItems(items: CleanupRulePage['items'], value: boolean): boolean {
    const next = updateCleanupSelection(selected, items, value);
    if (!next) {
      onerror('Select up to 500 files.');
      return false;
    }
    selected = next;
    return true;
  }
  function addSelection(): void {
    if (!manageable || busy || !page?.indexed || loadState !== 'ready' || selected.size === 0)
      return;
    const source: TraySource =
      activeRule === 'browser_cache' ? { kind: activeRule, application } : { kind: activeRule };
    if (onadd(source, [...selected.keys()], pageOwner)) selected = new Map();
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
      <button onclick={onclearapplication} disabled={busy}>All cleanup rules</button>
    {:else}
      <div class="rule-choices" role="group" aria-label="Cleanup rule">
        {#each rules as choice}<button
            class:active={rule === choice.id}
            aria-pressed={rule === choice.id}
            disabled={busy || loadState === 'loading'}
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
                        if (!page || !checkItems(page.items, event.currentTarget.checked)) {
                          event.currentTarget.checked = allPageSelected;
                        }
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
                          if (!checkItems([item], event.currentTarget.checked)) {
                            event.currentTarget.checked = selected.has(item.id);
                          }
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
    {#if page?.indexed && selected.size}
      <div class="selection-bar rule-review-actions">
        <span>{selected.size.toLocaleString()} selected</span>
        <button
          disabled={busy}
          onclick={() => {
            selected = new Map();
          }}>Clear</button
        >
        <button
          class="primary"
          disabled={!manageable || busy || loadState !== 'ready'}
          onclick={addSelection}>Add to review tray</button
        >
      </div>
    {/if}
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
  .rule-list {
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
  .rule-review-actions {
    margin-top: 12px;
  }
</style>
