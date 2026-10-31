<script lang="ts">
  import { invokeIndexed } from '../lib/index-context';
  import { onMount, onDestroy } from 'svelte';
  import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
  import Icon from './Icon.svelte';
  import {
    bytes,
    date,
    type DuplicatePage,
    type DuplicateItem,
    type DuplicateProgress,
    type Issue,
    type CleanupPreview,
  } from '../lib/types';
  let {
    scope,
    enabled,
    onbusy,
    onselect,
    onissues,
    onerror,
    onpreview,
  }: {
    scope: number;
    enabled: boolean;
    onbusy: (value: boolean) => void;
    onselect: (id: number) => void;
    onissues: (items: Issue[]) => void;
    onerror: (problem: unknown) => void;
    onpreview: (plan: CleanupPreview) => void;
  } = $props();
  const groupLimit = 20,
    memberLimit = 50;
  let page = $state<DuplicatePage | null>(null),
    members = $state<DuplicateItem[]>([]);
  let offset = $state(0),
    memberOffset = $state(0),
    openGroup = $state<number | null>(null);
  let operation = $state<DuplicateProgress | null>(null),
    jobId = $state<number | null>(null);
  let minimum = $state(0),
    loading = $state(false);
  let loadFailed = $state(false),
    groupRetryOffset = $state(0);
  let request = 0,
    memberRequest = 0;
  let memberLoading = $state(false),
    memberFailed = $state(false);
  let activeGroup = $derived(page?.groups.find((group) => group.key === openGroup));
  let working = $derived(operation?.phase === 'sampling' || operation?.phase === 'hashing');
  let checked = $state(new Map<number, number>());
  const groupSelected = $derived(
    activeGroup ? [...checked.values()].filter((key) => key === activeGroup.key).length : 0,
  );
  let reviewing = $state(false),
    reviewId = $state<number | null>(null);
  let alive = true;
  function check(id: number, group: number, copies: number, value: boolean): void {
    const next = new Map(checked);
    if (value) {
      if (next.size >= 500) {
        onerror('Select up to 500 files.');
        return;
      }
      const chosen = [...next.values()].filter((key) => key === group).length;
      if (chosen >= copies - 1) {
        onerror('Keep at least one copy in each group.');
        return;
      }
      next.set(id, group);
    } else next.delete(id);
    checked = next;
  }
  async function review(): Promise<void> {
    if (!enabled || reviewing || checked.size === 0) return;
    reviewing = true;
    onbusy(true);
    const progress = new Channel<number>();
    progress.onmessage = (id) => {
      reviewId = id;
      if (!alive) void cancelReview();
    };
    try {
      const plan = await invokeIndexed<CleanupPreview>('preview_duplicate_cleanup', {
        selection: [...checked.keys()],
        progress,
      });
      if (alive) onpreview(plan);
      else await invoke('discard_cleanup', { id: plan.id });
    } catch (problem) {
      if (alive) onerror(problem);
    } finally {
      reviewing = false;
      reviewId = null;
      onbusy(false);
    }
  }
  async function cancelReview(): Promise<void> {
    if (reviewId !== null) {
      try {
        await invoke('cancel_operation', { id: reviewId });
      } catch (problem) {
        onerror(problem);
      }
    }
  }
  async function load(next = offset): Promise<void> {
    const token = ++request;
    loading = true;
    loadFailed = false;
    groupRetryOffset = next;
    try {
      const result = await invokeIndexed<DuplicatePage | null>('query_duplicate_groups', {
        offset: next,
        limit: groupLimit,
      });
      if (token !== request) return;
      page = result;
      offset = next;
      openGroup = null;
      members = [];
      memberRequest += 1;
      if (result?.groups[0]) void expand(result.groups[0].key);
    } catch (problem) {
      if (token === request) {
        loadFailed = true;
        onerror(problem);
      }
    } finally {
      if (token === request) loading = false;
    }
  }
  async function expand(group: number, next = 0): Promise<void> {
    const token = ++memberRequest;
    openGroup = group;
    members = [];
    memberOffset = next;
    memberLoading = true;
    memberFailed = false;
    try {
      const result = await invokeIndexed<DuplicateItem[]>('query_duplicate_members', {
        group,
        offset: next,
        limit: memberLimit,
      });
      if (token === memberRequest) members = result;
    } catch (problem) {
      if (token === memberRequest) {
        memberFailed = true;
        onerror(problem);
      }
    } finally {
      if (token === memberRequest) memberLoading = false;
    }
  }
  async function start(): Promise<void> {
    if (!enabled || working) return;
    request += 1;
    memberRequest += 1;
    page = null;
    loadFailed = false;
    loading = false;
    memberLoading = false;
    memberFailed = false;
    checked = new Map();
    members = [];
    openGroup = null;
    operation = {
      phase: 'sampling',
      examined: 0,
      candidates: 0,
      bytes_read: 0,
      cache_hits: 0,
      errors: 0,
      elapsed_ms: 0,
      message: '',
    };
    onbusy(true);
    const channel = new Channel<DuplicateProgress>();
    channel.onmessage = (update) => {
      operation = update;
      if (
        update.phase === 'complete' ||
        update.phase === 'cancelled' ||
        update.phase === 'failed'
      ) {
        jobId = null;
        onbusy(false);
        if (update.phase === 'failed') onerror(update.message);
        else void load(0);
      }
    };
    try {
      const id = await invokeIndexed<number>('start_duplicates', {
        scope,
        minSize: Math.max(0, minimum || 0) * 1024 * 1024,
        progress: channel,
      });
      if (working) jobId = id;
    } catch (problem) {
      operation = null;
      onbusy(false);
      onerror(problem);
    }
  }
  async function cancel(): Promise<void> {
    if (jobId === null) return;
    try {
      await invoke('cancel_operation', { id: jobId });
    } catch (problem) {
      onerror(problem);
    }
  }
  async function issues(): Promise<void> {
    try {
      onissues(await invokeIndexed<Issue[]>('get_duplicate_issues'));
    } catch (problem) {
      onerror(problem);
    }
  }
  onMount(() => {
    if (isTauri()) void load(0);
  });
  onDestroy(() => {
    alive = false;
    if (reviewing) void cancelReview();
    request += 1;
    memberRequest += 1;
  });
