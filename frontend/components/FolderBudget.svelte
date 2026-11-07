<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import Icon from './Icon.svelte';
  import { budgetQueries } from '../lib/queries';
  import type { IndexId } from '../lib/index-context';
  import {
    bytes,
    date,
    type BudgetReport,
    type FolderBudget,
    type ForecastReason,
  } from '../lib/types';
  let {
    indexId,
    id,
    path,
    sourceKey,
    disabled,
  }: { indexId: IndexId; id: number; path: string; sourceKey: string; disabled: boolean } =
    $props();
  type Context = { indexId: IndexId; id: number; sourceKey: string; detailed: boolean };
  type Model =
    | { kind: 'loading' }
    | { kind: 'failed'; context: Context; message: string }
    | { kind: 'ready'; context: Context; report: BudgetReport };
  let model = $state.raw<Model>({ kind: 'loading' });
  let expanded = $state(false),
    retry = $state(0),
    draft = $state(''),
    dirty = $state(false);
  let draftOwner = $state('');
  let draftVersion = '0';
  let saving = $state(false),
    problem = $state(''),
    notice = $state('');
  const day = (value: number): string =>
    new Date(value * 1000).toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
    });
  const key = $derived(`${indexId}:${id}`);
  const current = $derived(
    model.kind !== 'loading' &&
      model.context.indexId === indexId &&
      model.context.id === id &&
      model.context.sourceKey === sourceKey &&
      model.context.detailed === expanded
      ? model
      : null,
  );
  const report = $derived(current?.kind === 'ready' ? current.report : null);
  const reason: Record<ForecastReason, string> = {
    not_requested: 'Open for details.',
    no_budget: 'Set a limit to estimate when it may be reached.',
    unmeasured: 'Measurement date is unavailable.',
    partial: 'Coverage is incomplete.',
    insufficient_history: 'Needs at least six measurement days spanning a week.',
    stale: 'Scan again before estimating future growth.',
    volatile: 'Growth is too variable for a bounded estimate.',
    not_growing: 'No observed growth.',
    beyond_horizon: 'Limit is beyond the one-year estimate horizon.',
    at_limit: 'The indexed size is already at or above the limit.',
    incompatible: 'Historical accounting is incompatible.',
  };
  const status = $derived(
    report?.status === 'exceeded'
      ? 'Over limit'
      : report?.status === 'at_limit'
        ? 'At limit'
        : report?.status === 'within'
          ? 'Within limit'
          : report?.status === 'no_budget'
            ? 'Not set'
            : 'Coverage incomplete',
  );
  $effect(() => {
    void retry;
    const context: Context = { indexId, id, sourceKey, detailed: expanded };
    const target = `${indexId}:${id}`;
    model = { kind: 'loading' };
    let alive = true;
    void budgetQueries.request({ indexId, id, detailed: expanded }).result.then(
      (value) => {
        if (!alive) return;
        if (!value) {
          model = {
            kind: 'failed',
            context,
            message: 'Budget query was cancelled. Reload to retry.',
          };
          return;
        }
        model = { kind: 'ready', context, report: value };
        if (draftOwner !== target) {
          problem = '';
          notice = '';
        }
        if (draftOwner !== target || !dirty) {
          draft = value.budget?.expression ?? '';
          draftOwner = target;
          draftVersion = value.budget?.version ?? '0';
          dirty = false;
        }
      },
      (error: unknown) => {
        if (alive) model = { kind: 'failed', context, message: String(error) };
      },
    );
    return () => {
      alive = false;
      budgetQueries.cancel();
    };
  });
  async function save(remove = false): Promise<void> {
    if (!report || saving || disabled || draftOwner !== key || (remove && !report.budget)) return;
    const owner = key,
      expected = { indexId, nodeId: id, version: draftVersion };
    saving = true;
    problem = '';
    notice = '';
    try {
      if (remove && report.budget)
        await invoke<void>('remove_folder_budget', { ...expected, budgetId: report.budget.id });
      else await invoke<FolderBudget>('save_folder_budget', { ...expected, expression: draft });
      if (owner === key) {
        dirty = false;
        notice = remove ? 'Budget removed.' : 'Budget saved.';
        retry++;
      }
    } catch (error) {
      if (owner === key) problem = String(error);
    } finally {
      saving = false;
    }
  }
  function reload(): void {
    dirty = false;
    problem = '';
    notice = '';
    retry++;
  }
