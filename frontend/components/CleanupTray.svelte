<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { applicationBytes as exactBytes } from '../lib/applications';
  import {
    trayFolders,
    trayPercent,
    type TrayGroup,
    type TrayMetric,
    type TraySummary,
    type TrayTarget,
  } from '../lib/cleanup-tray';
  import type { IndexId } from '../lib/index-context';
  import { cleanupTrayQueries } from '../lib/queries';
  import { date } from '../lib/types';
  import Icon from './Icon.svelte';

  let {
    indexId,
    groups,
    target,
    busy,
    preparing,
    preparationProblem,
    canCancel,
    onclose,
    onremove,
    onclear,
    ontarget,
    onprepare,
    oncancel,
  }: {
    indexId: IndexId | null;
    groups: readonly TrayGroup[];
    target: TrayTarget | null;
    busy: boolean;
    preparing: boolean;
    preparationProblem: string;
    canCancel: boolean;
    onclose: () => void;
    onremove: (id: number) => void;
    onclear: () => void;
    ontarget: (value: TrayTarget | null) => void;
    onprepare: () => void;
    oncancel: () => void;
  } = $props();

  type Measurement = {
    indexId: IndexId;
    groups: readonly TrayGroup[];
    target: TrayTarget | null;
    summary: TraySummary;
  };
  let measurement = $state.raw<Measurement | null>(null);
  let loading = $state(false),
    problem = $state(''),
    retry = $state(0);
  let expression = $state(untrack(() => target?.expression ?? ''));
  let metric = $state<TrayMetric>(untrack(() => target?.metric ?? 'logical'));
  let region: HTMLElement;
  const count = $derived(groups.reduce((total, group) => total + group.selection.length, 0));
  const summary = $derived(
    measurement?.indexId === indexId && measurement.groups === groups ? measurement.summary : null,
  );
  const progress = $derived(summary && measurement?.target === target ? summary.target : null);
  const folders = $derived(summary?.groups.map((group) => trayFolders(group.items)) ?? []);

  $effect(() => {
    void retry;
    problem = '';
    if (indexId === null || groups.length === 0) {
      loading = false;
      measurement = null;
      return;
    }
    const owner = { indexId, groups, target };
    loading = true;
    const request = cleanupTrayQueries.request(owner);
    let current = true;
    void request.result.then(
      (result) => {
        if (!current) return;
        loading = false;
        if (result) measurement = { ...owner, summary: result };
        else problem = 'The review summary was cancelled.';
      },
      (error: unknown) => {
        if (!current) return;
        loading = false;
        problem = String(error);
      },
    );
    return () => {
      current = false;
      request.cancel();
    };
  });

  onMount(() => region.focus({ preventScroll: true }));
  function remove(id: number) {
    onremove(id);
    void tick().then(() => {
      if (region?.isConnected && document.activeElement === document.body)
        region.focus({ preventScroll: true });
    });
  }
  function applyTarget(event: SubmitEvent) {
    event.preventDefault();
    const value = expression.trim();
    ontarget(value ? { metric, expression: value } : null);
  }
</script>

<svelte:window
  onkeydown={(event) => {
    if (
      event.key === 'Escape' &&
      !event.defaultPrevented &&
      !preparing &&
      event.target instanceof Node &&
      region?.contains(event.target)
    ) {
      event.preventDefault();
      onclose();
    }
  }}
/>

<section
  id="cleanup-review-tray"
  class="cleanup-tray"
  aria-labelledby="cleanup-tray-title"
  tabindex="-1"
  bind:this={region}
