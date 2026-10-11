<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
  import Icon from './Icon.svelte';
  import {
    bytes,
    type DuplicatePage,
    type DuplicateItem,
    type DuplicateProgress,
    type Issue,
  } from '../lib/types';
  let {
    scope,
    enabled,
    onbusy,
    onselect,
    onissues,
    onerror,
  }: {
    scope: number;
    enabled: boolean;
    onbusy: (value: boolean) => void;
    onselect: (id: number) => void;
    onissues: (items: Issue[]) => void;
    onerror: (problem: unknown) => void;
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
  let request = 0,
    memberRequest = 0;
  let working = $derived(operation?.phase === 'sampling' || operation?.phase === 'hashing');
  async function load(next = offset): Promise<void> {
    const token = ++request;
    loading = true;
    try {
      const result = await invoke<DuplicatePage | null>('query_duplicate_groups', {
        offset: next,
        limit: groupLimit,
      });
      if (token !== request) return;
      page = result;
      offset = next;
      openGroup = null;
      members = [];
      memberRequest += 1;
    } catch (problem) {
      if (token === request) onerror(problem);
    } finally {
      if (token === request) loading = false;
    }
  }
  async function expand(group: number, next = 0): Promise<void> {
    const token = ++memberRequest;
    openGroup = group;
    members = [];
    memberOffset = next;
    try {
      const result = await invoke<DuplicateItem[]>('query_duplicate_members', {
        group,
        offset: next,
        limit: memberLimit,
      });
      if (token === memberRequest) members = result;
    } catch (problem) {
      if (token === memberRequest) onerror(problem);
    }
  }
  async function start(): Promise<void> {
    if (!enabled || working) return;
    request += 1;
    memberRequest += 1;
    page = null;
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
      const id = await invoke<number>('start_duplicates', {
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
      onissues(await invoke<Issue[]>('get_duplicate_issues'));
    } catch (problem) {
      onerror(problem);
    }
  }
  onMount(() => {
    if (isTauri()) void load(0);
  });
  onDestroy(() => {
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
  {#if page}
    <div class="duplicate-summary">
      <div><span>Duplicate groups</span><strong>{page.total.toLocaleString()}</strong></div>
      <div><span>Potential allocation</span><strong>{bytes(page.recoverable)}</strong></div>
      {#if page.cancelled}<span>Partial results</span>{/if}
      {#if page.errors}<button onclick={issues}>{page.errors} issues</button>{/if}
    </div>
    {#each page.groups as group (group.key)}
      <section class="panel duplicate-group">
        <button
          class="duplicate-group-heading"
          aria-expanded={openGroup === group.key}
          onclick={() => {
            if (openGroup === group.key) {
              openGroup = null;
              memberRequest += 1;
            } else void expand(group.key);
          }}
        >
          <Icon name={openGroup === group.key ? 'collapse' : 'expand'} />
          <strong>{group.copies.toLocaleString()} copies</strong><span
            >{bytes(group.logical)} each</span
          >
          <span class="number">{bytes(group.recoverable)}</span>
        </button>
        {#if openGroup === group.key}
          <div class="duplicate-members">
            {#each members as item (item.id)}
              <button class="duplicate-member" onclick={() => onselect(item.id)} title={item.path}>
                <Icon name="file" /><span
                  ><strong>{item.name}</strong><small>{item.path}</small></span
                >
                {#if item.links > 1}<small>Hard link</small>{/if}<span class="number"
                  >{bytes(item.allocated)}</span
                >
              </button>
            {/each}
          </div>
          {#if group.copies > memberLimit}<div class="duplicate-pagination">
              <button
                onclick={() => expand(group.key, Math.max(0, memberOffset - memberLimit))}
                disabled={memberOffset === 0}>Previous</button
              >
              <span
                >{memberOffset + 1}–{Math.min(group.copies, memberOffset + memberLimit)} of {group.copies}</span
              >
              <button
                onclick={() => expand(group.key, memberOffset + memberLimit)}
                disabled={memberOffset + memberLimit >= group.copies}>Next</button
              >
            </div>{/if}
        {/if}
      </section>
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
  {:else if !working && !loading}<div class="table-empty">No duplicate analysis</div>{/if}
</section>
