<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import OfflineSearch from './OfflineSearch.svelte';
  import type { IndexId } from '../lib/index-context';
  import type { Query, SearchInspection } from '../lib/types';
  import { createQueryRunner } from '../lib/query-runner';
  import {
    settleSearchDraft,
    validatedSearchDraft,
    searchDraftIsCurrent,
    searchErrorPosition,
    type SearchDraftRequest,
    type SearchDraftState,
  } from '../lib/search-editor';

  let {
    value,
    query,
    indexId,
    disabled,
    onapply,
    onclose,
  }: {
    value: string;
    query: Query;
    indexId: IndexId | null;
    disabled: boolean;
    onapply: (request: SearchDraftRequest) => void;
    onclose: (restore: boolean) => void;
  } = $props();
  let draft = $state(untrack(() => value));
  let input: HTMLInputElement;
  let panel: HTMLElement;
  let retry = $state(0);
  let inspectionState = $state.raw<SearchDraftState>(
    untrack(() => ({ kind: 'pending', request: { query, indexId, expression: draft } })),
  );
  const validation = createQueryRunner<SearchInspection, SearchDraftRequest, 'search_validation'>(
    'search_validation',
    {
      run: (_family, _id, request) =>
        invoke<SearchInspection>('inspect_search', {
          query: { ...request.query, expression: request.expression },
        }),
      // Inspection is a bounded pure compilation; local cancellation suppresses replies and queues.
      cancel: async () => {},
    },
  );
  const accepted = $derived(validatedSearchDraft(inspectionState, draft, query, indexId));
  const current = $derived(
    searchDraftIsCurrent(inspectionState, draft, query, indexId) ? inspectionState : null,
  );
  const errorPosition = $derived(
    current?.kind === 'invalid' ? searchErrorPosition(draft, current.offset) : null,
  );
  $effect(() => {
    void retry;
    const request: SearchDraftRequest = { query, indexId, expression: draft };
    inspectionState = { kind: 'pending', request };
    let alive = true;
    const timer = setTimeout(() => {
      void validation.request(request).result.then(
        (result) => {
          if (alive && result)
            inspectionState = settleSearchDraft(inspectionState, request, result);
          else if (alive)
            inspectionState = settleSearchDraft(inspectionState, request, {
              kind: 'failed',
              message: 'Validation was cancelled.',
            });
        },
        () => {
          if (alive)
            inspectionState = settleSearchDraft(inspectionState, request, {
              kind: 'failed',
              message: 'Syntax check unavailable.',
            });
        },
      );
    }, 150);
    return () => {
      alive = false;
      clearTimeout(timer);
      validation.cancel();
    };
  });
  onMount(() => {
    input.focus();
    input.select();
  });
  function apply(): void {
    const current = validatedSearchDraft(inspectionState, draft, query, indexId);
    if (current && !disabled) onapply(current);
  }
  function close(): void {
    onclose(panel.contains(document.activeElement));
  }
</script>

<div
  class="advanced-search"
  role="dialog"
  aria-modal="false"
  tabindex="-1"
  aria-label="Advanced query editor"
  bind:this={panel}
  onkeydown={(event) => {
    if (event.key === 'Escape') {
      event.stopPropagation();
      event.preventDefault();
      close();
    }
  }}
>
  <form
    onsubmit={(event) => {
      event.preventDefault();
      apply();
    }}
  >
    <label for="advanced-expression">Advanced query</label>
    <div class="advanced-search-entry">
      <input
        id="advanced-expression"
        bind:this={input}
        bind:value={draft}
        spellcheck="false"
        autocomplete="off"
        aria-describedby="advanced-search-help advanced-search-status"
        aria-invalid={current?.kind === 'invalid'}
        placeholder="type:file size>1GiB glob:*.mp4"
      />
      <button class="primary" type="submit" disabled={!accepted || disabled}>Apply query</button>
      <button type="button" onclick={close}>Cancel</button>
    </div>
    <div class="advanced-search-meta">
      <span id="advanced-search-help"
        >Other current filters also apply. Quotes keep phrases together.</span
      >
      <span
        id="advanced-search-status"
        role={current?.kind === 'invalid' || current?.kind === 'failed' ? 'alert' : 'status'}
      >
        {#if !current || current.kind === 'pending'}Checking syntax…{:else if current.kind === 'valid'}Syntax
          checked · {current.conditions} conditions{:else if current.kind === 'invalid'}{#if errorPosition !== null}Position
            {errorPosition}:
          {/if}{current.message}{:else}{current.message}
          <button type="button" onclick={() => retry++}>Retry</button>{/if}
      </span>
    </div>
  </form>
  <OfflineSearch
    {draft}
    {query}
    {indexId}
    {disabled}
    onchoose={(expression) => {
      draft = expression;
      input.focus();
    }}
  />
</div>
