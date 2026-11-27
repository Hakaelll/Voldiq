<script lang="ts">
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { onDestroy, tick, untrack } from 'svelte';
  import { applicationBytes as exactBytes } from '../lib/applications';
  import type { TraySource } from '../lib/cleanup-tray';
  import type {
    CustomRule,
    CustomRuleDraft,
    CustomRulePage,
    CustomRuleReference,
  } from '../lib/custom-cleanup';
  import type { IndexId } from '../lib/index-context';
  import { customCleanupRuleQueries } from '../lib/queries';
  import type { ReviewWeights } from '../lib/review-priority';
  import { date, type NodeId } from '../lib/types';
  import ReviewPriorityDetails from './ReviewPriorityDetails.svelte';
  import ReviewPriorityMeter from './ReviewPriorityMeter.svelte';

  let {
    indexId,
    revision,
    enabled,
    durable,
    scope,
    weights,
    onadd,
    onchanged,
  }: {
    indexId: IndexId | null;
    revision: number;
    enabled: boolean;
    durable: boolean;
    scope: { id: NodeId; path: string } | null;
    weights: ReviewWeights;
    onadd: (source: TraySource, ids: readonly NodeId[], owner: IndexId | null) => boolean;
    onchanged: (id: number) => void;
  } = $props();

  const limit = 50;
  const initialDraft: CustomRuleDraft = {
    name: '',
    extension: '',
    minimum_age_days: 30,
    minimum_logical: '1',
    expression: '',
  };
  let rules = $state.raw<CustomRule[]>([]);
  let selectedId = $state<number | null>(null);
  const selected = $derived(rules.find((rule) => rule.reference.id === selectedId));
  let loadingRules = $state(true);
  let listProblem = $state('');
  let listGeneration = 0;
  let page = $state.raw<CustomRulePage | null>(null);
  let pageOwner = $state<IndexId | null>(null);
  let offset = $state(0);
  let pageBusy = $state(false);
  let pageProblem = $state('');
  let pageRetry = $state(0);
  let picked = $state.raw<ReadonlySet<NodeId>>(new Set());
  let draft = $state({ ...initialDraft });
  let form = $state<
    | { kind: 'create'; index: IndexId; scope: { id: NodeId; path: string } }
    | { kind: 'edit'; index: IndexId; rule: CustomRule }
    | null
  >(null);
  let removing = $state<CustomRule | null>(null);
  let mutation = $state(false);
  let mutationProblem = $state('');
  let nameField = $state<HTMLInputElement>();
  let resultHeading = $state<HTMLHeadingElement>();
  let heading = $state<HTMLHeadingElement>();
  let formOpener: HTMLElement | null = null;

  async function loadRules(preferred = selectedId): Promise<void> {
    const token = ++listGeneration;
    loadingRules = true;
    listProblem = '';
    try {
      const saved = await invoke<CustomRule[]>('list_custom_cleanup_rules');
      if (token !== listGeneration) return;
      if (selected?.reference.id !== preferred) offset = 0;
      rules = saved;
      selectedId =
        saved.find((rule) => rule.reference.id === preferred)?.reference.id ??
        saved[0]?.reference.id ??
        null;
    } catch (problem) {
      if (token === listGeneration) listProblem = String(problem);
    } finally {
      if (token === listGeneration) loadingRules = false;
    }
  }
  $effect(() => {
    if (durable && isTauri()) untrack(() => void loadRules());
    else loadingRules = false;
  });
  onDestroy(() => listGeneration++);

  $effect(() => {
    const owner = indexId;
    const reference = selected?.reference;
    const position = offset;
    const activeWeights = weights;
    void revision;
    void pageRetry;
    page = null;
    pageProblem = '';
    pageOwner = null;
    picked = new Set();
    if (!owner || !reference || !enabled || !durable || !isTauri()) {
      pageBusy = false;
      return;
    }
    pageBusy = true;
    let current = true;
    const pending = customCleanupRuleQueries.request({
      indexId: owner,
      reference,
      request: { offset: position, limit, weights: activeWeights },
    });
    void pending.result
      .then((result) => {
        if (!current || result === null || indexId !== owner) return;
        if (
          result.rule.reference.id !== reference.id ||
          result.rule.reference.revision !== reference.revision
        ) {
          throw Error('The rule changed. Reload the saved rules.');
        }
        if (position > 0 && position >= result.total) {
          offset = Math.max(0, Math.floor((result.total - 1) / limit) * limit);
          return;
        }
        page = result;
        pageOwner = owner;
      })
      .catch((problem) => {
        if (current) pageProblem = String(problem);
      })
      .finally(() => {
        if (current) pageBusy = false;
      });
    return () => {
      current = false;
      pending.cancel();
    };
  });

  function selectRule(id: number): void {
    if (selectedId === id) return;
    selectedId = id;
    offset = 0;
    mutationProblem = '';
  }

  async function startForm(rule?: CustomRule): Promise<void> {
    if (!enabled || indexId === null || (!rule && !scope)) return;
    formOpener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    if (rule) {
      form = { kind: 'edit', index: indexId, rule };
      draft = {
        name: rule.name,
        extension: rule.extension,
        minimum_age_days: rule.minimum_age_days,
        minimum_logical: rule.minimum_logical,
        expression: rule.expression,
      };
    } else if (scope) {
      form = { kind: 'create', index: indexId, scope };
      draft = { ...initialDraft };
    }
    mutationProblem = '';
    await tick();
    nameField?.focus();
  }

  async function closeForm(): Promise<void> {
    form = null;
    const restore = formOpener;
    formOpener = null;
    await tick();
    if (restore?.isConnected && !restore.matches(':disabled')) restore.focus();
    else heading?.focus();
  }

  async function saveRule(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const editing = form;
    if (!editing || !enabled || editing.index !== indexId || mutation) return;
    mutation = true;
    mutationProblem = '';
    try {
      const saved = await invoke<CustomRule>('save_custom_cleanup_rule', {
        indexId: editing.index,
        request: {
          scope: editing.kind === 'create' ? editing.scope.id : null,
          expected: editing.kind === 'edit' ? editing.rule.reference : null,
          draft: { ...draft, name: draft.name.trim(), extension: draft.extension.trim() },
        },
      });
      if (editing.kind === 'edit') onchanged(saved.reference.id);
      offset = 0;
      await loadRules(saved.reference.id);
      mutation = false;
      await closeForm();
    } catch (problem) {
      mutationProblem = String(problem);
    } finally {
      mutation = false;
    }
  }

  async function removeRule(rule: CustomRule): Promise<void> {
    if (mutation) return;
    mutation = true;
    mutationProblem = '';
    try {
      await invoke<void>('remove_custom_cleanup_rule', { reference: rule.reference });
      onchanged(rule.reference.id);
      removing = null;
      offset = 0;
      await loadRules(null);
      await tick();
      heading?.focus();
    } catch (problem) {
      mutationProblem = String(problem);
    } finally {
      mutation = false;
    }
  }

  function changePicked(id: NodeId, checked: boolean): void {
    const next = new Set(picked);
    if (checked) next.add(id);
    else next.delete(id);
    picked = next;
  }
  function addPicked(reference: CustomRuleReference): void {
    if (onadd({ kind: 'custom', reference }, [...picked], pageOwner)) picked = new Set();
  }
  async function changePage(position: number): Promise<void> {
    offset = position;
    await tick();
    resultHeading?.focus();
  }
