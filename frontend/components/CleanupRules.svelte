<script lang="ts">
  import { onDestroy, untrack, tick } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { bytes, type CleanupRulePage, type CleanupPreview } from '../lib/types';
  let {
    revision,
    manageable,
    busy,
    scanning,
    onscan,
    onbusy,
    onpreview,
    onerror,
  }: {
    revision: number;
    manageable: boolean;
    busy: boolean;
    scanning: boolean;
    onscan: (path: string) => Promise<void>;
    onbusy: (value: boolean) => void;
    onpreview: (plan: CleanupPreview) => void;
    onerror: (problem: unknown) => void;
  } = $props();
  const limit = 50;
  let rule = $state<'user_temp' | 'old_files' | 'project_temp'>('user_temp');
  let page = $state<CleanupRulePage | null>(null),
    offset = $state(0);
  let loadState = $state<'loading' | 'ready' | 'unavailable'>('loading');
  let selected = $state(new Set<number>()),
    reviewing = $state(false);
  let generation = 0,
    alive = true;
  let region: HTMLDivElement;
  async function load(next: number): Promise<void> {
    const token = ++generation;
    const restoreFocus = region?.contains(document.activeElement);
    loadState = 'loading';
    try {
      const result = await invoke<CleanupRulePage>('query_cleanup_rule', {
        rule,
        offset: next,
        limit,
      });
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
      if (token === generation) region?.focus();
    }
  }
  function check(id: number, value: boolean): void {
    const next = new Set(selected);
    if (value) {
      if (next.size >= 500) {
        onerror('Select up to 500 files.');
        return;
      }
      next.add(id);
    } else next.delete(id);
    selected = next;
  }
  async function review(): Promise<void> {
    if (!manageable || busy || reviewing || selected.size === 0) return;
    reviewing = true;
    onbusy(true);
    try {
      const plan = await invoke<CleanupPreview>('preview_cleanup_rule', {
        rule,
        selection: [...selected],
      });
      if (alive) onpreview(plan);
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
    untrack(() => {
      selected = new Set();
      offset = 0;
      if (isTauri()) void load(0);
      else loadState = 'unavailable';
    });
  });
  onDestroy(() => {
    alive = false;
    generation += 1;
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
    <select aria-label="Cleanup rule" bind:value={rule} disabled={busy || reviewing}>
      <option value="user_temp">User temporary files</option>
      <option value="old_files">Old files</option>
      <option value="project_temp">Project temporary files</option>
    </select>
  </div>
  {#if loadState === 'loading'}<div class="rule-empty" role="status">Loading</div>
  {:else if loadState === 'unavailable'}<div class="rule-empty">
      Unavailable<button onclick={() => load(offset)} disabled={busy}>Retry</button>
    </div>
  {:else if page}
    <div class="rule-heading">
      <div>
        <h2>{page.rule.name}</h2>
        <span
          >{rule === 'old_files' ? '90+ days · Manual selection' : '7+ days · .tmp / .temp'}</span
        >
      </div>
      <button
        onclick={() => {
          if (page) void onscan(page.root);
        }}
        disabled={busy || !page.root}>{rule === 'user_temp' ? 'Scan user temp' : 'Rescan'}</button
      >
    </div>
    {#if !page.indexed}<div class="rule-empty">Not indexed</div>
    {:else}
      <div class="rule-summary">
        <span
          >{page.total} files · {bytes(page.logical)} logical · {bytes(page.reclaimable)} potential{#if scanning || !page.complete}
            · Partial{/if}</span
        >
      </div>
      {#if selected.size}<div class="rule-actions">
          <span>{selected.size} selected</span><button
            class="primary"
            onclick={review}
            disabled={!manageable || busy || loadState !== 'ready'}>Review selection</button
          ><button
            onclick={() => {
              selected = new Set();
            }}
            disabled={busy}>Clear</button
          >
        </div>{/if}
      {#if page.items.length === 0}<div class="rule-empty">No matching files</div>
      {:else}<div class="rule-scroll">
          <table>
            <thead
              ><tr
                ><th><span class="sr-only">Selection</span></th><th>Path</th><th class="number"
                  >Logical</th
                ><th class="number">Potential</th></tr
              ></thead
            ><tbody
              >{#each page.items as item (item.id)}<tr
                  ><td
                    ><input
                      type="checkbox"
                      aria-label={`Select ${item.path}`}
                      checked={selected.has(item.id)}
                      disabled={!manageable || busy}
                      onchange={(event) => {
                        check(item.id, event.currentTarget.checked);
                        event.currentTarget.checked = selected.has(item.id);
                      }}
                    /></td
                  ><td
                    >{item.path}{#if item.hard_link}<span class="muted"> · Hard link</span>{/if}</td
                  ><td class="number">{bytes(item.logical)}</td><td class="number"
                    >{bytes(item.reclaimable)}</td
                  ></tr
                >{/each}</tbody
            >
          </table>
        </div>
        {#if page.total > limit}<div class="rule-actions">
            <span>{offset + 1}–{offset + page.items.length} of {page.total}</span><button
              onclick={() => load(Math.max(0, offset - limit))}
              disabled={offset === 0 || busy}>Previous</button
            ><button
              onclick={() => load(offset + limit)}
              disabled={offset + limit >= page.total || busy}>Next</button
            >
          </div>{/if}
      {/if}
    {/if}
  {/if}
</div>

<style>
  .cleanup-rules {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--canvas);
    overflow: hidden;
  }
  .rule-heading,
  .rule-summary,
  .rule-actions {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    flex-wrap: wrap;
  }
  .rule-heading {
    justify-content: space-between;
  }
  .rule-heading span,
  .rule-summary,
  .rule-actions > span {
    color: var(--secondary);
    font-size: 12px;
  }
  .rule-heading span {
    display: block;
    margin-top: 4px;
  }
  .rule-summary,
  .rule-actions {
    border-top: 1px solid var(--border);
  }
  .rule-actions > span {
    margin-right: auto;
  }
  .rule-empty {
    min-height: 140px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--muted);
  }
  .rule-scroll {
    overflow: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  th,
  td {
    border-top: 1px solid var(--border);
    padding: 10px 16px;
    text-align: left;
  }
  th {
    font-weight: 500;
    color: var(--secondary);
  }
  td:nth-child(2) {
    overflow-wrap: anywhere;
  }
  td:first-child {
    width: 44px;
  }
  input[type='checkbox'] {
    width: 14px;
    height: 14px;
    accent-color: var(--accent);
  }
  .number {
    text-align: right;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .muted {
    color: var(--muted);
  }
</style>
