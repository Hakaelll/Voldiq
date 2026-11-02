<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import type { IndexId } from '../lib/index-context';
  import type { Query, SearchTranslation } from '../lib/types';
  import { createQueryRunner } from '../lib/query-runner';
  import {
    settleTranslation,
    translationIsCurrent,
    translationDraft,
    type TranslationRequest,
    type TranslationState,
  } from '../lib/search-translation';

  let {
    draft,
    query,
    indexId,
    disabled,
    onchoose,
  }: {
    draft: string;
    query: Query;
    indexId: IndexId | null;
    disabled: boolean;
    onchoose: (expression: string) => void;
  } = $props();
  let source = $state('');
  let partialAccepted = $state(false);
  let translationState = $state.raw<TranslationState | null>(null);
  let alive = true;
  const context = $derived<TranslationRequest>({ source, draft, query, indexId });
  const current = $derived(
    translationIsCurrent(translationState, context) ? translationState : null,
  );
  const proposalDraft = $derived(translationDraft(translationState, context, partialAccepted));
  const runner = createQueryRunner<SearchTranslation, TranslationRequest, 'search_translation'>(
    'search_translation',
    {
      run: (_family, _id, request) =>
        invoke<SearchTranslation>('translate_search', { source: request.source }),
      cancel: async () => {},
    },
  );
  function invalidate(): void {
    translationState = null;
    partialAccepted = false;
    runner.cancel();
  }
  $effect(() => {
    void draft;
    void query;
    void indexId;
    invalidate();
  });
  onMount(() => () => {
    alive = false;
    runner.cancel();
  });
  function translate(): void {
    if (disabled || indexId === null || !source.trim()) return;
    const request: TranslationRequest = { source, draft, query, indexId };
    partialAccepted = false;
    translationState = { kind: 'pending', request };
    void runner.request(request).result.then(
      (proposal) => {
        if (alive)
          translationState = settleTranslation(
            translationState,
            request,
            proposal
              ? { kind: 'ready', proposal }
              : { kind: 'failed', message: 'Interpretation was cancelled. Try again.' },
          );
      },
      (problem: unknown) => {
        if (alive)
          translationState = settleTranslation(translationState, request, {
            kind: 'failed',
            message: String(problem),
          });
      },
    );
  }
  function choose(): void {
    const expression = translationDraft(translationState, context, partialAccepted);
    if (expression !== null && !disabled) onchoose(expression);
  }
</script>

<details class="offline-search">
  <summary>Describe a search</summary>
  <form
    onsubmit={(event) => {
      event.preventDefault();
      translate();
    }}
  >
    <label for="offline-search-source">Search description</label>
    <div class="advanced-search-entry">
      <input
        id="offline-search-source"
        bind:value={source}
        oninput={invalidate}
        aria-describedby="offline-search-help"
        autocomplete="off"
        spellcheck="false"
        placeholder="archivos mayores de 1 GiB"
      />
      <button
        type="submit"
        disabled={disabled || indexId === null || !source.trim() || current?.kind === 'pending'}
        >Interpret</button
      >
    </div>
    <span id="offline-search-help" class="offline-search-help"
      >Spanish or English. Use explicit dates. Media categories use filename extensions.</span
    >
    {#if current?.kind === 'pending'}
      <span role="status">Interpreting…</span>
    {:else if current?.kind === 'failed'}
      <span role="alert">{current.message}</span>
    {:else if current?.kind === 'ready'}
      <div class="offline-search-proposal" role="status">
        <span>{current.proposal.conditions} interpreted conditions</span>
        {#if current.proposal.expression}<code>{current.proposal.expression}</code>{:else}<span
            >No supported conditions found.</span
          >{/if}
        {#if current.proposal.unparsed.length}
          <span
            >Not interpreted: {current.proposal.unparsed.map((span) => span.text).join(' · ')}</span
          >
          {#if current.proposal.expression}
            <label class="offline-search-partial"
              ><input type="checkbox" bind:checked={partialAccepted} />Use only the interpreted
              conditions</label
            >
          {/if}
        {/if}
      </div>
      <div>
        <button type="button" disabled={proposalDraft === null || disabled} onclick={choose}
          >Use as editable draft</button
        >
      </div>
    {/if}
  </form>
</details>