</script>

<section class="duplicates-view">
  <div class="filter-bar">
    <label>Minimum MiB<input type="number" min="0" bind:value={minimum} disabled={working} /></label
    >
    <button class="primary" onclick={start} disabled={!enabled || working}
      ><Icon name="duplicate" />Find duplicates</button
    >
    {#if working}<button onclick={cancel} disabled={jobId === null}>Cancel</button>{/if}
  </div>
  {#if operation && working}<div class="scan-status" aria-live="polite">
      <span class="status-dot working"></span><span
        >{operation.phase === 'hashing' ? 'Verifying content' : 'Comparing files'}</span
      >
      <span
        >{operation.examined.toLocaleString()} / {operation.candidates.toLocaleString()} files · {bytes(
          operation.bytes_read,
        )} read</span
      >
    </div>{/if}
  {#if loadFailed}<div class="table-empty">
      <button onclick={() => load(groupRetryOffset)}>Retry loading groups</button>
    </div>{/if}
  {#if page}
    <div class="duplicate-summary">
      <div><span>Duplicate groups</span><strong>{page.total.toLocaleString()}</strong></div>
      <div><span>Potential allocation</span><strong>{bytes(page.recoverable)}</strong></div>
      {#if page.cancelled}<span>Partial results</span>{/if}
      {#if page.errors}<button onclick={issues}>{page.errors} issues</button>{/if}
    </div>
    <div class="duplicate-layout">
      <section class="duplicate-groups" aria-label="Duplicate groups">
        <h2>Groups</h2>
        <div class="duplicate-group-columns">
          <span>Name</span><span>Copies</span><span>Potential allocation</span>
        </div>
        {#each page.groups as group (group.key)}
          <button
            class="duplicate-group-heading"
            class:selected={openGroup === group.key}
            aria-pressed={openGroup === group.key}
            disabled={loading || reviewing}
            onclick={() => expand(group.key)}
          >
            <span class="duplicate-group-name"
              ><Icon name="file" /><strong title={group.name}>{group.name}</strong></span
            >
            <span>{group.copies.toLocaleString()}</span>
            <span class="number"
              >{bytes(group.recoverable)}<span class="group-size-track" aria-hidden="true"
                ><span
                  style:width={`${page.recoverable ? Math.min(100, (group.recoverable / page.recoverable) * 100) : 0}%`}
                ></span></span
              ></span
            >
          </button>
        {/each}
        {#if page.total === 0}<div class="table-empty">No duplicates found</div>{/if}
        {#if page.total > groupLimit}<div class="duplicate-pagination">
            <button
              onclick={() => load(Math.max(0, offset - groupLimit))}
              disabled={offset === 0 || loading}>Previous</button
            >
            <span>{offset + 1}–{Math.min(page.total, offset + groupLimit)} of {page.total}</span>
            <button
              onclick={() => load(offset + groupLimit)}
              disabled={offset + groupLimit >= page.total || loading}>Next</button
            >
          </div>{/if}
      </section>
      <section class="duplicate-content" aria-label="Group files">
        {#if activeGroup}
          <div class="duplicate-content-heading">
            <div>
              <h2>{activeGroup.name}</h2>
              <p>
                {bytes(activeGroup.logical)} each · {activeGroup.copies.toLocaleString()} copies
              </p>
            </div>
            <div class="duplicate-potential">
              <strong>{bytes(activeGroup.recoverable)}</strong><small>Potential allocation</small>
            </div>
          </div>
          <div
            class="copy-balance"
            role="img"
            aria-label={`${activeGroup.copies - groupSelected} copies unselected, ${groupSelected} selected for review`}
          >
            <span
              class="copies-kept"
              style:flex-grow={activeGroup.copies - groupSelected}
              title={`${activeGroup.copies - groupSelected} copies unselected`}
              >Keep {activeGroup.copies - groupSelected}</span
            >
            {#if groupSelected > 0}<span
                class="copies-selected"
                style:flex-grow={groupSelected}
                title={`${groupSelected} selected for review`}>Selected {groupSelected}</span
              >{/if}
          </div>
          <div class="duplicate-member-columns">
            <span>Path</span><span>Modified</span><span>Allocated</span>
          </div>
          {#if memberLoading}<div class="table-empty" role="status">Loading files</div>
          {:else if memberFailed}<div class="table-empty">
              <button onclick={() => expand(activeGroup.key, memberOffset)}>Retry</button>
            </div>
          {:else}
            <div class="duplicate-members">
              {#each members as item (item.id)}
                <div class="duplicate-choice" class:chosen={checked.has(item.id)}>
                  <input
                    type="checkbox"
                    aria-label={`Select ${item.path}`}
                    checked={checked.has(item.id)}
                    disabled={!enabled || reviewing}
                    onchange={(event) => {
                      check(
                        item.id,
                        activeGroup.key,
                        activeGroup.copies,
                        event.currentTarget.checked,
                      );
                      event.currentTarget.checked = checked.has(item.id);
                    }}
                  />
                  <button
                    class="duplicate-member"
                    onclick={() => onselect(item.id)}
                    title={item.path}
                  >
                    <Icon name="file" /><span
                      ><strong>{item.name}</strong><small>{item.path}</small><small
                        class="copy-state"
                        >{checked.has(item.id) ? 'Selected for review' : 'Keep'}</small
                      ></span
                    >
                    <span class="duplicate-modified"
                      >{date(item.modified)}{#if item.links > 1}<small>Hard link</small>{/if}</span
                    ><span class="number">{bytes(item.allocated)}</span>
                  </button>
                </div>
              {/each}
            </div>
          {/if}
          {#if activeGroup.copies > memberLimit}<div class="duplicate-pagination">
              <button
                onclick={() => expand(activeGroup.key, Math.max(0, memberOffset - memberLimit))}
                disabled={memberLoading || memberOffset === 0}>Previous</button
              >
              <span
                >{memberOffset + 1}–{Math.min(activeGroup.copies, memberOffset + memberLimit)} of {activeGroup.copies}</span
              >
              <button
                onclick={() => expand(activeGroup.key, memberOffset + memberLimit)}
                disabled={memberLoading || memberOffset + memberLimit >= activeGroup.copies}
                >Next</button
              >
            </div>{/if}
        {/if}
      </section>
    </div>
    <div class="selection-bar duplicate-selection">
      <span>{checked.size ? `${checked.size} selected` : 'No files selected'}</span>
      {#if reviewing}<span>Verifying content</span><button
          onclick={cancelReview}
          disabled={reviewId === null}>Cancel</button
        >{/if}
      {#if checked.size}<button
          onclick={() => {
            checked = new Map();
          }}
          disabled={reviewing}>Clear</button
        >{/if}
      <button
        class="primary"
        onclick={review}
        disabled={!enabled || reviewing || checked.size === 0}>Review selection</button
      >
    </div>
  {:else if loading}<div class="table-empty" role="status">Loading groups</div>
  {:else if !working && !loadFailed}<div class="table-empty">No duplicate analysis</div>{/if}
</section>

<style>
  .group-size-track {
    display: block;
    height: 3px;
    background: var(--paper);
    margin-top: 6px;
  }
  .group-size-track > span {
    display: block;
    height: 100%;
    background: var(--accent-fill);
  }
  .copy-balance {
    display: flex;
    height: 34px;
    gap: 2px;
    margin: 0 0 18px;
    overflow: hidden;
  }
  .copy-balance > span {
    flex-basis: 0;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    line-height: 34px;
    text-indent: 10px;
    font-size: 12px;
    font-weight: 600;
  }
  .copies-kept {
    background: #075bdd;
    color: #ffffff;
  }
  .copies-selected {
    background: #ed701b;
    color: #101820;
  }
  .duplicate-choice.chosen {
    background: var(--selected);
  }
  .duplicate-choice :global(.copy-state) {
    color: var(--accent);
    font-weight: 500;
    margin-top: 4px;
  }
  .duplicate-choice {
    display: flex;
    align-items: center;
    border-bottom: 1px solid var(--border);
  }
  .duplicate-choice input {
    margin-left: 16px;
    width: 14px;
    height: 14px;
    accent-color: var(--accent);
    flex-shrink: 0;
  }
  .duplicate-choice :global(.duplicate-member) {
    border-bottom: 0;
    min-width: 0;
  }
</style>