>
  <header class="cleanup-tray-heading">
    <h2 id="cleanup-tray-title">Cleanup review</h2>
    <span aria-live="polite">{count.toLocaleString()} {count === 1 ? 'file' : 'files'}</span>
    <button
      class="tray-close"
      aria-label="Close cleanup review"
      title="Close cleanup review"
      disabled={preparing}
      onclick={onclose}><Icon name="close" size={16} /></button
    >
  </header>
  {#if !count}
    <p class="cleanup-tray-empty">
      Select files in Explorer, or add candidates from Cleanup and Duplicates.
    </p>
  {:else}
    <div class="cleanup-tray-body">
      {#if loading && !summary}<p role="status">Reading selected files…</p>{/if}
      {#if problem}<p role="alert">{problem} <button onclick={() => retry++}>Retry</button></p>{/if}
      {#if summary}
        <div class="cleanup-tray-layout">
          <section class="tray-files" aria-label="Files by source and folder">
            {#each summary.groups as group, at}
              <details class="tray-group" open={summary.groups.length === 1}>
                <summary
                  ><strong>{group.origin.label}</strong><span
                    >{group.items.length} {group.items.length === 1 ? 'file' : 'files'}</span
                  ></summary
                >
                <p>{group.origin.description}</p>
                {#each folders[at] as folder}
                  <h3 title={folder.path}>{folder.path}</h3>
                  <ul>
                    {#each folder.items as item (item.id)}
                      <li>
                        <span class="tray-file-name" title={item.name}
                          >{item.name}{#if item.hard_link}<small> · Hard link</small>{/if}</span
                        >
                        <span
                          class="number"
                          title={`${item.logical} logical bytes. ${item.allocated === null ? 'Allocation unknown.' : `${item.allocated} allocated bytes.`}`}
                          >{exactBytes(item.logical)}</span
                        >
                        <button
                          onclick={() => remove(item.id)}
                          aria-label={`Remove ${item.name} from review`}
                          disabled={busy}>Remove</button
                        >
                      </li>
                    {/each}
                  </ul>
                {/each}
              </details>
            {/each}
          </section>
          <section class="tray-simulation" aria-labelledby="tray-simulation-title">
            <h3 id="tray-simulation-title">Simulated removal</h3>
            <p class="tray-scope" title={summary.root}>{summary.root}</p>
            <dl class="tray-totals">
              <div>
                <dt>Selected logical size</dt>
                <dd title={`${summary.selected_logical} bytes`}>
                  {exactBytes(summary.selected_logical)}
                </dd>
              </div>
              <div>
                <dt>Indexed allocation</dt>
                <dd>
                  {!summary.allocation_complete && summary.known_allocated === '0'
                    ? 'Unknown'
                    : exactBytes(
                        summary.known_allocated,
                      )}{#if !summary.allocation_complete && summary.known_allocated !== '0'}
                    <small>known</small>{/if}
                </dd>
              </div>
            </dl>
            {#if summary.repeated_identity_paths > 0}<p>
                Allocation counts shared file identities once.
              </p>{/if}
            {#if summary.hard_link_paths > 0}<p>
                Hard links can retain their allocation through other names.
              </p>{/if}
            {#if summary.remaining_logical !== null}
              <div class="tray-chart">
                <div>
                  <span>Indexed</span><span class="number">{exactBytes(summary.root_logical)}</span>
                </div>
                <div class="tray-track" aria-hidden="true">
                  <span style:width={`${trayPercent(summary.root_logical, summary.root_logical)}%`}
                  ></span>
                </div>
                <div>
                  <span>After selected paths are removed</span><span class="number"
                    >{exactBytes(summary.remaining_logical)}</span
                  >
                </div>
                <div class="tray-track" aria-hidden="true">
                  <span
                    style:width={`${trayPercent(summary.remaining_logical, summary.root_logical)}%`}
                  ></span>
                </div>
              </div>
              <p>
                Both bars use the indexed logical size as their scale. Disk free space is measured
                separately.
              </p>
            {:else}<p>
                Simulation unavailable. The selected sizes exceed the recorded index total.
              </p>{/if}
            {#if summary.incomplete}<p>The index is partial or provisional.</p>{/if}
            <p>Index measured {date(summary.indexed_at)}.</p>
            <details class="tray-target">
              <summary
                >Review target{#if progress?.kind === 'measured'}<span
                    >{exactBytes(progress.bytes)}</span
                  >{/if}</summary
              >
              <form onsubmit={applyTarget}>
                <label
                  >Amount <input
                    bind:value={expression}
                    placeholder="1 GiB"
                    maxlength="128"
                    disabled={busy}
                  /></label
                >
                <label
                  >Measure <select bind:value={metric} disabled={busy}
                    ><option value="logical">Logical size</option><option value="allocated"
                      >Indexed allocation</option
                    ></select
                  ></label
                >
                <button type="submit" disabled={busy}>Set target</button>
                {#if target}<button
                    type="button"
                    disabled={busy}
                    onclick={() => {
                      expression = '';
                      ontarget(null);
                    }}>Clear target</button
                  >{/if}
              </form>
              {#if progress?.kind === 'invalid'}<p role="alert">{progress.message}</p>
              {:else if progress?.kind === 'measured'}<p role="status">
                  {!progress.complete && progress.selected_bytes === '0'
                    ? 'Allocation unknown'
                    : `${exactBytes(progress.selected_bytes)}${progress.complete ? '' : ' known'}`} of
                  {exactBytes(progress.bytes)} · {progress.reached
                    ? 'Target reached'
                    : progress.complete
                      ? 'Below target'
                      : 'Allocation incomplete'}
                </p>{/if}
              <p>The target tracks the files you choose.</p>
            </details>
          </section>
        </div>
      {/if}
    </div>
    <footer class="cleanup-tray-actions">
      {#if preparationProblem}<p class="tray-preparation-error" role="alert">
          {preparationProblem}
        </p>{/if}
      <button onclick={onclear} disabled={busy}>Clear tray</button>
      {#if preparing}<span role="status">Preparing the plan…</span><button
          onclick={oncancel}
          disabled={!canCancel}>Cancel preparation</button
        >
      {:else}<button class="primary" onclick={onprepare} disabled={busy || !summary}
          >Prepare plan</button
        >{/if}
    </footer>
  {/if}
</section>

<style>
  .cleanup-tray {
    flex: none;
    min-width: 0;
    display: flex;
    flex-direction: column;
    max-height: min(48vh, 430px);
    background: var(--panel);
    border-top: 1px solid var(--border);
  }
  .cleanup-tray-heading,
  .cleanup-tray-actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px 14px;
    padding: 8px 24px;
  }
  .tray-preparation-error {
    flex: 1 0 100%;
    max-height: 4em;
    overflow: auto;
    overflow-wrap: anywhere;
    color: var(--text);
  }
  .cleanup-tray-heading h2 {
    margin: 0;
    font-size: 17px;
  }
  .cleanup-tray-heading > span,
  .tray-group summary span,
  p,
  small {
    color: var(--muted);
  }
  .tray-close {
    margin-left: auto;
  }
  .cleanup-tray-heading button,
  .cleanup-tray-actions button:not(.primary),
  .cleanup-tray-body button {
    background: transparent;
    border-color: transparent;
  }
  .cleanup-tray-heading button:hover:not(:disabled),
  .cleanup-tray-actions button:not(.primary):hover:not(:disabled),
  .cleanup-tray-body button:hover:not(:disabled) {
    background: var(--paper);
  }
  .cleanup-tray-body {
    min-height: 0;
    overflow: auto;
    padding: 0 24px;
  }
  .cleanup-tray-body > p,
  .cleanup-tray-empty {
    margin: 4px 24px 14px;
  }
  .cleanup-tray-layout {
    display: grid;
    grid-template-columns: minmax(0, 1.3fr) minmax(275px, 1fr);
    gap: 28px;
  }
  .tray-group,
  .tray-target {
    border-top: 1px solid var(--border);
  }
  summary {
    cursor: pointer;
    padding: 8px 0;
  }
  summary span {
    margin-inline-start: 10px;
    font-size: 12px;
  }
  h3 {
    margin: 7px 0 4px;
    font-size: 14px;
  }
  .tray-files h3 {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
    font-size: 12px;
    font-weight: 500;
  }
  .tray-files ul {
    margin: 0 0 8px;
    padding: 0;
    list-style: none;
  }
  .tray-files li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    align-items: center;
    gap: 9px;
    min-height: 32px;
    border-bottom: 1px solid var(--border);
  }
  .tray-file-name,
  .tray-scope {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  p {
    margin: 3px 0 8px;
    font-size: 12px;
    line-height: 1.4;
  }
  .tray-totals {
    display: grid;
    gap: 4px;
    margin: 10px 0;
  }
  .tray-totals div,
  .tray-chart > div:not(.tray-track) {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 8px;
  }
  .tray-totals dd {
    margin: 0;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .tray-chart {
    display: grid;
    gap: 4px;
    margin: 7px 0;
    font-size: 12px;
  }
  .tray-track {
    height: 6px;
    background: var(--border);
    border-radius: 3px;
    overflow: hidden;
  }
  .tray-track span {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  form {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 10px;
    align-items: end;
    margin: 4px 0 8px;
  }
  form label {
    display: grid;
    min-width: 0;
    gap: 4px;
    font-size: 12px;
  }
  form input {
    width: 96px;
  }
  .cleanup-tray-actions {
    justify-content: flex-end;
    border-top: 1px solid var(--border);
  }
  @media (max-width: 1020px) {
    .cleanup-tray-layout {
      grid-template-columns: minmax(0, 1fr);
      gap: 12px;
    }
  }
</style>
