<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { retentionQueries } from '../lib/queries';
  import {
    bytes,
    date,
    type SnapshotRetentionSettings,
    type SnapshotRetentionPreview,
    type SnapshotRetentionResult,
  } from '../lib/types';

  let {
    snapshot,
    disabled,
    onconfigure,
    onapply,
  }: {
    snapshot: number;
    disabled: boolean;
    onconfigure: (
      snapshot: number,
      version: string,
      keep: number | null,
    ) => Promise<SnapshotRetentionSettings | undefined>;
    onapply: (
      snapshot: number,
      fingerprint: string,
    ) => Promise<SnapshotRetentionResult | null | undefined>;
  } = $props();
  type Settings =
    | { kind: 'loading' }
    | { kind: 'ready'; value: SnapshotRetentionSettings }
    | { kind: 'failed'; message: string };
  type Review =
    | { kind: 'idle' }
    | { kind: 'loading' }
    | { kind: 'ready'; value: SnapshotRetentionPreview }
    | { kind: 'failed'; message: string };
  let settings = $state.raw<Settings>({ kind: 'loading' }),
    review = $state.raw<Review>({ kind: 'idle' });
  let keepAll = $state(true),
    count = $state<number | undefined>(100),
    accepted = $state(false);
  let region = $state<HTMLElement>();
  let alive = true,
    settingsRequest = 0,
    reviewRequest = 0;
  const configured = $derived(settings.kind === 'ready' ? settings.value : null);
  const valid = $derived(
    keepAll ||
      (typeof count === 'number' && Number.isInteger(count) && count >= 1 && count <= 10000),
  );
  const dirty = $derived(
    configured !== null && configured.keep_latest !== (keepAll ? null : count),
  );
  function clearReview(): void {
    reviewRequest++;
    retentionQueries.cancel();
    accepted = false;
    review = { kind: 'idle' };
  }
  function adopt(value: SnapshotRetentionSettings): void {
    settings = { kind: 'ready', value };
    keepAll = value.keep_latest === null;
    count = value.keep_latest ?? 100;
  }
  async function restoreFocus(retain: boolean): Promise<void> {
    if (!retain) return;
    await tick();
    if (alive && document.activeElement === document.body)
      (
        region?.querySelector<HTMLElement>(
          'button:not(:disabled),select:not(:disabled),input:not(:disabled)',
        ) ?? region
      )?.focus();
  }
  async function reload(): Promise<void> {
    const owner = ++settingsRequest,
      anchor = snapshot;
    const retainFocus = region?.contains(document.activeElement) ?? false;
    clearReview();
    settings = { kind: 'loading' };
    try {
      const value = await invoke<SnapshotRetentionSettings>('get_snapshot_retention', {
        snapshot: anchor,
      });
      if (alive && owner === settingsRequest && anchor === snapshot) adopt(value);
    } catch (problem) {
      if (alive && owner === settingsRequest && anchor === snapshot)
        settings = { kind: 'failed', message: String(problem) };
    }
    if (alive && owner === settingsRequest) await restoreFocus(retainFocus);
  }
  $effect(() => {
    void snapshot;
    void reload();
  });
  $effect(() => {
    if (disabled || dirty) clearReview();
  });
  onDestroy(() => {
    alive = false;
    settingsRequest++;
    clearReview();
  });
  async function save(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!configured || disabled || !dirty || !valid) return;
    const held = configured,
      anchor = snapshot;
    const keep = keepAll ? null : count;
    if (keep === undefined) return;
    const result = await onconfigure(anchor, held.version, keep);
    if (alive && anchor === snapshot && configured === held && result) adopt(result);
  }
  async function inspect(): Promise<void> {
    if (!configured || configured.keep_latest === null || disabled || dirty) return;
    clearReview();
    const owner = reviewRequest,
      anchor = snapshot,
      held = configured;
    const retainFocus = region?.contains(document.activeElement) ?? false;
    review = { kind: 'loading' };
    try {
      const value = await retentionQueries.request({ snapshot: anchor }).result;
      if (!alive || owner !== reviewRequest || anchor !== snapshot) return;
      if (!value) review = { kind: 'failed', message: 'Review cancelled. Review again to retry.' };
      else if (value.settings.version !== held.version || value.settings.root_key !== held.root_key)
        review = { kind: 'failed', message: 'Preference changed. Reload before reviewing.' };
      else {
        adopt(value.settings);
        review = { kind: 'ready', value };
      }
    } catch (problem) {
      if (alive && owner === reviewRequest && anchor === snapshot)
        review = { kind: 'failed', message: String(problem) };
    }
    if (alive && owner === reviewRequest) await restoreFocus(retainFocus);
  }
  async function cancelReview(): Promise<void> {
    const retainFocus = region?.contains(document.activeElement) ?? false;
    clearReview();
    await restoreFocus(retainFocus);
  }
  async function apply(): Promise<void> {
    if (review.kind !== 'ready' || !accepted || disabled || dirty || !review.value.captures.length)
      return;
    const fingerprint = review.value.fingerprint;
    const retainFocus = region?.contains(document.activeElement) ?? false;
    await onapply(snapshot, fingerprint);
    await restoreFocus(retainFocus);
  }
