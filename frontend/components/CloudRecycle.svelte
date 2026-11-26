<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onDestroy, tick } from 'svelte';
  import { applicationBytes as formatBytes } from '../lib/applications';

  type Provider = 'one_drive' | 'google_drive' | 'dropbox';
  type Review = { id: string; name: string; folder: string; bytes: string | null };
  type State =
    | { kind: 'idle' | 'review' | 'browser' | 'ready' | 'recycled' | 'declined' }
    | { kind: 'device'; code: string; retry_seconds: number };
  type View = { review: Review | null; status: State };
  type Action =
    | { kind: 'review'; id: string }
    | { kind: 'status' }
    | { kind: 'authorize' | 'poll' | 'open_sign_in' | 'cancel' | 'confirm'; review: string };

  let {
    provider,
    item,
    onrecycled,
  }: {
    provider: Provider;
    item: { id: string; name: string };
    onrecycled: () => Promise<void>;
  } = $props();
  let view = $state<View | null>(null);
  let busy = $state<Action['kind'] | null>(null);
  let error = $state('');
  let heading = $state<HTMLHeadingElement>();
  let destroyed = false;
  const target = $derived(
    provider === 'dropbox' ? 'Dropbox Deleted files' : 'the provider’s recycle bin',
  );

  function command(action: Action): Promise<View> {
    return invoke<View>('cloud_recycle', { provider, action });
  }

  function discard(current: View | null): void {
    if (current?.review)
      void command({ kind: 'cancel', review: current.review.id }).catch(() => {});
  }

  onDestroy(() => {
    destroyed = true;
    if (busy !== 'confirm') discard(view);
  });

  async function run(action: Action): Promise<void> {
    if (busy) return;
    busy = action.kind;
    error = '';
    const prior = view?.status.kind;
    try {
      const next = await command(action);
      if (destroyed) {
        discard(next);
        return;
      }
      view = next;
      if (next.status.kind === 'recycled') {
        await onrecycled();
      } else if (prior !== next.status.kind && action.kind !== 'status') {
        await tick();
        heading?.focus({ preventScroll: true });
      }
    } catch (failure) {
      if (destroyed) return;
      error = String(failure);
      if (action.kind === 'poll' || action.kind === 'confirm') {
        try {
          view = await command({ kind: 'status' });
        } catch {
          // Keep the last known review and the operation error visible.
        }
      }
    } finally {
      busy = null;
    }
  }

  function withReview(kind: 'authorize' | 'poll' | 'open_sign_in' | 'cancel' | 'confirm'): void {
    if (view?.review) void run({ kind, review: view.review.id });
  }

  $effect(() => {
    if (busy || error || !view?.review) return;
    const state = view.status;
    if (state.kind === 'ready') {
      const timeout = setTimeout(() => void run({ kind: 'status' }), 120_000);
      return () => clearTimeout(timeout);
    }
    if (state.kind !== 'device' && state.kind !== 'browser') return;
    const timeout = setTimeout(
      () => withReview('poll'),
      state.kind === 'device' ? state.retry_seconds * 1000 : 1000,
    );
    return () => clearTimeout(timeout);
  });
</script>

<div class="review">
  <h4 bind:this={heading} tabindex="-1">{view?.review ? 'Recycle cloud file' : item.name}</h4>
  {#if view?.review}
    <dl>
      <div>
        <dt>File</dt>
        <dd>{view.review.name}</dd>
      </div>
      <div>
        <dt>Folder</dt>
        <dd>{view.review.folder}</dd>
      </div>
      <div>
        <dt>Size</dt>
        <dd>{view.review.bytes === null ? 'Unknown' : formatBytes(view.review.bytes)}</dd>
      </div>
    </dl>
    <p>The provider moves the cloud file to {target}. Synced copies can be affected.</p>
    {#if view.status.kind === 'review'}
      {#if provider === 'google_drive'}
        <p>Google asks for Drive write access. Voldiq uses it for this review.</p>
      {:else if provider === 'dropbox'}
        <p>Enable files.content.write in the Dropbox app registration to authorize recycling.</p>
      {:else}
        <p>Enable delegated Files.ReadWrite in the Microsoft Entra app registration.</p>
      {/if}
      <div class="actions">
        <button class="primary" disabled={busy !== null} onclick={() => withReview('authorize')}
          >Authorize recycling</button
        >
        <button class="quiet-action" disabled={busy !== null} onclick={() => withReview('cancel')}
          >Cancel review</button
        >
      </div>
    {:else if view.status.kind === 'device'}
      <p>Enter this code in Microsoft sign-in to grant permission.</p>
      <code>{view.status.code}</code>
      <div class="actions">
        <button class="primary" disabled={busy !== null} onclick={() => withReview('open_sign_in')}
          >Open Microsoft sign-in</button
        >
        <button class="quiet-action" disabled={busy !== null} onclick={() => withReview('cancel')}
          >Cancel review</button
        >
      </div>
    {:else if view.status.kind === 'browser'}
      <p>Finish granting permission in your browser.</p>
      <button class="quiet-action" disabled={busy !== null} onclick={() => withReview('cancel')}
        >Cancel review</button
      >
    {:else if view.status.kind === 'ready'}
      <div class="actions">
        <button class="primary" disabled={busy !== null} onclick={() => withReview('confirm')}
          >Confirm in Windows</button
        >
        <button class="quiet-action" disabled={busy !== null} onclick={() => withReview('cancel')}
          >Cancel review</button
        >
      </div>
    {/if}
  {:else}
    <button
      class="quiet-action"
      disabled={busy !== null}
      onclick={() => void run({ kind: 'review', id: item.id })}>Review recycling</button
    >
  {/if}
  <div role="status">
    {busy === 'review'
      ? 'Checking provider metadata…'
      : busy === 'confirm'
        ? 'Waiting for confirmation and provider result…'
        : view?.status.kind === 'declined'
          ? 'Recycling declined.'
          : ''}
  </div>
  {#if error}<p role="alert" class="problem">{error}</p>{/if}
</div>

<style>
  .review {
    display: grid;
    justify-items: start;
    gap: 7px;
    min-width: 0;
    padding-block: 10px;
    border-top: 1px solid var(--border);
  }
  h4,
  dl,
  dd,
  p {
    margin: 0;
    overflow-wrap: anywhere;
  }
  h4 {
    font-size: 14px;
  }
  dl,
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 14px;
  }
  dt,
  p {
    color: var(--secondary);
  }
  dt {
    font-size: 12px;
  }
  code {
    color: var(--accent);
    font-size: 18px;
    user-select: text;
  }
  [role='status']:empty {
    position: absolute;
  }
  .problem {
    color: var(--error);
  }
</style>
