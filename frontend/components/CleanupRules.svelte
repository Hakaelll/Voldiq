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
  import {
    defaultReviewWeights,
    priorityPoints,
    readReviewWeights,
    reviewEvidence,
    reviewFactors,
    type ReviewWeights,
  } from '../lib/review-priority';
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
    weights,
    onweights,
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
    weights: ReviewWeights;
    onweights: (next: ReviewWeights) => void;
  } = $props();
  const limit = 50;
  let applicationName = $state<string | null>(null);
  let cancelRead = () => {};
  let page = $state<CleanupRulePage | null>(null),
    offset = $state(0);
  let loadState = $state<'loading' | 'ready' | 'unavailable'>('loading');
  let selected = $state(new Map<number, CleanupRulePage['items'][number]>());
  let pageOwner = $state<IndexId | null>(null);
  const rules = [
    { id: 'user_temp', name: 'User temporary files' },
    { id: 'old_files', name: 'Old files' },
    { id: 'project_temp', name: 'Project temporary files' },
    { id: 'browser_cache', name: 'Browser HTTP cache' },
  ] as const;
  let rule = $state<(typeof rules)[number]['id']>('user_temp');
  const activeRule = $derived(application ? 'browser_cache' : rule);
  let expandedPriority = $state<number | null>(null);
  let weightsProblem = $state('');
  let weightsOpen = $state(false);
  let draft = $state<Record<keyof ReviewWeights, number | undefined>>({ ...defaultReviewWeights });
  $effect(() => {
    draft = { ...weights };
  });

  function applyWeights(next: ReviewWeights): void {
    weightsProblem = '';
    onweights(next);
    weightsOpen = false;
    void load(0, next);
  }
  let pageSelected = $derived(page?.items.filter((item) => selected.has(item.id)).length ?? 0);
  let allPageSelected = $derived(
    Boolean(page?.items.length) && pageSelected === page?.items.length,
  );
  function filename(path: string): string {
    return path.slice(Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/')) + 1);
  }
  let generation = 0;
  let region: HTMLDivElement;
  async function load(next: number, activeWeights = weights): Promise<void> {
    const token = ++generation;
    const owner = getIndexContext();
    cancelRead();
    const focusTarget = document.activeElement;
    const restoreFocus = region?.contains(focusTarget);
    loadState = 'loading';
    page = null;
    expandedPriority = null;
    try {
      let result: CleanupRulePage;
      if (application) {
        const indexId = getIndexContext();
        if (indexId === null) throw Error('Scan a live folder before reviewing application cache.');
        const context = application;
        const request = applicationCacheQueries.request({
          indexId,
          request: { context, offset: next, limit, weights: activeWeights },
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
          request: { offset: next, limit, weights: activeWeights },
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
      <button class="quiet-action" onclick={onclearapplication} disabled={busy}
        >All cleanup rules</button
      >
    {:else}
      <label class="rule-picker">
        File category
        <select bind:value={rule} disabled={busy || loadState === 'loading'}>
          {#each rules as choice (choice.id)}
            <option value={choice.id}>{choice.name}</option>
          {/each}
        </select>
      </label>
    {/if}
    {#if page && loadState === 'ready'}<button
        class={page.indexed ? 'quiet-action' : 'primary'}
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
  {#if page?.indexed && loadState === 'ready'}
    <details class="review-weights" bind:open={weightsOpen}>
      <summary>Adjust review weights</summary>
      <form
        onsubmit={(event) => {
          event.preventDefault();
          const next = readReviewWeights(draft);
          if (next) applyWeights(next);
          else
            weightsProblem =
              'Enter a whole number from 0 to 100 for each factor. Use at least one nonzero weight.';
        }}
      >
        <p>
          Higher weights move supported matches up the list. Files stay unselected until you choose
          them.
        </p>
        <div class="weight-fields">
          {#each reviewFactors as [key, label] (key)}
            <label
              >{label}<input
                type="number"
                min="0"
                max="100"
                step="1"
                required
                bind:value={draft[key]}
              /></label
            >
          {/each}
        </div>
        {#if weightsProblem}<p class="weight-error" role="alert">{weightsProblem}</p>{/if}
        <div class="weight-actions">
          <button type="submit" class="primary" disabled={busy}>Apply weights</button>
          <button
            type="button"
            class="quiet-action"
            disabled={busy}
            onclick={() => applyWeights(defaultReviewWeights)}>Restore defaults</button
          >
        </div>
      </form>
    </details>
  {/if}
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
                  ><th>File</th><th class="number">Logical size</th><th class="number">Potential</th
                  ><th class="number">Review priority</th></tr
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
                          <strong>{filename(item.path)}</strong>
                          <small>{item.path}</small>
                          <small
                            >Modified {date(item.modified)}{#if item.hard_link}
                              · Hard link{/if}</small
                          >
                        </div>
                      </div></td
                    >
                    <td class="number">{bytes(item.logical)}</td>
                    <td class="number">{bytes(item.reclaimable)}</td>
                    <td class="number">
                      <button
                        class="quiet-action priority-button"
                        aria-label={`${filename(item.path)}, review priority ${priorityPoints(item.priority.points)} out of 100`}
                        aria-expanded={expandedPriority === item.id}
                        aria-controls={expandedPriority === item.id
                          ? `priority-${item.id}`
                          : undefined}
                        onclick={() => {
                          expandedPriority = expandedPriority === item.id ? null : item.id;
                        }}
                        >{priorityPoints(item.priority.points)}
                        <Icon name="expand" size={12} /></button
                      >
                    </td>
                  </tr>
                  {#if expandedPriority === item.id}
                    <tr class="priority-row">
                      <td colspan="5">
                        <div id={`priority-${item.id}`} class="priority-details">
                          <div class="priority-summary">
                            <strong>Evidence coverage {item.priority.coverage}%</strong>
                            <span>Assessed {date(page.assessed_at)}</span>
                          </div>
                          <dl>
                            {#each item.priority.signals as signal, at}
                              <div>
                                <dt>{reviewFactors[at][1]}</dt>
                                <dd>{reviewEvidence[signal.evidence]}</dd>
                                <dd class="priority-value">
                                  Weight {signal.weight} · {priorityPoints(signal.points)} pts
                                </dd>
                              </div>
                            {/each}
                          </dl>
                          <p>
                            Coverage reflects how much of the chosen weighting had evidence. A
                            matching local file provides no backup guarantee. Review the files
                            before confirming any action.
                          </p>
                        </div>
                      </td>
                    </tr>
                  {/if}
                {/each}</tbody
              >
            </table>
          </div>{/if}
      {/if}
      {#if page?.indexed && page.total > limit}<div class="rule-actions">
          <span>{offset + 1}–{offset + page.items.length} of {page.total}</span>
          <button
            class="quiet-action"
            onclick={() => load(Math.max(0, offset - limit))}
            disabled={offset === 0 || busy || loadState !== 'ready'}>Previous</button
          >
          <button
            class="quiet-action"
            onclick={() => load(offset + limit)}
            disabled={offset + limit >= page.total || busy || loadState !== 'ready'}>Next</button
          >
        </div>{/if}
    </div>
    {#if page?.indexed && selected.size}
      <div class="selection-bar rule-review-actions">
        <span>{selected.size.toLocaleString()} selected</span>
        <button
          class="quiet-action"
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
    max-width: 68ch;
    margin-top: 10px;
    line-height: 1.6;
  }
  .cleanup-rules {
    min-width: 0;
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
  .rule-picker {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    color: var(--secondary);
  }
  .rule-picker select {
    max-width: 100%;
  }
  .review-weights {
    margin-bottom: 14px;
    color: var(--secondary);
  }
  .review-weights > summary {
    width: fit-content;
    padding: 6px 0;
    cursor: pointer;
  }
  .review-weights form {
    border-inline-start: 1px solid var(--border);
    margin-top: 8px;
    padding-inline-start: 14px;
  }
  .weight-fields {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 12px 20px;
    max-width: 620px;
    margin-block: 12px;
  }
  .weight-fields label {
    display: grid;
    gap: 4px;
    font-size: 13px;
  }
  .weight-fields input {
    width: 80px;
  }
  .weight-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .weight-error {
    color: var(--error);
  }
  .priority-button {
    justify-content: flex-end;
    min-width: 64px;
    font-variant-numeric: tabular-nums;
  }
  .priority-button[aria-expanded='true'] {
    color: var(--accent);
  }
  .priority-button[aria-expanded='true'] :global(svg) {
    transform: rotate(90deg);
  }
  .priority-row {
    background: var(--surface);
  }
  .priority-details {
    max-width: 760px;
    margin-inline: 30px 0;
    padding-block: 6px;
  }
  .priority-summary {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 18px;
  }
  .priority-summary span,
  .priority-details dd,
  .priority-details p {
    color: var(--secondary);
  }
  .priority-details dl {
    margin: 12px 0;
  }
  .priority-details dl div {
    display: grid;
    grid-template-columns: 165px minmax(0, 1fr) max-content;
    gap: 12px;
    padding-block: 6px;
    border-bottom: 1px solid var(--border);
  }
  .priority-details dd {
    margin: 0;
  }
  .priority-value {
    font-variant-numeric: tabular-nums;
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
    min-width: 640px;
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