</script>

<section
  class="snapshot-retention"
  aria-label="Snapshot retention"
  tabindex="-1"
  bind:this={region}
>
  <div class="snapshot-retention-heading">
    <h2>Snapshot retention</h2>
    <button onclick={reload} disabled={disabled || settings.kind === 'loading'}
      >Reload preference</button
    >
  </div>
  {#if settings.kind === 'loading'}<p role="status">Reading retention preference…</p>
  {:else if settings.kind === 'failed'}<p role="alert">{settings.message}</p>
  {:else}
    <p class="snapshot-retention-scope">
      <strong>{settings.value.root}</strong> · {settings.value.total.toLocaleString()} saved captures
      in this history group
    </p>
    <form onsubmit={save}>
      <label
        >Keep<select bind:value={keepAll} {disabled}
          ><option value={true}>All saved captures</option><option value={false}
            >Newest saved captures</option
          ></select
        ></label
      >
      {#if !keepAll}<label
          >Count<input
            type="number"
            min="1"
            max="10000"
            step="1"
            required
            bind:value={count}
            {disabled}
          /></label
        >{/if}
      <button type="submit" disabled={disabled || !dirty || !valid}>Save preference</button>
    </form>
    <p class="snapshot-retention-help">
      Removal requires a separate review. This preference applies to this folder's saved history
      group; at least one newest capture is retained.
    </p>
    <div class="snapshot-retention-actions">
      <button
        onclick={inspect}
        disabled={disabled ||
          dirty ||
          settings.value.keep_latest === null ||
          review.kind === 'loading'}>Review oldest captures</button
      >{#if review.kind !== 'idle'}<button onclick={cancelReview} {disabled}>Cancel review</button
        >{/if}
    </div>
    {#if dirty}<p class="snapshot-retention-help">
        Save the changed preference before reviewing.
      </p>{/if}
    {#if review.kind === 'loading'}<p role="status">Preparing exact history review…</p>
    {:else if review.kind === 'failed'}<p role="alert">{review.message}</p>
    {:else if review.kind === 'ready'}
      {#if !review.value.captures.length}<p role="status">All saved captures fit the preference.</p>
      {:else}
        <p>
          Reviewing {review.value.captures.length} of {review.value.eligible.toLocaleString()} eligible
          captures. Further batches require a new review.
        </p>
        <!-- svelte-ignore a11y_no_noninteractive_tabindex (The named scroll region needs keyboard scrolling.) -->
        <div
          class="snapshot-retention-table"
          role="region"
          tabindex="0"
          aria-label="Exact captures to remove"
        >
          <table>
            <thead
              ><tr><th>Capture</th><th>Saved</th><th>Logical size</th><th>Coverage</th></tr></thead
            ><tbody
              >{#each review.value.captures as capture (capture.id)}<tr
                  ><td>#{capture.id}</td><td>{date(capture.captured)}</td><td
                    class="number"
                    title={`${capture.logical} logical bytes`}>{bytes(Number(capture.logical))}</td
                  ><td>{capture.complete ? 'Complete' : 'Partial'}</td></tr
                >{/each}</tbody
            >
          </table>
        </div>
        <p class="snapshot-retention-help">
          Removing these captures permanently discards their historical records. Source files remain
          on disk. Compact is a separate action for unused database pages.
        </p>
        <label class="snapshot-retention-confirm"
          ><input type="checkbox" bind:checked={accepted} {disabled} />I understand these saved
          captures cannot be restored.</label
        >
        <button class="danger" onclick={apply} disabled={disabled || dirty || !accepted}
          >Remove {review.value.captures.length} saved captures</button
        >
      {/if}
    {/if}
  {/if}
</section>