</script>

<details class="folder-budget" bind:open={expanded}>
  <summary title={path}>
    <span class="budget-heading"
      ><Icon name={expanded ? 'chevron-up' : 'chevron-down'} size={14} /><strong
        >Folder budget</strong
      >{#if expanded}<span title={path}>{path}</span>{/if}</span
    >
    {#if expanded || report?.status === 'exceeded' || report?.status === 'at_limit'}<span
        class="budget-summary"
        class:over={report?.status === 'exceeded'}
      >
        {#if report}<span
            >{report.coverage === 'complete' ? '' : 'At least '}{bytes(
              Number(report.logical_bytes),
            )}{#if report.budget}
              / {bytes(Number(report.budget.limit_bytes))}{/if}</span
          >
          {#if report.observed_percent !== null}<progress
              aria-label="Observed logical bytes relative to budget"
              value={Math.min(100, report.observed_percent)}
              max="100"
              aria-valuetext={`${report.observed_percent.toFixed(1)}% observed`}
            ></progress>{/if}<strong>{status}</strong>
        {:else if current?.kind === 'failed'}Unavailable{:else}Loading…{/if}
      </span>{/if}
  </summary>
  {#if expanded}<div class="budget-body">
      <span class="budget-basis"
        >Logical path bytes in this indexed folder. This is separate from volume free space.</span
      >
      {#if current?.kind === 'failed'}<p role="alert">
          {current.message} <button onclick={reload}>Reload</button>
        </p>{/if}
      {#if report}
        <form
          onsubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <label for="folder-budget-limit">Logical size limit</label>
          <div class="budget-entry">
            <input
              id="folder-budget-limit"
              bind:value={draft}
              oninput={() => (dirty = true)}
              placeholder="e.g. 100GiB"
              maxlength="128"
              disabled={disabled || saving}
            />
            <button type="submit" class="primary" disabled={disabled || saving || !draft.trim()}
              >Save limit</button
            >
            {#if report.budget}<button
                type="button"
                disabled={disabled || saving || dirty}
                onclick={() => void save(true)}>Remove limit</button
              >{/if}
            <button type="button" disabled={saving} onclick={reload}>Reload / discard edits</button>
          </div>
        </form>
        <div class="budget-insights">
          <div>
            <h3>Limit forecast</h3>
            {#if report.forecast.kind === 'estimate'}<strong
                >About {day(report.forecast.estimated_at)}</strong
              >
              <span>{day(report.forecast.earliest_at)} to {day(report.forecast.latest_at)}</span>
              <small
                >{report.forecast.samples} measurement days over {report.forecast.span_days} days · {bytes(
                  report.forecast.bytes_per_day,
                )}/day</small
              >
              <small
                >Observed-rate estimate; range uses the lowest and highest interval rates.</small
              >
            {:else}<span>{reason[report.forecast.reason]}</span>{/if}
          </div>
          <div>
            <h3>Image-file equivalent</h3>
            {#if report.image_equivalent}<strong
                >≈ {BigInt(report.image_equivalent.approximate_images).toLocaleString()} image files</strong
              >
              <small
                >Mean logical size: {bytes(
                  Number(report.image_equivalent.mean_logical_bytes),
                )}.</small
              >
              <small
                >First {report.image_equivalent.sample_count} of {report.image_equivalent.matched_images.toLocaleString()}
                indexed image-extension matches, ordered by name.</small
              >
            {:else}<span
                >{report.coverage === 'complete'
                  ? 'No positive-size image sample in this scope.'
                  : 'Unavailable while coverage is incomplete.'}</span
              >{/if}
          </div>
        </div>
        <span class="budget-basis"
          >{report.observation
            ? `Measured ${date(report.observation.completed_at)}`
            : 'Measurement date unavailable'} · {report.coverage} indexed coverage</span
        >
      {:else if current?.kind !== 'failed'}<p role="status">Loading budget details…</p>{/if}
      {#if problem && draftOwner === key}<p role="alert">
          {problem}
        </p>{/if}{#if notice && draftOwner === key}<p role="status">{notice}</p>{/if}
    </div>{/if}
</details>