</script>

<section class="custom-rules" aria-labelledby="custom-rules-title">
  <header class="rules-heading">
    <div>
      <h2 id="custom-rules-title" tabindex="-1" bind:this={heading}>Custom cleanup rules</h2>
      <p>Find files in a saved folder by extension, size and time since modification.</p>
    </div>
    {#if durable}
      <button
        disabled={!enabled || !scope || mutation || form !== null}
        onclick={() => startForm()}
      >
        New rule
      </button>
    {/if}
  </header>

  {#if !durable}
    <p class="rules-empty">Saved cleanup rules require a profile with durable storage.</p>
  {:else if !isTauri()}
    <p class="rules-empty">Open Voldiq on Windows to use your saved rules.</p>
  {:else if loadingRules && rules.length === 0 && !form}
    <p class="rules-empty" role="status">Loading saved rules…</p>
  {:else}
    {#if listProblem}
      <p role="alert">{listProblem} <button onclick={() => loadRules()}>Reload rules</button></p>
    {/if}
    {#if !enabled && !rules.length}
      <p class="rules-empty">
        Scan a folder to create a rule. To use a subfolder, open it in Explorer first.
      </p>
    {:else}
      <div class="rules-layout" class:has-list={rules.length > 0}>
        {#if rules.length}
          <ul class="rules-list" aria-label="Saved rules">
            {#each rules as rule (rule.reference.id)}
              <li>
                <button
                  aria-current={rule.reference.id === selectedId ? 'true' : undefined}
                  disabled={mutation || form !== null}
                  onclick={() => selectRule(rule.reference.id)}
                >
                  <strong>{rule.name}</strong>
                  <small title={rule.root}>{rule.root}</small>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
        <div class="rule-content">
          {#if form}
            <form
              onsubmit={saveRule}
              aria-label={form.kind === 'create' ? 'Create rule' : 'Edit rule'}
            >
              <h3>{form.kind === 'create' ? 'New rule' : 'Edit rule'}</h3>
              <p class="rule-scope">
                {form.kind === 'create' ? form.scope.path : form.rule.root}
              </p>
              <div class="form-fields">
                <label
                  >Name <input
                    bind:this={nameField}
                    bind:value={draft.name}
                    required
                    maxlength="80"
                    disabled={mutation}
                  /></label
                >
                <label
                  >Extension <input
                    bind:value={draft.extension}
                    required
                    maxlength="17"
                    pattern={'\\.?[A-Za-z0-9]{1,16}'}
                    disabled={mutation}
                  /></label
                >
                <label
                  >Minimum age in days <input
                    type="number"
                    bind:value={draft.minimum_age_days}
                    required
                    min="1"
                    max="36500"
                    disabled={mutation}
                  /></label
                >
                <label
                  >Minimum size in bytes <input
                    bind:value={draft.minimum_logical}
                    inputmode="numeric"
                    required
                    pattern={'[1-9][0-9]{0,19}'}
                    disabled={mutation}
                  /></label
                >
                <label class="rule-expression">
                  Additional search filter (optional) <input
                    bind:value={draft.expression}
                    spellcheck="false"
                    placeholder="name:report"
                    disabled={mutation}
                  />
                </label>
              </div>
              {#if indexId !== form.index}
                <p role="status">The analysis changed. Close this form and start again.</p>
              {/if}
              {#if mutationProblem}<p role="alert">{mutationProblem}</p>{/if}
              <div class="rule-actions">
                <button type="submit" disabled={mutation || !enabled || form.index !== indexId}>
                  {mutation ? 'Saving…' : form.kind === 'edit' ? 'Save revision' : 'Save rule'}
                </button>
                <button type="button" class="quiet-action" disabled={mutation} onclick={closeForm}
                  >Cancel</button
                >
              </div>
            </form>
          {:else if selected}
            <div class="rules-heading">
              <div>
                <h3>{selected.name}</h3>
                <p class="rule-scope">{selected.root}</p>
                <p>
                  .{selected.extension} · {selected.minimum_age_days}+ days · {exactBytes(
                    selected.minimum_logical,
                  )}+ · Revision {selected.reference.revision}
                </p>
                {#if selected.expression}<p class="rule-scope">Filter <code>{selected.expression}</code></p>{/if}
              </div>
              <div class="rule-actions">
                <button
                  class="quiet-action"
                  disabled={mutation || !enabled}
                  onclick={() => startForm(selected)}>Edit</button
                >
                <button
                  class="quiet-action"
                  disabled={mutation}
                  onclick={() => {
                    removing = selected ?? null;
                    mutationProblem = '';
                  }}>Remove</button
                >
              </div>
            </div>
            {#if removing?.reference.id === selected.reference.id}
              <div class="remove-rule">
                <p>Remove this rule and its selections from the tray? Files stay on disk.</p>
                <button disabled={mutation} onclick={() => selected && removeRule(selected)}
                  >Remove rule</button
                >
                <button
                  class="quiet-action"
                  disabled={mutation}
                  onclick={() => {
                    removing = null;
                    mutationProblem = '';
                  }}>Keep rule</button
                >
                {#if mutationProblem}<p role="alert">{mutationProblem}</p>{/if}
              </div>
            {/if}
            <h4 tabindex="-1" bind:this={resultHeading}>Matches</h4>
            {#if !enabled}
              <p class="rules-empty">Open a current scan that includes the saved folder.</p>
            {:else if pageProblem}
              <p role="alert">
                {pageProblem}
                <button class="quiet-action" onclick={() => pageRetry++}>Retry</button>
              </p>
            {:else if pageBusy}
              <p class="rules-empty" role="status">Finding matches…</p>
            {:else if page}
              <p class="results-summary">
                {page.total.toLocaleString()} matches · {exactBytes(page.logical)} logical size
              </p>
              {#if !page.complete || page.unverified_branches || page.unverified_files}
                <p class="rules-empty">
                  Coverage is incomplete. {page.unverified_branches} unverified branches · {page.unverified_files}
                  files with incomplete evidence.
                </p>
              {/if}
              {#if page.items.length}
                <p class="rule-note">
                  Candidates need review. Current use, regeneration and recoverable space require
                  separate evidence.
                </p>
                <ul class="matches">
                  {#each page.items as item (item.id)}
                    <li>
                      <div class="match-row">
                        <label>
                          <input
                            type="checkbox"
                            checked={picked.has(item.id)}
                            onchange={(event) => changePicked(item.id, event.currentTarget.checked)}
                          />
                          <span
                            ><strong>{item.path.split(/[\\/]/).at(-1)}</strong><small
                              >{item.path}</small
                            ></span
                          >
                        </label>
                        <div class="match-metadata number">
                          <strong title={`${item.logical} logical bytes`}
                            >{exactBytes(item.logical)}</strong
                          >
                          <small
                            >Modified {date(item.modified)}{item.hard_link
                              ? ' · Hard link'
                              : ''}</small
                          >
                          <small
                            >{item.allocated === null
                              ? 'Allocation unknown'
                              : `${exactBytes(item.allocated)} allocated`}</small
                          >
                        </div>
                      </div>
                      <details>
                        <summary>
                          <span>Review priority</span>
                          <ReviewPriorityMeter priority={item.priority} />
                        </summary>
                        <ReviewPriorityDetails
                          priority={item.priority}
                          assessedAt={page.assessed_at}
                        />
                      </details>
                    </li>
                  {/each}
                </ul>
                <div class="rule-actions">
                  <button
                    disabled={!picked.size || mutation || !enabled}
                    onclick={() => selected && addPicked(selected.reference)}
                  >
                    {picked.size ? `Add ${picked.size} to review tray` : 'Add to review tray'}
                  </button>
                  {#if page.total > limit}
                    <span>{offset + 1}–{offset + page.items.length} of {page.total}</span>
                    <button
                      class="quiet-action"
                      disabled={offset === 0}
                      onclick={() => changePage(offset - limit)}>Previous</button
                    >
                    <button
                      class="quiet-action"
                      disabled={offset + limit >= page.total}
                      onclick={() => changePage(offset + limit)}>Next</button
                    >
                  {/if}
                </div>
              {:else}
                <p class="rules-empty">No indexed files meet all criteria.</p>
              {/if}
            {/if}
          {:else}
            <p class="rules-empty">
              Use the current folder to create your first rule. Open a subfolder in Explorer to
              narrow its scope.
            </p>
          {/if}
        </div>
      </div>
    {/if}
  {/if}
</section>

<style>
  .custom-rules {
    max-width: 1200px;
  }
  .rules-heading,
  .rule-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: start;
    justify-content: space-between;
    gap: 6px 16px;
  }
  .rule-actions {
    align-items: center;
    justify-content: start;
  }
  h2,
  h3,
  h4,
  p,
  ul {
    margin: 0;
  }
  .rules-heading p,
  .rules-empty,
  small,
  .rule-note {
    color: var(--secondary);
  }
  .rules-heading p {
    margin: 4px 0 0;
  }
  h4 {
    margin-top: 20px;
  }
  .rule-scope,
  small {
    overflow-wrap: anywhere;
  }
  .rules-empty,
  .rule-note {
    margin-block: 8px;
  }
  .rules-layout {
    margin-top: 16px;
  }
  .rules-layout.has-list {
    display: grid;
    grid-template-columns: clamp(160px, 23%, 245px) minmax(0, 1fr);
    gap: clamp(16px, 2vw, 26px);
  }
  .rules-list,
  .matches {
    list-style: none;
    padding: 0;
  }
  .rules-list {
    max-height: min(420px, 48vh);
    overflow: auto;
    padding-inline-end: 10px;
    border-inline-end: 1px solid var(--border);
  }
  .rules-list button {
    display: grid;
    gap: 4px;
    width: 100%;
    padding: 8px 10px;
    text-align: left;
    overflow-wrap: anywhere;
    border-color: transparent;
    background: transparent;
  }
  .rules-list button:hover {
    background: var(--action-hover);
  }
  .rules-list button[aria-current] {
    border-inline-start-color: var(--accent);
    background: var(--accent-fill);
  }
  .form-fields {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 220px));
    gap: 10px 14px;
    margin-block: 12px;
  }
  .form-fields label {
    display: grid;
    gap: 4px;
    min-width: 0;
  }
  .form-fields input {
    min-width: 0;
  }
  .form-fields .rule-expression {
    grid-column: 1 / -1;
  }
  .remove-rule {
    padding-block: 8px;
  }
  .remove-rule p {
    margin-bottom: 4px;
  }
  .results-summary {
    margin: 4px 0;
  }
  .matches {
    margin-block: 8px 12px;
  }
  .matches li {
    border-bottom: 1px solid var(--border);
    padding-block: 8px;
  }
  .match-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) max-content;
    gap: 6px 14px;
  }
  .matches label {
    display: flex;
    gap: 8px;
    align-items: start;
    min-width: 0;
  }
  .matches label strong {
    overflow-wrap: anywhere;
  }
  .matches small {
    display: block;
  }
  .match-metadata {
    text-align: right;
  }
  .matches summary {
    margin-inline-start: 22px;
    margin-top: 6px;
    cursor: pointer;
    color: var(--secondary);
  }
  .matches summary > span {
    margin-inline-end: 8px;
  }
  @media (max-width: 860px) {
    .rules-layout.has-list {
      grid-template-columns: minmax(0, 1fr);
    }
    .rules-list {
      max-height: 155px;
      border-inline-end: none;
    }
  }
  @media (max-width: 620px) {
    .match-row {
      grid-template-columns: minmax(0, 1fr);
    }
    .match-metadata {
      padding-inline-start: 22px;
      text-align: start;
    }
  }
</style>
